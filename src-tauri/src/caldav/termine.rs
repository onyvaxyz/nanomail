//! ICS-Auswertung und Termin-Expansion (Meilenstein M4).
//!
//! Aus einem Termin-Objekt (VCALENDAR mit einem oder mehreren VEVENTs)
//! werden die konkreten Vorkommen in einem Zeitfenster berechnet:
//! Wiederholungsregeln (RRULE) expandiert `rrule` — immer nur fürs
//! angefragte Fenster —, Ausnahmen (EXDATE) und verschobene Einzeltermine
//! (RECURRENCE-ID) werden explizit behandelt.
//!
//! Zeitzonen: intern rechnet alles in UTC-Sekunden; TZID-Angaben werden
//! über die IANA-Datenbank (chrono-tz) aufgelöst. Ganztags-Termine haben
//! keine Zeitzone — sie werden auf die lokale Mitternacht gelegt, damit
//! die Anzeige sie dem richtigen Tag zuordnet.

use std::str::FromStr;

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeZone, Utc};
use icalendar::{
    Calendar, CalendarDateTime, Component, DatePerhapsTime, Event, EventLike, EventStatus, Property,
};
use serde::Serialize;

/// Obergrenze an Vorkommen je Wiederholungsregel und Fenster —
/// schützt vor Endlos-Expansion kaputter Regeln.
const MAX_VORKOMMEN: u16 = 600;

/// Ein konkretes Termin-Vorkommen fürs Zeitfenster (anzeigefertig).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Termin {
    pub titel: String,
    pub ort: String,
    pub beschreibung: String,
    pub teilnehmer: Vec<Teilnehmer>,
    /// UTC-Sekunden; bei Ganztags-Terminen die lokale Mitternacht.
    pub beginn: i64,
    /// UTC-Sekunden, exklusiv (Ganztags: Mitternacht nach dem letzten Tag).
    pub ende: i64,
    pub ganztags: bool,
}

/// Teilnehmer eines Termins mit CalDAV-Teilnahmestatus (`PARTSTAT`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Teilnehmer {
    pub email: String,
    /// `needs_action`, `accepted`, `declined`, `tentative` oder `unknown`.
    pub status: String,
}

/// Eingabe für ein schreibbares, einfaches Termin-Objekt (M5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminEntwurf {
    pub uid: String,
    pub sequence: u32,
    pub organisator: Option<String>,
    pub titel: String,
    pub ort: String,
    pub beschreibung: String,
    pub teilnehmer: Vec<Teilnehmer>,
    pub beginn: i64,
    pub ende: i64,
    pub ganztags: bool,
}

/// Eckdaten eines Termin-Objekts für den Cache (Bereichsfilter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadaten {
    pub beginn: Option<i64>,
    pub ende: Option<i64>,
    pub hat_wiederholung: bool,
}

/// Liest die Eckdaten fürs Cachen: frühester Beginn, spätestes Ende,
/// Wiederholungs-Kennzeichen.
pub fn metadaten(ics: &str) -> Result<Metadaten> {
    let kalender = kalender_lesen(ics)?;
    let mut beginn: Option<i64> = None;
    let mut ende: Option<i64> = None;
    let mut hat_wiederholung = false;
    for ereignis in ereignisse(&kalender) {
        if ereignis.property_value("RRULE").is_some()
            || ereignis.properties().contains_key("RDATE")
            || ereignis.multi_properties().contains_key("RDATE")
        {
            hat_wiederholung = true;
        }
        if let Ok(zeiten) = ereignis_zeiten(ereignis) {
            beginn = Some(beginn.map_or(zeiten.beginn, |b| b.min(zeiten.beginn)));
            ende = Some(ende.map_or(zeiten.ende, |e| e.max(zeiten.ende)));
        }
    }
    Ok(Metadaten {
        beginn,
        ende,
        hat_wiederholung,
    })
}

/// Berechnet alle Vorkommen des Termin-Objekts, die das Fenster
/// `[von, bis)` (UTC-Sekunden) überschneiden.
pub fn expandiere(ics: &str, von: i64, bis: i64) -> Result<Vec<Termin>> {
    let kalender = kalender_lesen(ics)?;
    let alle: Vec<&Event> = ereignisse(&kalender).collect();

    // Verschobene Einzeltermine (RECURRENCE-ID) ersetzen das jeweilige
    // Vorkommen der Grundregel.
    let mut ueberschriebene: Vec<i64> = Vec::new();
    let mut ergebnis: Vec<Termin> = Vec::new();
    for ereignis in alle.iter().filter(|e| ist_ueberschreibung(e)) {
        let original = ereignis
            .properties()
            .get("RECURRENCE-ID")
            .and_then(DatePerhapsTime::from_property)
            .context("RECURRENCE-ID unlesbar")?;
        ueberschriebene.push(zeitpunkt(&original)?.0);
        if ereignis.get_status() == Some(EventStatus::Cancelled) {
            continue; // abgesagtes Einzelvorkommen — Lücke bleibt
        }
        let zeiten = ereignis_zeiten(ereignis)?;
        if ueberschneidet(&zeiten, von, bis) {
            ergebnis.push(als_termin(ereignis, &zeiten));
        }
    }

    for ereignis in alle.iter().filter(|e| !ist_ueberschreibung(e)) {
        if ereignis.get_status() == Some(EventStatus::Cancelled) {
            continue;
        }
        let zeiten = ereignis_zeiten(ereignis)?;
        let hat_regel = ereignis.property_value("RRULE").is_some()
            || ereignis.properties().contains_key("RDATE")
            || ereignis.multi_properties().contains_key("RDATE");
        if !hat_regel {
            if ueberschneidet(&zeiten, von, bis) {
                ergebnis.push(als_termin(ereignis, &zeiten));
            }
            continue;
        }

        let dauer = zeiten.ende - zeiten.beginn;
        for beginn in regel_expandieren(ics, ereignis, von - dauer.max(0), bis)? {
            if ueberschriebene.contains(&beginn) {
                continue;
            }
            let vorkommen = Zeiten {
                beginn,
                ende: beginn + dauer,
                ganztags: zeiten.ganztags,
            };
            if ueberschneidet(&vorkommen, von, bis) {
                ergebnis.push(als_termin(ereignis, &vorkommen));
            }
        }
    }

    ergebnis.sort_by_key(|t| (t.beginn, t.titel.clone()));
    Ok(ergebnis)
}

// ------------------------------------------------------------ Grundlagen --

fn kalender_lesen(ics: &str) -> Result<Calendar> {
    ics.parse::<Calendar>()
        .map_err(|fehler| anyhow!("ICS lesen: {fehler}"))
}

fn ereignisse(kalender: &Calendar) -> impl Iterator<Item = &Event> {
    kalender.components.iter().filter_map(|k| k.as_event())
}

fn ist_ueberschreibung(ereignis: &Event) -> bool {
    ereignis.properties().contains_key("RECURRENCE-ID")
}

#[derive(Debug, Clone, Copy)]
struct Zeiten {
    beginn: i64,
    ende: i64,
    ganztags: bool,
}

/// Überschneidet das Vorkommen das Fenster `[von, bis)`?
/// Punkt-Termine (Ende = Beginn) zählen wie eine Minute.
fn ueberschneidet(zeiten: &Zeiten, von: i64, bis: i64) -> bool {
    zeiten.beginn < bis && zeiten.ende.max(zeiten.beginn + 60) > von
}

fn als_termin(ereignis: &Event, zeiten: &Zeiten) -> Termin {
    Termin {
        titel: ereignis.get_summary().unwrap_or("(ohne Titel)").to_string(),
        ort: ereignis.get_location().unwrap_or_default().to_string(),
        beschreibung: ereignis.get_description().unwrap_or_default().to_string(),
        teilnehmer: teilnehmer(ereignis),
        beginn: zeiten.beginn,
        ende: zeiten.ende,
        ganztags: zeiten.ganztags,
    }
}

fn teilnehmer(ereignis: &Event) -> Vec<Teilnehmer> {
    let mut werte = Vec::new();
    if let Some(eintrag) = ereignis.properties().get("ATTENDEE") {
        werte.push(teilnehmer_aus_property(eintrag));
    }
    if let Some(eintraege) = ereignis.multi_properties().get("ATTENDEE") {
        for eintrag in eintraege {
            werte.push(teilnehmer_aus_property(eintrag));
        }
    }
    // Kleinschreibung auch beim Sortieren, sonst überleben Groß-/Klein-
    // Duplikate, die nicht nebeneinander einsortiert werden (dedup_by
    // entfernt nur benachbarte Einträge).
    werte.sort_by(|a, b| {
        a.email
            .to_ascii_lowercase()
            .cmp(&b.email.to_ascii_lowercase())
    });
    werte.dedup_by(|a, b| a.email.eq_ignore_ascii_case(&b.email));
    werte
}

fn teilnehmer_aus_property(eintrag: &Property) -> Teilnehmer {
    Teilnehmer {
        email: mailto_bereinigen(eintrag.value()),
        status: teilnehmer_status(eintrag),
    }
}

fn teilnehmer_status(eintrag: &Property) -> String {
    let status = eintrag
        .get_param_as("PARTSTAT", |wert| Some(wert.to_ascii_uppercase()))
        .unwrap_or_else(|| "NEEDS-ACTION".to_string());
    match status.as_str() {
        "ACCEPTED" => "accepted",
        "DECLINED" => "declined",
        "TENTATIVE" => "tentative",
        "NEEDS-ACTION" => "needs_action",
        _ => "unknown",
    }
    .to_string()
}

fn mailto_bereinigen(wert: &str) -> String {
    let wert = wert.trim();
    // URI-Schemata sind laut RFC 3986 case-insensitiv („Mailto:“ ist gültig).
    match wert.get(..7) {
        Some(schema) if schema.eq_ignore_ascii_case("mailto:") => &wert[7..],
        _ => wert,
    }
    .to_string()
}

/// Prüft, ob eine Adresse gefahrlos in ATTENDEE-/ORGANIZER-Zeilen passt.
/// ICS-Parameterwerte kennen kein Backslash-Escaping; Doppelpunkt, Semikolon
/// oder Komma würden die Zeilenstruktur aufbrechen.
pub fn email_fuer_ics_geeignet(email: &str) -> bool {
    email.contains('@')
        && !email.chars().any(|zeichen| {
            zeichen.is_whitespace()
                || zeichen.is_control()
                || matches!(zeichen, ':' | ';' | ',' | '"' | '\\' | '<' | '>')
        })
}

// ------------------------------------------------------------- Schreiben --

/// Baut ein neues VCALENDAR-Objekt für einfache Termine. Wiederholungen
/// werden in M5 bewusst nicht geschrieben; bestehende Serien bleiben lesbar.
pub fn ics_bauen(entwurf: &TerminEntwurf) -> Result<String> {
    if entwurf.titel.trim().is_empty() {
        anyhow::bail!("Termin ohne Titel");
    }
    if entwurf.ende <= entwurf.beginn {
        anyhow::bail!("Termin-Ende liegt nicht nach dem Beginn");
    }

    let mut zeilen = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//Nanomail//DE".to_string(),
        "CALSCALE:GREGORIAN".to_string(),
        "BEGIN:VEVENT".to_string(),
        format!("UID:{}", text_escapen(&entwurf.uid)),
        format!("SEQUENCE:{}", entwurf.sequence),
        format!("DTSTAMP:{}", utc_format(Utc::now().timestamp())?),
        format!("SUMMARY:{}", text_escapen(entwurf.titel.trim())),
    ];
    if entwurf.ganztags {
        zeilen.push(format!(
            "DTSTART;VALUE=DATE:{}",
            datum_format(entwurf.beginn)?
        ));
        zeilen.push(format!("DTEND;VALUE=DATE:{}", datum_format(entwurf.ende)?));
    } else {
        zeilen.push(format!("DTSTART:{}", utc_format(entwurf.beginn)?));
        zeilen.push(format!("DTEND:{}", utc_format(entwurf.ende)?));
    }
    if !entwurf.ort.trim().is_empty() {
        zeilen.push(format!("LOCATION:{}", text_escapen(entwurf.ort.trim())));
    }
    if !entwurf.beschreibung.trim().is_empty() {
        zeilen.push(format!(
            "DESCRIPTION:{}",
            text_escapen(entwurf.beschreibung.trim())
        ));
    }
    if let Some(organisator) = entwurf
        .organisator
        .as_deref()
        .map(str::trim)
        .filter(|wert| email_fuer_ics_geeignet(wert))
    {
        zeilen.push(format!("ORGANIZER;CN={organisator}:mailto:{organisator}"));
    }
    for teilnehmer in &entwurf.teilnehmer {
        let email = teilnehmer.email.trim();
        if email.is_empty() {
            continue;
        }
        if !email_fuer_ics_geeignet(email) {
            anyhow::bail!("Teilnehmeradresse „{email}“ enthält unzulässige Zeichen");
        }
        let status = status_fuer_ics(&teilnehmer.status);
        // SCHEDULE-AGENT=CLIENT (RFC 6638): Nanomail verschickt die Einladung
        // selbst per Mail — der CalDAV-Server (Nextcloud) darf keine eigene
        // Einladungs-Mail mit Web-Link senden, das gäbe doppelte Einladungen.
        zeilen.push(format!(
            "ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT={status};RSVP=TRUE;SCHEDULE-AGENT=CLIENT;CN={email}:mailto:{email}"
        ));
    }
    zeilen.push("END:VEVENT".to_string());
    zeilen.push("END:VCALENDAR".to_string());

    let mut ics = String::new();
    for zeile in zeilen {
        ics.push_str(&zeile_falten(&zeile));
        ics.push_str("\r\n");
    }
    Ok(ics)
}

fn status_fuer_ics(status: &str) -> &'static str {
    match status {
        "accepted" => "ACCEPTED",
        "declined" => "DECLINED",
        "tentative" => "TENTATIVE",
        "needs_action" => "NEEDS-ACTION",
        _ => "NEEDS-ACTION",
    }
}

/// UID des ersten Grundtermins (ohne RECURRENCE-ID) aus einem Objekt.
pub fn uid(ics: &str) -> Option<String> {
    kalender_lesen(ics).ok().and_then(|kalender| {
        ereignisse(&kalender)
            .find(|e| !ist_ueberschreibung(e))
            .and_then(|e| e.get_uid().map(str::to_string))
    })
}

/// Teilnehmer des ersten Grundtermins, ohne Wiederholungen zu expandieren.
pub fn teilnehmer_grundtermin(ics: &str) -> Vec<Teilnehmer> {
    kalender_lesen(ics)
        .ok()
        .and_then(|kalender| {
            ereignisse(&kalender)
                .find(|e| !ist_ueberschreibung(e))
                .map(teilnehmer)
        })
        .unwrap_or_default()
}

/// SEQUENCE des ersten Grundtermins. Für iTIP-Änderungs-Mails wird der Wert
/// beim Speichern erhöht, damit Mailprogramme Updates zuordnen.
pub fn sequence(ics: &str) -> u32 {
    kalender_lesen(ics)
        .ok()
        .and_then(|kalender| {
            ereignisse(&kalender)
                .find(|e| !ist_ueberschreibung(e))
                .and_then(|e| e.property_value("SEQUENCE"))
                .and_then(|wert| wert.trim().parse::<u32>().ok())
        })
        .unwrap_or(0)
}

/// Einfache Sicherheitsgrenze für M5: Serien/Ausnahmen nicht bearbeiten,
/// damit beim Speichern keine Wiederholungslogik verloren geht.
pub fn ist_einfacher_termin(ics: &str) -> bool {
    let Ok(kalender) = kalender_lesen(ics) else {
        return false;
    };
    let ereignisse: Vec<&Event> = ereignisse(&kalender).collect();
    ereignisse.len() == 1
        && ereignisse[0].property_value("RRULE").is_none()
        && !ereignisse[0].properties().contains_key("RDATE")
        && !ereignisse[0].multi_properties().contains_key("RDATE")
        && !ereignisse[0].properties().contains_key("RECURRENCE-ID")
}

fn utc_format(sekunden: i64) -> Result<String> {
    Ok(DateTime::<Utc>::from_timestamp(sekunden, 0)
        .context("Zeitpunkt außerhalb des darstellbaren Bereichs")?
        .format("%Y%m%dT%H%M%SZ")
        .to_string())
}

fn datum_format(sekunden: i64) -> Result<String> {
    let datum = Local
        .timestamp_opt(sekunden, 0)
        .single()
        .context("Datum außerhalb des darstellbaren Bereichs")?
        .date_naive();
    Ok(format!(
        "{:04}{:02}{:02}",
        datum.year(),
        datum.month(),
        datum.day()
    ))
}

fn text_escapen(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
        .replace('\r', "")
}

fn zeile_falten(zeile: &str) -> String {
    const MAX: usize = 75;
    if zeile.len() <= MAX {
        return zeile.to_string();
    }
    let mut ergebnis = String::new();
    let mut start = 0;
    let mut erste = true;
    while start < zeile.len() {
        let mut ende = (start + MAX).min(zeile.len());
        while !zeile.is_char_boundary(ende) {
            ende -= 1;
        }
        if !erste {
            ergebnis.push_str("\r\n ");
        }
        ergebnis.push_str(&zeile[start..ende]);
        start = ende;
        erste = false;
    }
    ergebnis
}

/// Beginn/Ende/Ganztags eines VEVENTs. Ende-Reihenfolge laut RFC 5545:
/// DTEND, sonst DURATION, sonst Beginn (Ganztags: ein voller Tag).
fn ereignis_zeiten(ereignis: &Event) -> Result<Zeiten> {
    let start = ereignis.get_start().context("Termin ohne Beginn")?;
    let (beginn, ganztags) = zeitpunkt(&start)?;
    let ende = if let Some(ende) = ereignis.get_end() {
        zeitpunkt(&ende)?.0
    } else if let Some(dauer) = ereignis.property_value("DURATION") {
        beginn + dauer_sekunden(dauer)?
    } else if ganztags {
        beginn + 24 * 3600
    } else {
        beginn
    };
    Ok(Zeiten {
        beginn,
        ende,
        ganztags,
    })
}

/// Wandelt einen ICS-Zeitpunkt in UTC-Sekunden um; `true` = reines Datum.
fn zeitpunkt(wert: &DatePerhapsTime) -> Result<(i64, bool)> {
    match wert {
        DatePerhapsTime::Date(datum) => {
            let mitternacht = datum
                .and_hms_opt(0, 0, 0)
                .context("ungültiges Datum im Termin")?;
            Ok((lokal_zu_utc(&chrono::Local, mitternacht)?, true))
        }
        DatePerhapsTime::DateTime(zeit) => match zeit {
            CalendarDateTime::Utc(dt) => Ok((dt.timestamp(), false)),
            // Ohne Zeitzonen-Angabe gilt die lokale Zeit des Nutzers.
            CalendarDateTime::Floating(ndt) => Ok((lokal_zu_utc(&chrono::Local, *ndt)?, false)),
            CalendarDateTime::WithTimezone { date_time, tzid } => {
                match chrono_tz::Tz::from_str(tzid) {
                    Ok(zone) => Ok((lokal_zu_utc(&zone, *date_time)?, false)),
                    Err(_) => {
                        // Unbekannte Zone (z. B. Windows-Name): lokale Zeit
                        // ist die am wenigsten falsche Annahme.
                        tracing::warn!(tzid, "Unbekannte Zeitzone — nehme lokale Zeit an");
                        Ok((lokal_zu_utc(&chrono::Local, *date_time)?, false))
                    }
                }
            }
        },
    }
}

/// Lokale Wandzeit → UTC-Sekunden. In der Frühjahrs-Lücke der
/// Zeitumstellung wird auf die nächste gültige Zeit ausgewichen.
fn lokal_zu_utc<Z: TimeZone>(zone: &Z, zeit: NaiveDateTime) -> Result<i64> {
    zone.from_local_datetime(&zeit)
        .earliest()
        .or_else(|| {
            zone.from_local_datetime(&(zeit + chrono::Duration::hours(1)))
                .earliest()
        })
        .map(|dt| dt.timestamp())
        .context("Zeitpunkt in dieser Zeitzone nicht darstellbar")
}

/// ISO-8601-Dauer aus ICS (`PT1H30M`, `P2D`, `-PT15M`, `P1W`).
fn dauer_sekunden(wert: &str) -> Result<i64> {
    let wert = wert.trim();
    let (vorzeichen, rest) = match wert.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, wert.strip_prefix('+').unwrap_or(wert)),
    };
    let rest = rest
        .strip_prefix('P')
        .with_context(|| format!("Unlesbare Dauer „{wert}“"))?;
    let mut summe: i64 = 0;
    let mut zahl: i64 = 0;
    let mut in_zeitteil = false;
    for zeichen in rest.chars() {
        match zeichen {
            '0'..='9' => zahl = zahl * 10 + i64::from(zeichen as u8 - b'0'),
            'T' => in_zeitteil = true,
            'W' => summe += zahl * 7 * 24 * 3600,
            'D' => summe += zahl * 24 * 3600,
            'H' if in_zeitteil => summe += zahl * 3600,
            'M' if in_zeitteil => summe += zahl * 60,
            'S' if in_zeitteil => summe += zahl,
            _ => anyhow::bail!("Unlesbare Dauer „{wert}“"),
        }
        if !zeichen.is_ascii_digit() {
            zahl = 0;
        }
    }
    Ok(vorzeichen * summe)
}

// ------------------------------------------------- Wiederholungsregeln --

/// Expandiert die Wiederholungsregel eines VEVENTs im Fenster.
/// Die Regel-Zeilen kommen unverändert aus dem Roh-ICS (entfaltet und um
/// unbekannte Parameter bereinigt) — so wertet `rrule` exakt das aus,
/// was der Server geliefert hat.
fn regel_expandieren(ics: &str, ereignis: &Event, von: i64, bis: i64) -> Result<Vec<i64>> {
    let uid = ereignis.get_uid().unwrap_or_default();
    let zeilen = regel_zeilen(ics, uid)?;
    let regelwerk: rrule::RRuleSet = zeilen
        .parse()
        .map_err(|fehler| anyhow!("Wiederholungsregel lesen: {fehler}"))?;

    let von_dt = utc_zeit(von)?;
    let bis_dt = utc_zeit(bis)?;
    let ergebnis = regelwerk.after(von_dt).before(bis_dt).all(MAX_VORKOMMEN);
    if ergebnis.limited {
        tracing::warn!(
            uid,
            "Wiederholungsregel bei {MAX_VORKOMMEN} Vorkommen gekappt"
        );
    }
    Ok(ergebnis.dates.iter().map(DateTime::timestamp).collect())
}

fn utc_zeit(sekunden: i64) -> Result<DateTime<rrule::Tz>> {
    Ok(DateTime::<Utc>::from_timestamp(sekunden, 0)
        .context("Zeitfenster außerhalb des darstellbaren Bereichs")?
        .with_timezone(&rrule::Tz::UTC))
}

/// Sucht im Roh-ICS den VEVENT-Block des Grundtermins (gleiche UID, ohne
/// RECURRENCE-ID) und liefert dessen DTSTART/RRULE/RDATE/EXDATE-Zeilen
/// als Eingabe für `rrule`.
fn regel_zeilen(ics: &str, uid: &str) -> Result<String> {
    let entfaltet = zeilen_entfalten(ics);
    let mut beste: Option<Vec<String>> = None;
    let mut aktuelle: Option<Vec<String>> = None;
    let mut block_passt = true;

    for zeile in &entfaltet {
        if zeile.eq_ignore_ascii_case("BEGIN:VEVENT") {
            aktuelle = Some(Vec::new());
            block_passt = uid.is_empty();
            continue;
        }
        if zeile.eq_ignore_ascii_case("END:VEVENT") {
            if let Some(block) = aktuelle.take() {
                if block_passt && beste.is_none() {
                    beste = Some(block);
                }
            }
            continue;
        }
        let Some(block) = aktuelle.as_mut() else {
            continue;
        };
        let name = zeile
            .split([';', ':'])
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        match name.as_str() {
            "UID" => {
                if let Some((_, wert)) = zeile.split_once(':') {
                    block_passt = wert.trim() == uid;
                }
            }
            "RECURRENCE-ID" => block_passt = false,
            "DTSTART" | "RRULE" | "RDATE" | "EXDATE" | "EXRULE" => {
                block.push(parameter_bereinigen(zeile));
            }
            _ => {}
        }
    }

    let mut block = beste.context("Grundtermin der Wiederholung nicht gefunden")?;
    // DTSTART muss vorn stehen; danach die Regeln.
    block.sort_by_key(|zeile| !zeile.to_ascii_uppercase().starts_with("DTSTART"));
    Ok(block.join("\n"))
}

/// Entfaltet ICS-Zeilen (Fortsetzungszeilen beginnen mit Leerzeichen/Tab).
fn zeilen_entfalten(ics: &str) -> Vec<String> {
    let mut zeilen: Vec<String> = Vec::new();
    for roh in ics.lines() {
        let roh = roh.strip_suffix('\r').unwrap_or(roh);
        if let Some(fortsetzung) = roh.strip_prefix([' ', '\t']) {
            if let Some(letzte) = zeilen.last_mut() {
                letzte.push_str(fortsetzung);
                continue;
            }
        }
        zeilen.push(roh.to_string());
    }
    zeilen
}

/// Behält nur die Parameter, die `rrule` kennt (TZID, VALUE) — andere
/// (z. B. X-Params) würden das Einlesen der Regel scheitern lassen.
fn parameter_bereinigen(zeile: &str) -> String {
    let Some((kopf, wert)) = zeile.split_once(':') else {
        return zeile.to_string();
    };
    let mut teile = kopf.split(';');
    let name = teile.next().unwrap_or_default();
    let params: Vec<&str> = teile
        .filter(|p| {
            let p = p.to_ascii_uppercase();
            p.starts_with("TZID=") || p.starts_with("VALUE=")
        })
        .collect();
    if params.is_empty() {
        format!("{name}:{wert}")
    } else {
        format!("{name};{}:{wert}", params.join(";"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vevent(inhalt: &str) -> String {
        format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//DE\r\n\
             BEGIN:VEVENT\r\nUID:test-1\r\nDTSTAMP:20260101T000000Z\r\n{inhalt}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        )
    }

    /// UTC-Sekunden eines Zeitpunkts (Kurzschreibweise für Tests).
    fn utc(text: &str) -> i64 {
        DateTime::parse_from_rfc3339(text).unwrap().timestamp()
    }

    #[test]
    fn einfacher_termin_mit_zeitzone_wird_nach_utc_gerechnet() {
        // 10:00 in Berlin (Winterzeit, UTC+1) = 09:00 UTC.
        let ics = vevent(
            "SUMMARY:Zahnarzt\r\nLOCATION:Praxis\r\nDESCRIPTION:Kontrolle\r\n\
             DTSTART;TZID=Europe/Berlin:20260119T100000\r\nDTEND;TZID=Europe/Berlin:20260119T103000",
        );
        let termine = expandiere(
            &ics,
            utc("2026-01-01T00:00:00Z"),
            utc("2026-02-01T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(termine.len(), 1);
        let t = &termine[0];
        assert_eq!(t.titel, "Zahnarzt");
        assert_eq!(t.ort, "Praxis");
        assert_eq!(t.beschreibung, "Kontrolle");
        assert!(t.teilnehmer.is_empty());
        assert_eq!(t.beginn, utc("2026-01-19T09:00:00Z"));
        assert_eq!(t.ende, utc("2026-01-19T09:30:00Z"));
        assert!(!t.ganztags);

        // Außerhalb des Fensters taucht er nicht auf.
        assert!(expandiere(
            &ics,
            utc("2026-02-01T00:00:00Z"),
            utc("2026-03-01T00:00:00Z")
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn teilnehmer_werden_aus_mailto_gelesen() {
        let ics = vevent(
            "SUMMARY:Termin\r\nDTSTART:20260713T100000Z\r\nDTEND:20260713T110000Z\r\n\
             ATTENDEE;CN=Alice;PARTSTAT=ACCEPTED:mailto:alice@example.com\r\n\
             ATTENDEE:MAILTO:bob@example.com",
        );
        let termine = expandiere(
            &ics,
            utc("2026-07-01T00:00:00Z"),
            utc("2026-08-01T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(
            termine[0].teilnehmer,
            vec![
                Teilnehmer {
                    email: "alice@example.com".to_string(),
                    status: "accepted".to_string()
                },
                Teilnehmer {
                    email: "bob@example.com".to_string(),
                    status: "needs_action".to_string()
                }
            ]
        );
    }

    #[test]
    fn teilnehmer_dedupliziert_gross_klein_und_gemischtes_mailto() {
        let ics = vevent(
            "SUMMARY:Termin\r\nDTSTART:20260713T100000Z\r\nDTEND:20260713T110000Z\r\n\
             ATTENDEE:mailto:Bob@x.com\r\n\
             ATTENDEE:mailto:apple@x.com\r\n\
             ATTENDEE:Mailto:bob@x.com",
        );
        let termine = expandiere(
            &ics,
            utc("2026-07-01T00:00:00Z"),
            utc("2026-08-01T00:00:00Z"),
        )
        .unwrap();
        let emails: Vec<&str> = termine[0]
            .teilnehmer
            .iter()
            .map(|t| t.email.as_str())
            .collect();
        assert_eq!(emails, vec!["apple@x.com", "Bob@x.com"]);
    }

    #[test]
    fn ics_bauen_lehnt_teilnehmer_mit_sonderzeichen_ab() {
        let mut entwurf = TerminEntwurf {
            uid: "uid-2".into(),
            sequence: 0,
            organisator: Some("kein-email-login".into()),
            titel: "Test".into(),
            ort: String::new(),
            beschreibung: String::new(),
            teilnehmer: vec![Teilnehmer {
                email: "a:b@x.com".into(),
                status: "needs_action".into(),
            }],
            beginn: utc("2026-07-13T10:00:00Z"),
            ende: utc("2026-07-13T11:00:00Z"),
            ganztags: false,
        };
        assert!(ics_bauen(&entwurf).is_err());
        // Ungeeigneter Organisator wird weggelassen statt das ICS zu beschädigen.
        entwurf.teilnehmer.clear();
        let ics = ics_bauen(&entwurf).unwrap();
        assert!(!ics.contains("ORGANIZER"));
    }

    #[test]
    fn ics_bauen_schreibt_ort_und_teilnehmer() {
        let entwurf = TerminEntwurf {
            uid: "uid-1".into(),
            sequence: 2,
            organisator: Some("ich@example.com".into()),
            titel: "Planung, Q3".into(),
            ort: "https://meet.example.com/raum".into(),
            beschreibung: "Zeile 1\nZeile 2".into(),
            teilnehmer: vec![Teilnehmer {
                email: "alice@example.com".into(),
                status: "tentative".into(),
            }],
            beginn: utc("2026-07-13T10:00:00Z"),
            ende: utc("2026-07-13T11:00:00Z"),
            ganztags: false,
        };
        let ics = ics_bauen(&entwurf).unwrap();
        assert!(ics.contains("UID:uid-1\r\n"));
        assert!(ics.contains("SEQUENCE:2\r\n"));
        assert!(!ics.contains("\r\nMETHOD:"));
        assert!(ics.contains("SUMMARY:Planung\\, Q3\r\n"));
        assert!(ics.contains("LOCATION:https://meet.example.com/raum\r\n"));
        assert!(ics.contains("DESCRIPTION:Zeile 1\\nZeile 2\r\n"));
        assert!(ics.contains("ORGANIZER;CN=ich@example.com:mailto:ich@example.com\r\n"));
        assert!(ics.contains("PARTSTAT=TENTATIVE"));
        assert!(ics.contains("SCHEDULE-AGENT=CLIENT"));
        assert!(ics.contains(":mailto:alice@example.com\r\n"));
        assert!(ist_einfacher_termin(&ics));
        assert_eq!(uid(&ics), Some("uid-1".to_string()));
        assert_eq!(sequence(&ics), 2);
    }

    #[test]
    fn sommerzeit_wird_beruecksichtigt() {
        // 10:00 in Berlin im Juli (Sommerzeit, UTC+2) = 08:00 UTC.
        let ics = vevent(
            "SUMMARY:Sommer\r\nDTSTART;TZID=Europe/Berlin:20260713T100000\r\n\
             DTEND;TZID=Europe/Berlin:20260713T110000",
        );
        let termine = expandiere(
            &ics,
            utc("2026-07-01T00:00:00Z"),
            utc("2026-08-01T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(termine[0].beginn, utc("2026-07-13T08:00:00Z"));
    }

    #[test]
    fn ganztags_termin_dauert_einen_tag() {
        let ics = vevent("SUMMARY:Geburtstag\r\nDTSTART;VALUE=DATE:20260710");
        let termine = expandiere(
            &ics,
            utc("2026-07-01T00:00:00Z"),
            utc("2026-08-01T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(termine.len(), 1);
        assert!(termine[0].ganztags);
        assert_eq!(termine[0].ende - termine[0].beginn, 24 * 3600);
    }

    #[test]
    fn dauer_statt_dtend() {
        let ics = vevent("SUMMARY:Kurz\r\nDTSTART:20260713T100000Z\r\nDURATION:PT1H30M");
        let termine = expandiere(
            &ics,
            utc("2026-07-01T00:00:00Z"),
            utc("2026-08-01T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(termine[0].ende - termine[0].beginn, 5400);
    }

    #[test]
    fn dauer_parser_versteht_gaengige_formen() {
        assert_eq!(dauer_sekunden("PT1H30M").unwrap(), 5400);
        assert_eq!(dauer_sekunden("P2D").unwrap(), 2 * 24 * 3600);
        assert_eq!(dauer_sekunden("P1W").unwrap(), 7 * 24 * 3600);
        assert_eq!(dauer_sekunden("-PT15M").unwrap(), -900);
        assert_eq!(dauer_sekunden("P1DT12H").unwrap(), 36 * 3600);
        assert!(dauer_sekunden("1H").is_err());
    }

    #[test]
    fn woechentliche_wiederholung_mit_exdate() {
        // Jeden Montag 09:00 UTC, vier Wochen; der 2. Termin fällt aus.
        let ics = vevent(
            "SUMMARY:Jour fixe\r\nDTSTART:20260706T090000Z\r\nDTEND:20260706T100000Z\r\n\
             RRULE:FREQ=WEEKLY;COUNT=4\r\nEXDATE:20260713T090000Z",
        );
        let termine = expandiere(
            &ics,
            utc("2026-07-01T00:00:00Z"),
            utc("2026-08-01T00:00:00Z"),
        )
        .unwrap();
        let beginne: Vec<i64> = termine.iter().map(|t| t.beginn).collect();
        assert_eq!(
            beginne,
            vec![
                utc("2026-07-06T09:00:00Z"),
                utc("2026-07-20T09:00:00Z"),
                utc("2026-07-27T09:00:00Z"),
            ]
        );
        // Dauer bleibt je Vorkommen erhalten.
        assert!(termine.iter().all(|t| t.ende - t.beginn == 3600));
    }

    #[test]
    fn wiederholung_nur_im_fenster() {
        // Täglich ohne Ende — expandiert wird nur das Fenster.
        let ics = vevent(
            "SUMMARY:Täglich\r\nDTSTART:20200101T060000Z\r\nDTEND:20200101T063000Z\r\nRRULE:FREQ=DAILY",
        );
        let termine = expandiere(
            &ics,
            utc("2026-07-01T00:00:00Z"),
            utc("2026-07-04T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(termine.len(), 3);
        assert_eq!(termine[0].beginn, utc("2026-07-01T06:00:00Z"));
        assert_eq!(termine[2].beginn, utc("2026-07-03T06:00:00Z"));
    }

    #[test]
    fn recurrence_id_verschiebt_ein_vorkommen() {
        // Wöchentlich montags 09:00 UTC; das Vorkommen vom 13.07. ist auf
        // Dienstag 14:00 verschoben (eigenes VEVENT mit RECURRENCE-ID).
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//DE\r\n\
            BEGIN:VEVENT\r\nUID:serie-1\r\nDTSTAMP:20260101T000000Z\r\n\
            SUMMARY:Teamrunde\r\nDTSTART:20260706T090000Z\r\nDTEND:20260706T093000Z\r\n\
            RRULE:FREQ=WEEKLY;COUNT=3\r\nEND:VEVENT\r\n\
            BEGIN:VEVENT\r\nUID:serie-1\r\nDTSTAMP:20260101T000000Z\r\n\
            RECURRENCE-ID:20260713T090000Z\r\nSUMMARY:Teamrunde (verschoben)\r\n\
            DTSTART:20260714T140000Z\r\nDTEND:20260714T143000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let termine = expandiere(
            ics,
            utc("2026-07-01T00:00:00Z"),
            utc("2026-08-01T00:00:00Z"),
        )
        .unwrap();
        let paare: Vec<(i64, &str)> = termine
            .iter()
            .map(|t| (t.beginn, t.titel.as_str()))
            .collect();
        assert_eq!(
            paare,
            vec![
                (utc("2026-07-06T09:00:00Z"), "Teamrunde"),
                (utc("2026-07-14T14:00:00Z"), "Teamrunde (verschoben)"),
                (utc("2026-07-20T09:00:00Z"), "Teamrunde"),
            ]
        );
    }

    #[test]
    fn abgesagte_termine_verschwinden() {
        let ics = vevent(
            "SUMMARY:Abgesagt\r\nSTATUS:CANCELLED\r\nDTSTART:20260713T100000Z\r\nDTEND:20260713T110000Z",
        );
        assert!(expandiere(
            &ics,
            utc("2026-07-01T00:00:00Z"),
            utc("2026-08-01T00:00:00Z")
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn metadaten_liefern_eckdaten() {
        let einfach = vevent("SUMMARY:A\r\nDTSTART:20260713T100000Z\r\nDTEND:20260713T110000Z");
        let m = metadaten(&einfach).unwrap();
        assert_eq!(m.beginn, Some(utc("2026-07-13T10:00:00Z")));
        assert_eq!(m.ende, Some(utc("2026-07-13T11:00:00Z")));
        assert!(!m.hat_wiederholung);

        let wiederholt = vevent(
            "SUMMARY:B\r\nDTSTART:20260713T100000Z\r\nDTEND:20260713T110000Z\r\nRRULE:FREQ=DAILY",
        );
        assert!(metadaten(&wiederholt).unwrap().hat_wiederholung);
    }

    #[test]
    fn lange_zeilen_werden_entfaltet_und_params_bereinigt() {
        let zeilen = zeilen_entfalten("DTSTART;TZID=Europe/\r\n Berlin:20260713T100000\r\nX:1");
        assert_eq!(zeilen[0], "DTSTART;TZID=Europe/Berlin:20260713T100000");
        assert_eq!(
            parameter_bereinigen("EXDATE;X-FOO=bar;TZID=Europe/Berlin:20260713T100000"),
            "EXDATE;TZID=Europe/Berlin:20260713T100000"
        );
        assert_eq!(parameter_bereinigen("RRULE:FREQ=DAILY"), "RRULE:FREQ=DAILY");
    }
}

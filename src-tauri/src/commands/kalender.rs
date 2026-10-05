//! Kalender-Konten, Termine und CalDAV-Abgleich (M4/M5).

use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::{Local, TimeZone};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::caldav::verbindung::{CaldavVerbindung, SyncAntwort};
use crate::caldav::{termine, xml};
use crate::db::{self, kalender as db_kalender};
use crate::schluesselbund;
use crate::smtp::nachricht;
// ----------------------------------------------------- Kalender (M4) --

/// Wie viele Termin-Objekte je `calendar-multiget` angefragt werden.
const MULTIGET_BATCH: usize = 50;

/// Schlüsselbund-Zugriffe für Kalender-Konten — wie bei den Mail-Konten
/// immer über `spawn_blocking` (zbus blockiert intern).
async fn kalender_passwort_holen(konto_id: i64) -> Result<String> {
    tauri::async_runtime::spawn_blocking(move || schluesselbund::kalender_passwort_holen(konto_id))
        .await
        .context("Schlüsselbund-Task abgebrochen")?
}

async fn kalender_passwort_speichern(konto_id: i64, passwort: String) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        schluesselbund::kalender_passwort_speichern(konto_id, &passwort)
    })
    .await
    .context("Schlüsselbund-Task abgebrochen")?
}

/// Eingaben des Kalender-Konto-Dialogs.
#[derive(serde::Deserialize)]
pub struct KalenderKontoFormular {
    pub name: String,
    pub server: String,
    pub benutzer: String,
    pub passwort: String,
}

#[tauri::command]
pub async fn kalender_konto_anlegen(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    formular: KalenderKontoFormular,
) -> Result<db_kalender::KalenderKonto, String> {
    let konto = kalender_konto_anlegen_intern(&zustand, formular)
        .await
        .map_err(|f| als_meldung(&f))?;
    // Termine im Hintergrund laden — die Oberfläche zeigt den Fortschritt
    // über das `kalender:aktualisiert`-Event.
    tauri::async_runtime::spawn(async move {
        let _ = kalender_sync_ausfuehren(&app).await;
    });
    Ok(konto)
}

async fn kalender_konto_anlegen_intern(
    zustand: &AppZustand,
    formular: KalenderKontoFormular,
) -> Result<db_kalender::KalenderKonto> {
    let name = formular.name.trim().to_string();
    let server = formular.server.trim().trim_end_matches('/').to_string();
    let benutzer = formular.benutzer.trim().to_string();
    if name.is_empty() || server.is_empty() || benutzer.is_empty() || formular.passwort.is_empty() {
        return Err(nutzerfehler("Bitte alle Felder ausfüllen."));
    }

    // Zugangsdaten prüfen und Kalender entdecken, bevor etwas gespeichert wird.
    let verbindung = CaldavVerbindung::neu(&server, &benutzer, &formular.passwort)?;
    let funde = verbindung.kalender_finden().await?;
    if funde.is_empty() {
        return Err(nutzerfehler(
            "Die Anmeldung hat geklappt, aber auf dem Server wurden keine \
             Kalender gefunden.",
        ));
    }

    let konto = mit_db(zustand, |conn| {
        db_kalender::konto_anlegen(conn, &name, &server, &benutzer)
    })?;
    if let Err(fehler) = kalender_passwort_speichern(konto.id, formular.passwort).await {
        // Ohne Passwort im Schlüsselbund ist das Konto nutzlos → zurückrollen.
        let _ = mit_db(zustand, |conn| db_kalender::konto_loeschen(conn, konto.id));
        return Err(fehler);
    }
    mit_db(zustand, |conn| {
        for fund in &funde {
            db_kalender::kalender_upsert(
                conn,
                konto.id,
                &fund.href,
                &fund.anzeige_name,
                &fund.farbe,
            )?;
        }
        Ok(())
    })?;
    tracing::info!(
        konto_id = konto.id,
        anzahl = funde.len(),
        "Kalender-Konto angelegt"
    );
    Ok(konto)
}

#[tauri::command]
pub async fn kalender_konto_loeschen(
    zustand: State<'_, AppZustand>,
    konto_id: i64,
) -> Result<(), String> {
    mit_db(&zustand, |conn| db_kalender::konto_loeschen(conn, konto_id))
        .map_err(|f| als_meldung(&f))?;
    tauri::async_runtime::spawn_blocking(move || {
        schluesselbund::kalender_passwort_loeschen(konto_id)
    })
    .await
    .map_err(|_| "Interner Fehler beim Aufräumen des Schlüsselbunds".to_string())?
    .map_err(|f| als_meldung(&f))?;
    tracing::info!(konto_id, "Kalender-Konto entfernt");
    Ok(())
}

#[tauri::command]
pub fn kalender_konten_liste(
    zustand: State<'_, AppZustand>,
) -> Result<Vec<db_kalender::KalenderKonto>, String> {
    mit_db(&zustand, db_kalender::konten_liste).map_err(|f| als_meldung(&f))
}

/// Ein Kalender, wie ihn die Oberfläche anzeigt (mit wirksamer Farbe).
#[derive(Serialize)]
pub struct KalenderAnsicht {
    pub id: i64,
    pub konto_id: i64,
    pub konto_name: String,
    pub anzeige_name: String,
    /// Wirksame Farbe (eigene Wahl vor Nextcloud-Farbe vor Standard).
    pub farbe: String,
    /// Wurde die Farbe in Nanomail überschrieben?
    pub farbe_ist_eigen: bool,
    pub sichtbar: bool,
}

impl From<db_kalender::Kalender> for KalenderAnsicht {
    fn from(kalender: db_kalender::Kalender) -> Self {
        Self {
            farbe: kalender.farbe().to_string(),
            farbe_ist_eigen: !kalender.farbe_eigen.is_empty(),
            id: kalender.id,
            konto_id: kalender.konto_id,
            konto_name: kalender.konto_name,
            anzeige_name: kalender.anzeige_name,
            sichtbar: kalender.sichtbar,
        }
    }
}

#[tauri::command]
pub fn kalender_liste(zustand: State<'_, AppZustand>) -> Result<Vec<KalenderAnsicht>, String> {
    mit_db(&zustand, db_kalender::kalender_liste)
        .map(|liste| liste.into_iter().map(KalenderAnsicht::from).collect())
        .map_err(|f| als_meldung(&f))
}

/// Setzt die eigene Kalender-Farbe (leer = zurück zur Nextcloud-Farbe).
#[tauri::command]
pub fn kalender_farbe_setzen(
    zustand: State<'_, AppZustand>,
    kalender_id: i64,
    farbe: String,
) -> Result<(), String> {
    let farbe = farbe.trim().to_lowercase();
    let gueltig = farbe.is_empty()
        || (farbe.len() == 7
            && farbe.starts_with('#')
            && farbe[1..].chars().all(|z| z.is_ascii_hexdigit()));
    if !gueltig {
        return Err("Die Farbe muss ein Hex-Wert wie #61afef sein.".to_string());
    }
    mit_db(&zustand, |conn| {
        db_kalender::farbe_setzen(conn, kalender_id, &farbe)
    })
    .map_err(|f| als_meldung(&f))
}

#[tauri::command]
pub fn kalender_sichtbar_setzen(
    zustand: State<'_, AppZustand>,
    kalender_id: i64,
    sichtbar: bool,
) -> Result<(), String> {
    mit_db(&zustand, |conn| {
        db_kalender::sichtbar_setzen(conn, kalender_id, sichtbar)
    })
    .map_err(|f| als_meldung(&f))
}

/// Ein anzeigefertiges Termin-Vorkommen fürs Monatsraster.
#[derive(Serialize)]
pub struct TerminAnzeige {
    pub kalender_id: i64,
    /// CalDAV-Objektpfad — für Bearbeiten/Löschen mit Konfliktschutz.
    pub href: String,
    pub etag: String,
    /// Vorkommen gehört zu einer Wiederholungsserie (Löschen trifft alle).
    pub serie: bool,
    pub kalender_name: String,
    pub farbe: String,
    #[serde(flatten)]
    pub termin: termine::Termin,
}

/// Alle Termin-Vorkommen sichtbarer Kalender im Fenster `[von, bis)`
/// (UTC-Sekunden) — aus dem lokalen Cache, funktioniert auch offline.
#[tauri::command]
pub fn kalender_termine(
    zustand: State<'_, AppZustand>,
    von: i64,
    bis: i64,
) -> Result<Vec<TerminAnzeige>, String> {
    kalender_termine_intern(&zustand, von, bis).map_err(|f| als_meldung(&f))
}

pub(crate) fn kalender_termine_intern(
    zustand: &AppZustand,
    von: i64,
    bis: i64,
) -> Result<Vec<TerminAnzeige>> {
    let kalender = mit_db(zustand, db_kalender::kalender_liste)?;
    let quellen = mit_db(zustand, |conn| {
        db_kalender::termine_im_zeitraum(conn, von, bis)
    })?;

    let mut anzeige = Vec::new();
    for quelle in quellen {
        let Some(kal) = kalender.iter().find(|k| k.id == quelle.kalender_id) else {
            continue;
        };
        match termine::expandiere(&quelle.ics, von, bis) {
            Ok(vorkommen) => {
                for termin in vorkommen {
                    anzeige.push(TerminAnzeige {
                        kalender_id: kal.id,
                        href: quelle.href.clone(),
                        etag: quelle.etag.clone(),
                        serie: quelle.wiederholung,
                        kalender_name: kal.anzeige_name.clone(),
                        farbe: kal.farbe().to_string(),
                        termin,
                    });
                }
            }
            // Ein unlesbares Objekt darf die Ansicht nicht verhindern.
            Err(fehler) => tracing::warn!("Termin-Objekt übersprungen: {fehler:#}"),
        }
    }
    anzeige.sort_by_key(|t| t.termin.beginn);
    Ok(anzeige)
}

/// Formular für Termin erstellen/bearbeiten (M5). `href = None` legt neu an;
/// sonst wird mit `If-Match` gegen fremde Änderungen gespeichert.
#[derive(Deserialize)]
pub struct TerminFormular {
    pub kalender_id: i64,
    pub href: Option<String>,
    pub etag: Option<String>,
    pub titel: String,
    pub ort: String,
    pub beschreibung: String,
    pub teilnehmer: String,
    pub beginn: i64,
    pub ende: i64,
    pub ganztags: bool,
    #[serde(default)]
    pub einladung_senden: bool,
    #[serde(default)]
    pub einladung_konto_id: Option<i64>,
}

#[tauri::command]
pub async fn kalender_termin_speichern(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    formular: TerminFormular,
) -> Result<String, String> {
    kalender_termin_speichern_intern(&app, &zustand, formular)
        .await
        .map_err(|f| als_meldung(&f))
}

async fn kalender_termin_speichern_intern(
    app: &AppHandle,
    zustand: &AppZustand,
    formular: TerminFormular,
) -> Result<String> {
    let titel = formular.titel.trim().to_string();
    if titel.is_empty() {
        return Err(nutzerfehler("Bitte einen Titel für den Termin eingeben."));
    }
    if formular.ende <= formular.beginn {
        return Err(nutzerfehler("Das Ende muss nach dem Beginn liegen."));
    }
    let teilnehmer = adressliste(&formular.teilnehmer);
    for email in &teilnehmer {
        if !termine::email_fuer_ics_geeignet(email) {
            return Err(nutzerfehler(format!(
                "„{email}“ sieht nicht wie eine E-Mail-Adresse aus."
            )));
        }
    }

    let (kalender, konto, verbindung) =
        caldav_verbindung_zum_kalender(zustand, formular.kalender_id).await?;
    let vorhandener_href = formular
        .href
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty());
    let (href, uid, sequence, alte_teilnehmer, etag_alt, ist_aenderung) =
        if let Some(href) = vorhandener_href {
            let ics_alt = mit_db(zustand, |conn| {
                db_kalender::termin_ics(conn, kalender.id, href)
            })?
            .ok_or_else(|| nutzerfehler("Der Termin ist lokal nicht mehr vorhanden."))?;
            if !termine::ist_einfacher_termin(&ics_alt) {
                return Err(nutzerfehler(
                    "Wiederholungstermine können in Nanomail aktuell nicht bearbeitet werden — \
                 bitte diesen Termin direkt in Nextcloud ändern.",
                ));
            }
            let uid = termine::uid(&ics_alt).unwrap_or_else(|| neue_termin_uid(kalender.id));
            let sequence = termine::sequence(&ics_alt).saturating_add(1);
            let alte_teilnehmer = termine::teilnehmer_grundtermin(&ics_alt);
            let etag = formular
                .etag
                .as_deref()
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    mit_db(zustand, |conn| {
                        db_kalender::termin_etag(conn, kalender.id, href)
                    })
                    .ok()
                    .flatten()
                    // Leerer Cache-ETag würde ein leeres If-Match erzeugen.
                    .filter(|etag| !etag.is_empty())
                })
                .ok_or_else(|| {
                    nutzerfehler(
                        "Der Termin kann ohne Server-Version nicht sicher gespeichert werden — \
                     bitte den Kalender aktualisieren und den Termin erneut öffnen.",
                    )
                })?;
            (
                href.to_string(),
                uid,
                sequence,
                alte_teilnehmer,
                Some(etag),
                true,
            )
        } else {
            let uid = neue_termin_uid(kalender.id);
            let datei = uid
                .chars()
                .map(|z| {
                    if z.is_ascii_alphanumeric() || z == '-' {
                        z
                    } else {
                        '-'
                    }
                })
                .collect::<String>();
            (
                format!("{}{}.ics", kalender.href, datei),
                uid,
                0,
                Vec::new(),
                None,
                false,
            )
        };

    let einladung_senden = formular.einladung_senden;
    // Das Mailkonto für die Einladung wird vor dem Speichern geladen: Seine
    // Adresse ist der Organisator im Termin, damit ORGANIZER und Mail-Absender
    // übereinstimmen — ohne ORGANIZER ist eine iTIP-Einladung ungültig und
    // Mailprogramme zeigen keine Zusagen-/Absagen-Knöpfe.
    let einladung_konto = match (
        einladung_senden && !teilnehmer.is_empty(),
        formular.einladung_konto_id,
    ) {
        (true, Some(id)) => Some(konto_laden(zustand, id)?),
        _ => None,
    };
    let organisator = einladung_konto
        .as_ref()
        .map(|mail_konto| mail_konto.email.clone())
        .or_else(|| konto.benutzer.contains('@').then(|| konto.benutzer.clone()))
        .filter(|email| termine::email_fuer_ics_geeignet(email));
    let teilnehmer_entwurf = termin_teilnehmer(&teilnehmer, &alte_teilnehmer);
    let entwurf = termine::TerminEntwurf {
        uid,
        sequence,
        organisator,
        titel,
        ort: formular.ort.trim().to_string(),
        beschreibung: formular.beschreibung.trim().to_string(),
        teilnehmer: teilnehmer_entwurf,
        beginn: formular.beginn,
        ende: formular.ende,
        ganztags: formular.ganztags,
    };
    let ics = termine::ics_bauen(&entwurf, termine::IcsZiel::CaldavObjekt)?;
    let etag_neu = verbindung
        .termin_speichern(&href, &ics, etag_alt.as_deref())
        .await?;
    gespeichertes_objekt_cachen(app, zustand, &verbindung, &kalender, href, &ics, etag_neu).await?;
    if einladung_senden && !teilnehmer.is_empty() {
        let Some(mail_konto) = einladung_konto else {
            return Ok(
                "Termin gespeichert — aber es wurde kein Mailkonto für die Einladung ausgewählt."
                    .to_string(),
            );
        };
        // Die Mail bekommt eine eigene ICS-Fassung ohne ORGANIZER — mit
        // ORGANIZER lehnen manche Mail-Anbieter die Einladung pauschal ab
        // („550 Reject for policy reason“, Schutz vor Kalender-Spoofing).
        let mail_ics = termine::ics_bauen(&entwurf, termine::IcsZiel::EinladungsMail)?;
        if let Err(fehler) = kalender_einladung_senden(
            app,
            zustand,
            &mail_konto,
            &teilnehmer,
            &entwurf,
            &mail_ics,
            ist_aenderung,
        )
        .await
        {
            tracing::error!("Kalender-Einladung nicht versendet: {fehler:#}");
            return Ok(format!(
                "Termin gespeichert — aber die Einladungs-Mail konnte nicht versendet werden: {}",
                als_meldung(&fehler)
            ));
        }
    }
    tracing::info!(
        konto_id = konto.id,
        kalender_id = kalender.id,
        "Termin gespeichert"
    );
    Ok(if einladung_senden && !teilnehmer.is_empty() {
        if ist_aenderung {
            "Termin gespeichert und Änderungs-Mail versendet.".to_string()
        } else {
            "Termin gespeichert und Einladung versendet.".to_string()
        }
    } else {
        "Termin gespeichert.".to_string()
    })
}

fn termin_teilnehmer(
    adressen: &[String],
    alte_teilnehmer: &[termine::Teilnehmer],
) -> Vec<termine::Teilnehmer> {
    adressen
        .iter()
        .map(|email| {
            let status = alte_teilnehmer
                .iter()
                .find(|t| t.email.eq_ignore_ascii_case(email))
                .map(|t| t.status.clone())
                .unwrap_or_else(|| "needs_action".to_string());
            termine::Teilnehmer {
                email: email.clone(),
                status,
            }
        })
        .collect()
}

async fn kalender_einladung_senden(
    app: &AppHandle,
    zustand: &AppZustand,
    konto: &db::Konto,
    teilnehmer: &[String],
    termin: &termine::TerminEntwurf,
    ics: &str,
    ist_aenderung: bool,
) -> Result<()> {
    if konto.smtp_host.is_empty() {
        return Err(nutzerfehler(
            "Für das gewählte Mailkonto ist noch kein Versand-Server (SMTP) hinterlegt.",
        ));
    }
    let einladung = nachricht::KalenderEinladung {
        von_name: konto.anzeigename.clone(),
        von_adresse: konto.email.clone(),
        an: teilnehmer.to_vec(),
        betreff: if ist_aenderung {
            format!("Aktualisierung: {}", termin.titel)
        } else {
            format!("Einladung: {}", termin.titel)
        },
        text: einladung_text(termin, ist_aenderung),
        ics: ics.to_string(),
    };
    let (fertig, rohbytes) =
        nachricht::baue_kalender_einladung(&einladung).map_err(|f| nutzerfehler(f.to_string()))?;
    smtp_senden(konto, fertig).await?;
    if let Err(fehler) = mit_db(zustand, |conn| {
        for adresse in teilnehmer {
            db::adresse_merken(conn, adresse)?;
        }
        Ok(())
    }) {
        tracing::warn!("Teilnehmeradressen nicht gemerkt: {fehler:#}");
    }
    let alle_ordner = mit_db(zustand, |conn| db::ordner_liste(conn, konto.id))?;
    if let Some(gesendet) = db::finde_gesendet_ordner(&alle_ordner) {
        if let Err(fehler) = sent_ablage(app, zustand, konto, gesendet, &rohbytes).await {
            tracing::warn!("Gesendet-Ablage der Kalendereinladung fehlgeschlagen: {fehler:#}");
        }
    }
    Ok(())
}

fn einladung_text(termin: &termine::TerminEntwurf, ist_aenderung: bool) -> String {
    let beginn = zeit_text(termin.beginn);
    let ende = zeit_text(termin.ende);
    let einleitung = if ist_aenderung {
        "Der folgende Termin wurde aktualisiert:"
    } else {
        "Du bist zu folgendem Termin eingeladen:"
    };
    let mut text = format!("{einleitung}\n\n{}\n{beginn} – {ende}", termin.titel);
    if !termin.ort.trim().is_empty() {
        text.push_str("\nOrt: ");
        text.push_str(termin.ort.trim());
    }
    if !termin.beschreibung.trim().is_empty() {
        text.push_str("\n\n");
        text.push_str(termin.beschreibung.trim());
    }
    text.push_str("\n\nDiese Einladung wurde mit Nanomail versendet.");
    text
}

fn zeit_text(sekunden: i64) -> String {
    Local
        .timestamp_opt(sekunden, 0)
        .single()
        .map(|zeit| zeit.format("%d.%m.%Y %H:%M").to_string())
        .unwrap_or_else(|| "unbekannte Zeit".to_string())
}

#[tauri::command]
pub async fn kalender_termin_loeschen(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    kalender_id: i64,
    href: String,
    etag: String,
) -> Result<(), String> {
    kalender_termin_loeschen_intern(&app, &zustand, kalender_id, href, etag)
        .await
        .map_err(|f| als_meldung(&f))
}

async fn kalender_termin_loeschen_intern(
    app: &AppHandle,
    zustand: &AppZustand,
    kalender_id: i64,
    href: String,
    etag: String,
) -> Result<()> {
    let (kalender, _konto, verbindung) =
        caldav_verbindung_zum_kalender(zustand, kalender_id).await?;
    let href = href.trim().to_string();
    if href.is_empty() || etag.trim().is_empty() {
        return Err(nutzerfehler(
            "Der Termin kann ohne Server-Version nicht sicher gelöscht werden.",
        ));
    }
    verbindung.termin_loeschen(&href, etag.trim()).await?;
    mit_db(zustand, |conn| {
        db_kalender::termin_loeschen(conn, kalender.id, &href)
    })?;
    let _ = app.emit("kalender:aktualisiert", ());
    tracing::info!(kalender_id = kalender.id, "Termin gelöscht");
    Ok(())
}

pub(crate) async fn caldav_verbindung_zum_kalender(
    zustand: &AppZustand,
    kalender_id: i64,
) -> Result<(
    db_kalender::Kalender,
    db_kalender::KalenderKonto,
    CaldavVerbindung,
)> {
    let kalender = mit_db(zustand, |conn| {
        db_kalender::kalender_holen(conn, kalender_id)
    })?
    .ok_or_else(|| nutzerfehler("Der Kalender ist nicht mehr vorhanden."))?;
    let konto = mit_db(zustand, |conn| {
        db_kalender::konto_holen(conn, kalender.konto_id)
    })?
    .ok_or_else(|| nutzerfehler("Das Kalender-Konto ist nicht mehr vorhanden."))?;
    let passwort = kalender_passwort_holen(konto.id).await?;
    let verbindung = CaldavVerbindung::neu(&konto.server, &konto.benutzer, &passwort)?;
    Ok((kalender, konto, verbindung))
}

fn neue_termin_uid(kalender_id: i64) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dauer| dauer.as_millis())
        .unwrap_or_default();
    format!("nanomail-{kalender_id}-{millis}")
}

#[tauri::command]
pub async fn kalender_sync(app: AppHandle) -> Result<(), String> {
    kalender_sync_ausfuehren(&app).await
}

/// Gleicht alle Kalender-Konten ab (mit Doppelstart-Schutz). Wird vom
/// Command, beim App-Start und vom periodischen Sync genutzt.
pub async fn kalender_sync_ausfuehren(app: &AppHandle) -> Result<(), String> {
    let zustand = app.state::<AppZustand>();
    {
        let mut laeuft = zustand
            .kalender_sync_laeuft
            .lock()
            .map_err(|_| "Interner Fehler beim Kalender-Abgleich".to_string())?;
        if *laeuft {
            return Ok(()); // läuft bereits — kein Fehler
        }
        *laeuft = true;
    }

    let konten = mit_db(&zustand, db_kalender::konten_liste).unwrap_or_default();
    let mut erster_fehler: Option<String> = None;
    for konto in konten {
        if let Err(fehler) = kalender_konto_synchronisieren(app, &zustand, &konto).await {
            tracing::error!(
                konto_id = konto.id,
                "Kalender-Abgleich fehlgeschlagen: {fehler:#}"
            );
            erster_fehler
                .get_or_insert_with(|| format!("{}: {}", konto.name, als_meldung(&fehler)));
        }
    }

    if let Ok(mut laeuft) = zustand.kalender_sync_laeuft.lock() {
        *laeuft = false;
    }
    let _ = app.emit("kalender:aktualisiert", ());
    match erster_fehler {
        None => Ok(()),
        Some(meldung) => Err(meldung),
    }
}

/// Abgleich eines Kontos: Kalenderliste auffrischen, dann jeden Kalender
/// über sein Sync-Token abgleichen. Ein kaputter Kalender bricht nicht
/// den ganzen Abgleich ab.
async fn kalender_konto_synchronisieren(
    app: &AppHandle,
    zustand: &AppZustand,
    konto: &db_kalender::KalenderKonto,
) -> Result<()> {
    let passwort = kalender_passwort_holen(konto.id).await?;
    let verbindung = CaldavVerbindung::neu(&konto.server, &konto.benutzer, &passwort)?;

    // 1) Kalenderliste abgleichen (neue/umbenannte/gelöschte Kalender).
    let funde = verbindung.kalender_finden().await?;
    let konto_id = konto.id;
    let kalender_liste = mit_db(zustand, |conn| {
        let hrefs: Vec<String> = funde.iter().map(|f| f.href.clone()).collect();
        db_kalender::kalender_bereinigen(conn, konto_id, &hrefs)?;
        for fund in &funde {
            db_kalender::kalender_upsert(
                conn,
                konto_id,
                &fund.href,
                &fund.anzeige_name,
                &fund.farbe,
            )?;
        }
        db_kalender::kalender_liste(conn)
    })?;

    // 2) Termine je Kalender abgleichen.
    for kalender in kalender_liste.iter().filter(|k| k.konto_id == konto_id) {
        if let Err(fehler) = kalender_abgleichen(zustand, &verbindung, kalender).await {
            tracing::error!(kalender = %kalender.anzeige_name, "Kalender-Sync fehlgeschlagen: {fehler:#}");
        } else {
            let _ = app.emit("kalender:aktualisiert", ());
        }
    }
    Ok(())
}

/// Sync-Token-Abgleich eines einzelnen Kalenders; bei verfallenem Token
/// wird der Kalender einmal komplett neu geladen.
async fn kalender_abgleichen(
    zustand: &AppZustand,
    verbindung: &CaldavVerbindung,
    kalender: &db_kalender::Kalender,
) -> Result<()> {
    let ergebnis = match verbindung
        .abgleichen(&kalender.href, &kalender.sync_token)
        .await?
    {
        SyncAntwort::Ergebnis(ergebnis) => ergebnis,
        SyncAntwort::TokenUngueltig => {
            mit_db(zustand, |conn| {
                db_kalender::termine_leeren(conn, kalender.id)
            })?;
            match verbindung.abgleichen(&kalender.href, "").await? {
                SyncAntwort::Ergebnis(ergebnis) => ergebnis,
                SyncAntwort::TokenUngueltig => {
                    anyhow::bail!("Server lehnt auch den Erstabgleich ab")
                }
            }
        }
    };

    // Gelöschte Objekte aus dem Cache räumen.
    mit_db(zustand, |conn| {
        for href in &ergebnis.geloescht {
            db_kalender::termin_loeschen(conn, kalender.id, href)?;
        }
        Ok(())
    })?;

    // Nur Objekte laden, deren ETag sich geändert hat.
    let mut zu_laden: Vec<String> = Vec::new();
    for (href, etag) in &ergebnis.geaendert {
        let gecacht = mit_db(zustand, |conn| {
            db_kalender::termin_etag(conn, kalender.id, href)
        })?;
        if gecacht.as_deref() != Some(etag.as_str()) {
            zu_laden.push(href.clone());
        }
    }
    for batch in zu_laden.chunks(MULTIGET_BATCH) {
        let objekte = verbindung.objekte_laden(&kalender.href, batch).await?;
        mit_db(zustand, |conn| {
            for objekt in &objekte {
                termin_objekt_speichern(conn, kalender.id, objekt)?;
            }
            Ok(())
        })?;
    }

    mit_db(zustand, |conn| {
        db_kalender::sync_token_setzen(conn, kalender.id, &ergebnis.token)
    })?;
    tracing::debug!(
        kalender = %kalender.anzeige_name,
        neu = zu_laden.len(),
        geloescht = ergebnis.geloescht.len(),
        "Kalender abgeglichen"
    );
    Ok(())
}

/// Legt ein geladenes Termin-Objekt samt Eckdaten im Cache ab.
pub(crate) fn termin_objekt_speichern(
    conn: &rusqlite::Connection,
    kalender_id: i64,
    objekt: &xml::ObjektDaten,
) -> Result<()> {
    // Ein unlesbares ICS wird trotzdem gecacht (ohne Eckdaten), damit
    // der Abgleich es nicht bei jedem Lauf erneut lädt.
    let eckdaten = termine::metadaten(&objekt.ics).unwrap_or_else(|fehler| {
        tracing::warn!("Termin-Eckdaten nicht lesbar: {fehler:#}");
        termine::Metadaten {
            beginn: None,
            ende: None,
            hat_wiederholung: false,
        }
    });
    db_kalender::termin_upsert(
        conn,
        kalender_id,
        &objekt.href,
        &objekt.etag,
        &objekt.ics,
        eckdaten.beginn,
        eckdaten.ende,
        eckdaten.hat_wiederholung,
    )
}

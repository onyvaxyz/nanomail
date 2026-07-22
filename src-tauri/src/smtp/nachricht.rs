//! Nachrichtenbau — reine, unit-getestete Funktionen (kein Netzwerk).
//!
//! Baut aus den Eingaben des Frontends eine versandfertige Mail
//! (`lettre::Message`) samt Rohbytes für die „Gesendet“-Ablage und
//! erzeugt die Vorlagen für Antworten/Weiterleiten.

use anyhow::{Context, Result};
use chrono::{Local, TimeZone};
use lettre::message::header::{ContentDisposition, ContentType};
use lettre::message::{Attachment, Body, Mailbox, MultiPart, SinglePart};
use lettre::Message;

/// Ein Anhang (bereits eingelesen).
#[derive(Debug, Clone)]
pub struct Anhang {
    pub dateiname: String,
    pub mime: String,
    pub daten: Vec<u8>,
}

/// Threading-Bezug bei Antworten.
#[derive(Debug, Clone, Default)]
pub struct AntwortKontext {
    /// Message-ID der Original-Mail (mit spitzen Klammern).
    pub message_id: Option<String>,
    /// References-Kette der Original-Mail (roh).
    pub references: Option<String>,
}

/// Alle Zutaten für eine ausgehende Mail.
#[derive(Debug, Clone)]
pub struct NeueNachricht {
    pub von_name: String,
    pub von_adresse: String,
    pub an: Vec<String>,
    pub cc: Vec<String>,
    pub betreff: String,
    pub text: String,
    /// Formatierte Fassung (bereits bereinigt) — wenn vorhanden, wird die
    /// Mail als multipart/alternative mit Text- und HTML-Teil gebaut.
    pub html: Option<String>,
    pub anhaenge: Vec<Anhang>,
    pub antwort: Option<AntwortKontext>,
}

/// Zutaten für eine Kalender-Einladung per Mail (iTIP REQUEST).
#[derive(Debug, Clone)]
pub struct KalenderEinladung {
    pub von_name: String,
    pub von_adresse: String,
    pub an: Vec<String>,
    pub betreff: String,
    pub text: String,
    /// VCALENDAR ohne METHOD (CalDAV-Objekt); für die Mail wird METHOD:REQUEST ergänzt.
    pub ics: String,
}

/// Programm-Kennung im `User-Agent`-Kopf. Ohne eine solche Kennung stufen
/// manche Versand-Server (z. B. appsuite/Open-Xchange) ausgehende Mails als
/// Bot/Spam ein und lehnen sie mit „550 Reject for policy reason“ ab.
const PROGRAMM_KENNUNG: &str = "Nanomail/0.1.0";

/// Baut die versandfertige Nachricht. Liefert zusätzlich die Rohbytes
/// für das IMAP-APPEND in den „Gesendet“-Ordner.
pub fn baue_nachricht(eingabe: &NeueNachricht) -> Result<(Message, Vec<u8>)> {
    let von = mailbox(&eingabe.von_name, &eingabe.von_adresse)?;

    let absender_adresse = von.email.clone();
    let mut builder = Message::builder()
        .from(von)
        .user_agent(PROGRAMM_KENNUNG.to_string())
        .message_id(Some(neue_message_id(&absender_adresse)));
    for adresse in &eingabe.an {
        builder = builder.to(parse_adresse(adresse)?);
    }
    for adresse in &eingabe.cc {
        builder = builder.cc(parse_adresse(adresse)?);
    }
    builder = builder.subject(&eingabe.betreff);

    // Entwürfe dürfen ohne Empfänger gebaut werden. `lettre` verlangt aber
    // einen Umschlag mit Ziel — der ist nur fürs Versenden relevant und
    // landet nicht in den Rohbytes, deshalb genügt die eigene Adresse.
    // (Der Versand-Weg prüft vorher, dass Empfänger vorhanden sind.)
    if eingabe.an.is_empty() && eingabe.cc.is_empty() {
        builder = builder.envelope(
            lettre::address::Envelope::new(Some(absender_adresse.clone()), vec![absender_adresse])
                .context("Umschlag ohne Empfänger bauen")?,
        );
    }

    if let Some(antwort) = &eingabe.antwort {
        if let Some(id) = &antwort.message_id {
            builder = builder.in_reply_to(id.clone());
            if let Some(referenzen) = neue_references(antwort.references.as_deref(), Some(id)) {
                builder = builder.references(referenzen);
            }
        }
    }

    let text_teil = SinglePart::builder()
        .header(ContentType::TEXT_PLAIN)
        .body(eingabe.text.clone());
    // Mit HTML-Fassung: Text + HTML als Alternative (Empfänger-Programm
    // wählt), sonst schlichter Text-Teil.
    let inhalt = match &eingabe.html {
        Some(html) => Inhalt::Mehrteilig(
            MultiPart::alternative().singlepart(text_teil).singlepart(
                SinglePart::builder()
                    .header(ContentType::TEXT_HTML)
                    .body(html.clone()),
            ),
        ),
        None => Inhalt::Einteilig(text_teil),
    };

    let nachricht = if eingabe.anhaenge.is_empty() {
        match inhalt {
            Inhalt::Einteilig(teil) => builder.singlepart(teil),
            Inhalt::Mehrteilig(teil) => builder.multipart(teil),
        }
        .context("Nachricht zusammenbauen")?
    } else {
        let mut mehrteilig = match inhalt {
            Inhalt::Einteilig(teil) => MultiPart::mixed().singlepart(teil),
            Inhalt::Mehrteilig(teil) => MultiPart::mixed().multipart(teil),
        };
        for anhang in &eingabe.anhaenge {
            let typ = anhang
                .mime
                .parse::<ContentType>()
                .unwrap_or(ContentType::parse("application/octet-stream").expect("gültiger Typ"));
            mehrteilig = mehrteilig.singlepart(
                Attachment::new(anhang.dateiname.clone())
                    .body(Body::new(anhang.daten.clone()), typ),
            );
        }
        builder
            .multipart(mehrteilig)
            .context("Nachricht zusammenbauen")?
    };

    let rohbytes = nachricht.formatted();
    Ok((nachricht, rohbytes))
}

/// Baut eine iTIP-Einladungs-Mail zu einem bereits gespeicherten Kalendertermin.
/// Der gespeicherte CalDAV-Termin darf kein METHOD enthalten; in der Mail ist
/// METHOD:REQUEST dagegen richtig, damit Mailprogramme sie als Einladung erkennen.
pub fn baue_kalender_einladung(eingabe: &KalenderEinladung) -> Result<(Message, Vec<u8>)> {
    let von = mailbox(&eingabe.von_name, &eingabe.von_adresse)?;
    let message_id = neue_message_id(&von.email);
    let mut builder = Message::builder()
        .from(von)
        .user_agent(PROGRAMM_KENNUNG.to_string())
        .message_id(Some(message_id))
        .subject(&eingabe.betreff);
    for adresse in &eingabe.an {
        builder = builder.to(parse_adresse(adresse)?);
    }
    let ics = ics_mit_methode(&eingabe.ics, "REQUEST");
    let text_teil = SinglePart::builder()
        .header(ContentType::TEXT_PLAIN)
        .body(eingabe.text.clone());
    let kalender_typ = ContentType::parse("text/calendar; method=REQUEST; charset=utf-8")
        .context("Kalender-Inhaltstyp bauen")?;
    let kalender_teil = SinglePart::builder()
        .header(kalender_typ)
        .header(ContentDisposition::inline_with_name("einladung.ics"))
        .body(ics);
    let nachricht = builder
        .multipart(
            MultiPart::alternative()
                .singlepart(text_teil)
                .singlepart(kalender_teil),
        )
        .context("Kalender-Einladung zusammenbauen")?;
    let rohbytes = nachricht.formatted();
    Ok((nachricht, rohbytes))
}

/// Baut die Absender-Mailbox strukturiert statt über String-Parsen, damit
/// Anzeigenamen mit Sonderzeichen (Komma, Klammern) korrekt zitiert werden.
fn mailbox(name: &str, adresse: &str) -> Result<Mailbox> {
    let adresse = adresse
        .trim()
        .parse::<lettre::Address>()
        .context("Absenderadresse ungültig")?;
    let name = name.trim();
    Ok(Mailbox::new(
        (!name.is_empty()).then(|| name.to_string()),
        adresse,
    ))
}

/// Erzeugt eine weltweit eindeutige Nachrichten-ID, ohne den lokalen
/// Rechnernamen preiszugeben. Die Domain des Absenders macht die ID
/// zugleich eindeutig Nanomails Absender zuordenbar.
fn neue_message_id(absender: &lettre::Address) -> String {
    format!("<{}@{}>", uuid::Uuid::new_v4(), absender.domain())
}

/// Inhaltsteil der Mail vor dem Anfügen der Anhänge.
enum Inhalt {
    Einteilig(SinglePart),
    Mehrteilig(MultiPart),
}

fn parse_adresse(eingabe: &str) -> Result<Mailbox> {
    eingabe
        .trim()
        .parse()
        .with_context(|| format!("Empfängeradresse „{}“ ist ungültig", eingabe.trim()))
}

fn ics_mit_methode(ics: &str, methode: &str) -> String {
    let ohne_methode: Vec<&str> = ics
        .lines()
        .filter(|zeile| {
            !zeile
                .trim_start()
                .to_ascii_uppercase()
                .starts_with("METHOD:")
        })
        .collect();
    let mut ergebnis = String::new();
    let mut eingefuegt = false;
    for zeile in ohne_methode {
        let zeile = zeile.trim_end_matches('\r');
        ergebnis.push_str(zeile);
        ergebnis.push_str("\r\n");
        if !eingefuegt && zeile.to_ascii_uppercase().starts_with("VERSION:") {
            ergebnis.push_str("METHOD:");
            ergebnis.push_str(methode);
            ergebnis.push_str("\r\n");
            eingefuegt = true;
        }
    }
    if !eingefuegt {
        ergebnis = format!("METHOD:{methode}\r\n{ergebnis}");
    }
    ergebnis
}

/// „Re:“ voranstellen — aber nicht verdoppeln (auch „AW:“ zählt).
pub fn antwort_betreff(betreff: &str) -> String {
    let b = betreff.trim();
    let klein = b.to_lowercase();
    if klein.starts_with("re:") || klein.starts_with("aw:") {
        b.to_string()
    } else {
        format!("Re: {b}")
    }
}

/// „Fwd:“ voranstellen — aber nicht verdoppeln (auch „WG:“ zählt).
pub fn weiterleit_betreff(betreff: &str) -> String {
    let b = betreff.trim();
    let klein = b.to_lowercase();
    if klein.starts_with("fwd:") || klein.starts_with("wg:") || klein.starts_with("fw:") {
        b.to_string()
    } else {
        format!("Fwd: {b}")
    }
}

/// Zitatblock: „Am {Datum} schrieb {Von}:“ + jede Zeile mit „> “.
pub fn zitat_block(datum: Option<i64>, von: &str, text: &str) -> String {
    let wann = datum
        .and_then(|ts| Local.timestamp_opt(ts, 0).single())
        .map(|zeit| zeit.format("%d.%m.%Y um %H:%M").to_string())
        .unwrap_or_else(|| "unbekanntem Datum".to_string());
    let zitat: String = text.lines().map(|zeile| format!("> {zeile}\n")).collect();
    format!("\n\nAm {wann} schrieb {von}:\n{zitat}")
}

/// Kopfblock für weitergeleitete Nachrichten (statt „> “-Zitat).
pub fn weiterleit_block(
    von: &str,
    datum: Option<i64>,
    betreff: &str,
    an: &str,
    text: &str,
) -> String {
    let wann = datum
        .and_then(|ts| Local.timestamp_opt(ts, 0).single())
        .map(|zeit| zeit.format("%d.%m.%Y um %H:%M").to_string())
        .unwrap_or_else(|| "unbekannt".to_string());
    format!(
        "\n\n---------- Weitergeleitete Nachricht ----------\n\
         Von: {von}\nDatum: {wann}\nBetreff: {betreff}\nAn: {an}\n\n{text}"
    )
}

/// Zieht die „echten“ Anhänge einer Roh-Mail heraus (für Weiterleiten).
/// Eingebettete `cid:`-Bilder bleiben außen vor.
pub fn anhaenge_extrahieren(roh: &[u8]) -> Vec<Anhang> {
    use mail_parser::{MessageParser, MimeHeaders};
    let Some(nachricht) = MessageParser::default().parse(roh) else {
        return Vec::new();
    };
    nachricht
        .attachments()
        .filter(|teil| {
            let als_anhang = teil
                .content_disposition()
                .is_some_and(|d| d.ctype().eq_ignore_ascii_case("attachment"));
            als_anhang || teil.content_id().is_none()
        })
        .map(|teil| {
            let mime = teil
                .content_type()
                .map(|typ| match typ.subtype() {
                    Some(unter) => format!("{}/{}", typ.ctype(), unter),
                    None => typ.ctype().to_string(),
                })
                .unwrap_or_else(|| "application/octet-stream".to_string());
            Anhang {
                dateiname: teil.attachment_name().unwrap_or("anhang.bin").to_string(),
                mime,
                daten: teil.contents().to_vec(),
            }
        })
        .collect()
}

/// References-Kette fortschreiben: alte Kette + Message-ID des Originals.
pub fn neue_references(alte: Option<&str>, message_id: Option<&str>) -> Option<String> {
    match (alte.map(str::trim).filter(|a| !a.is_empty()), message_id) {
        (Some(alte), Some(id)) => Some(format!("{alte} {id}")),
        (None, Some(id)) => Some(id.to_string()),
        (Some(alte), None) => Some(alte.to_string()),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beispiel() -> NeueNachricht {
        NeueNachricht {
            von_name: "Philipp".into(),
            von_adresse: "philipp@example.org".into(),
            an: vec!["anna@example.org".into()],
            cc: vec![],
            betreff: "Testbetreff".into(),
            text: "Hallo Anna!".into(),
            html: None,
            anhaenge: vec![],
            antwort: None,
        }
    }

    #[test]
    fn einfache_nachricht_enthaelt_kopf_und_text() {
        let (_, roh) = baue_nachricht(&beispiel()).unwrap();
        let roh = String::from_utf8_lossy(&roh);
        assert!(roh.contains("From: Philipp <philipp@example.org>"));
        assert!(roh.contains("To: anna@example.org"));
        assert!(roh.contains("Message-ID: <"));
        assert!(roh.contains("@example.org>"));
        assert!(roh.contains("Subject: Testbetreff"));
        assert!(roh.contains("Hallo Anna!"));
        // Ohne Programm-Kennung lehnen manche Server die Mail als Spam ab.
        assert!(roh.contains("User-Agent: Nanomail/"));
    }

    #[test]
    fn jede_nachricht_bekommt_eine_eigene_message_id() {
        let (_, erste) = baue_nachricht(&beispiel()).unwrap();
        let (_, zweite) = baue_nachricht(&beispiel()).unwrap();
        let erste_id = String::from_utf8_lossy(&erste)
            .lines()
            .find(|zeile| zeile.starts_with("Message-ID:"))
            .unwrap()
            .to_string();
        let zweite_id = String::from_utf8_lossy(&zweite)
            .lines()
            .find(|zeile| zeile.starts_with("Message-ID:"))
            .unwrap()
            .to_string();
        assert_ne!(erste_id, zweite_id);
    }

    #[test]
    fn antwort_setzt_threading_header() {
        let mut eingabe = beispiel();
        eingabe.antwort = Some(AntwortKontext {
            message_id: Some("<original@example.org>".into()),
            references: Some("<wurzel@example.org>".into()),
        });
        let (_, roh) = baue_nachricht(&eingabe).unwrap();
        let roh = String::from_utf8_lossy(&roh);
        assert!(roh.contains("In-Reply-To: <original@example.org>"));
        assert!(roh.contains("References: <wurzel@example.org> <original@example.org>"));
    }

    #[test]
    fn anhang_wird_mit_dateiname_eingebettet() {
        let mut eingabe = beispiel();
        eingabe.anhaenge.push(Anhang {
            dateiname: "notiz.txt".into(),
            mime: "text/plain".into(),
            daten: b"Inhalt".to_vec(),
        });
        let (_, roh) = baue_nachricht(&eingabe).unwrap();
        let roh = String::from_utf8_lossy(&roh);
        assert!(roh.contains("multipart/mixed"));
        assert!(roh.contains("attachment; filename=\"notiz.txt\""));
    }

    #[test]
    fn html_fassung_erzeugt_alternative_teile() {
        let mut eingabe = beispiel();
        eingabe.html = Some("<p>Hallo <b>Anna</b>!</p>".into());
        let (_, roh) = baue_nachricht(&eingabe).unwrap();
        let roh = String::from_utf8_lossy(&roh);
        assert!(roh.contains("multipart/alternative"));
        assert!(roh.contains("text/plain"));
        assert!(roh.contains("text/html"));
        assert!(roh.contains("Hallo <b>Anna</b>!"));
    }

    #[test]
    fn html_und_anhang_verschachteln_alternative_in_mixed() {
        let mut eingabe = beispiel();
        eingabe.html = Some("<p>Hallo</p>".into());
        eingabe.anhaenge.push(Anhang {
            dateiname: "notiz.txt".into(),
            mime: "text/plain".into(),
            daten: b"Inhalt".to_vec(),
        });
        let (_, roh) = baue_nachricht(&eingabe).unwrap();
        let roh = String::from_utf8_lossy(&roh);
        assert!(roh.contains("multipart/mixed"));
        assert!(roh.contains("multipart/alternative"));
        assert!(roh.contains("attachment; filename=\"notiz.txt\""));
    }

    #[test]
    fn anzeigename_mit_sonderzeichen_blockiert_versand_nicht() {
        let mut eingabe = beispiel();
        eingabe.von_name = "Bremer, Philipp (privat)".into();
        let (_, roh) = baue_nachricht(&eingabe).unwrap();
        let roh = String::from_utf8_lossy(&roh);
        // lettre kodiert Namen mit Sonderzeichen als RFC-2047-Encoded-Word
        // (Base64 von „Bremer, Philipp (privat)“) — gültig und lesbar.
        assert!(roh.contains("From: =?utf-8?b?QnJlbWVyLCBQaGlsaXBwIChwcml2YXQp?= <"));
    }

    #[test]
    fn kalender_einladung_enthaelt_itip_request() {
        let eingabe = KalenderEinladung {
            von_name: "Philipp".into(),
            von_adresse: "philipp@example.org".into(),
            an: vec!["anna@example.org".into()],
            betreff: "Einladung: Planung".into(),
            text: "Kalendereinladung".into(),
            ics: "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:PUBLISH\r\nBEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Planung\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n".into(),
        };
        let (_, roh) = baue_kalender_einladung(&eingabe).unwrap();
        let roh = String::from_utf8_lossy(&roh);
        assert!(roh.contains("From: Philipp <philipp@example.org>"));
        assert!(roh.contains("To: anna@example.org"));
        assert!(roh.contains("Message-ID: <"));
        assert!(roh.contains("@example.org>"));
        assert!(roh.contains("text/calendar"));
        assert!(roh.contains("method=REQUEST"));
        assert!(roh.contains("METHOD:REQUEST"));
        assert!(!roh.contains("METHOD:PUBLISH"));
        assert!(roh.contains("User-Agent: Nanomail/"));
    }

    #[test]
    fn entwurf_ohne_empfaenger_laesst_sich_bauen() {
        let mut eingabe = beispiel();
        eingabe.an = vec![];
        let (_, roh) = baue_nachricht(&eingabe).unwrap();
        let roh = String::from_utf8_lossy(&roh);
        assert!(roh.contains("Subject: Testbetreff"));
        assert!(roh.contains("Hallo Anna!"));
        // Kein To-Header in den Rohbytes — der Hilfs-Umschlag bleibt außen vor.
        assert!(!roh.contains("\r\nTo:"));
    }

    #[test]
    fn ungueltige_adresse_gibt_verstaendlichen_fehler() {
        let mut eingabe = beispiel();
        eingabe.an = vec!["keine-adresse".into()];
        let fehler = baue_nachricht(&eingabe).unwrap_err();
        assert!(format!("{fehler:#}").contains("keine-adresse"));
    }

    #[test]
    fn re_wird_nicht_verdoppelt() {
        assert_eq!(antwort_betreff("Hallo"), "Re: Hallo");
        assert_eq!(antwort_betreff("Re: Hallo"), "Re: Hallo");
        assert_eq!(antwort_betreff("RE: Hallo"), "RE: Hallo");
        assert_eq!(antwort_betreff("AW: Hallo"), "AW: Hallo");
    }

    #[test]
    fn fwd_wird_nicht_verdoppelt() {
        assert_eq!(weiterleit_betreff("Hallo"), "Fwd: Hallo");
        assert_eq!(weiterleit_betreff("Fwd: Hallo"), "Fwd: Hallo");
        assert_eq!(weiterleit_betreff("WG: Hallo"), "WG: Hallo");
    }

    #[test]
    fn zitat_setzt_von_und_groesser_zeichen() {
        let zitat = zitat_block(Some(1_783_065_600), "Anna", "Zeile 1\nZeile 2");
        assert!(zitat.contains("schrieb Anna:"));
        assert!(zitat.contains("> Zeile 1\n> Zeile 2\n"));
        // Datum wird formatiert (Zeitzone des Rechners, daher nur Datumsteil prüfen)
        assert!(zitat.contains(".2026 um "));
    }

    #[test]
    fn weiterleit_block_enthaelt_kopfdaten() {
        let block = weiterleit_block(
            "Anna <anna@example.org>",
            None,
            "Original",
            "philipp@example.org",
            "Inhalt",
        );
        assert!(block.contains("Weitergeleitete Nachricht"));
        assert!(block.contains("Von: Anna <anna@example.org>"));
        assert!(block.contains("Betreff: Original"));
        assert!(block.contains("\n\nInhalt"));
    }

    #[test]
    fn anhaenge_extrahieren_ueberspringt_inline_bilder() {
        let roh = "From: a@example.org\r\n\
             Subject: Mix\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=\"G\"\r\n\r\n\
             --G\r\n\
             Content-Type: text/plain\r\n\r\n\
             Text.\r\n\
             --G\r\n\
             Content-Type: image/png\r\n\
             Content-ID: <inline1>\r\n\
             Content-Transfer-Encoding: base64\r\n\r\n\
             aWNo\r\n\
             --G\r\n\
             Content-Type: application/pdf\r\n\
             Content-Disposition: attachment; filename=\"doku.pdf\"\r\n\
             Content-Transfer-Encoding: base64\r\n\r\n\
             JVBERg==\r\n\
             --G--\r\n"
            .as_bytes();
        let anhaenge = anhaenge_extrahieren(roh);
        assert_eq!(anhaenge.len(), 1);
        assert_eq!(anhaenge[0].dateiname, "doku.pdf");
        assert_eq!(anhaenge[0].mime, "application/pdf");
        assert_eq!(anhaenge[0].daten, b"%PDF");
    }

    #[test]
    fn references_kette_waechst_korrekt() {
        assert_eq!(
            neue_references(Some("<a> <b>"), Some("<c>")),
            Some("<a> <b> <c>".to_string())
        );
        assert_eq!(neue_references(None, Some("<c>")), Some("<c>".to_string()));
        assert_eq!(
            neue_references(Some(""), Some("<c>")),
            Some("<c>".to_string())
        );
        assert_eq!(neue_references(None, None), None);
    }
}

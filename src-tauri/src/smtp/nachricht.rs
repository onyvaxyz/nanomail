//! Nachrichtenbau — reine, unit-getestete Funktionen (kein Netzwerk).
//!
//! Baut aus den Eingaben des Frontends eine versandfertige Mail
//! (`lettre::Message`) samt Rohbytes für die „Gesendet“-Ablage und
//! erzeugt die Vorlagen für Antworten/Weiterleiten.

use anyhow::{Context, Result};
use chrono::{Local, TimeZone};
use lettre::message::header::ContentType;
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
    pub anhaenge: Vec<Anhang>,
    pub antwort: Option<AntwortKontext>,
}

/// Baut die versandfertige Nachricht. Liefert zusätzlich die Rohbytes
/// für das IMAP-APPEND in den „Gesendet“-Ordner.
pub fn baue_nachricht(eingabe: &NeueNachricht) -> Result<(Message, Vec<u8>)> {
    let von: Mailbox = if eingabe.von_name.trim().is_empty() {
        eingabe
            .von_adresse
            .parse()
            .context("Absenderadresse ungültig")?
    } else {
        format!("{} <{}>", eingabe.von_name.trim(), eingabe.von_adresse)
            .parse()
            .context("Absenderadresse ungültig")?
    };

    let mut builder = Message::builder().from(von);
    for adresse in &eingabe.an {
        builder = builder.to(parse_adresse(adresse)?);
    }
    for adresse in &eingabe.cc {
        builder = builder.cc(parse_adresse(adresse)?);
    }
    builder = builder.subject(&eingabe.betreff);

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

    let nachricht = if eingabe.anhaenge.is_empty() {
        builder
            .singlepart(text_teil)
            .context("Nachricht zusammenbauen")?
    } else {
        let mut mehrteilig = MultiPart::mixed().singlepart(text_teil);
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

fn parse_adresse(eingabe: &str) -> Result<Mailbox> {
    eingabe
        .trim()
        .parse()
        .with_context(|| format!("Empfängeradresse „{}“ ist ungültig", eingabe.trim()))
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
        assert!(roh.contains("Subject: Testbetreff"));
        assert!(roh.contains("Hallo Anna!"));
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

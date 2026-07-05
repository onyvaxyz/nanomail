//! Kopfzeilen-Parsing (reine Funktionen, unit-getestet, kein Netzwerk).
//!
//! Bekommt die rohen Header-Bytes aus dem IMAP-Fetch und liefert
//! anzeigefertige Felder (RFC-2047-dekodiert, z. B. `=?utf-8?...?=`).

use mail_parser::MessageParser;

#[derive(Debug, PartialEq, Eq)]
pub struct GeparsterKopf {
    pub betreff: String,
    pub von: String,
    /// Unix-Sekunden aus dem Date-Header, falls vorhanden und lesbar.
    pub datum: Option<i64>,
}

pub fn parse_kopf(header: &[u8]) -> GeparsterKopf {
    let Some(nachricht) = MessageParser::default().parse(header) else {
        return GeparsterKopf {
            betreff: String::new(),
            von: String::new(),
            datum: None,
        };
    };

    let betreff = nachricht.subject().unwrap_or_default().trim().to_string();

    let von = nachricht
        .from()
        .and_then(|adressen| adressen.first())
        .map(|adresse| {
            let name = adresse.name().map(str::trim).filter(|n| !n.is_empty());
            let mail = adresse.address().unwrap_or_default();
            match name {
                Some(name) => name.to_string(),
                None => mail.to_string(),
            }
        })
        .unwrap_or_default();

    let datum = nachricht.date().map(mail_parser::DateTime::to_timestamp);

    GeparsterKopf {
        betreff,
        von,
        datum,
    }
}

/// Alles, was für Antworten/Weiterleiten aus dem Original gebraucht wird.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AntwortDaten {
    /// Adresse, an die geantwortet wird (Reply-To vor From).
    pub antwort_an: String,
    pub betreff: String,
    pub datum: Option<i64>,
    /// Anzeigetext des Absenders („Name <adresse>“ oder nur Adresse).
    pub von_anzeige: String,
    /// An-Zeile des Originals (für den Weiterleitungs-Kopf).
    pub an_anzeige: String,
    pub text: String,
    /// Message-ID mit spitzen Klammern, z. B. `<abc@example.org>`.
    pub message_id: Option<String>,
    /// Rohe References-Kette des Originals.
    pub references: Option<String>,
}

pub fn parse_fuer_antwort(roh: &[u8]) -> AntwortDaten {
    let Some(nachricht) = MessageParser::default().parse(roh) else {
        return AntwortDaten::default();
    };

    let adress_anzeige = |adresse: Option<&mail_parser::Address>| -> (String, String) {
        // (nur Adresse, Anzeigetext) der ersten Adresse
        let Some(erste) = adresse.and_then(|a| a.first()) else {
            return (String::new(), String::new());
        };
        let mail = erste.address().unwrap_or_default().to_string();
        let anzeige = match erste.name().map(str::trim).filter(|n| !n.is_empty()) {
            Some(name) => format!("{name} <{mail}>"),
            None => mail.clone(),
        };
        (mail, anzeige)
    };

    let (von_adresse, von_anzeige) = adress_anzeige(nachricht.from());
    let (antwort_adresse, _) = adress_anzeige(nachricht.reply_to());
    let (_, an_anzeige) = adress_anzeige(nachricht.to());

    AntwortDaten {
        antwort_an: if antwort_adresse.is_empty() {
            von_adresse
        } else {
            antwort_adresse
        },
        betreff: nachricht.subject().unwrap_or_default().trim().to_string(),
        datum: nachricht.date().map(mail_parser::DateTime::to_timestamp),
        von_anzeige,
        an_anzeige,
        text: nachricht
            .body_text(0)
            .unwrap_or_default()
            .trim()
            .to_string(),
        message_id: nachricht.message_id().map(|id| format!("<{id}>")),
        references: nachricht
            .header_raw("References")
            .map(|wert| wert.split_whitespace().collect::<Vec<_>>().join(" ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn einfacher_kopf() {
        let kopf = parse_kopf(
            b"From: Anna Beispiel <anna@example.org>\r\n\
              Subject: Hallo Welt\r\n\
              Date: Fri, 03 Jul 2026 10:00:00 +0200\r\n\r\n",
        );
        assert_eq!(kopf.betreff, "Hallo Welt");
        assert_eq!(kopf.von, "Anna Beispiel");
        // 2026-07-03 10:00 +0200 = 08:00 UTC
        assert_eq!(kopf.datum, Some(1_783_065_600));
    }

    #[test]
    fn rfc2047_betreff_wird_dekodiert() {
        let kopf = parse_kopf(
            b"From: x@example.org\r\n\
              Subject: =?utf-8?Q?Gr=C3=BC=C3=9Fe_aus_Z=C3=BCrich?=\r\n\r\n",
        );
        assert_eq!(kopf.betreff, "Grüße aus Zürich");
        // Ohne Anzeigename wird die Adresse gezeigt.
        assert_eq!(kopf.von, "x@example.org");
    }

    #[test]
    fn kaputte_eingabe_liefert_leere_felder() {
        let kopf = parse_kopf(b"");
        assert_eq!(kopf.betreff, "");
        assert_eq!(kopf.von, "");
        assert_eq!(kopf.datum, None);
    }

    #[test]
    fn antwortdaten_bevorzugen_reply_to_und_liefern_threading() {
        let daten = parse_fuer_antwort(
            b"From: Anna Beispiel <anna@example.org>\r\n\
              Reply-To: <antworten@example.org>\r\n\
              To: Philipp <philipp@example.org>\r\n\
              Subject: Frage\r\n\
              Message-ID: <m123@example.org>\r\n\
              References: <wurzel@example.org>\r\n\
              Date: Fri, 03 Jul 2026 10:00:00 +0200\r\n\r\n\
              Wie sieht es aus?",
        );
        assert_eq!(daten.antwort_an, "antworten@example.org");
        assert_eq!(daten.von_anzeige, "Anna Beispiel <anna@example.org>");
        assert_eq!(daten.an_anzeige, "Philipp <philipp@example.org>");
        assert_eq!(daten.betreff, "Frage");
        assert_eq!(daten.message_id, Some("<m123@example.org>".into()));
        assert_eq!(daten.references, Some("<wurzel@example.org>".into()));
        assert_eq!(daten.text, "Wie sieht es aus?");
    }

    #[test]
    fn antwortdaten_ohne_reply_to_nehmen_from() {
        let daten = parse_fuer_antwort(
            b"From: anna@example.org\r\n\
              Subject: Hallo\r\n\r\nText",
        );
        assert_eq!(daten.antwort_an, "anna@example.org");
        assert_eq!(daten.message_id, None);
        assert_eq!(daten.references, None);
    }
}

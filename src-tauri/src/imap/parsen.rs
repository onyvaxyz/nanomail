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
}

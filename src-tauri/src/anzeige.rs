//! Aufbereitung von Mail-Inhalten für die Anzeige — sicherheitskritisch.
//!
//! Regeln (siehe CLAUDE.md):
//! - HTML wird **immer** mit `ammonia` bereinigt, bevor es das Backend
//!   verlässt. Skripte, Event-Handler und Formulare überleben das nie.
//! - Externe Bilder werden standardmäßig entfernt und nur gezählt
//!   („Bilder laden“-Hinweis). Auf Wunsch lädt das Backend sie herunter
//!   und bettet sie als `data:`-URIs ein — die CSP der App bleibt dicht.
//! - Eingebettete Bilder (`cid:`) werden direkt als `data:`-URIs eingebettet.
//! - `style`-Attribute bleiben erlaubt, damit Newsletter lesbar sind.
//!   Absicherung: Anzeige nur im `<iframe sandbox>` (kein Skript) und
//!   App-CSP blockiert jede externe Nachladung — auch aus CSS-`url()`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use base64::Engine;
use mail_parser::{Message, MessageParser, MimeHeaders, PartType};

/// Ergebnis der Aufbereitung einer kompletten Nachricht.
#[derive(Debug)]
pub struct AufbereiteteNachricht {
    pub text: String,
    /// Bereinigtes HTML — `None`, wenn die Mail nur Text enthält.
    pub html_bereinigt: Option<String>,
    pub hatte_externe_bilder: bool,
    pub hat_anhang: bool,
    /// „Echte“ Anhänge in Mail-Reihenfolge (für die Anhang-Leiste).
    pub anhaenge: Vec<AnhangInfo>,
    /// Dekodierte text/calendar-Teile, getrennt vom geschützten Mail-HTML.
    pub kalender: Vec<String>,
}

/// Ein Anhang, wie ihn die Anhang-Leiste anzeigt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnhangInfo {
    pub dateiname: String,
    pub groesse: usize,
}

/// Standard-Aufbereitung: externe Bilder blockieren.
pub fn nachricht_aufbereiten(roh: &[u8]) -> AufbereiteteNachricht {
    let Some(nachricht) = MessageParser::default().parse(roh) else {
        return AufbereiteteNachricht {
            text: String::new(),
            html_bereinigt: None,
            hatte_externe_bilder: false,
            hat_anhang: false,
            anhaenge: Vec::new(),
            kalender: Vec::new(),
        };
    };

    let text = nachricht
        .body_text(0)
        .unwrap_or_default()
        .trim()
        .to_string();
    let anhaenge: Vec<AnhangInfo> = anhang_teile(&nachricht)
        .into_iter()
        .map(|teil| AnhangInfo {
            dateiname: teil
                .attachment_name()
                .unwrap_or("anhang.bin")
                .trim()
                .to_string(),
            groesse: teil.contents().len(),
        })
        .collect();

    let (html_bereinigt, externe) = if hat_html_teil(&nachricht) {
        let html_roh = nachricht.body_html(0).unwrap_or_default();
        let cid = cid_bilder(&nachricht);
        let (sauber, externe) = sanitisieren(&html_roh, &cid, None);
        (Some(sauber), externe)
    } else {
        (None, Vec::new())
    };

    AufbereiteteNachricht {
        text,
        html_bereinigt,
        hatte_externe_bilder: !externe.is_empty(),
        hat_anhang: !anhaenge.is_empty(),
        anhaenge,
        kalender: nachricht
            .parts
            .iter()
            .filter(|teil| {
                teil.content_type().is_some_and(|ct| {
                    ct.ctype().eq_ignore_ascii_case("text")
                        && ct
                            .subtype()
                            .is_some_and(|s| s.eq_ignore_ascii_case("calendar"))
                }) || teil
                    .attachment_name()
                    .is_some_and(|n| n.to_ascii_lowercase().ends_with(".ics"))
            })
            .filter_map(|teil| {
                std::str::from_utf8(teil.contents())
                    .ok()
                    .map(str::to_string)
            })
            .collect(),
    }
}

/// Liefert Dateiname und Inhalt des Anhangs mit der angegebenen Nummer
/// (dieselbe Reihenfolge wie in `AufbereiteteNachricht::anhaenge`).
pub fn anhang_daten(roh: &[u8], index: usize) -> Option<(String, Vec<u8>)> {
    let nachricht = MessageParser::default().parse(roh)?;
    let teil = anhang_teile(&nachricht).into_iter().nth(index)?;
    Some((
        teil.attachment_name()
            .unwrap_or("anhang.bin")
            .trim()
            .to_string(),
        teil.contents().to_vec(),
    ))
}

/// Liefert die externen Bild-Adressen einer Nachricht (für „Bilder laden“).
pub fn externe_bild_urls(roh: &[u8]) -> Vec<String> {
    let Some(nachricht) = MessageParser::default().parse(roh) else {
        return Vec::new();
    };
    if !hat_html_teil(&nachricht) {
        return Vec::new();
    }
    let html_roh = nachricht.body_html(0).unwrap_or_default();
    let cid = cid_bilder(&nachricht);
    let (_, externe) = sanitisieren(&html_roh, &cid, None);
    externe
}

/// Aufbereitung mit heruntergeladenen externen Bildern
/// (`geladene`: Original-URL → `data:`-URI). Nicht auflösbare Bilder
/// bleiben blockiert.
pub fn nachricht_mit_bildern(roh: &[u8], geladene: &HashMap<String, String>) -> Option<String> {
    let nachricht = MessageParser::default().parse(roh)?;
    if !hat_html_teil(&nachricht) {
        return None;
    }
    let html_roh = nachricht.body_html(0).unwrap_or_default();
    let cid = cid_bilder(&nachricht);
    let (sauber, _) = sanitisieren(&html_roh, &cid, Some(geladene));
    Some(sauber)
}

/// Zweite, strengere Bereinigungsstufe für die App-Ansicht (M3.5):
/// entfernt zusätzlich alle mitgebrachten Stile (`style`-Attribute,
/// Farb- und Layout-Attribute), sodass nur der Inhalt übrig bleibt und
/// das Frontend ihn im Design der App darstellen kann. Erwartet bereits
/// bereinigtes HTML (`data:`-Bilder bleiben erhalten).
pub fn stil_entfernen(html_bereinigt: &str) -> String {
    let mut builder = ammonia::Builder::default();
    let schemata: HashSet<&str> = ["http", "https", "mailto", "data"]
        .iter()
        .copied()
        .collect();
    builder.url_schemes(schemata);
    // Auch in der App-Ansicht öffnen Links im Standard-Browser (siehe
    // `sanitisieren`).
    builder.set_tag_attribute_value("a", "target", "_top");
    builder.clean(html_bereinigt).to_string()
}

/// Hat die Nachricht einen echten HTML-Teil? (`body_html` würde reine
/// Text-Mails sonst automatisch in HTML umwandeln — das wollen wir nicht,
/// Text-Mails werden als Text angezeigt.)
fn hat_html_teil(nachricht: &Message) -> bool {
    nachricht
        .html_body
        .first()
        .and_then(|id| nachricht.parts.get(*id as usize))
        .is_some_and(|teil| matches!(teil.body, PartType::Html(_)))
}

/// „Echte“ Anhänge = Teile mit Anhang-Disposition oder ohne Content-ID.
/// Eingebettete `cid:`-Bilder zählen nicht als Anhang. Eine Funktion für
/// Auflisten und Herausgreifen — so bleibt die Nummerierung konsistent.
fn anhang_teile<'a>(nachricht: &'a Message<'a>) -> Vec<&'a mail_parser::MessagePart<'a>> {
    nachricht
        .attachments()
        .filter(|teil| {
            let als_anhang_markiert = teil
                .content_disposition()
                .is_some_and(|d| d.ctype().eq_ignore_ascii_case("attachment"));
            als_anhang_markiert || teil.content_id().is_none()
        })
        .collect()
}

/// Sammelt eingebettete Bilder: Content-ID → `data:`-URI.
fn cid_bilder(nachricht: &Message) -> HashMap<String, String> {
    let mut bilder = HashMap::new();
    for teil in &nachricht.parts {
        let Some(content_id) = teil.content_id() else {
            continue;
        };
        let Some(typ) = teil.content_type() else {
            continue;
        };
        if !typ.ctype().eq_ignore_ascii_case("image") {
            continue;
        }
        let mime = format!("image/{}", typ.subtype().unwrap_or("png").to_lowercase());
        let daten = base64::engine::general_purpose::STANDARD.encode(teil.contents());
        let schluessel = content_id.trim_matches(['<', '>']).to_string();
        bilder.insert(schluessel, format!("data:{mime};base64,{daten}"));
    }
    bilder
}

/// Kern-Sanitizing mit `ammonia`. Liefert das bereinigte HTML und die
/// Liste der angetroffenen externen Bild-URLs.
fn sanitisieren(
    html: &str,
    cid: &HashMap<String, String>,
    geladene: Option<&HashMap<String, String>>,
) -> (String, Vec<String>) {
    // ammonia verlangt einen thread-sicheren `'static`-Callback — deshalb
    // Arc/Mutex und geklonte Nachschlagetabellen.
    let externe = Arc::new(Mutex::new(Vec::<String>::new()));
    let externe_im_filter = Arc::clone(&externe);
    let cid = cid.clone();
    let geladene = geladene.cloned();

    let mut builder = ammonia::Builder::default();
    let schemata: HashSet<&str> = ["http", "https", "mailto", "data", "cid"]
        .iter()
        .copied()
        .collect();
    builder
        .url_schemes(schemata)
        // Links im _top-Ziel öffnen: Der Klick navigiert das Fenster, das
        // Backend fängt das ab und öffnet den Link im Standard-Browser.
        .set_tag_attribute_value("a", "target", "_top")
        // `style` für lesbare Newsletter — abgesichert durch iframe-Sandbox
        // + App-CSP (blockiert externe Nachladungen auch aus CSS).
        .add_generic_attributes(["style"])
        .add_tag_attributes(
            "table",
            [
                "border",
                "cellpadding",
                "cellspacing",
                "bgcolor",
                "align",
                "width",
            ],
        )
        .add_tag_attributes(
            "td",
            [
                "bgcolor", "align", "valign", "width", "height", "colspan", "rowspan",
            ],
        )
        .add_tag_attributes(
            "th",
            [
                "bgcolor", "align", "valign", "width", "height", "colspan", "rowspan",
            ],
        )
        .add_tag_attributes("tr", ["bgcolor", "align", "valign"])
        .add_tag_attributes("body", ["bgcolor"])
        .attribute_filter(move |element, attribut, wert| {
            match (element, attribut) {
                ("img", "src") => {
                    if let Some(id) = wert.strip_prefix("cid:") {
                        // Eingebettetes Bild → data:-URI aus der Nachricht selbst
                        return cid.get(id).cloned().map(Into::into);
                    }
                    if wert.starts_with("data:image/") {
                        return Some(wert.into());
                    }
                    if wert.starts_with("https://") || wert.starts_with("http://") {
                        if let Some(karte) = &geladene {
                            if let Some(daten_uri) = karte.get(wert) {
                                return Some(daten_uri.clone().into());
                            }
                        }
                        if let Ok(mut liste) = externe_im_filter.lock() {
                            liste.push(wert.to_string());
                        }
                        return None; // blockiert
                    }
                    None
                }
                ("a", "href") => {
                    // Nur klassische Link-Ziele; alles andere (data:, cid:, …) fliegt.
                    if wert.starts_with("https://")
                        || wert.starts_with("http://")
                        || wert.starts_with("mailto:")
                    {
                        Some(wert.into())
                    } else {
                        None
                    }
                }
                _ => Some(wert.into()),
            }
        });

    let sauber = builder.clean(html).to_string();
    let urls = externe
        .lock()
        .map(|liste| liste.clone())
        .unwrap_or_default();
    (sauber, urls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kalenderteile_werden_dekodiert_ohne_mail_html_zu_aktivieren() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:test\r\nDTSTART:20260911T120000Z\r\nSUMMARY:Termin\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        for typ in [
            "text/calendar; method=REQUEST",
            "application/octet-stream; name=einladung.ics",
        ] {
            let roh = format!("MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=test\r\n\r\n--test\r\nContent-Type: text/plain\r\n\r\nErster Absatz\r\n\r\nZweiter Absatz\r\nNeue Zeile\r\n--test\r\nContent-Type: {typ}\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n--test--\r\n", base64::engine::general_purpose::STANDARD.encode(ics));
            let mail = nachricht_aufbereiten(roh.as_bytes());
            assert_eq!(mail.kalender, vec![ics]);
            assert!(mail.html_bereinigt.is_none());
            assert_eq!(
                mail.text.replace("\r\n", "\n"),
                "Erster Absatz\n\nZweiter Absatz\nNeue Zeile"
            );
        }
        let normal = nachricht_aufbereiten(b"Content-Type: text/plain\r\n\r\nBEGIN:VCALENDAR");
        assert!(normal.kalender.is_empty());
    }

    // 1×1 transparentes PNG
    const PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    fn html_mail(body_html: &str) -> Vec<u8> {
        format!(
            "From: absender@example.org\r\n\
             Subject: Test\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: text/html; charset=utf-8\r\n\r\n\
             {body_html}"
        )
        .into_bytes()
    }

    #[test]
    fn skripte_und_event_handler_werden_entfernt() {
        let roh = html_mail(
            r#"<p>Hallo</p><script>alert(1)</script><img src="x" onerror="boese()"><form action="https://x"><input></form>"#,
        );
        let ergebnis = nachricht_aufbereiten(&roh);
        let html = ergebnis.html_bereinigt.unwrap();
        assert!(html.contains("<p>Hallo</p>"));
        assert!(!html.contains("script"));
        assert!(!html.contains("onerror"));
        assert!(!html.contains("alert"));
        assert!(!html.contains("<form"));
        assert!(!html.contains("<input"));
    }

    #[test]
    fn externe_bilder_werden_blockiert_und_gezaehlt() {
        let roh = html_mail(
            r#"<p>Angebot!</p><img src="https://tracker.example/pixel.png"><img src="http://cdn.example/foto.jpg">"#,
        );
        let ergebnis = nachricht_aufbereiten(&roh);
        let html = ergebnis.html_bereinigt.unwrap();
        assert!(ergebnis.hatte_externe_bilder);
        assert!(!html.contains("tracker.example"));
        assert!(!html.contains("cdn.example"));
        assert_eq!(
            externe_bild_urls(&roh),
            vec![
                "https://tracker.example/pixel.png".to_string(),
                "http://cdn.example/foto.jpg".to_string()
            ]
        );
    }

    #[test]
    fn geladene_bilder_werden_als_data_uri_eingebettet() {
        let roh = html_mail(r#"<img src="https://cdn.example/foto.jpg">"#);
        let mut geladene = HashMap::new();
        geladene.insert(
            "https://cdn.example/foto.jpg".to_string(),
            format!("data:image/png;base64,{PNG_BASE64}"),
        );
        let html = nachricht_mit_bildern(&roh, &geladene).unwrap();
        assert!(html.contains("data:image/png;base64,"));
        assert!(!html.contains("cdn.example"));
    }

    #[test]
    fn cid_bilder_werden_direkt_eingebettet() {
        let roh = format!(
            "From: a@example.org\r\n\
             Subject: Inline\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: multipart/related; boundary=\"GRENZE\"\r\n\r\n\
             --GRENZE\r\n\
             Content-Type: text/html; charset=utf-8\r\n\r\n\
             <p>Bild:</p><img src=\"cid:bild1@example.org\">\r\n\
             --GRENZE\r\n\
             Content-Type: image/png\r\n\
             Content-ID: <bild1@example.org>\r\n\
             Content-Transfer-Encoding: base64\r\n\r\n\
             {PNG_BASE64}\r\n\
             --GRENZE--\r\n"
        );
        let ergebnis = nachricht_aufbereiten(roh.as_bytes());
        let html = ergebnis.html_bereinigt.unwrap();
        assert!(html.contains("data:image/png;base64,"));
        assert!(!ergebnis.hatte_externe_bilder);
        // Das eingebettete Bild ist kein „echter“ Anhang.
        assert!(!ergebnis.hat_anhang);
    }

    #[test]
    fn stil_entfernen_behaelt_inhalt_und_wirft_design_weg() {
        let roh = html_mail(
            r##"<table bgcolor="#ff0000" width="600"><tr><td style="color:red;font-family:Comic Sans">Wichtig</td></tr></table><p style="background:black">Hallo</p><img src="data:image/png;base64,AAAA" alt="Logo"><a href="https://example.org">Link</a>"##,
        );
        let bereinigt = nachricht_aufbereiten(&roh).html_bereinigt.unwrap();
        // Erste Stufe behält Stile (Originalansicht) …
        assert!(bereinigt.contains("style="));
        assert!(bereinigt.contains("bgcolor"));

        // … die zweite Stufe entfernt sie, der Inhalt bleibt.
        let schlicht = stil_entfernen(&bereinigt);
        assert!(!schlicht.contains("style="));
        assert!(!schlicht.contains("bgcolor"));
        assert!(schlicht.contains("Wichtig"));
        assert!(schlicht.contains("Hallo"));
        assert!(schlicht.contains("data:image/png;base64,AAAA"));
        assert!(schlicht.contains("https://example.org"));
    }

    #[test]
    fn gefaehrliche_links_werden_entschaerft() {
        let roh = html_mail(
            r#"<a href="javascript:alert(1)">klick</a><a href="https://example.org">ok</a>"#,
        );
        let html = nachricht_aufbereiten(&roh).html_bereinigt.unwrap();
        assert!(!html.contains("javascript:"));
        assert!(html.contains("https://example.org"));
    }

    #[test]
    fn reine_textmail_hat_kein_html() {
        let roh = b"From: a@example.org\r\n\
                    Subject: Nur Text\r\n\r\n\
                    Hallo, nur Text hier."
            .to_vec();
        let ergebnis = nachricht_aufbereiten(&roh);
        assert!(ergebnis.html_bereinigt.is_none());
        assert_eq!(ergebnis.text, "Hallo, nur Text hier.");
        assert!(!ergebnis.hat_anhang);
    }

    #[test]
    fn anhang_wird_erkannt() {
        let roh = "From: a@example.org\r\n\
             Subject: Mit Anhang\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=\"GRENZE\"\r\n\r\n\
             --GRENZE\r\n\
             Content-Type: text/plain\r\n\r\n\
             Siehe Anhang.\r\n\
             --GRENZE\r\n\
             Content-Type: application/pdf\r\n\
             Content-Disposition: attachment; filename=\"doku.pdf\"\r\n\
             Content-Transfer-Encoding: base64\r\n\r\n\
             JVBERi0xLjQK\r\n\
             --GRENZE--\r\n"
            .as_bytes();
        let ergebnis = nachricht_aufbereiten(roh);
        assert!(ergebnis.hat_anhang);
        // Anhang-Leiste: Name und Größe des dekodierten Inhalts.
        assert_eq!(ergebnis.anhaenge.len(), 1);
        assert_eq!(ergebnis.anhaenge[0].dateiname, "doku.pdf");
        assert_eq!(ergebnis.anhaenge[0].groesse, "%PDF-1.4\n".len());

        // Herausgreifen liefert denselben Anhang mit Inhalt.
        let (name, daten) = anhang_daten(roh, 0).unwrap();
        assert_eq!(name, "doku.pdf");
        assert_eq!(daten, b"%PDF-1.4\n");
        // Außerhalb des Bereichs: nichts.
        assert!(anhang_daten(roh, 1).is_none());
    }

    #[test]
    fn kaputte_nachricht_liefert_leeres_ergebnis() {
        let ergebnis = nachricht_aufbereiten(&[]);
        assert!(ergebnis.html_bereinigt.is_none());
        assert_eq!(ergebnis.text, "");
    }
}

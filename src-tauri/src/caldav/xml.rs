//! XML-Erzeugung und -Auswertung für CalDAV (RFC 4791/6578).
//!
//! Schmale, eigene Schicht: nur die drei Requests, die Nanomail braucht
//! (Discovery-PROPFIND, `sync-collection`-REPORT, `calendar-multiget`).
//! Alles reine Funktionen ohne Netzwerk — vollständig testbar.
//! Das Parsing ist namespace-tolerant: Es zählt nur der lokale Name
//! (`d:href`, `D:href` und `href` sind gleichwertig).

use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::Reader;

// ------------------------------------------------------------ Anfragen --

/// PROPFIND-Body der Kalender-Discovery: Name, Typ, Farbe je Collection.
pub fn discovery_anfrage() -> String {
    r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/">
  <d:prop>
    <d:displayname/>
    <d:resourcetype/>
    <a:calendar-color/>
    <c:supported-calendar-component-set/>
  </d:prop>
</d:propfind>"#
        .to_string()
}

/// `sync-collection`-REPORT: liefert Änderungen seit dem Token
/// (leeres Token = kompletter Erstabgleich).
pub fn sync_anfrage(token: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:sync-collection xmlns:d="DAV:">
  <d:sync-token>{}</d:sync-token>
  <d:sync-level>1</d:sync-level>
  <d:prop>
    <d:getetag/>
  </d:prop>
</d:sync-collection>"#,
        quick_xml::escape::escape(token)
    )
}

/// `calendar-multiget`-REPORT: holt ETag + ICS-Daten der angefragten Objekte.
pub fn multiget_anfrage(hrefs: &[String]) -> String {
    let href_zeilen: String = hrefs
        .iter()
        .map(|href| format!("  <d:href>{}</d:href>\n", quick_xml::escape::escape(href)))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-multiget xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:getetag/>
    <c:calendar-data/>
  </d:prop>
{href_zeilen}</c:calendar-multiget>"#
    )
}

// ----------------------------------------------------------- Antworten --

/// Ein bei der Discovery gefundener Kalender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KalenderFund {
    pub href: String,
    pub anzeige_name: String,
    /// `#rrggbb` (Nextcloud liefert teils `#rrggbbaa` — wird gekürzt), sonst leer.
    pub farbe: String,
}

/// Ergebnis eines `sync-collection`-REPORTs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncErgebnis {
    /// Neues Token für den nächsten Abgleich.
    pub token: String,
    /// Geänderte/neue Objekte: (href, etag).
    pub geaendert: Vec<(String, String)>,
    /// Auf dem Server gelöschte Objekte.
    pub geloescht: Vec<String>,
}

/// Ein per `calendar-multiget` geladenes Termin-Objekt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjektDaten {
    pub href: String,
    pub etag: String,
    pub ics: String,
}

/// Wertet die Discovery-Antwort aus: nur echte Kalender-Collections
/// (resourcetype enthält `calendar`), die Termine (VEVENT) tragen können.
pub fn parse_discovery(xml: &str) -> Result<Vec<KalenderFund>> {
    let (antworten, _) = parse_multistatus(xml)?;
    let mut funde = Vec::new();
    for antwort in antworten {
        let ist_kalender = antwort
            .prop(&["resourcetype"])
            .is_some_and(|p| p.hat_kind("calendar"));
        if !ist_kalender {
            continue;
        }
        // Ohne Angabe gilt die Collection als Termin-Kalender; nennt der
        // Server Komponenten, muss VEVENT dabei sein (filtert reine
        // Aufgaben-/Journal-Listen aus).
        if let Some(komponenten) = antwort.prop(&["supported-calendar-component-set"]) {
            if !komponenten.kinder.is_empty() && !komponenten.hat_kind_mit_name("VEVENT") {
                continue;
            }
        }
        let name = antwort
            .prop(&["displayname"])
            .map(|p| p.text.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| name_aus_href(&antwort.href));
        funde.push(KalenderFund {
            href: antwort.href.clone(),
            anzeige_name: name,
            farbe: farbe_normalisieren(
                antwort
                    .prop(&["calendar-color"])
                    .map(|p| p.text.trim())
                    .unwrap_or(""),
            ),
        });
    }
    Ok(funde)
}

/// Wertet die `sync-collection`-Antwort aus.
pub fn parse_sync(xml: &str) -> Result<SyncErgebnis> {
    let (antworten, token) = parse_multistatus(xml)?;
    let token = token.context("Antwort ohne sync-token")?;
    let mut geaendert = Vec::new();
    let mut geloescht = Vec::new();
    for antwort in antworten {
        // Die Collection selbst (endet auf `/`) ist kein Termin-Objekt.
        if antwort.href.ends_with('/') {
            continue;
        }
        if antwort.status.contains("404") {
            geloescht.push(antwort.href);
        } else if let Some(etag) = antwort.prop(&["getetag"]) {
            geaendert.push((antwort.href.clone(), etag.text.trim().to_string()));
        }
    }
    Ok(SyncErgebnis {
        token,
        geaendert,
        geloescht,
    })
}

/// Wertet die `calendar-multiget`-Antwort aus (nur Treffer mit Daten).
pub fn parse_multiget(xml: &str) -> Result<Vec<ObjektDaten>> {
    let (antworten, _) = parse_multistatus(xml)?;
    Ok(antworten
        .into_iter()
        .filter_map(|antwort| {
            let ics = antwort.prop(&["calendar-data"])?.text.clone();
            if ics.trim().is_empty() {
                return None;
            }
            let etag = antwort
                .prop(&["getetag"])
                .map(|p| p.text.trim().to_string())
                .unwrap_or_default();
            Some(ObjektDaten {
                href: antwort.href,
                etag,
                ics,
            })
        })
        .collect())
}

// ---------------------------------------------- Multistatus-Grundgerüst --

/// Ein Kind-Element eines Props (z. B. `<calendar/>` in `resourcetype`
/// oder `<comp name="VEVENT"/>` im Komponenten-Set).
#[derive(Debug, Clone)]
struct PropKind {
    name: String,
    name_attribut: String,
}

/// Ein Prop aus einem `propstat`-Block mit Status 200.
#[derive(Debug, Clone)]
struct Prop {
    name: String,
    text: String,
    kinder: Vec<PropKind>,
}

impl Prop {
    fn hat_kind(&self, name: &str) -> bool {
        self.kinder.iter().any(|k| k.name == name)
    }
    fn hat_kind_mit_name(&self, attribut: &str) -> bool {
        self.kinder
            .iter()
            .any(|k| k.name_attribut.eq_ignore_ascii_case(attribut))
    }
}

/// Eine `<response>` mit href, Direkt-Status und den 200er-Props.
#[derive(Debug, Clone, Default)]
struct Antwort {
    href: String,
    status: String,
    props: Vec<Prop>,
}

impl Antwort {
    fn prop(&self, namen: &[&str]) -> Option<&Prop> {
        self.props.iter().find(|p| namen.contains(&p.name.as_str()))
    }
}

fn lokaler_name(roh: &[u8]) -> String {
    let name = roh.rsplit(|b| *b == b':').next().unwrap_or(roh);
    String::from_utf8_lossy(name).to_lowercase()
}

/// Liest ein `multistatus`-Dokument in Antworten + Top-Level-Sync-Token.
/// Namespace-tolerant: verglichen wird nur der lokale Elementname.
fn parse_multistatus(xml: &str) -> Result<(Vec<Antwort>, Option<String>)> {
    let mut reader = Reader::from_str(xml);
    let mut pfad: Vec<String> = Vec::new();
    let mut antworten: Vec<Antwort> = Vec::new();
    let mut token: Option<String> = None;

    let mut aktuelle: Antwort = Antwort::default();
    // Props des laufenden propstat-Blocks samt dessen Status.
    let mut propstat = Propstat::default();

    loop {
        match reader.read_event().context("CalDAV-Antwort lesen")? {
            Event::Start(start) => {
                element_beginnen(
                    &mut pfad,
                    lokaler_name(start.name().as_ref()),
                    name_attribut(&start)?,
                    &mut aktuelle,
                    &mut propstat,
                );
            }
            Event::Empty(start) => {
                element_beginnen(
                    &mut pfad,
                    lokaler_name(start.name().as_ref()),
                    name_attribut(&start)?,
                    &mut aktuelle,
                    &mut propstat,
                );
                element_beenden(&mut pfad, &mut antworten, &mut aktuelle, &mut propstat);
            }
            Event::Text(text) => {
                let text = text.decode().context("XML-Text dekodieren")?;
                let text = quick_xml::escape::unescape(&text).context("XML-Text auswerten")?;
                text_zuordnen(&pfad, &text, &mut aktuelle, &mut propstat, &mut token);
            }
            // Entities wie `&amp;` liefert quick-xml als eigenes Ereignis.
            Event::GeneralRef(referenz) => {
                let text = match referenz
                    .resolve_char_ref()
                    .context("Zeichenreferenz auflösen")?
                {
                    Some(zeichen) => zeichen.to_string(),
                    None => match referenz.decode().context("Entity dekodieren")?.as_ref() {
                        "amp" => "&".to_string(),
                        "lt" => "<".to_string(),
                        "gt" => ">".to_string(),
                        "apos" => "'".to_string(),
                        "quot" => "\"".to_string(),
                        unbekannt => format!("&{unbekannt};"),
                    },
                };
                text_zuordnen(&pfad, &text, &mut aktuelle, &mut propstat, &mut token);
            }
            Event::CData(cdata) => {
                let text = String::from_utf8_lossy(&cdata).into_owned();
                text_zuordnen(&pfad, &text, &mut aktuelle, &mut propstat, &mut token);
            }
            Event::End(_) => {
                element_beenden(&mut pfad, &mut antworten, &mut aktuelle, &mut propstat);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok((antworten, token))
}

fn name_attribut(start: &quick_xml::events::BytesStart<'_>) -> Result<String> {
    Ok(start
        .try_get_attribute("name")
        .context("XML-Attribut lesen")?
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
        .unwrap_or_default())
}

/// Passt der Pfad (lokale Namen ab `multistatus`) auf das Muster?
fn pfad_ist(pfad: &[String], muster: &[&str]) -> bool {
    pfad.len() == muster.len() && pfad.iter().zip(muster).all(|(a, b)| a == b)
}

/// Props des laufenden `propstat`-Blocks samt dessen Status.
#[derive(Debug, Default)]
struct Propstat {
    props: Vec<Prop>,
    status: String,
}

fn element_beginnen(
    pfad: &mut Vec<String>,
    name: String,
    name_attribut: String,
    aktuelle: &mut Antwort,
    propstat: &mut Propstat,
) {
    pfad.push(name.clone());
    if pfad_ist(pfad, &["multistatus", "response"]) {
        *aktuelle = Antwort::default();
    } else if pfad_ist(pfad, &["multistatus", "response", "propstat"]) {
        *propstat = Propstat::default();
    } else if pfad.len() == 5
        && pfad_ist(&pfad[..4], &["multistatus", "response", "propstat", "prop"])
    {
        propstat.props.push(Prop {
            name,
            text: String::new(),
            kinder: Vec::new(),
        });
    } else if pfad.len() == 6
        && pfad_ist(&pfad[..4], &["multistatus", "response", "propstat", "prop"])
    {
        if let Some(prop) = propstat.props.last_mut() {
            prop.kinder.push(PropKind {
                name,
                name_attribut,
            });
        }
    }
}

fn element_beenden(
    pfad: &mut Vec<String>,
    antworten: &mut Vec<Antwort>,
    aktuelle: &mut Antwort,
    propstat: &mut Propstat,
) {
    if pfad_ist(pfad, &["multistatus", "response"]) {
        antworten.push(std::mem::take(aktuelle));
    } else if pfad_ist(pfad, &["multistatus", "response", "propstat"]) {
        // Nur erfolgreich gelieferte Props übernehmen (Status 200).
        if propstat.status.contains("200") {
            aktuelle.props.append(&mut propstat.props);
        }
        propstat.props.clear();
    }
    pfad.pop();
}

fn text_zuordnen(
    pfad: &[String],
    text: &str,
    aktuelle: &mut Antwort,
    propstat: &mut Propstat,
    token: &mut Option<String>,
) {
    if pfad_ist(pfad, &["multistatus", "response", "href"]) {
        aktuelle.href.push_str(text.trim());
    } else if pfad_ist(pfad, &["multistatus", "response", "status"]) {
        aktuelle.status.push_str(text.trim());
    } else if pfad_ist(pfad, &["multistatus", "response", "propstat", "status"]) {
        propstat.status.push_str(text.trim());
    } else if pfad_ist(pfad, &["multistatus", "sync-token"]) {
        *token = Some(text.trim().to_string());
    } else if pfad.len() == 5
        && pfad_ist(&pfad[..4], &["multistatus", "response", "propstat", "prop"])
    {
        if let Some(prop) = propstat.props.last_mut() {
            prop.text.push_str(text);
        }
    }
}

// --------------------------------------------------------------- Hilfen --

/// Anzeigename-Ersatz aus dem letzten Pfadteil des hrefs.
fn name_aus_href(href: &str) -> String {
    href.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("Kalender")
        .to_string()
}

/// Nextcloud liefert Farben teils als `#rrggbbaa` — auf `#rrggbb` kürzen;
/// alles, was keine Hex-Farbe ist, wird verworfen.
fn farbe_normalisieren(farbe: &str) -> String {
    let farbe = farbe.trim().to_lowercase();
    let hex_ok =
        |s: &str| s.starts_with('#') && s[1..].chars().all(|zeichen| zeichen.is_ascii_hexdigit());
    match farbe.len() {
        7 if hex_ok(&farbe) => farbe,
        9 if hex_ok(&farbe) => farbe[..7].to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_anfrage_enthaelt_token_und_escaped() {
        let xml = sync_anfrage("http://sabre.io/ns/sync/22");
        assert!(xml.contains("<d:sync-token>http://sabre.io/ns/sync/22</d:sync-token>"));
        assert!(xml.contains("<d:sync-level>1</d:sync-level>"));
        // Sonderzeichen werden entschärft.
        assert!(sync_anfrage("a&b").contains("a&amp;b"));
        // Erstabgleich: leeres Token.
        assert!(sync_anfrage("").contains("<d:sync-token></d:sync-token>"));
    }

    #[test]
    fn multiget_anfrage_enthaelt_alle_hrefs() {
        let xml = multiget_anfrage(&[
            "/dav/calendars/u/privat/a.ics".to_string(),
            "/dav/calendars/u/privat/b&c.ics".to_string(),
        ]);
        assert!(xml.contains("<d:href>/dav/calendars/u/privat/a.ics</d:href>"));
        assert!(xml.contains("<d:href>/dav/calendars/u/privat/b&amp;c.ics</d:href>"));
        assert!(xml.contains("<c:calendar-data/>"));
    }

    #[test]
    fn discovery_findet_kalender_mit_name_und_farbe() {
        // Nachbau einer typischen Nextcloud-Antwort: Home-Collection ohne
        // Kalender-Typ, ein Kalender mit Farbe (#rrggbbaa), eine
        // Aufgabenliste (nur VTODO), ein Kalender ohne Komponenten-Angabe.
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:x1="http://apple.com/ns/ical/">
  <d:response>
    <d:href>/remote.php/dav/calendars/philipp/</d:href>
    <d:propstat>
      <d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/dav/calendars/philipp/personal/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Persönlich</d:displayname>
        <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
        <x1:calendar-color>#0082C9FF</x1:calendar-color>
        <cal:supported-calendar-component-set><cal:comp name="VEVENT"/></cal:supported-calendar-component-set>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/dav/calendars/philipp/aufgaben/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Aufgaben</d:displayname>
        <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
        <cal:supported-calendar-component-set><cal:comp name="VTODO"/></cal:supported-calendar-component-set>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/dav/calendars/philipp/ohne-name/</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
    <d:propstat>
      <d:prop><d:displayname/></d:prop>
      <d:status>HTTP/1.1 404 Not Found</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        let funde = parse_discovery(xml).unwrap();
        assert_eq!(
            funde,
            vec![
                KalenderFund {
                    href: "/remote.php/dav/calendars/philipp/personal/".into(),
                    anzeige_name: "Persönlich".into(),
                    farbe: "#0082c9".into(),
                },
                KalenderFund {
                    href: "/remote.php/dav/calendars/philipp/ohne-name/".into(),
                    anzeige_name: "ohne-name".into(),
                    farbe: String::new(),
                },
            ]
        );
    }

    #[test]
    fn discovery_ist_namespace_tolerant() {
        // Gleiche Struktur, aber Default-Namespace statt Präfix d:.
        let xml = r#"<?xml version="1.0"?>
<multistatus xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <response>
    <href>/cal/u/privat/</href>
    <propstat>
      <prop>
        <displayname>Privat</displayname>
        <resourcetype><collection/><C:calendar/></resourcetype>
      </prop>
      <status>HTTP/1.1 200 OK</status>
    </propstat>
  </response>
</multistatus>"#;
        let funde = parse_discovery(xml).unwrap();
        assert_eq!(funde.len(), 1);
        assert_eq!(funde[0].anzeige_name, "Privat");
    }

    #[test]
    fn sync_antwort_trennt_geaendert_und_geloescht() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/cal/u/privat/neu.ics</d:href>
    <d:propstat>
      <d:prop><d:getetag>"etag-neu"</d:getetag></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/cal/u/privat/weg.ics</d:href>
    <d:status>HTTP/1.1 404 Not Found</d:status>
  </d:response>
  <d:response>
    <d:href>/cal/u/privat/</d:href>
    <d:propstat>
      <d:prop><d:getetag>"collection"</d:getetag></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:sync-token>http://sabre.io/ns/sync/23</d:sync-token>
</d:multistatus>"#;
        let ergebnis = parse_sync(xml).unwrap();
        assert_eq!(ergebnis.token, "http://sabre.io/ns/sync/23");
        assert_eq!(
            ergebnis.geaendert,
            vec![(
                "/cal/u/privat/neu.ics".to_string(),
                "\"etag-neu\"".to_string()
            )]
        );
        assert_eq!(
            ergebnis.geloescht,
            vec!["/cal/u/privat/weg.ics".to_string()]
        );
    }

    #[test]
    fn sync_antwort_ohne_token_ist_fehler() {
        let xml = r#"<d:multistatus xmlns:d="DAV:"></d:multistatus>"#;
        assert!(parse_sync(xml).is_err());
    }

    #[test]
    fn multiget_liefert_ics_daten() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/cal/u/privat/a.ics</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"etag-a"</d:getetag>
        <cal:calendar-data>BEGIN:VCALENDAR
BEGIN:VEVENT
SUMMARY:Kaffee &amp; Kuchen
END:VEVENT
END:VCALENDAR
</cal:calendar-data>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/cal/u/privat/fehlt.ics</d:href>
    <d:status>HTTP/1.1 404 Not Found</d:status>
  </d:response>
</d:multistatus>"#;
        let objekte = parse_multiget(xml).unwrap();
        assert_eq!(objekte.len(), 1);
        assert_eq!(objekte[0].href, "/cal/u/privat/a.ics");
        assert_eq!(objekte[0].etag, "\"etag-a\"");
        // XML-Entities sind aufgelöst, der ICS-Text bleibt mehrzeilig.
        assert!(objekte[0].ics.contains("SUMMARY:Kaffee & Kuchen"));
        assert!(objekte[0].ics.starts_with("BEGIN:VCALENDAR"));
    }

    #[test]
    fn farbe_wird_normalisiert() {
        assert_eq!(farbe_normalisieren("#0082C9"), "#0082c9");
        assert_eq!(farbe_normalisieren("#0082C9FF"), "#0082c9");
        assert_eq!(farbe_normalisieren("blau"), "");
        assert_eq!(farbe_normalisieren(""), "");
    }
}

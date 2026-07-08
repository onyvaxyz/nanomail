//! Dünne HTTP-Schicht für CalDAV (reqwest, Basic Auth über HTTPS).
//!
//! Enthält bewusst keine Logik: XML bauen/auswerten übernimmt
//! `caldav::xml`, die Sync-Entscheidungen trifft die Command-Schicht.

use anyhow::{anyhow, Context, Result};
use reqwest::Method;

use super::xml;

/// Antwort eines `sync-collection`-REPORTs: entweder Änderungen oder
/// „Token nicht mehr gültig“ (dann muss der Kalender neu geladen werden).
pub enum SyncAntwort {
    Ergebnis(xml::SyncErgebnis),
    TokenUngueltig,
}

pub struct CaldavVerbindung {
    client: reqwest::Client,
    /// `schema://host[:port]` — hrefs vom Server sind Pfade darunter.
    origin: String,
    /// Pfad der Kalender-Übersicht (`.../remote.php/dav/calendars/<user>/`).
    kalender_home: String,
    benutzer: String,
    passwort: String,
}

impl CaldavVerbindung {
    /// `server` ist die Nextcloud-Basis-Adresse, z. B.
    /// `https://cloud.example.com` (auch mit Unterverzeichnis).
    pub fn neu(server: &str, benutzer: &str, passwort: &str) -> Result<Self> {
        let server = server.trim().trim_end_matches('/');
        let url = reqwest::Url::parse(server)
            .map_err(|_| anyhow!("Serveradresse „{server}“ ist keine gültige Adresse"))?;
        if url.scheme() != "https" {
            anyhow::bail!(
                "NUTZERFEHLER:Die Serveradresse muss mit https:// beginnen — \
                 Zugangsdaten werden nur verschlüsselt übertragen."
            );
        }
        let host = url.host_str().context("Serveradresse ohne Rechnernamen")?;
        let origin = match url.port() {
            Some(port) => format!("https://{host}:{port}"),
            None => format!("https://{host}"),
        };
        let basis_pfad = url.path().trim_end_matches('/');
        let kalender_home = format!(
            "{basis_pfad}/remote.php/dav/calendars/{}/",
            pfadteil_kodieren(benutzer)
        );
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("HTTP-Client erstellen")?;
        Ok(Self {
            client,
            origin,
            kalender_home,
            benutzer: benutzer.to_string(),
            passwort: passwort.to_string(),
        })
    }

    /// Discovery: alle Termin-Kalender des Kontos (PROPFIND, Tiefe 1).
    pub async fn kalender_finden(&self) -> Result<Vec<xml::KalenderFund>> {
        let antwort = self
            .anfrage(
                "PROPFIND",
                &self.kalender_home,
                "1",
                xml::discovery_anfrage(),
            )
            .await?;
        xml::parse_discovery(&antwort).context("Kalender-Übersicht auswerten")
    }

    /// Abgleich eines Kalenders über sein Sync-Token (leer = Erstabgleich).
    pub async fn abgleichen(&self, kalender_href: &str, token: &str) -> Result<SyncAntwort> {
        let ergebnis = self
            .anfrage("REPORT", kalender_href, "1", xml::sync_anfrage(token))
            .await;
        match ergebnis {
            Ok(antwort) => Ok(SyncAntwort::Ergebnis(
                xml::parse_sync(&antwort).context("Sync-Antwort auswerten")?,
            )),
            // Sabre/Nextcloud meldet ein verfallenes Token als 403
            // (valid-sync-token); manche Server nutzen 409/412.
            Err(fehler) if !token.is_empty() && ist_token_fehler(&fehler) => {
                tracing::info!(
                    kalender_href,
                    "Sync-Token verfallen — Kalender wird neu geladen"
                );
                Ok(SyncAntwort::TokenUngueltig)
            }
            Err(fehler) => Err(fehler),
        }
    }

    /// Lädt die ICS-Daten der angefragten Objekte (calendar-multiget).
    pub async fn objekte_laden(
        &self,
        kalender_href: &str,
        hrefs: &[String],
    ) -> Result<Vec<xml::ObjektDaten>> {
        if hrefs.is_empty() {
            return Ok(Vec::new());
        }
        let antwort = self
            .anfrage("REPORT", kalender_href, "1", xml::multiget_anfrage(hrefs))
            .await?;
        xml::parse_multiget(&antwort).context("Termin-Daten auswerten")
    }

    /// Führt eine WebDAV-Anfrage aus und liefert den Antwort-Body.
    async fn anfrage(
        &self,
        methode: &str,
        pfad: &str,
        tiefe: &str,
        body: String,
    ) -> Result<String> {
        let url = format!("{}{}", self.origin, pfad);
        let methode = Method::from_bytes(methode.as_bytes()).context("HTTP-Methode")?;
        let antwort = self
            .client
            .request(methode, &url)
            .basic_auth(&self.benutzer, Some(&self.passwort))
            .header("Depth", tiefe)
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(body)
            .send()
            .await
            .map_err(|fehler| {
                tracing::warn!("CalDAV-Anfrage fehlgeschlagen: {fehler:#}");
                anyhow!("Kalender-Server nicht erreichbar — bitte Serveradresse und Internetverbindung prüfen.")
            })?;

        let status = antwort.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            anyhow::bail!(
                "NUTZERFEHLER:Der Kalender-Server hat die Anmeldung abgelehnt. Bitte \
                 Benutzername und App-Passwort prüfen (in Nextcloud unter Einstellungen → \
                 Sicherheit ein App-Passwort erstellen)."
            );
        }
        let text = antwort.text().await.context("Antwort lesen")?;
        if !status.is_success() {
            anyhow::bail!("HTTP-Status {status} von {url}");
        }
        Ok(text)
    }
}

fn ist_token_fehler(fehler: &anyhow::Error) -> bool {
    let text = format!("{fehler:#}");
    text.contains("HTTP-Status 403")
        || text.contains("HTTP-Status 409")
        || text.contains("HTTP-Status 412")
}

/// Prozent-Kodierung für einen Pfadteil (Benutzernamen können z. B.
/// eine E-Mail-Adresse mit Sonderzeichen sein).
fn pfadteil_kodieren(teil: &str) -> String {
    let mut ergebnis = String::with_capacity(teil.len());
    for byte in teil.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'@' => {
                ergebnis.push(byte as char);
            }
            _ => ergebnis.push_str(&format!("%{byte:02X}")),
        }
    }
    ergebnis
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbindung_baut_kalender_home_pfad() {
        // In der App installiert `run()` den Provider — hier für den Test.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let v = CaldavVerbindung::neu("https://cloud.example.com/", "philipp", "x").unwrap();
        assert_eq!(v.origin, "https://cloud.example.com");
        assert_eq!(v.kalender_home, "/remote.php/dav/calendars/philipp/");

        // Unterverzeichnis-Installation und Sonderzeichen im Benutzer.
        let v =
            CaldavVerbindung::neu("https://example.com:8443/nextcloud", "p b@x.de", "x").unwrap();
        assert_eq!(v.origin, "https://example.com:8443");
        assert_eq!(
            v.kalender_home,
            "/nextcloud/remote.php/dav/calendars/p%20b@x.de/"
        );
    }

    #[test]
    fn verbindung_verlangt_https() {
        let fehler = CaldavVerbindung::neu("http://cloud.example.com", "u", "x")
            .err()
            .unwrap();
        assert!(format!("{fehler}").contains("https"));
        assert!(CaldavVerbindung::neu("cloud.example.com", "u", "x").is_err());
    }
}

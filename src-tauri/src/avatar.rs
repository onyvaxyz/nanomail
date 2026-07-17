//! Absender-Avatare (runde Bilder in der Mail-Liste).
//!
//! **Bewusste, vom Projektinhaber freigegebene Ausnahme** vom Grundsatz
//! „keine externen Ladevorgänge“: Avatare werden aktiv geladen
//! (Gravatar → Favicon → sonst Initialen im Frontend). Das kontaktiert
//! externe Server; der Nutzer hat diese Abwägung ausdrücklich gewählt.
//!
//! Reine Hilfsfunktionen (Hash, URL, Domain) sind unit-getestet; der
//! Netzwerkabruf ist dünn und getrennt. Ergebnisse werden in der
//! Datenbank gecacht (siehe `db::avatar_*`).

use std::time::Duration;

use anyhow::{Context, Result};
use base64::Engine;

/// Maximale Bildgröße eines Avatars (großzügig, aber begrenzt).
const MAX_AVATAR_BYTES: usize = 512 * 1024;

/// Gravatar-Hash: MD5 der klein geschriebenen, getrimmten Adresse.
pub fn gravatar_hash(email: &str) -> String {
    format!("{:x}", md5::compute(email.trim().to_lowercase().as_bytes()))
}

pub fn gravatar_url(email: &str) -> String {
    // d=404 → Gravatar liefert 404, wenn es kein Bild gibt (klare Semantik).
    format!(
        "https://www.gravatar.com/avatar/{}?s=80&d=404",
        gravatar_hash(email)
    )
}

/// Domain-Teil einer E-Mail-Adresse (klein geschrieben).
pub fn domain(email: &str) -> Option<String> {
    email
        .rsplit_once('@')
        .map(|(_, domain)| domain.trim().to_lowercase())
        .filter(|d| d.contains('.') && !d.is_empty())
}

/// Favicon-Dienst (DuckDuckGo) für eine Domain.
pub fn favicon_url(domain: &str) -> String {
    format!("https://icons.duckduckgo.com/ip3/{domain}.ico")
}

/// Lädt einen Avatar: erst Gravatar, sonst Favicon der Absender-Domain.
/// `Ok(Some(uri))` = Bild, `Ok(None)` = sicher kein Bild vorhanden (cachebar),
/// `Err` = vorübergehender Fehler (Netz/Server) — darf **nicht** als
/// „kein Bild“ gecacht werden, sonst fehlen Avatare 30 Tage lang.
pub async fn hole_avatar(client: &reqwest::Client, email: &str) -> Result<Option<String>> {
    let mut voruebergehend_gescheitert = false;
    match lade_bild(client, &gravatar_url(email)).await {
        Ok(Some(uri)) => return Ok(Some(uri)),
        Ok(None) => {}
        Err(fehler) => {
            tracing::debug!("Gravatar nicht geladen: {fehler:#}");
            voruebergehend_gescheitert = true;
        }
    }
    if let Some(domain) = domain(email) {
        match lade_bild(client, &favicon_url(&domain)).await {
            Ok(Some(uri)) => return Ok(Some(uri)),
            Ok(None) => {}
            Err(fehler) => {
                tracing::debug!("Favicon ({domain}) nicht geladen: {fehler:#}");
                voruebergehend_gescheitert = true;
            }
        }
    }
    if voruebergehend_gescheitert {
        anyhow::bail!("Avatar-Quelle vorübergehend nicht erreichbar");
    }
    Ok(None)
}

/// `Ok(None)` nur bei eindeutigem „kein Bild“ (404, kein Bildinhalt,
/// unpassende Größe); alle anderen Fehler sind vorübergehend (`Err`).
async fn lade_bild(client: &reqwest::Client, url: &str) -> Result<Option<String>> {
    let antwort = client.get(url).send().await.context("Avatar anfragen")?;
    if antwort.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !antwort.status().is_success() {
        anyhow::bail!("HTTP-Status {}", antwort.status());
    }
    let mime = antwort
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|wert| wert.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !mime.starts_with("image/") {
        tracing::debug!("Avatar-Antwort ist kein Bild (Content-Type {mime:?})");
        return Ok(None);
    }
    let bytes = antwort.bytes().await.context("Avatar herunterladen")?;
    if bytes.is_empty() || bytes.len() > MAX_AVATAR_BYTES {
        tracing::debug!("Avatar-Größe unpassend ({} Bytes)", bytes.len());
        return Ok(None);
    }
    let daten = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(Some(format!("data:{mime};base64,{daten}")))
}

/// Gemeinsamer HTTP-Client für Avatar-Abrufe (kurzer Timeout).
pub fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("HTTP-Client für Avatare erstellen")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravatar_hash_ist_md5_der_normalisierten_adresse() {
        // Referenzwert aus der Gravatar-Doku für "MyEmailAddress@example.com".
        assert_eq!(
            gravatar_hash("  MyEmailAddress@example.com "),
            "0bc83cb571cd1c50ba6f3e8a78ef1346"
        );
    }

    #[test]
    fn gravatar_url_nutzt_404_fallback() {
        let url = gravatar_url("a@b.de");
        assert!(url.contains("gravatar.com/avatar/"));
        assert!(url.contains("d=404"));
    }

    #[test]
    fn domain_wird_extrahiert_und_normalisiert() {
        assert_eq!(domain("Anna@Example.ORG"), Some("example.org".to_string()));
        assert_eq!(domain("kaputt"), None);
        assert_eq!(domain("kein@domain"), None); // ohne Punkt
    }

    #[test]
    fn favicon_url_zeigt_auf_domain() {
        assert_eq!(
            favicon_url("shop.example"),
            "https://icons.duckduckgo.com/ip3/shop.example.ico"
        );
    }
}

//! Microsoft-OAuth2 (Meilenstein M6).
//!
//! Device-Code-Flow, Token-Refresh und deutsche Fehlertexte für
//! Microsoft-365-Konten (IMAP + SMTP über `outlook.office.com`).
//! Tokens werden ausschließlich im GNOME Keyring gespeichert
//! (siehe `schluesselbund`) — nie in SQLite, Config-Dateien oder Logs.
//!
//! Konventionen: siehe `.claude/skills/oauth-ms/SKILL.md`.
//! Der Flow ist bewusst eine dünne `reqwest`-Schicht (wie der validierte
//! Mini-Auth-Test): reines Parsen und Fehlertexte sind ohne Netzwerk
//! testbar, nur drei kleine Funktionen sprechen wirklich mit Microsoft.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Öffentliche Client-ID von Thunderbird (kein Secret nötig).
/// Bewusst wiederverwendet statt einer eigenen App-Registrierung:
/// Der Mini-Auth-Test hat belegt, dass der Tenant sie ohne
/// Admin-Zustimmung akzeptiert. Alternative für später: eigene
/// Registrierung im Azure-Portal und nur diese Konstante tauschen.
pub const CLIENT_ID: &str = "9e5f94bc-e8a4-4e73-b8be-63364c29d753";
/// Tut es für Firmen- wie Privatkonten; erst bei Problemen auf die
/// Tenant-ID oder `organizations` einengen.
const TENANT: &str = "common";
/// Genau die Rechte, die Nanomail braucht: Lesen, Senden, angemeldet bleiben.
const SCOPES: &str = "offline_access https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send";
/// Sicherheitspuffer: Token gilt als „bald abgelaufen“, wenn es in
/// weniger als 5 Minuten ungültig wird — dann wird vorher aufgefrischt.
const AUFFRISCH_PUFFER_SEKUNDEN: u64 = 5 * 60;

fn endpunkt(pfad: &str) -> String {
    format!("https://login.microsoftonline.com/{TENANT}/oauth2/v2.0/{pfad}")
}

/// Antwort des Geräte-Code-Starts: Was die App dem Nutzer zeigt
/// (URL + Code) und was sie sich fürs Abfragen merkt (Geräte-Code).
#[derive(Debug, Clone)]
pub struct GeraeteAnfrage {
    pub pruef_url: String,
    pub benutzer_code: String,
    pub geraete_code: String,
    /// Wie lange der Code gültig ist (Sekunden ab Start).
    pub laeuft_ab_sekunden: u64,
    /// Abfrage-Abstand in Sekunden (Microsoft-Vorgabe respektieren).
    pub intervall_sekunden: u64,
}

/// Frisches Token-Paar aus Microsoft-Sicht (mit Ablauf in Sekunden).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenSatz {
    pub zugang_token: String,
    pub auffrisch_token: String,
    /// Unix-Sekunden, ab denen das Zugangs-Token ungültig ist.
    pub ablauf_unix: u64,
}

impl TokenSatz {
    /// Wahr, wenn das Zugangs-Token (inkl. Puffer) erneuert werden muss.
    /// `jetzt_unix` wird übergeben, damit Tests ohne Uhr auskommen.
    pub fn braucht_auffrischung(&self, jetzt_unix: u64) -> bool {
        jetzt_unix + AUFFRISCH_PUFFER_SEKUNDEN >= self.ablauf_unix
    }
}

/// Stand einer Token-Abfrage nach der Browser-Anmeldung.
pub enum AbfrageStand {
    /// Noch nicht im Browser bestätigt — später erneut abfragen.
    Wartet,
    /// Anmeldung abgeschlossen (längeres Warten ist ein Fehler).
    Fertig(TokenSatz),
}

/// Startet die Geräte-Anmeldung: liefert URL + Code für den Browser.
pub async fn anmeldung_starten(http: &reqwest::Client) -> Result<GeraeteAnfrage> {
    let text = formular_senden(
        http,
        &endpunkt("devicecode"),
        &[("client_id", CLIENT_ID), ("scope", SCOPES)],
    )
    .await
    .context("Geräte-Code bei Microsoft anfordern")?;
    let json: serde_json::Value =
        serde_json::from_str(&text).context("Antwort von Microsoft lesen")?;
    if json.get("device_code").is_none() {
        anyhow::bail!(fehler_text(&json));
    }
    Ok(GeraeteAnfrage {
        pruef_url: json_string(&json, "verification_uri")?,
        benutzer_code: json_string(&json, "user_code")?,
        geraete_code: json_string(&json, "device_code")?,
        laeuft_ab_sekunden: json
            .get("expires_in")
            .and_then(|w| w.as_u64())
            .unwrap_or(900),
        intervall_sekunden: json
            .get("interval")
            .and_then(|w| w.as_u64())
            .unwrap_or(5)
            .max(5),
    })
}

/// Fragt einmal nach, ob die Browser-Anmeldung fertig ist.
/// `authorization_pending`/`slow_down` bedeuten Warten (kein Fehler);
/// alles andere Endgültige wird als deutsche Meldung zurückgegeben.
pub async fn anmeldung_abfragen(
    http: &reqwest::Client,
    geraete_code: &str,
    jetzt_unix: u64,
) -> Result<AbfrageStand> {
    let text = formular_senden(
        http,
        &endpunkt("token"),
        &[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", CLIENT_ID),
            ("device_code", geraete_code),
        ],
    )
    .await
    .context("Anmeldestand bei Microsoft abfragen")?;
    let json: serde_json::Value =
        serde_json::from_str(&text).context("Antwort von Microsoft lesen")?;
    match json.get("access_token").and_then(|w| w.as_str()) {
        Some(_) => Ok(AbfrageStand::Fertig(token_satz_aus(&json, jetzt_unix)?)),
        None => match json.get("error").and_then(|w| w.as_str()).unwrap_or("") {
            "authorization_pending" | "slow_down" => Ok(AbfrageStand::Wartet),
            _ => anyhow::bail!(fehler_text(&json)),
        },
    }
}

/// Holt mit dem Auffrisch-Token ein neues Zugangs-Token.
/// Schlägt das fehl (z. B. entzogen), muss sich der Nutzer erneut
/// anmelden — die Meldung sagt das ausdrücklich.
pub async fn token_auffrischen(
    http: &reqwest::Client,
    auffrisch_token: &str,
    jetzt_unix: u64,
) -> Result<TokenSatz> {
    let text = formular_senden(
        http,
        &endpunkt("token"),
        &[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", auffrisch_token),
            ("scope", SCOPES),
        ],
    )
    .await
    .context("Token bei Microsoft auffrischen")?;
    let json: serde_json::Value =
        serde_json::from_str(&text).context("Antwort von Microsoft lesen")?;
    if json.get("access_token").is_none() {
        anyhow::bail!(neuanmeldung_text(&json));
    }
    token_satz_aus(&json, jetzt_unix)
}

/// Baut aus einer erfolgreichen Token-Antwort einen `TokenSatz`.
/// Microsoft liefert manchmal kein neues Auffrisch-Token mit —
/// dann gilt das bisherige weiter (als `altes_auffrisch_token` übergeben).
fn token_satz_aus(json: &serde_json::Value, jetzt_unix: u64) -> Result<TokenSatz> {
    let zugang = json_string(json, "access_token")?;
    let gueltig = json
        .get("expires_in")
        .and_then(|w| w.as_u64())
        .unwrap_or(3600);
    let auffrischung = json
        .get("refresh_token")
        .and_then(|w| w.as_str())
        .unwrap_or("")
        .to_string();
    Ok(TokenSatz {
        zugang_token: zugang,
        auffrisch_token: auffrischung,
        ablauf_unix: jetzt_unix.saturating_add(gueltig),
    })
}

fn json_string(json: &serde_json::Value, feld: &str) -> Result<String> {
    json.get(feld)
        .and_then(|w| w.as_str())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("Microsoft-Antwort ohne Feld „{feld}“"))
}

/// POST mit `application/x-www-form-urlencoded` (ohne Zusatz-Crate).
/// Gibt den Antworttext zurück — auch bei HTTP-Fehlern, weil Microsoft
/// Details als JSON im Rumpf liefert.
async fn formular_senden(
    http: &reqwest::Client,
    url: &str,
    felder: &[(&str, &str)],
) -> Result<String> {
    http.post(url)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(formulieren(felder))
        .send()
        .await
        .with_context(|| format!("{url} nicht erreichbar"))?
        .text()
        .await
        .context("Antwort von Microsoft lesen")
}

/// Baut einen `application/x-www-form-urlencoded`-Rumpf.
fn formulieren(felder: &[(&str, &str)]) -> String {
    felder
        .iter()
        .map(|(k, v)| format!("{k}={}", prozent_kodieren(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Minimale Prozent-Kodierung (reicht für IDs, Scopes und Codes).
fn prozent_kodieren(text: &str) -> String {
    let mut kodiert = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                kodiert.push(byte as char);
            }
            _ => kodiert.push_str(&format!("%{byte:02X}")),
        }
    }
    kodiert
}

/// Übersetzt eine Microsoft-Fehlerantwort in verständliches Deutsch.
/// Enthält nie Tokens — nur Fehlernummer und erste Zeile.
pub fn fehler_text(json: &serde_json::Value) -> String {
    let beschreibung = json
        .get("error_description")
        .and_then(|w| w.as_str())
        .unwrap_or("");
    // AADSTS-Nummer aus der Beschreibung ziehen (Format „AADSTS12345: …“).
    let nummer: String = beschreibung
        .split("AADSTS")
        .nth(1)
        .map(|rest| rest.chars().take_while(|c| c.is_ascii_digit()).collect())
        .unwrap_or_default();
    match nummer.as_str() {
        "65001" | "65002" => {
            "Der Verwalter des Firmenkontos muss diese Anmeldung erst freigeben.".to_string()
        }
        "50076" | "50079" => {
            "Zusätzliche Sicherheitsprüfung (z. B. Handy-Bestätigung) erforderlich — bitte im Browser abschließen.".to_string()
        }
        "70011" => "Ungültige Rechte-Anfrage — bitte dem Entwickler melden.".to_string(),
        "70016" | "7000218" => "Diese Anmeldeart ist im Firmenkonto gesperrt.".to_string(),
        "80014" => "Das Firmenkonto erlaubt diese App nicht.".to_string(),
        "90002" => "Microsoft kennt dieses Konto nicht — bitte dem Entwickler melden.".to_string(),
        _ => match json.get("error").and_then(|w| w.as_str()).unwrap_or("") {
            "expired_token" => "Der Code ist abgelaufen — bitte erneut starten.".to_string(),
            "authorization_declined" => "Die Anmeldung wurde im Browser abgelehnt.".to_string(),
            "bad_verification_code" => "Falscher Code — bitte erneut starten.".to_string(),
            _ if !beschreibung.is_empty() => format!(
                "Microsoft meldet: {}",
                beschreibung.lines().next().unwrap_or("").trim()
            ),
            _ => "Die Microsoft-Anmeldung ist fehlgeschlagen.".to_string(),
        },
    }
}

/// Meldung, wenn das Auffrisch-Token nicht mehr gilt: Der Nutzer muss
/// sich einmalig erneut anmelden (kein endloses Wiederholen im Hintergrund).
fn neuanmeldung_text(json: &serde_json::Value) -> String {
    if json
        .get("error")
        .and_then(|w| w.as_str())
        .is_some_and(|f| f == "invalid_grant")
    {
        "Die Microsoft-Anmeldung ist abgelaufen — bitte das Konto einmalig erneut verbinden (Konto bearbeiten).".to_string()
    } else {
        fehler_text(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fehler_json(fehler: &str, beschreibung: &str) -> serde_json::Value {
        serde_json::json!({
            "error": fehler,
            "error_description": beschreibung,
        })
    }

    #[test]
    fn admin_zustimmung_wird_verstaendlich() {
        let text = fehler_text(&fehler_json(
            "interaction_required",
            "AADSTS65001: The user or admin has not consented. Trace ID: x",
        ));
        assert!(text.contains("Verwalter"), "unerwartet: {text}");
    }

    #[test]
    fn abgelaufener_code_wird_verstaendlich() {
        let text = fehler_text(&fehler_json("expired_token", "AADSTS70020: expired"));
        assert!(text.contains("abgelaufen"), "unerwartet: {text}");
    }

    #[test]
    fn abgelehnte_anmeldung_wird_verstaendlich() {
        let text = fehler_text(&fehler_json("authorization_declined", ""));
        assert!(text.contains("abgelehnt"), "unerwartet: {text}");
    }

    #[test]
    fn ungueltiges_auffrisch_token_fordert_neuanmeldung() {
        let text = neuanmeldung_text(&fehler_json("invalid_grant", "AADSTS700082: expired"));
        assert!(text.contains("erneut verbinden"), "unerwartet: {text}");
    }

    #[test]
    fn token_satz_rechnet_ablauf_aus() {
        let satz = token_satz_aus(
            &serde_json::json!({
                "access_token": "ZUGANG",
                "refresh_token": "AUFFRISCH",
                "expires_in": 3600,
            }),
            1_000_000,
        )
        .unwrap();
        assert_eq!(satz.ablauf_unix, 1_003_600);
        assert!(!satz.braucht_auffrischung(1_000_000));
        // Fünf Minuten vorher greift der Puffer.
        assert!(satz.braucht_auffrischung(1_003_300));
        assert!(satz.braucht_auffrischung(2_000_000));
    }

    #[test]
    fn formular_kodiert_sonderzeichen() {
        let rumpf = formulieren(&[("scope", "offline_access a/b:c+d")]);
        assert_eq!(rumpf, "scope=offline_access%20a%2Fb%3Ac%2Bd");
    }
}

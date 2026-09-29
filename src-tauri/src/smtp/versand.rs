//! Dünne SMTP-Versandschicht auf `lettre`.
//!
//! Immer verschlüsselt: Port 465 = implizites TLS, alle anderen Ports
//! (typisch 587) = STARTTLS zwingend. `NANOMAIL_EXTRA_CA` wird wie bei
//! IMAP respektiert (Test-CA), die Zertifikatsprüfung nie abgeschaltet.
//! Loggt nie Zugangsdaten oder Mail-Inhalte.

use anyhow::{Context, Result};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::{Certificate, Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// Anmeldeart für den Versand: Passwort oder Microsoft-Token (M6).
pub enum Anmeldung<'a> {
    Passwort(&'a str),
    MicrosoftToken(&'a str),
}

pub async fn senden(
    host: &str,
    port: u16,
    benutzer: &str,
    passwort: &str,
    nachricht: Message,
) -> Result<()> {
    senden_mit(
        host,
        port,
        benutzer,
        &Anmeldung::Passwort(passwort),
        nachricht,
    )
    .await
}

/// Versendet über ein Microsoft-Konto (XOAUTH2 statt Passwort, M6).
pub async fn senden_mit_token(
    host: &str,
    port: u16,
    benutzer: &str,
    token: &str,
    nachricht: Message,
) -> Result<()> {
    senden_mit(
        host,
        port,
        benutzer,
        &Anmeldung::MicrosoftToken(token),
        nachricht,
    )
    .await
}

async fn senden_mit(
    host: &str,
    port: u16,
    benutzer: &str,
    anmeldung: &Anmeldung<'_>,
    nachricht: Message,
) -> Result<()> {
    let transport = transport(host, port, benutzer, anmeldung)?;
    transport
        .send(nachricht)
        .await
        .map_err(|fehler| smtp_fehler(&fehler))?;
    tracing::info!(host, port, "Mail über SMTP versendet");
    Ok(())
}

/// Nur Verbindung + Anmeldung testen (für die Konto-Einrichtung).
pub async fn probe(host: &str, port: u16, benutzer: &str, passwort: &str) -> Result<()> {
    let transport = transport(host, port, benutzer, &Anmeldung::Passwort(passwort))?;
    let ok = transport
        .test_connection()
        .await
        .map_err(|fehler| smtp_fehler(&fehler))?;
    if !ok {
        anyhow::bail!("SMTP-Server hat die Verbindung nicht bestätigt");
    }
    Ok(())
}

/// Nur Verbindung prüfen (für die Konto-Einrichtung) — mit
/// Microsoft-Token statt Passwort (M6). Wie `probe` wird hier nicht
/// angemeldet, nur Erreichbarkeit und TLS geprüft.
pub async fn probe_mit_token(host: &str, port: u16, benutzer: &str, token: &str) -> Result<()> {
    let transport = transport(host, port, benutzer, &Anmeldung::MicrosoftToken(token))?;
    let ok = transport
        .test_connection()
        .await
        .map_err(|fehler| smtp_fehler(&fehler))?;
    if !ok {
        anyhow::bail!("SMTP-Server hat die Verbindung nicht bestätigt");
    }
    Ok(())
}

fn transport(
    host: &str,
    port: u16,
    benutzer: &str,
    anmeldung: &Anmeldung<'_>,
) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let mut tls_params = TlsParameters::builder(host.to_string());
    if let Ok(pfad) = std::env::var("NANOMAIL_EXTRA_CA") {
        let pem = std::fs::read(&pfad).with_context(|| format!("Zusatz-CA {pfad} lesen"))?;
        tls_params = tls_params
            .add_root_certificate(Certificate::from_pem(&pem).context("Zusatz-CA parsen")?);
        tracing::warn!(pfad, "Zusätzliche Test-CA für SMTP geladen");
    }
    let tls_params = tls_params.build().context("TLS-Konfiguration erstellen")?;

    let tls = if port == 465 {
        Tls::Wrapper(tls_params) // implizites TLS
    } else {
        Tls::Required(tls_params) // STARTTLS, niemals unverschlüsselt
    };

    Ok(
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
            .port(port)
            .tls(tls)
            .credentials(geheimnis(benutzer, anmeldung))
            .authentication(mechanismen(anmeldung))
            .build(),
    )
}

/// Nutzername plus Geheimnis: Passwort oder Zugangs-Token.
/// Das Geheimnis landet nie im Log.
fn geheimnis(benutzer: &str, anmeldung: &Anmeldung<'_>) -> Credentials {
    match anmeldung {
        Anmeldung::Passwort(passwort) => {
            Credentials::new(benutzer.to_string(), passwort.to_string())
        }
        Anmeldung::MicrosoftToken(token) => {
            Credentials::new(benutzer.to_string(), token.to_string())
        }
    }
}

/// Microsoft nutzt XOAUTH2; Passwort-Konten behalten die
/// bisherige Mechanismus-Wahl (Plain/Login) unverändert.
fn mechanismen(anmeldung: &Anmeldung<'_>) -> Vec<Mechanism> {
    match anmeldung {
        Anmeldung::Passwort(_) => vec![Mechanism::Plain, Mechanism::Login],
        Anmeldung::MicrosoftToken(_) => vec![Mechanism::Xoauth2],
    }
}

/// SMTP-Fehler in eine diagnostizierbare Meldung übersetzen
/// (Muster „Anmeldung abgelehnt“ wird von `als_meldung` erkannt).
fn smtp_fehler(fehler: &lettre::transport::smtp::Error) -> anyhow::Error {
    if fehler.is_permanent() && format!("{fehler}").contains("535") {
        anyhow::anyhow!("SMTP-Anmeldung abgelehnt: {fehler}")
    } else {
        anyhow::anyhow!("SMTP-Versand fehlgeschlagen: {fehler}")
    }
}

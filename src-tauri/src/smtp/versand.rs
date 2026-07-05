//! Dünne SMTP-Versandschicht auf `lettre`.
//!
//! Immer verschlüsselt: Port 465 = implizites TLS, alle anderen Ports
//! (typisch 587) = STARTTLS zwingend. `NANOMAIL_EXTRA_CA` wird wie bei
//! IMAP respektiert (Test-CA), die Zertifikatsprüfung nie abgeschaltet.
//! Loggt nie Zugangsdaten oder Mail-Inhalte.

use anyhow::{Context, Result};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Certificate, Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

pub async fn senden(
    host: &str,
    port: u16,
    benutzer: &str,
    passwort: &str,
    nachricht: Message,
) -> Result<()> {
    let transport = transport(host, port, benutzer, passwort)?;
    transport
        .send(nachricht)
        .await
        .map_err(|fehler| smtp_fehler(&fehler))?;
    tracing::info!(host, port, "Mail über SMTP versendet");
    Ok(())
}

/// Nur Verbindung + Anmeldung testen (für die Konto-Einrichtung).
pub async fn probe(host: &str, port: u16, benutzer: &str, passwort: &str) -> Result<()> {
    let transport = transport(host, port, benutzer, passwort)?;
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
    passwort: &str,
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
            .credentials(Credentials::new(benutzer.to_string(), passwort.to_string()))
            .build(),
    )
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

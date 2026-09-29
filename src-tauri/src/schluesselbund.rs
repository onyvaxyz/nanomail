//! Zugangsdaten im GNOME Keyring (Secret Service).
//!
//! Einzige Stelle, an der Passwörter gespeichert oder gelesen werden.
//! Dienstname `nanomail`, ein Eintrag pro Konto (`konto:<id>`).
//! Passwörter tauchen nie in SQLite, Config-Dateien oder Logs auf.
//!
//! Verwendet `keyring-core` + Secret-Service-Store direkt (statt der
//! `keyring`-Fassade): Der Store wird beim App-Start einmal explizit
//! gesetzt — deterministisch und mit sichtbarer Fehlermeldung.

use anyhow::{Context, Result};

const DIENST: &str = "nanomail";

/// Beim App-Start aufrufen: verbindet den Schlüsselbund (Secret Service
/// über DBus). Schlägt das fehl, funktioniert das Anlegen/Lesen von
/// Konten nicht — die Commands liefern dann eine verständliche Meldung.
pub fn initialisieren() -> Result<()> {
    let store = zbus_secret_service_keyring_store::Store::new()
        .context("Schlüsselbund (Secret Service) nicht erreichbar")?;
    keyring_core::set_default_store(store);
    Ok(())
}

fn eintrag(konto_id: i64) -> Result<keyring_core::Entry> {
    keyring_core::Entry::new(DIENST, &format!("konto:{konto_id}"))
        .context("Schlüsselbund-Eintrag anlegen")
}

pub fn passwort_speichern(konto_id: i64, passwort: &str) -> Result<()> {
    eintrag(konto_id)?
        .set_password(passwort)
        .context("Passwort im Schlüsselbund speichern")
}

pub fn passwort_holen(konto_id: i64) -> Result<String> {
    eintrag(konto_id)?
        .get_password()
        .context("Passwort aus dem Schlüsselbund lesen")
}

/// Räumt den Eintrag auf (Konto-Löschung). Ein bereits fehlender
/// Eintrag ist kein Fehler.
pub fn passwort_loeschen(konto_id: i64) -> Result<()> {
    match eintrag(konto_id)?.delete_credential() {
        Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(fehler) => Err(fehler).context("Passwort aus dem Schlüsselbund löschen"),
    }
}

// ------------------------------------------------- Microsoft-Tokens (M6) --
// Eigener Eintrag je Microsoft-Konto (`microsoft:<id>`): JSON mit
// Auffrisch- und Zugangs-Token plus Ablaufzeit. Wie Passwörter: nur hier,
// nie in SQLite, Config-Dateien oder Logs.

fn microsoft_eintrag(konto_id: i64) -> Result<keyring_core::Entry> {
    keyring_core::Entry::new(DIENST, &format!("microsoft:{konto_id}"))
        .context("Schlüsselbund-Eintrag anlegen")
}

pub fn microsoft_token_speichern(konto_id: i64, token_json: &str) -> Result<()> {
    microsoft_eintrag(konto_id)?
        .set_password(token_json)
        .context("Microsoft-Token im Schlüsselbund speichern")
}

pub fn microsoft_token_holen(konto_id: i64) -> Result<String> {
    microsoft_eintrag(konto_id)?
        .get_password()
        .context("Microsoft-Token aus dem Schlüsselbund lesen")
}

pub fn microsoft_token_loeschen(konto_id: i64) -> Result<()> {
    match microsoft_eintrag(konto_id)?.delete_credential() {
        Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(fehler) => Err(fehler).context("Microsoft-Token aus dem Schlüsselbund löschen"),
    }
}

// ------------------------------------------------- Kalender-Konten (M4) --
// Eigener Eintrag je Kalender-Konto (`kalender:<id>`), getrennt von den
// Mail-Konten — gleiche Regeln, gleicher Dienstname.

fn kalender_eintrag(konto_id: i64) -> Result<keyring_core::Entry> {
    keyring_core::Entry::new(DIENST, &format!("kalender:{konto_id}"))
        .context("Schlüsselbund-Eintrag anlegen")
}

pub fn kalender_passwort_speichern(konto_id: i64, passwort: &str) -> Result<()> {
    kalender_eintrag(konto_id)?
        .set_password(passwort)
        .context("Passwort im Schlüsselbund speichern")
}

pub fn kalender_passwort_holen(konto_id: i64) -> Result<String> {
    kalender_eintrag(konto_id)?
        .get_password()
        .context("Passwort aus dem Schlüsselbund lesen")
}

pub fn kalender_passwort_loeschen(konto_id: i64) -> Result<()> {
    match kalender_eintrag(konto_id)?.delete_credential() {
        Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(fehler) => Err(fehler).context("Passwort aus dem Schlüsselbund löschen"),
    }
}

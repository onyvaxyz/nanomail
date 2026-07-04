//! Nanomail — Backend (Rust).
//!
//! Enthält die gesamte Datenlogik (IMAP, SMTP, CalDAV, OAuth, SQLite-Cache).
//! Das Frontend (HTML/CSS/JS in `ui/`) spricht ausschließlich über
//! Tauri-Commands mit diesem Backend.

mod anzeige;
mod caldav;
mod commands;
mod db;
mod imap;
mod logging;
mod oauth;
mod pfade;
mod schluesselbund;
mod smtp;

use std::collections::HashSet;
use std::sync::Mutex;

use tauri::Manager;

use commands::AppZustand;

/// Antwort des `ping`-Commands — dient dem Frontend als Nachweis,
/// dass die Brücke zum Rust-Backend funktioniert.
#[derive(serde::Serialize)]
struct PingAntwort {
    app: String,
    version: String,
    meilenstein: String,
}

#[tauri::command]
fn ping() -> PingAntwort {
    PingAntwort {
        app: "Nanomail".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        meilenstein: "M1 — Erstes Konto lesend".into(),
    }
}

pub fn run() {
    let _log_guard = logging::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Nanomail startet");

    // TLS-Krypto-Provider (ring) für rustls und reqwest festlegen.
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        tracing::warn!("Krypto-Provider war bereits installiert");
    }

    // Schlüsselbund verbinden — ohne ihn können keine Konten angelegt werden.
    if let Err(fehler) = schluesselbund::initialisieren() {
        tracing::error!("{fehler:#}");
    }

    tauri::Builder::default()
        .setup(|app| {
            let pfad = pfade::db_pfad().ok_or("Datenverzeichnis nicht bestimmbar")?;
            let conn = db::oeffnen(&pfad).map_err(|fehler| format!("{fehler:#}"))?;
            app.manage(AppZustand {
                db: Mutex::new(conn),
                sync_laeuft: Mutex::new(HashSet::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            commands::konto_anlegen,
            commands::konten_liste,
            commands::ordner_liste,
            commands::sync_starten,
            commands::mails_liste,
            commands::mail_lesen,
            commands::mail_bilder_laden,
        ])
        .run(tauri::generate_context!())
        .expect("Fehler beim Starten der Tauri-Anwendung");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_liefert_appname_und_version() {
        let antwort = ping();
        assert_eq!(antwort.app, "Nanomail");
        assert_eq!(antwort.version, env!("CARGO_PKG_VERSION"));
    }
}

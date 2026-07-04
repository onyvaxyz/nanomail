//! Nanomail — Backend (Rust).
//!
//! Enthält die gesamte Datenlogik (IMAP, SMTP, CalDAV, OAuth, SQLite-Cache).
//! Das Frontend (HTML/CSS/JS in `ui/`) spricht ausschließlich über
//! Tauri-Commands mit diesem Backend.

mod caldav;
mod db;
mod imap;
mod logging;
mod oauth;
mod smtp;

use serde::Serialize;

/// Antwort des `ping`-Commands — dient dem Frontend als Nachweis,
/// dass die Brücke zum Rust-Backend funktioniert.
#[derive(Serialize)]
struct PingAntwort {
    app: String,
    version: String,
    meilenstein: String,
}

#[tauri::command]
fn ping() -> PingAntwort {
    tracing::info!("ping vom Frontend empfangen");
    PingAntwort {
        app: "Nanomail".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        meilenstein: "M0 — Projektgerüst".into(),
    }
}

pub fn run() {
    let _log_guard = logging::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Nanomail startet");

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![ping])
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

//! Nanomail — Backend (Rust).
//!
//! Enthält die gesamte Datenlogik (IMAP, SMTP, CalDAV, OAuth, SQLite-Cache).
//! Das Frontend (HTML/CSS/JS in `ui/`) spricht ausschließlich über
//! Tauri-Commands mit diesem Backend.

mod anzeige;
mod avatar;
mod caldav;
mod commands;
mod db;
mod imap;
mod logging;
mod oauth;
mod pfade;
mod schluesselbund;
mod smtp;
mod thema;

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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        // Links aus Mail-Inhalten (im Sandbox-iframe) navigieren das Fenster;
        // externe Ziele (http/https/mailto) fangen wir hier ab und öffnen sie
        // im Standard-Programm des Systems, statt in der App zu navigieren.
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("externe-links")
                .on_navigation(|window, url| {
                    let schema = url.scheme();
                    let ist_extern = matches!(schema, "http" | "https" | "mailto");
                    // Eigene App-Seiten laufen im Release unter dem
                    // tauri-Protokoll, im Dev-Modus über den lokalen Server
                    // (localhost bzw. 127.0.0.1) — die dürfen ganz normal
                    // navigieren und werden nicht nach außen umgeleitet.
                    let ist_app = url
                        .host_str()
                        .is_some_and(|h| h == "localhost" || h == "127.0.0.1");
                    if ist_extern && !ist_app {
                        use tauri_plugin_opener::OpenerExt;
                        if let Err(fehler) = window
                            .app_handle()
                            .opener()
                            .open_url(url.as_str(), None::<&str>)
                        {
                            tracing::warn!("Externen Link öffnen: {fehler:#}");
                        }
                        return false; // In-App-Navigation abbrechen.
                    }
                    true
                })
                .build(),
        )
        .setup(|app| {
            let pfad = pfade::db_pfad().ok_or("Datenverzeichnis nicht bestimmbar")?;
            let conn = db::oeffnen(&pfad).map_err(|fehler| format!("{fehler:#}"))?;
            let konten = db::konten_liste(&conn).map_err(|fehler| format!("{fehler:#}"))?;
            app.manage(AppZustand {
                db: Mutex::new(conn),
                sync_laeuft: Mutex::new(HashSet::new()),
                idle_tasks: Mutex::new(std::collections::HashMap::new()),
                kalender_sync_laeuft: Mutex::new(false),
            });
            // Live-Update je Konto + periodischer Voll-Sync als Sicherheitsnetz.
            let handle = app.handle();
            for konto in konten {
                commands::idle_starten(handle, konto.id);
            }
            tauri::async_runtime::spawn(commands::periodischer_sync(handle.clone()));
            tauri::async_runtime::spawn(commands::periodische_termin_erinnerungen(handle.clone()));
            // Kalender beim Start einmal abgleichen (Fehler zeigt die UI
            // beim manuellen Abgleich — hier nur ins Protokoll).
            let kalender_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(fehler) = commands::kalender_sync_ausfuehren(&kalender_handle).await {
                    tracing::warn!("Kalender-Abgleich beim Start: {fehler}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            pfade::datei_standardpfad,
            thema::omarchy_thema,
            commands::konto_anlegen,
            commands::konto_bearbeiten,
            commands::konto_loeschen,
            commands::konten_liste,
            commands::ordner_liste,
            commands::sync_starten,
            commands::mails_liste,
            commands::mails_suchen,
            commands::mail_gelesen_setzen,
            commands::adress_vorschlaege,
            commands::mail_lesen,
            commands::mail_einladung_antworten,
            commands::mail_loeschen,
            commands::mail_bilder_laden,
            commands::mail_bild_quelle_erlauben,
            commands::anhang_speichern,
            commands::antwort_vorbereiten,
            commands::mail_senden,
            commands::entwurf_speichern,
            commands::entwurf_laden,
            commands::absender_avatar,
            commands::kalender_konto_anlegen,
            commands::kalender_konto_loeschen,
            commands::kalender_konten_liste,
            commands::kalender_liste,
            commands::kalender_farbe_setzen,
            commands::kalender_sichtbar_setzen,
            commands::kalender_termine,
            commands::kalender_termin_speichern,
            commands::kalender_termin_loeschen,
            commands::kalender_sync,
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

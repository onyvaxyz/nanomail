//! Logging in Datei + Terminal.
//!
//! Logs landen unter `~/.local/share/nanomail/logs/` (XDG-Datenverzeichnis),
//! eine Datei pro Tag. Bei Problemen kann diese Datei als Fehlerbericht
//! weitergegeben werden. Log-Level über die Umgebungsvariable `NANOMAIL_LOG`
//! steuerbar (Standard: `info`).
//!
//! Wichtig: Es dürfen niemals Passwörter, Tokens oder Mail-Inhalte
//! geloggt werden — nur technische Abläufe und Fehler.

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::pfade;

/// Initialisiert Logging. Der zurückgegebene Guard muss bis zum
/// Programmende leben, sonst gehen gepufferte Log-Zeilen verloren.
pub fn init() -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_env("NANOMAIL_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    let dateilog = pfade::log_verzeichnis().and_then(|verzeichnis| {
        std::fs::create_dir_all(&verzeichnis).ok()?;
        let appender = tracing_appender::rolling::daily(verzeichnis, "nanomail.log");
        Some(tracing_appender::non_blocking(appender))
    });

    match dateilog {
        Some((writer, guard)) => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_writer(std::io::stderr))
                .with(fmt::layer().with_writer(writer).with_ansi(false))
                .init();
            Some(guard)
        }
        None => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_writer(std::io::stderr))
                .init();
            None
        }
    }
}

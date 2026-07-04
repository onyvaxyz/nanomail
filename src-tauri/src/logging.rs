//! Logging in Datei + Terminal.
//!
//! Logs landen unter `~/.local/share/nanomail/logs/` (XDG-Datenverzeichnis),
//! eine Datei pro Tag. Bei Problemen kann diese Datei als Fehlerbericht
//! weitergegeben werden. Log-Level über die Umgebungsvariable `NANOMAIL_LOG`
//! steuerbar (Standard: `info`).
//!
//! Wichtig: Es dürfen niemals Passwörter, Tokens oder Mail-Inhalte
//! geloggt werden — nur technische Abläufe und Fehler.

use std::path::PathBuf;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Verzeichnis für Log-Dateien nach XDG-Standard.
pub fn log_verzeichnis() -> Option<PathBuf> {
    directories::ProjectDirs::from("io.github", "onyvaxyz", "nanomail")
        .map(|dirs| dirs.data_dir().join("logs"))
}

/// Initialisiert Logging. Der zurückgegebene Guard muss bis zum
/// Programmende leben, sonst gehen gepufferte Log-Zeilen verloren.
pub fn init() -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_env("NANOMAIL_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    let dateilog = log_verzeichnis().and_then(|verzeichnis| {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_verzeichnis_endet_auf_logs() {
        let verzeichnis = log_verzeichnis().expect("XDG-Verzeichnis auflösbar");
        assert!(verzeichnis.ends_with("logs"));
        assert!(verzeichnis.to_string_lossy().contains("nanomail"));
    }
}

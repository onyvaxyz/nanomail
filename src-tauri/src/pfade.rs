//! Zentrale Ablagepfade nach XDG-Standard (über den `directories`-Crate).
//!
//! Daten: `~/.local/share/nanomail/` — Konfiguration käme später unter
//! `~/.config/nanomail/`.

use std::path::PathBuf;

fn projekt_verzeichnisse() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("io.github", "onyvaxyz", "nanomail")
}

/// Verzeichnis für Log-Dateien.
pub fn log_verzeichnis() -> Option<PathBuf> {
    projekt_verzeichnisse().map(|d| d.data_dir().join("logs"))
}

/// Pfad der SQLite-Cache-Datenbank.
pub fn db_pfad() -> Option<PathBuf> {
    projekt_verzeichnisse().map(|d| d.data_dir().join("nanomail.db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pfade_liegen_im_nanomail_datenverzeichnis() {
        let logs = log_verzeichnis().expect("XDG-Verzeichnis auflösbar");
        assert!(logs.ends_with("logs"));
        assert!(logs.to_string_lossy().contains("nanomail"));
        let db = db_pfad().expect("XDG-Verzeichnis auflösbar");
        assert!(db.ends_with("nanomail.db"));
    }
}

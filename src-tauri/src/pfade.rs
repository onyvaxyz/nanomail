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

/// Plattformgerechter Desktop (unter Linux inklusive XDG-Benutzerordner).
/// Fehlende Desktop-Ordner fallen auf das Home-Verzeichnis zurück.
#[tauri::command]
pub fn datei_standardpfad(dateiname: Option<String>) -> Option<PathBuf> {
    let dirs = directories::UserDirs::new()?;
    let mut pfad = dirs
        .desktop_dir()
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| dirs.home_dir())
        .to_path_buf();
    if let Some(name) = dateiname {
        // MIME-Dateinamen sind nicht vertrauenswürdig, nie Pfade übernehmen.
        if let Some(name) = name
            .rsplit(['/', '\\'])
            .next()
            .filter(|n| !n.is_empty() && *n != "." && *n != "..")
        {
            pfad.push(name);
        }
    }
    Some(pfad)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dateidialog_bleibt_im_standardordner() {
        let basis = datei_standardpfad(None).unwrap();
        assert!(basis.is_dir());
        for name in ["../../brief.pdf", "C:\\privat\\brief.pdf", "brief.pdf"] {
            assert_eq!(
                datei_standardpfad(Some(name.into())).unwrap(),
                basis.join("brief.pdf")
            );
        }
        assert_eq!(datei_standardpfad(Some("..".into())).unwrap(), basis);
    }

    #[test]
    fn pfade_liegen_im_nanomail_datenverzeichnis() {
        let logs = log_verzeichnis().expect("XDG-Verzeichnis auflösbar");
        assert!(logs.ends_with("logs"));
        assert!(logs.to_string_lossy().contains("nanomail"));
        let db = db_pfad().expect("XDG-Verzeichnis auflösbar");
        assert!(db.ends_with("nanomail.db"));
    }
}

//! Lokale Datenhaltung mit SQLite/`rusqlite` (ab Meilenstein M1).
//!
//! Cache für Mails und Termine (Offline-Zugriff), Volltextsuche über FTS5,
//! Schema-Migrationen mit fortlaufender Versionsnummer.
//! Ablage nach XDG-Standard unter `~/.local/share/nanomail/`.

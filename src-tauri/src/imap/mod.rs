//! IMAP-Anbindung (ab Meilenstein M1).
//!
//! - `sync`: reine, unit-getestete Sync-Entscheidungslogik (kein Netzwerk)
//! - `parsen`: Kopfzeilen-Parsing mit `mail-parser` (kein Netzwerk)
//! - `verbindung`: dünne TLS/IMAP-Netzschicht auf `async-imap`
//!
//! Konventionen: siehe `.claude/skills/imap/SKILL.md`.

pub mod parsen;
pub mod sync;
pub mod verbindung;

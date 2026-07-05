//! Mail-Versand über SMTP mit `lettre` (ab Meilenstein M2).
//!
//! - `nachricht`: reiner, unit-getesteter Nachrichtenbau (Re:/Fwd:,
//!   Zitat, Threading-Header, Anhänge)
//! - `versand`: dünne TLS-Versandschicht (465 implizit, sonst STARTTLS)
//!
//! Die Ablage im „Gesendet“-Ordner läuft über IMAP APPEND (`imap/`).

pub mod nachricht;
pub mod versand;

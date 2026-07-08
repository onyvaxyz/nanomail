//! CalDAV-Anbindung an Nextcloud (ab Meilenstein M4).
//!
//! Eigene XML-Schicht auf `reqwest`-Basis: Calendar-Discovery (PROPFIND),
//! Abgleich (`sync-collection` REPORT mit Sync-Token), ICS-Parsing mit
//! `icalendar`, Wiederholungstermine (RRULE) und Zeitzonen.
//!
//! Konventionen: siehe `.claude/skills/caldav/SKILL.md`.

pub mod termine;
pub mod verbindung;
pub mod xml;

//! Hilfslogik für das Live-Update (IMAP IDLE) — reine Funktionen.
//!
//! Der eigentliche IDLE-Ablauf (Verbindung, Warten, Sync) liegt in
//! `commands.rs`; hier steht nur die getestete Wiederverbindungs-Logik.

use std::time::Duration;

/// Startabstand nach einem Verbindungsabriss.
pub const BACKOFF_START: Duration = Duration::from_secs(5);
/// Obergrenze, damit die App nie länger als eine Minute wegbleibt.
pub const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// IDLE spätestens nach 25 Minuten neu aufsetzen — viele Server (und
/// NAT-Router) trennen stille Verbindungen nach ~30 Minuten (RFC 2177).
pub const IDLE_RUNDE: Duration = Duration::from_secs(25 * 60);

/// Nächster Wiederverbindungs-Abstand: verdoppeln bis zur Obergrenze.
pub fn naechster_backoff(bisher: Duration) -> Duration {
    (bisher * 2).min(BACKOFF_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_verdoppelt_bis_zur_obergrenze() {
        let mut abstand = BACKOFF_START;
        let mut folge = Vec::new();
        for _ in 0..6 {
            folge.push(abstand.as_secs());
            abstand = naechster_backoff(abstand);
        }
        assert_eq!(folge, vec![5, 10, 20, 40, 60, 60]);
    }
}

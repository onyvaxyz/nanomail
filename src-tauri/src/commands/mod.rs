//! Tauri-Commands — die einzige Schnittstelle zum Frontend.
//!
//! Namensschema `bereich_aktion` (siehe `.claude/skills/frontend/SKILL.md`).
//! Fehler verlassen diese Schicht ausschließlich als verständliche
//! deutsche Meldung; die technischen Details landen im Log.

//! Geteilte Grundlagen: Zustand, Fehler-Meldungen, Helfer.
//! Die Bereiche liegen in Untermodulen und werden hier re-exportiert,
//! damit `commands::name` von aussen stabil bleibt.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db::{self, Konto};
use crate::imap::verbindung::ImapVerbindung;
use crate::oauth::{self, TokenSatz};
use crate::schluesselbund;
use crate::smtp::versand;

/// Kopfzeilen-Batchgröße beim Sync — klein genug, dass die UI früh
/// etwas anzeigen kann.
pub(crate) const KOEPFE_BATCH: usize = 200;
/// Obergrenzen für „Bilder laden“.
pub(crate) const MAX_BILDER: usize = 30;
pub(crate) const MAX_BILD_BYTES: usize = 10 * 1024 * 1024;
/// Fester Erinnerungszeitpunkt vor Kalenderterminen.
pub(crate) const TERMIN_ERINNERUNG_SEKUNDEN: i64 = 30 * 60;

pub struct AppZustand {
    pub db: Mutex<rusqlite::Connection>,
    /// Konten, für die gerade ein Sync läuft (verhindert Doppel-Syncs).
    pub sync_laeuft: Mutex<HashSet<i64>>,
    /// Live-Update-Hintergrundtasks (IMAP IDLE), einer je Konto.
    pub idle_tasks: Mutex<HashMap<i64, tauri::async_runtime::JoinHandle<()>>>,
    /// Läuft gerade ein Kalender-Abgleich? (verhindert Doppel-Syncs)
    pub kalender_sync_laeuft: Mutex<bool>,
    /// Laufende Microsoft-Anmeldungen (M6): Geräte-Code plus fertige
    /// Tokens, bis das Konto angelegt/erneuert ist. Nur im Speicher —
    /// nie auf Platte; verwaiste Einträge verfallen nach 15 Minuten.
    pub ms_anmeldungen: Mutex<HashMap<String, MsAnmeldung>>,
}

/// Eine begonnene Microsoft-Anmeldung (Device-Code-Flow, M6).
pub struct MsAnmeldung {
    pub geraete_code: String,
    pub tokens: Option<TokenSatz>,
    pub begonnen: std::time::Instant,
}

// ---------------------------------------------------------------- Hilfen --

/// Kurzer, synchroner Datenbank-Zugriff (Guard nie über ein `await` halten).
pub(crate) fn mit_db<T>(
    zustand: &AppZustand,
    aktion: impl FnOnce(&rusqlite::Connection) -> Result<T>,
) -> Result<T> {
    let conn = zustand
        .db
        .lock()
        .map_err(|_| anyhow!("Interner Datenbank-Zugriffsfehler"))?;
    aktion(&conn)
}

/// Markiert eine Meldung als „direkt für den Nutzer bestimmt“ —
/// `als_meldung` reicht sie unverändert durch.
pub(crate) fn nutzerfehler(text: impl std::fmt::Display) -> anyhow::Error {
    anyhow!("NUTZERFEHLER:{text}")
}

/// Übersetzt technische Fehler in eine verständliche deutsche Meldung
/// und protokolliert die Details.
pub(crate) fn als_meldung(fehler: &anyhow::Error) -> String {
    let kette = format!("{fehler:#}");
    tracing::error!("{kette}");
    if let Some(pos) = kette.find("NUTZERFEHLER:") {
        return kette[pos + "NUTZERFEHLER:".len()..].trim().to_string();
    }
    if kette.contains("close_notify")
        || kette.contains("unexpected EOF")
        || kette.contains("peer closed")
        || kette.contains("Connection reset")
    {
        return "Der Server hat die Verbindung unerwartet getrennt. Das passiert oft bei \
                einer vorübergehenden Sperre nach mehreren fehlgeschlagenen \
                Anmeldeversuchen — bitte 30–60 Minuten warten und dann erneut versuchen. \
                Prüfe auch Sicherheitswarnungen im Konto deines Anbieters."
            .into();
    }
    if let Some(pos) = kette.find("Anmeldung abgelehnt:") {
        // Die konkrete Serverantwort hilft bei der Diagnose (enthält nie
        // Zugangsdaten — der Server nennt nur den Ablehnungsgrund).
        let detail: String = kette[pos + "Anmeldung abgelehnt:".len()..]
            .trim()
            .chars()
            .take(160)
            .collect();
        format!(
            "Anmeldung fehlgeschlagen. Bitte prüfen: Benutzername muss meist die \
             vollständige E-Mail-Adresse sein; bei aktivierter Zwei-Faktor-Anmeldung \
             ist ein App-Passwort zwingend. Serverantwort: „{detail}“"
        )
    } else if kette.contains("nicht erreichbar") || kette.contains("TLS-Verbindung") {
        "Server nicht erreichbar — bitte Serveradresse, Port und Internetverbindung prüfen.".into()
    } else if kette.contains("Schlüsselbund") {
        "Zugriff auf den Schlüsselbund fehlgeschlagen — das Passwort konnte nicht sicher gespeichert/gelesen werden.".into()
    } else {
        "Es ist ein Fehler aufgetreten. Details stehen im Protokoll unter ~/.local/share/nanomail/logs/.".into()
    }
}

/// Schlüsselbund-Zugriffe blockieren intern (zbus) und dürfen deshalb nie
/// direkt auf dem Async-Runtime-Thread laufen — sonst Deadlock.
pub(crate) async fn passwort_holen(konto_id: i64) -> Result<String> {
    tauri::async_runtime::spawn_blocking(move || schluesselbund::passwort_holen(konto_id))
        .await
        .context("Schlüsselbund-Task abgebrochen")?
}

pub(crate) async fn passwort_speichern(konto_id: i64, passwort: String) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        schluesselbund::passwort_speichern(konto_id, &passwort)
    })
    .await
    .context("Schlüsselbund-Task abgebrochen")?
}

pub(crate) async fn verbindung_zum_konto(konto: &Konto) -> Result<ImapVerbindung> {
    if konto.auth_art == "microsoft" {
        let token = microsoft_zugang_token(konto.id).await?;
        return ImapVerbindung::verbinden_mit_token(
            &konto.imap_host,
            konto.imap_port,
            &konto.benutzer,
            &token,
        )
        .await
        .map_err(|fehler| {
            nutzerfehler(format!(
                "Microsoft-Anmeldung fehlgeschlagen ({fehler:#}) — hilft das öfter, \
                 bitte das Konto einmalig erneut verbinden (Konto bearbeiten)."
            ))
        });
    }
    let passwort = passwort_holen(konto.id).await?;
    ImapVerbindung::verbinden(
        &konto.imap_host,
        konto.imap_port,
        &konto.benutzer,
        &passwort,
    )
    .await
}

/// Versendet über das Konto — Passwort- und Microsoft-Konten (M6)
/// teilen sich diesen einen Einstieg.
pub(crate) async fn smtp_senden(konto: &Konto, nachricht: lettre::Message) -> Result<()> {
    if konto.auth_art == "microsoft" {
        let token = microsoft_zugang_token(konto.id).await?;
        return versand::senden_mit_token(
            &konto.smtp_host,
            konto.smtp_port,
            &konto.benutzer,
            &token,
            nachricht,
        )
        .await
        .map_err(|fehler| {
            nutzerfehler(format!(
                "Microsoft hat den Versand abgelehnt ({fehler:#}) — hilft das öfter, \
                 bitte das Konto einmalig erneut verbinden (Konto bearbeiten)."
            ))
        });
    }
    let passwort = passwort_holen(konto.id).await?;
    versand::senden(
        &konto.smtp_host,
        konto.smtp_port,
        &konto.benutzer,
        &passwort,
        nachricht,
    )
    .await
}

/// Liefert ein gültiges Microsoft-Zugangs-Token für das Konto:
/// aus dem Schlüsselbund, bei Bedarf vorher aufgefrischt (M6).
/// Das Token steht nie im Log.
pub(crate) async fn microsoft_zugang_token(konto_id: i64) -> Result<String> {
    let json = tauri::async_runtime::spawn_blocking(move || {
        schluesselbund::microsoft_token_holen(konto_id)
    })
    .await
    .context("Schlüsselbund-Task abgebrochen")?
    .map_err(|fehler| nutzerfehler(format!("{fehler:#}")))?;
    let mut satz: TokenSatz = serde_json::from_str(&json)
        .context("Microsoft-Token lesen")
        .map_err(|fehler| nutzerfehler(format!("{fehler:#}")))?;
    if !satz.braucht_auffrischung(jetzt_unix()) {
        return Ok(satz.zugang_token);
    }
    let http = reqwest::Client::new();
    let neu = oauth::token_auffrischen(&http, &satz.auffrisch_token, jetzt_unix())
        .await
        .map_err(|fehler| nutzerfehler(format!("{fehler:#}")))?;
    // Microsoft liefert nicht immer ein neues Auffrisch-Token mit —
    // dann gilt das bisherige weiter.
    satz.zugang_token = neu.zugang_token;
    satz.ablauf_unix = neu.ablauf_unix;
    if !neu.auffrisch_token.is_empty() {
        satz.auffrisch_token = neu.auffrisch_token;
    }
    let json = serde_json::to_string(&satz).context("Microsoft-Token ablegen")?;
    tauri::async_runtime::spawn_blocking(move || {
        schluesselbund::microsoft_token_speichern(konto_id, &json)
    })
    .await
    .context("Schlüsselbund-Task abgebrochen")?
    .map_err(|fehler| nutzerfehler(format!("{fehler:#}")))?;
    Ok(satz.zugang_token)
}

/// Unix-Sekunden (für Token-Ablaufvergleiche).
pub(crate) fn jetzt_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn konto_laden(zustand: &AppZustand, konto_id: i64) -> Result<Konto> {
    mit_db(zustand, |conn| db::konto_holen(conn, konto_id))?
        .ok_or_else(|| anyhow!("Konto {konto_id} ist nicht (mehr) vorhanden"))
}

// -------------------------------------------------------------- Ereignisse --

#[derive(Serialize, Clone)]
pub(crate) struct SyncStatus {
    konto_id: i64,
    status: &'static str, // "laeuft" | "fertig" | "fehler"
    meldung: Option<String>,
}

#[derive(Serialize, Clone)]
pub(crate) struct MailsNeu {
    ordner_id: i64,
}

pub(crate) fn melde_sync(
    app: &AppHandle,
    konto_id: i64,
    status: &'static str,
    meldung: Option<String>,
) {
    let _ = app.emit(
        "sync:status",
        SyncStatus {
            konto_id,
            status,
            meldung,
        },
    );
}

mod avatar;
mod kalender;
mod konten;
mod mails;
mod senden;
mod sync;

pub use avatar::*;
pub use kalender::*;
pub use konten::*;
pub use mails::*;
pub use senden::*;
pub use sync::*;

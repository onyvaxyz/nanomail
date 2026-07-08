//! Tauri-Commands — die einzige Schnittstelle zum Frontend.
//!
//! Namensschema `bereich_aktion` (siehe `.claude/skills/frontend/SKILL.md`).
//! Fehler verlassen diese Schicht ausschließlich als verständliche
//! deutsche Meldung; die technischen Details landen im Log.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::caldav::verbindung::{CaldavVerbindung, SyncAntwort};
use crate::caldav::{termine, xml};
use crate::db::{self, kalender as db_kalender, Konto, MailKopf, NeuerMailKopf, Ordner};
use crate::imap::verbindung::ImapVerbindung;
use crate::imap::{idle, parsen, sync};
use crate::smtp::{nachricht, versand};
use crate::{anzeige, avatar, schluesselbund};

/// Kopfzeilen-Batchgröße beim Sync — klein genug, dass die UI früh
/// etwas anzeigen kann.
const KOEPFE_BATCH: usize = 200;
/// Obergrenzen für „Bilder laden“.
const MAX_BILDER: usize = 30;
const MAX_BILD_BYTES: usize = 10 * 1024 * 1024;

pub struct AppZustand {
    pub db: Mutex<rusqlite::Connection>,
    /// Konten, für die gerade ein Sync läuft (verhindert Doppel-Syncs).
    pub sync_laeuft: Mutex<HashSet<i64>>,
    /// Live-Update-Hintergrundtasks (IMAP IDLE), einer je Konto.
    pub idle_tasks: Mutex<HashMap<i64, tauri::async_runtime::JoinHandle<()>>>,
    /// Läuft gerade ein Kalender-Abgleich? (verhindert Doppel-Syncs)
    pub kalender_sync_laeuft: Mutex<bool>,
}

// ---------------------------------------------------------------- Hilfen --

/// Kurzer, synchroner Datenbank-Zugriff (Guard nie über ein `await` halten).
fn mit_db<T>(
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
fn nutzerfehler(text: impl std::fmt::Display) -> anyhow::Error {
    anyhow!("NUTZERFEHLER:{text}")
}

/// Übersetzt technische Fehler in eine verständliche deutsche Meldung
/// und protokolliert die Details.
fn als_meldung(fehler: &anyhow::Error) -> String {
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
async fn passwort_holen(konto_id: i64) -> Result<String> {
    tauri::async_runtime::spawn_blocking(move || schluesselbund::passwort_holen(konto_id))
        .await
        .context("Schlüsselbund-Task abgebrochen")?
}

async fn passwort_speichern(konto_id: i64, passwort: String) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        schluesselbund::passwort_speichern(konto_id, &passwort)
    })
    .await
    .context("Schlüsselbund-Task abgebrochen")?
}

async fn verbindung_zum_konto(konto: &Konto) -> Result<ImapVerbindung> {
    let passwort = passwort_holen(konto.id).await?;
    ImapVerbindung::verbinden(
        &konto.imap_host,
        konto.imap_port,
        &konto.benutzer,
        &passwort,
    )
    .await
}

fn konto_laden(zustand: &AppZustand, konto_id: i64) -> Result<Konto> {
    mit_db(zustand, |conn| db::konto_holen(conn, konto_id))?
        .ok_or_else(|| anyhow!("Konto {konto_id} ist nicht (mehr) vorhanden"))
}

// -------------------------------------------------------------- Ereignisse --

#[derive(Serialize, Clone)]
struct SyncStatus {
    konto_id: i64,
    status: &'static str, // "laeuft" | "fertig" | "fehler"
    meldung: Option<String>,
}

#[derive(Serialize, Clone)]
struct MailsNeu {
    ordner_id: i64,
}

fn melde_sync(app: &AppHandle, konto_id: i64, status: &'static str, meldung: Option<String>) {
    let _ = app.emit(
        "sync:status",
        SyncStatus {
            konto_id,
            status,
            meldung,
        },
    );
}

// ---------------------------------------------------------------- Konten --

/// Eingaben des Konto-Dialogs (Anlegen und Bearbeiten).
#[derive(serde::Deserialize)]
pub struct KontoFormular {
    pub name: String,
    pub email: String,
    pub benutzer: String,
    /// Beim Bearbeiten leer lassen = Passwort unverändert.
    pub passwort: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    /// Signatur (leer = keine).
    #[serde(default)]
    pub signatur: String,
    /// Akzentfarbe als Hex-Wert (leer = Standard).
    #[serde(default)]
    pub farbe: String,
}

impl KontoFormular {
    fn bereinigt(mut self) -> Result<Self> {
        self.name = self.name.trim().to_string();
        self.email = self.email.trim().to_string();
        self.benutzer = self.benutzer.trim().to_string();
        self.imap_host = self.imap_host.trim().to_string();
        self.smtp_host = self.smtp_host.trim().to_string();
        self.farbe = self.farbe.trim().to_lowercase();
        // Nur echte Hex-Farben übernehmen — alles andere fällt auf Standard.
        if !(self.farbe.len() == 7
            && self.farbe.starts_with('#')
            && self.farbe[1..].chars().all(|z| z.is_ascii_hexdigit()))
        {
            self.farbe = String::new();
        }
        if self.name.is_empty()
            || self.benutzer.is_empty()
            || self.imap_host.is_empty()
            || self.smtp_host.is_empty()
        {
            anyhow::bail!("Anmeldung abgelehnt: Pflichtfelder fehlen");
        }
        Ok(self)
    }

    fn als_daten(&self) -> db::KontoDaten {
        db::KontoDaten {
            name: self.name.clone(),
            email: self.email.clone(),
            imap_host: self.imap_host.clone(),
            imap_port: self.imap_port,
            benutzer: self.benutzer.clone(),
            smtp_host: self.smtp_host.clone(),
            smtp_port: self.smtp_port,
            signatur: self.signatur.clone(),
            farbe: self.farbe.clone(),
        }
    }
}

/// Prüft IMAP- und SMTP-Zugangsdaten, bevor irgendetwas gespeichert wird.
async fn zugangsdaten_pruefen(formular: &KontoFormular, passwort: &str) -> Result<()> {
    let probe = ImapVerbindung::verbinden(
        &formular.imap_host,
        formular.imap_port,
        &formular.benutzer,
        passwort,
    )
    .await?;
    probe.abmelden().await;
    crate::smtp::versand::probe(
        &formular.smtp_host,
        formular.smtp_port,
        &formular.benutzer,
        passwort,
    )
    .await
}

#[tauri::command]
pub async fn konto_anlegen(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    formular: KontoFormular,
) -> Result<Konto, String> {
    let konto = konto_anlegen_intern(&zustand, formular)
        .await
        .map_err(|f| als_meldung(&f))?;
    idle_starten(&app, konto.id);
    Ok(konto)
}

async fn konto_anlegen_intern(zustand: &AppZustand, formular: KontoFormular) -> Result<Konto> {
    let formular = formular.bereinigt()?;
    if formular.passwort.is_empty() {
        anyhow::bail!("Anmeldung abgelehnt: Passwort fehlt");
    }
    zugangsdaten_pruefen(&formular, &formular.passwort).await?;

    let konto = mit_db(zustand, |conn| {
        db::konto_anlegen(conn, &formular.als_daten())
    })?;

    if let Err(fehler) = passwort_speichern(konto.id, formular.passwort).await {
        // Ohne Passwort im Schlüsselbund ist das Konto nutzlos → zurückrollen.
        let _ = mit_db(zustand, |conn| {
            conn.execute("DELETE FROM konten WHERE id = ?1", [konto.id])
                .context("Konto zurückrollen")?;
            Ok(())
        });
        return Err(fehler);
    }
    tracing::info!(konto_id = konto.id, "Konto angelegt");
    Ok(konto)
}

#[tauri::command]
pub async fn konto_bearbeiten(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    konto_id: i64,
    formular: KontoFormular,
) -> Result<Konto, String> {
    let konto = konto_bearbeiten_intern(&zustand, konto_id, formular)
        .await
        .map_err(|f| als_meldung(&f))?;
    // Zugangsdaten/Server können sich geändert haben → Live-Update neu aufsetzen.
    idle_starten(&app, konto_id);
    Ok(konto)
}

#[tauri::command]
pub async fn konto_loeschen(zustand: State<'_, AppZustand>, konto_id: i64) -> Result<(), String> {
    idle_stoppen(&zustand, konto_id);
    mit_db(&zustand, |conn| {
        conn.execute("DELETE FROM konten WHERE id = ?1", [konto_id])
            .context("Konto löschen")?;
        Ok(())
    })
    .map_err(|f| als_meldung(&f))?;
    // Keyring-Eintrag entfernen (blockiert intern → eigener Thread).
    tauri::async_runtime::spawn_blocking(move || schluesselbund::passwort_loeschen(konto_id))
        .await
        .map_err(|_| "Interner Fehler beim Aufräumen des Schlüsselbunds".to_string())?
        .map_err(|f| als_meldung(&f))?;
    tracing::info!(konto_id, "Konto entfernt");
    Ok(())
}

async fn konto_bearbeiten_intern(
    zustand: &AppZustand,
    konto_id: i64,
    formular: KontoFormular,
) -> Result<Konto> {
    let formular = formular.bereinigt()?;
    konto_laden(zustand, konto_id)?; // muss existieren

    // Leeres Passwort = bestehendes weiterverwenden.
    let passwort = if formular.passwort.is_empty() {
        passwort_holen(konto_id).await?
    } else {
        formular.passwort.clone()
    };
    zugangsdaten_pruefen(&formular, &passwort).await?;

    mit_db(zustand, |conn| {
        db::konto_aktualisieren(conn, konto_id, &formular.als_daten())
    })?;
    if !formular.passwort.is_empty() {
        passwort_speichern(konto_id, formular.passwort).await?;
    }
    tracing::info!(konto_id, "Konto aktualisiert");
    konto_laden(zustand, konto_id)
}

#[tauri::command]
pub fn konten_liste(zustand: State<'_, AppZustand>) -> Result<Vec<Konto>, String> {
    mit_db(&zustand, db::konten_liste).map_err(|f| als_meldung(&f))
}

// ---------------------------------------------------------------- Ordner --

#[tauri::command]
pub fn ordner_liste(zustand: State<'_, AppZustand>, konto_id: i64) -> Result<Vec<Ordner>, String> {
    mit_db(&zustand, |conn| db::ordner_liste(conn, konto_id)).map_err(|f| als_meldung(&f))
}

// ------------------------------------------------------------------ Sync --

#[tauri::command]
pub async fn sync_starten(app: AppHandle, konto_id: i64) -> Result<(), String> {
    sync_ausfuehren(&app, konto_id).await
}

/// Kompletter Konto-Sync mit Doppelstart-Schutz und Status-Events.
/// Wird vom Command, vom periodischen Sync und vom Live-Update genutzt.
async fn sync_ausfuehren(app: &AppHandle, konto_id: i64) -> Result<(), String> {
    let zustand = app.state::<AppZustand>();
    // Doppel-Sync fürs selbe Konto verhindern.
    {
        let mut laufend = zustand
            .sync_laeuft
            .lock()
            .map_err(|_| "Interner Fehler beim Sync-Start".to_string())?;
        if !laufend.insert(konto_id) {
            return Ok(()); // läuft bereits — kein Fehler
        }
    }
    melde_sync(app, konto_id, "laeuft", None);

    let ergebnis = konto_synchronisieren(app, &zustand, konto_id).await;

    if let Ok(mut laufend) = zustand.sync_laeuft.lock() {
        laufend.remove(&konto_id);
    }
    match ergebnis {
        Ok(()) => {
            melde_sync(app, konto_id, "fertig", None);
            Ok(())
        }
        Err(fehler) => {
            let meldung = als_meldung(&fehler);
            melde_sync(app, konto_id, "fehler", Some(meldung.clone()));
            Err(meldung)
        }
    }
}

/// Sicherheitsnetz: alle Konten regelmäßig voll abgleichen (das
/// Live-Update lauscht nur auf dem Posteingang).
pub async fn periodischer_sync(app: AppHandle) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        let konten = {
            let zustand = app.state::<AppZustand>();
            mit_db(&zustand, db::konten_liste).unwrap_or_default()
        };
        for konto in konten {
            let _ = sync_ausfuehren(&app, konto.id).await;
        }
        // Kalender hängen am selben Sicherheitsnetz (das Sync-Token macht
        // den Abgleich billig, wenn sich nichts geändert hat).
        let _ = kalender_sync_ausfuehren(&app).await;
    }
}

// ------------------------------------------------------- Live-Update --

/// Startet (bzw. ersetzt) den Live-Update-Task eines Kontos.
pub fn idle_starten(app: &AppHandle, konto_id: i64) {
    let app_im_task = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let mut backoff = idle::BACKOFF_START;
        loop {
            let begonnen = std::time::Instant::now();
            if let Err(fehler) = idle_runde(&app_im_task, konto_id).await {
                let existiert = {
                    let zustand = app_im_task.state::<AppZustand>();
                    mit_db(&zustand, |conn| db::konto_holen(conn, konto_id))
                        .ok()
                        .flatten()
                        .is_some()
                };
                if !existiert {
                    tracing::info!(konto_id, "Live-Update beendet (Konto entfernt)");
                    return;
                }
                tracing::warn!(konto_id, "Live-Update unterbrochen: {fehler:#}");
            }
            // Lief die Runde eine Weile stabil, fängt der Backoff von vorn an.
            if begonnen.elapsed() > std::time::Duration::from_secs(120) {
                backoff = idle::BACKOFF_START;
            }
            tokio::time::sleep(backoff).await;
            backoff = idle::naechster_backoff(backoff);
        }
    });
    let zustand = app.state::<AppZustand>();
    if let Ok(mut tasks) = zustand.idle_tasks.lock() {
        if let Some(alter_task) = tasks.insert(konto_id, task) {
            alter_task.abort();
        }
    };
}

fn idle_stoppen(zustand: &AppZustand, konto_id: i64) {
    if let Ok(mut tasks) = zustand.idle_tasks.lock() {
        if let Some(task) = tasks.remove(&konto_id) {
            task.abort();
        }
    }
}

/// Eine IDLE-Runde: Posteingang abgleichen, dann auf Server-Meldungen
/// lauschen; wiederholt sich, bis die Verbindung abreißt (→ `Err`).
async fn idle_runde(app: &AppHandle, konto_id: i64) -> Result<()> {
    let zustand = app.state::<AppZustand>();
    let konto = konto_laden(&zustand, konto_id)?;
    let mut verbindung = verbindung_zum_konto(&konto).await?;
    tracing::info!(konto_id, "Live-Update verbunden");
    loop {
        let inbox = mit_db(&zustand, |conn| db::ordner_liste(conn, konto_id))?
            .into_iter()
            .find(|ordner| ordner.name.eq_ignore_ascii_case("INBOX"))
            .ok_or_else(|| anyhow!("Posteingang noch nicht bekannt — warte auf ersten Sync"))?;
        ordner_synchronisieren(app, &zustand, &mut verbindung, &inbox).await?;
        let (naechste, _neuigkeiten) = verbindung.warte_auf_neuigkeiten(idle::IDLE_RUNDE).await?;
        verbindung = naechste;
    }
}

async fn konto_synchronisieren(app: &AppHandle, zustand: &AppZustand, konto_id: i64) -> Result<()> {
    let konto = konto_laden(zustand, konto_id)?;
    let mut verbindung = verbindung_zum_konto(&konto).await?;

    // 1) Ordnerliste abgleichen
    tracing::debug!(konto_id, "Sync: Ordnerliste anfragen");
    let server_ordner = verbindung.ordner_auflisten().await?;
    tracing::debug!(anzahl = server_ordner.len(), "Sync: Ordnerliste erhalten");
    let ordner_liste = mit_db(zustand, |conn| {
        let namen: Vec<String> = server_ordner.iter().map(|o| o.name.clone()).collect();
        db::ordner_bereinigen(conn, konto_id, &namen)?;
        for eintrag in &server_ordner {
            db::ordner_upsert(
                conn,
                konto_id,
                &eintrag.name,
                &eintrag.anzeige_name,
                eintrag.rolle.as_deref(),
            )?;
        }
        db::ordner_liste(conn, konto_id)
    })?;
    let _ = app.emit("ordner:aktualisiert", konto_id);

    // 2) Jeden Ordner abgleichen
    for ordner in ordner_liste {
        if let Err(fehler) = ordner_synchronisieren(app, zustand, &mut verbindung, &ordner).await {
            // Ein kaputter Ordner bricht nicht den ganzen Sync ab.
            tracing::error!(ordner = %ordner.name, "Ordner-Sync fehlgeschlagen: {fehler:#}");
        }
    }

    verbindung.abmelden().await;
    Ok(())
}

async fn ordner_synchronisieren(
    app: &AppHandle,
    zustand: &AppZustand,
    verbindung: &mut ImapVerbindung,
    ordner: &Ordner,
) -> Result<()> {
    tracing::debug!(ordner = %ordner.name, "Sync: Ordner wird abgeglichen");
    let status = verbindung.ordner_waehlen(&ordner.name).await?;

    // UIDVALIDITY-Regel anwenden (reine, getestete Logik).
    if sync::braucht_cache_reset(ordner.uidvalidity, status.uidvalidity) {
        tracing::info!(ordner = %ordner.name, "UIDVALIDITY geändert — Cache wird verworfen");
        mit_db(zustand, |conn| {
            db::ordner_cache_verwerfen(conn, ordner.id, status.uidvalidity)
        })?;
    } else if ordner.uidvalidity.is_none() {
        mit_db(zustand, |conn| {
            db::ordner_setze_uidvalidity(conn, ordner.id, status.uidvalidity)
        })?;
    }

    // Server- und Cache-Stand vergleichen.
    let server_stand = verbindung.uid_stand(status.anzahl).await?;
    let cache_stand = mit_db(zustand, |conn| db::mails_cache_stand(conn, ordner.id))?;
    let abgleich = sync::vergleiche_ordner(&server_stand, &cache_stand);

    if !abgleich.geloeschte.is_empty() || !abgleich.flag_aenderungen.is_empty() {
        mit_db(zustand, |conn| {
            db::mails_loeschen(conn, ordner.id, &abgleich.geloeschte)?;
            db::mails_flags_setzen(conn, ordner.id, &abgleich.flag_aenderungen)
        })?;
        let _ = app.emit(
            "mails:neu",
            MailsNeu {
                ordner_id: ordner.id,
            },
        );
    }

    // Neue Mails batchweise holen — neueste zuerst, damit früh etwas sichtbar ist.
    for batch in sync::batches(&abgleich.neue, KOEPFE_BATCH) {
        let koepfe = verbindung.koepfe_laden(&batch).await?;
        mit_db(zustand, |conn| {
            let neue: Vec<NeuerMailKopf> = koepfe
                .iter()
                .map(|kopf| {
                    let geparst = parsen::parse_kopf(&kopf.header);
                    NeuerMailKopf {
                        uid: kopf.uid,
                        betreff: geparst.betreff,
                        von: geparst.von,
                        von_email: geparst.von_email,
                        datum: geparst.datum,
                        gelesen: kopf.gelesen,
                        beantwortet: kopf.beantwortet,
                        hat_anhang: kopf.hat_anhang,
                    }
                })
                .collect();
            db::mails_einfuegen(conn, ordner.id, &neue)
        })?;
        let _ = app.emit(
            "mails:neu",
            MailsNeu {
                ordner_id: ordner.id,
            },
        );
    }
    Ok(())
}

// ----------------------------------------------------------------- Mails --

#[tauri::command]
pub fn mails_liste(
    zustand: State<'_, AppZustand>,
    ordner_id: i64,
    nur_ungelesen: bool,
    offset: i64,
    limit: i64,
) -> Result<Vec<MailKopf>, String> {
    mit_db(&zustand, |conn| {
        db::mails_liste(
            conn,
            ordner_id,
            nur_ungelesen,
            offset.max(0),
            limit.clamp(1, 500),
        )
    })
    .map_err(|f| als_meldung(&f))
}

/// Setzt das Gelesen-Flag einer Mail in beide Richtungen (Kontextmenü
/// „Als (un)gelesen markieren“). Der Cache wird sofort geändert, das
/// Server-Flag nebenläufig — scheitert das (z. B. offline), korrigiert
/// es der nächste Sync.
#[tauri::command]
pub fn mail_gelesen_setzen(
    zustand: State<'_, AppZustand>,
    mail_id: i64,
    gelesen: bool,
) -> Result<(), String> {
    mail_gelesen_setzen_intern(&zustand, mail_id, gelesen).map_err(|f| als_meldung(&f))
}

fn mail_gelesen_setzen_intern(zustand: &AppZustand, mail_id: i64, gelesen: bool) -> Result<()> {
    let (mail, ordner, konto) = mail_kontext(zustand, mail_id)?;
    mit_db(zustand, |conn| {
        db::mail_gelesen_setzen(conn, mail_id, gelesen)
    })?;

    let uid = mail.uid;
    let ordner_name = ordner.name.clone();
    tauri::async_runtime::spawn(async move {
        match verbindung_zum_konto(&konto).await {
            Ok(mut verbindung) => {
                if verbindung.ordner_waehlen(&ordner_name).await.is_ok() {
                    let ergebnis = if gelesen {
                        verbindung.als_gelesen_markieren(uid).await
                    } else {
                        verbindung.als_ungelesen_markieren(uid).await
                    };
                    if let Err(fehler) = ergebnis {
                        tracing::warn!("Gelesen-Flag nicht übertragen: {fehler:#}");
                    }
                }
                verbindung.abmelden().await;
            }
            Err(fehler) => {
                tracing::warn!("Gelesen-Flag nicht übertragen: {fehler:#}");
            }
        }
    });
    Ok(())
}

/// Volltextsuche über alle Ordner des Kontos (Betreff, Absender und —
/// soweit lokal im Cache — Mailtext). Neueste Treffer zuerst.
#[tauri::command]
pub fn mails_suchen(
    zustand: State<'_, AppZustand>,
    konto_id: i64,
    eingabe: String,
) -> Result<Vec<db::SuchTreffer>, String> {
    mit_db(&zustand, |conn| {
        db::mails_suchen(conn, konto_id, &eingabe, 100)
    })
    .map_err(|f| als_meldung(&f))
}

/// Vorschläge fürs Empfänger-Feld beim Verfassen (bekannte Empfänger
/// plus Absender aus dem Mail-Cache).
#[tauri::command]
pub fn adress_vorschlaege(
    zustand: State<'_, AppZustand>,
    eingabe: String,
) -> Result<Vec<db::AdressVorschlag>, String> {
    mit_db(&zustand, |conn| db::adress_vorschlaege(conn, &eingabe, 8)).map_err(|f| als_meldung(&f))
}

/// Anzeigefertige Mail für den Lesebereich.
#[derive(Serialize)]
pub struct MailAnsicht {
    pub kopf: MailKopf,
    pub text: String,
    /// Bereinigtes HTML — das Frontend zeigt es nur im Sandbox-iframe an.
    pub html: Option<String>,
    /// Zusätzlich vom Absender-Design befreites HTML für die
    /// Standard-Ansicht im App-Stil (M3.5).
    pub html_schlicht: Option<String>,
    pub hatte_externe_bilder: bool,
    /// Anhänge für die Anhang-Leiste (M3.6).
    pub anhaenge: Vec<db::AnhangEintrag>,
}

#[tauri::command]
pub async fn mail_lesen(
    zustand: State<'_, AppZustand>,
    mail_id: i64,
) -> Result<MailAnsicht, String> {
    mail_lesen_intern(&zustand, mail_id)
        .await
        .map_err(|f| als_meldung(&f))
}

async fn mail_lesen_intern(zustand: &AppZustand, mail_id: i64) -> Result<MailAnsicht> {
    let (mail, ordner, konto) = mail_kontext(zustand, mail_id)?;

    let (inhalt, hat_anhang, server_flag_gesetzt) =
        match mit_db(zustand, |conn| db::inhalt_holen(conn, mail_id))? {
            Some(inhalt) => (inhalt, mail.hat_anhang, false),
            None => {
                // Body fehlt im Cache → vom Server nachladen (lazy).
                let mut verbindung = verbindung_zum_konto(&konto).await?;
                verbindung.ordner_waehlen(&ordner.name).await?;
                let roh = verbindung.nachricht_laden(mail.uid).await?;
                if !mail.gelesen {
                    // Verbindung steht ohnehin — Flag direkt mitsetzen.
                    if let Err(fehler) = verbindung.als_gelesen_markieren(mail.uid).await {
                        tracing::warn!("Gelesen-Flag nicht gesetzt: {fehler:#}");
                    }
                }
                verbindung.abmelden().await;

                let aufbereitet = anzeige::nachricht_aufbereiten(&roh);
                let anhang_paare: Vec<(String, usize)> = aufbereitet
                    .anhaenge
                    .iter()
                    .map(|a| (a.dateiname.clone(), a.groesse))
                    .collect();
                let inhalt = db::MailInhalt {
                    text: aufbereitet.text,
                    html_bereinigt: aufbereitet.html_bereinigt,
                    hatte_externe_bilder: aufbereitet.hatte_externe_bilder,
                };
                mit_db(zustand, |conn| {
                    db::inhalt_speichern(conn, mail_id, &inhalt)?;
                    db::anhaenge_speichern(conn, mail_id, &anhang_paare)?;
                    db::mail_setze_hat_anhang(conn, mail_id, aufbereitet.hat_anhang)
                })?;
                (inhalt, aufbereitet.hat_anhang, true)
            }
        };

    if !mail.gelesen {
        mit_db(zustand, |conn| db::mail_gelesen_setzen(conn, mail_id, true))?;
        if !server_flag_gesetzt {
            // Cache-Treffer: Flag nebenläufig auf dem Server setzen —
            // scheitert das (offline), korrigiert es der nächste Sync.
            let uid = mail.uid;
            let ordner_name = ordner.name.clone();
            tauri::async_runtime::spawn(async move {
                match verbindung_zum_konto(&konto).await {
                    Ok(mut verbindung) => {
                        if let Ok(_status) = verbindung.ordner_waehlen(&ordner_name).await {
                            if let Err(fehler) = verbindung.als_gelesen_markieren(uid).await {
                                tracing::warn!("Gelesen-Flag nicht gesetzt: {fehler:#}");
                            }
                        }
                        verbindung.abmelden().await;
                    }
                    Err(fehler) => {
                        tracing::warn!("Gelesen-Flag nicht übertragen: {fehler:#}");
                    }
                }
            });
        }
    }

    let anhaenge = mit_db(zustand, |conn| db::anhaenge_liste(conn, mail_id))?;

    Ok(MailAnsicht {
        kopf: MailKopf {
            gelesen: true,
            hat_anhang,
            ..mail
        },
        text: inhalt.text,
        html_schlicht: inhalt
            .html_bereinigt
            .as_deref()
            .map(anzeige::stil_entfernen),
        html: inhalt.html_bereinigt,
        hatte_externe_bilder: inhalt.hatte_externe_bilder,
        anhaenge,
    })
}

/// Speichert einen Anhang der Mail unter dem angegebenen Zielpfad
/// (der Pfad kommt aus dem Speichern-Dialog). Der Inhalt wird frisch
/// vom Server geholt — Anhänge liegen nie im lokalen Cache.
#[tauri::command]
pub async fn anhang_speichern(
    zustand: State<'_, AppZustand>,
    mail_id: i64,
    index: i64,
    ziel_pfad: String,
) -> Result<(), String> {
    anhang_speichern_intern(&zustand, mail_id, index, ziel_pfad)
        .await
        .map_err(|f| als_meldung(&f))
}

async fn anhang_speichern_intern(
    zustand: &AppZustand,
    mail_id: i64,
    index: i64,
    ziel_pfad: String,
) -> Result<()> {
    let (mail, ordner, konto) = mail_kontext(zustand, mail_id)?;
    let roh = roh_nachricht_laden(&konto, &ordner.name, mail.uid).await?;
    let (_, daten) = anzeige::anhang_daten(&roh, usize::try_from(index).unwrap_or(usize::MAX))
        .ok_or_else(|| nutzerfehler("Der Anhang wurde in der Mail nicht gefunden."))?;
    tokio::fs::write(&ziel_pfad, daten)
        .await
        .map_err(|f| nutzerfehler(format!("Die Datei ließ sich nicht speichern: {f}")))?;
    tracing::info!(mail_id, index, "Anhang gespeichert");
    Ok(())
}

/// Löscht eine Mail: außerhalb des Papierkorbs wird sie dorthin
/// verschoben, im Papierkorb (oder ohne Papierkorb) endgültig entfernt.
#[tauri::command]
pub async fn mail_loeschen(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    mail_id: i64,
) -> Result<(), String> {
    mail_loeschen_intern(&app, &zustand, mail_id)
        .await
        .map_err(|f| als_meldung(&f))
}

async fn mail_loeschen_intern(app: &AppHandle, zustand: &AppZustand, mail_id: i64) -> Result<()> {
    let (mail, ordner, konto) = mail_kontext(zustand, mail_id)?;
    let alle_ordner = mit_db(zustand, |conn| db::ordner_liste(conn, konto.id))?;
    let papierkorb = db::finde_papierkorb_ordner(&alle_ordner)
        .filter(|ziel| ziel.id != ordner.id)
        .cloned();

    let mut verbindung = verbindung_zum_konto(&konto).await?;
    verbindung.ordner_waehlen(&ordner.name).await?;
    match &papierkorb {
        Some(ziel) => verbindung.verschieben(mail.uid, &ziel.name).await?,
        None => verbindung.endgueltig_loeschen(mail.uid).await?,
    }

    // Cache sofort nachziehen, damit die Mail aus der Liste verschwindet.
    mit_db(zustand, |conn| db::mail_entfernen(conn, mail_id))?;
    let _ = app.emit(
        "mails:neu",
        MailsNeu {
            ordner_id: ordner.id,
        },
    );

    // Papierkorb direkt abgleichen, damit die Mail dort sofort auftaucht
    // (Fehler dabei sind unkritisch — der nächste Sync korrigiert).
    if let Some(ziel) = &papierkorb {
        if let Err(fehler) = ordner_synchronisieren(app, zustand, &mut verbindung, ziel).await {
            tracing::warn!("Papierkorb-Abgleich nach dem Löschen fehlgeschlagen: {fehler:#}");
        }
    }
    verbindung.abmelden().await;
    tracing::info!(mail_id, endgueltig = papierkorb.is_none(), "Mail gelöscht");
    Ok(())
}

/// Ergebnis von „Bilder laden“: dasselbe HTML in beiden Ansichten.
#[derive(Serialize)]
pub struct BilderAnsicht {
    pub html: String,
    pub html_schlicht: String,
}

#[tauri::command]
pub async fn mail_bilder_laden(
    zustand: State<'_, AppZustand>,
    mail_id: i64,
) -> Result<BilderAnsicht, String> {
    mail_bilder_laden_intern(&zustand, mail_id)
        .await
        .map(|html| BilderAnsicht {
            html_schlicht: anzeige::stil_entfernen(&html),
            html,
        })
        .map_err(|f| als_meldung(&f))
}

/// Mail + Ordner + Konto zu einer Mail-ID aus dem Cache laden.
fn mail_kontext(zustand: &AppZustand, mail_id: i64) -> Result<(MailKopf, Ordner, Konto)> {
    let mail = mit_db(zustand, |conn| db::mail_holen(conn, mail_id))?
        .ok_or_else(|| anyhow!("Mail {mail_id} ist nicht (mehr) im Cache"))?;
    let ordner = mit_db(zustand, |conn| db::ordner_holen(conn, mail.ordner_id))?
        .ok_or_else(|| anyhow!("Ordner der Mail ist nicht (mehr) vorhanden"))?;
    let konto = konto_laden(zustand, ordner.konto_id)?;
    Ok((mail, ordner, konto))
}

/// Holt die Original-Rohbytes einer Mail frisch vom Server.
async fn roh_nachricht_laden(konto: &Konto, ordner_name: &str, uid: u32) -> Result<Vec<u8>> {
    let mut verbindung = verbindung_zum_konto(konto).await?;
    verbindung.ordner_waehlen(ordner_name).await?;
    let roh = verbindung.nachricht_laden(uid).await?;
    verbindung.abmelden().await;
    Ok(roh)
}

/// Lädt die externen Bilder einer Mail herunter und liefert das HTML mit
/// eingebetteten `data:`-URIs. Wird bewusst nicht gecacht: Der Nutzer
/// entscheidet pro Anzeige, ob externe Inhalte geladen werden.
async fn mail_bilder_laden_intern(zustand: &AppZustand, mail_id: i64) -> Result<String> {
    let (mail, ordner, konto) = mail_kontext(zustand, mail_id)?;
    // Original frisch vom Server holen — unbereinigtes HTML wird nie gecacht.
    let roh = roh_nachricht_laden(&konto, &ordner.name, mail.uid).await?;

    let urls = anzeige::externe_bild_urls(&roh);
    let geladene = bilder_herunterladen(&urls).await;
    anzeige::nachricht_mit_bildern(&roh, &geladene)
        .ok_or_else(|| anyhow!("Die Mail enthält keinen HTML-Teil"))
}

/// Lädt externe Bilder herunter (nur HTTPS) und liefert URL → `data:`-URI.
/// Fehlgeschlagene Downloads bleiben einfach blockiert.
async fn bilder_herunterladen(urls: &[String]) -> HashMap<String, String> {
    let mut geladene = HashMap::new();
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(client) => client,
        Err(fehler) => {
            tracing::error!("HTTP-Client nicht erstellbar: {fehler:#}");
            return geladene;
        }
    };

    for url in urls.iter().take(MAX_BILDER) {
        if !url.starts_with("https://") {
            tracing::info!("Bild über unverschlüsseltes HTTP bleibt blockiert");
            continue;
        }
        match bild_holen(&client, url).await {
            Ok(daten_uri) => {
                geladene.insert(url.clone(), daten_uri);
            }
            Err(fehler) => {
                tracing::warn!("Bild-Download fehlgeschlagen: {fehler:#}");
            }
        }
    }
    geladene
}

async fn bild_holen(client: &reqwest::Client, url: &str) -> Result<String> {
    let antwort = client.get(url).send().await.context("Bild anfragen")?;
    if !antwort.status().is_success() {
        anyhow::bail!("HTTP-Status {}", antwort.status());
    }
    let mime = antwort
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|wert| wert.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !mime.starts_with("image/") {
        anyhow::bail!("Kein Bild (Content-Type {mime:?})");
    }
    let bytes = antwort.bytes().await.context("Bild herunterladen")?;
    if bytes.len() > MAX_BILD_BYTES {
        anyhow::bail!("Bild zu groß ({} Bytes)", bytes.len());
    }
    let daten = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{daten}"))
}

// ----------------------------------------------------------------- Senden --

/// Eingaben des Verfassen-Dialogs.
#[derive(serde::Deserialize)]
pub struct SendeFormular {
    /// Empfänger, mehrere durch Komma/Semikolon getrennt.
    pub an: String,
    pub cc: String,
    pub betreff: String,
    pub text: String,
    /// Formatierte Fassung aus dem Editor — wird vor dem Versand bereinigt.
    #[serde(default)]
    pub html: Option<String>,
    /// Dateipfade der Anhänge (aus dem Datei-Dialog).
    pub anhaenge: Vec<String>,
    /// Mail-ID des Originals bei Antworten/Weiterleiten.
    pub antwort_auf: Option<i64>,
    pub weiterleiten: bool,
    /// Mail-ID des Entwurfs, aus dem dieses Fenster hervorging —
    /// er wird nach dem Senden bzw. erneuten Speichern entfernt.
    #[serde(default)]
    pub entwurf_von: Option<i64>,
}

/// Liest die Datei-Anhänge aus dem Verfassen-Fenster ein
/// (Pfade kommen aus dem Datei-Dialog bzw. vom Hineinziehen).
async fn anhaenge_einlesen(pfade: &[String]) -> Result<Vec<nachricht::Anhang>> {
    let mut anhaenge = Vec::new();
    for pfad in pfade {
        let daten = tokio::fs::read(pfad)
            .await
            .map_err(|f| nutzerfehler(format!("Anhang „{pfad}“ ließ sich nicht lesen: {f}")))?;
        anhaenge.push(nachricht::Anhang {
            dateiname: std::path::Path::new(pfad)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "anhang.bin".to_string()),
            mime: mime_guess::from_path(pfad)
                .first_or_octet_stream()
                .to_string(),
            daten,
        });
    }
    Ok(anhaenge)
}

/// Bereinigt die HTML-Fassung aus dem Editor (gleiche Schutzschicht wie
/// bei der Anzeige — sie verlässt nie ungefiltert die App).
fn html_aus_editor(html: Option<&str>) -> Option<String> {
    html.map(str::trim)
        .filter(|h| !h.is_empty())
        .map(ammonia::clean)
}

/// Vorbelegung für den Verfassen-Dialog (Antworten/Weiterleiten).
#[derive(Serialize)]
pub struct Vorlage {
    pub an: String,
    pub betreff: String,
    pub text: String,
}

/// Gesamtlimit für Anhänge (viele Server lehnen mehr ab).
const MAX_ANHANG_BYTES: usize = 25 * 1024 * 1024;

#[tauri::command]
pub async fn antwort_vorbereiten(
    zustand: State<'_, AppZustand>,
    mail_id: i64,
    weiterleiten: bool,
) -> Result<Vorlage, String> {
    antwort_vorbereiten_intern(&zustand, mail_id, weiterleiten)
        .await
        .map_err(|f| als_meldung(&f))
}

async fn antwort_vorbereiten_intern(
    zustand: &AppZustand,
    mail_id: i64,
    weiterleiten: bool,
) -> Result<Vorlage> {
    let (mail, ordner, konto) = mail_kontext(zustand, mail_id)?;
    let roh = roh_nachricht_laden(&konto, &ordner.name, mail.uid).await?;
    let daten = parsen::parse_fuer_antwort(&roh);

    if weiterleiten {
        Ok(Vorlage {
            an: String::new(),
            betreff: nachricht::weiterleit_betreff(&daten.betreff),
            text: nachricht::weiterleit_block(
                &daten.von_anzeige,
                daten.datum,
                &daten.betreff,
                &daten.an_anzeige,
                &daten.text,
            ),
        })
    } else {
        Ok(Vorlage {
            an: daten.antwort_an,
            betreff: nachricht::antwort_betreff(&daten.betreff),
            text: nachricht::zitat_block(daten.datum, &daten.von_anzeige, &daten.text),
        })
    }
}

#[tauri::command]
pub async fn mail_senden(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    konto_id: i64,
    formular: SendeFormular,
) -> Result<String, String> {
    mail_senden_intern(&app, &zustand, konto_id, formular)
        .await
        .map_err(|f| als_meldung(&f))
}

async fn mail_senden_intern(
    app: &AppHandle,
    zustand: &AppZustand,
    konto_id: i64,
    formular: SendeFormular,
) -> Result<String> {
    let konto = konto_laden(zustand, konto_id)?;
    if konto.smtp_host.is_empty() {
        return Err(nutzerfehler(
            "Für dieses Konto ist noch kein Versand-Server (SMTP) hinterlegt — \
             bitte über „Konto bearbeiten“ ergänzen.",
        ));
    }

    let an = adressliste(&formular.an);
    let cc = adressliste(&formular.cc);
    if an.is_empty() {
        return Err(nutzerfehler("Bitte mindestens einen Empfänger angeben."));
    }

    let mut anhaenge = anhaenge_einlesen(&formular.anhaenge).await?;

    // Bezug zum Original (Antwort: Threading-Header; Weiterleiten: Anhänge).
    let mut antwort = None;
    if let Some(original_id) = formular.antwort_auf {
        let (original, original_ordner, original_konto) = mail_kontext(zustand, original_id)?;
        let roh = roh_nachricht_laden(&original_konto, &original_ordner.name, original.uid).await?;
        if formular.weiterleiten {
            anhaenge.extend(nachricht::anhaenge_extrahieren(&roh));
        } else {
            let daten = parsen::parse_fuer_antwort(&roh);
            antwort = Some(nachricht::AntwortKontext {
                message_id: daten.message_id,
                references: daten.references,
            });
        }
    }

    let gesamt: usize = anhaenge.iter().map(|a| a.daten.len()).sum();
    if gesamt > MAX_ANHANG_BYTES {
        return Err(nutzerfehler(
            "Die Anhänge sind zusammen größer als 25 MB — das lehnen die meisten \
             Mail-Server ab. Bitte verkleinern.",
        ));
    }

    let html = html_aus_editor(formular.html.as_deref());

    let neue = nachricht::NeueNachricht {
        von_name: String::new(), // M2: schlichte Absenderadresse, Name kommt mit Signaturen
        von_adresse: konto.email.clone(),
        an,
        cc,
        betreff: formular.betreff.trim().to_string(),
        text: formular.text.clone(),
        html,
        anhaenge,
        antwort,
    };
    let (fertig, rohbytes) =
        nachricht::baue_nachricht(&neue).map_err(|f| nutzerfehler(f.to_string()))?;

    // Versand — schlägt das fehl, wird nichts abgelegt.
    let passwort = passwort_holen(konto.id).await?;
    versand::senden(
        &konto.smtp_host,
        konto.smtp_port,
        &konto.benutzer,
        &passwort,
        fertig,
    )
    .await?;

    // Empfänger für die Adress-Vorschläge merken (Fehler dabei unkritisch).
    if let Err(fehler) = mit_db(zustand, |conn| {
        for eintrag in neue.an.iter().chain(neue.cc.iter()) {
            db::adresse_merken(conn, eintrag)?;
        }
        Ok(())
    }) {
        tracing::warn!("Empfängeradressen nicht gemerkt: {fehler:#}");
    }

    // Nach einer Antwort: Original als beantwortet markieren
    // (Fehler unkritisch — der nächste Sync bringt nichts durcheinander,
    // schlimmstenfalls fehlt die Markierung).
    if let Some(original_id) = formular.antwort_auf {
        if !formular.weiterleiten {
            if let Err(fehler) = beantwortet_markieren(app, zustand, original_id) {
                tracing::warn!("Beantwortet-Markierung fehlgeschlagen: {fehler:#}");
            }
        }
    }

    // Ging die Mail aus einem Entwurf hervor: Entwurf entfernen
    // (Fehler unkritisch — schlimmstenfalls bleibt er liegen).
    if let Some(entwurf_id) = formular.entwurf_von {
        if let Err(fehler) = entwurf_entfernen(app, zustand, entwurf_id).await {
            tracing::warn!("Entwurf nach dem Senden nicht entfernt: {fehler:#}");
        }
    }

    // Kopie in den „Gesendet“-Ordner (Fehler hier machen den Versand nicht kaputt).
    let alle_ordner = mit_db(zustand, |conn| db::ordner_liste(conn, konto.id))?;
    match db::finde_gesendet_ordner(&alle_ordner) {
        Some(gesendet) => match sent_ablage(app, zustand, &konto, gesendet, &rohbytes).await {
            Ok(()) => Ok("Mail gesendet.".to_string()),
            Err(fehler) => {
                tracing::error!("Gesendet-Ablage fehlgeschlagen: {fehler:#}");
                Ok("Mail gesendet — aber die Kopie im „Gesendet“-Ordner \
                    konnte nicht abgelegt werden."
                    .to_string())
            }
        },
        None => {
            tracing::warn!(konto_id, "Kein Gesendet-Ordner gefunden");
            Ok(
                "Mail gesendet — es wurde aber kein „Gesendet“-Ordner gefunden, \
                daher liegt dort keine Kopie."
                    .to_string(),
            )
        }
    }
}

/// Markiert das Original einer beantworteten Mail: Cache sofort, das
/// \Answered-Flag auf dem Server nebenläufig — scheitert das (z. B.
/// offline), stellt der nächste Sync den Server-Stand wieder her
/// (Server gewinnt, wie beim Gelesen-Flag).
fn beantwortet_markieren(app: &AppHandle, zustand: &AppZustand, mail_id: i64) -> Result<()> {
    let (mail, ordner, konto) = mail_kontext(zustand, mail_id)?;
    mit_db(zustand, |conn| db::mail_beantwortet_setzen(conn, mail_id))?;
    let _ = app.emit(
        "mails:neu",
        MailsNeu {
            ordner_id: ordner.id,
        },
    );

    let uid = mail.uid;
    let ordner_name = ordner.name.clone();
    tauri::async_runtime::spawn(async move {
        match verbindung_zum_konto(&konto).await {
            Ok(mut verbindung) => {
                if verbindung.ordner_waehlen(&ordner_name).await.is_ok() {
                    if let Err(fehler) = verbindung.als_beantwortet_markieren(uid).await {
                        tracing::warn!("Beantwortet-Flag nicht übertragen: {fehler:#}");
                    }
                }
                verbindung.abmelden().await;
            }
            Err(fehler) => {
                tracing::warn!("Beantwortet-Flag nicht übertragen: {fehler:#}");
            }
        }
    });
    Ok(())
}

async fn sent_ablage(
    app: &AppHandle,
    zustand: &AppZustand,
    konto: &Konto,
    gesendet: &Ordner,
    rohbytes: &[u8],
) -> Result<()> {
    let mut verbindung = verbindung_zum_konto(konto).await?;
    verbindung
        .nachricht_ablegen(&gesendet.name, rohbytes, "(\\Seen)")
        .await?;
    // Ordner direkt abgleichen, damit die Kopie sofort in der App auftaucht.
    ordner_synchronisieren(app, zustand, &mut verbindung, gesendet).await?;
    verbindung.abmelden().await;
    Ok(())
}

// --------------------------------------------------------------- Entwürfe --

/// Ein geladener Entwurf fürs Weiterbearbeiten im Verfassen-Fenster.
#[derive(Serialize)]
pub struct Entwurf {
    pub an: String,
    pub cc: String,
    pub betreff: String,
    pub text: String,
    /// Formatierte Fassung (bereinigt) — erhält fett/kursiv/Listen
    /// beim Weiterbearbeiten.
    pub html: Option<String>,
}

/// Lädt einen gespeicherten Entwurf in den Editor.
#[tauri::command]
pub async fn entwurf_laden(
    zustand: State<'_, AppZustand>,
    mail_id: i64,
) -> Result<Entwurf, String> {
    entwurf_laden_intern(&zustand, mail_id)
        .await
        .map_err(|f| als_meldung(&f))
}

async fn entwurf_laden_intern(zustand: &AppZustand, mail_id: i64) -> Result<Entwurf> {
    let (mail, ordner, konto) = mail_kontext(zustand, mail_id)?;
    let roh = roh_nachricht_laden(&konto, &ordner.name, mail.uid).await?;
    let daten = parsen::parse_fuer_entwurf(&roh);
    // Die HTML-Fassung durchläuft dieselbe Bereinigung wie bei der
    // Anzeige, bevor sie in den Editor darf.
    let html = anzeige::nachricht_aufbereiten(&roh).html_bereinigt;
    Ok(Entwurf {
        an: daten.an,
        cc: daten.cc,
        betreff: daten.betreff,
        text: daten.text,
        html,
    })
}

/// Speichert den Stand des Verfassen-Fensters als Entwurf im
/// Entwürfe-Ordner des Kontos (ersetzt ggf. den bearbeiteten Entwurf).
#[tauri::command]
pub async fn entwurf_speichern(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    konto_id: i64,
    formular: SendeFormular,
) -> Result<String, String> {
    entwurf_speichern_intern(&app, &zustand, konto_id, formular)
        .await
        .map_err(|f| als_meldung(&f))
}

async fn entwurf_speichern_intern(
    app: &AppHandle,
    zustand: &AppZustand,
    konto_id: i64,
    formular: SendeFormular,
) -> Result<String> {
    let konto = konto_laden(zustand, konto_id)?;
    let alle_ordner = mit_db(zustand, |conn| db::ordner_liste(conn, konto.id))?;
    let entwuerfe = db::finde_entwuerfe_ordner(&alle_ordner)
        .ok_or_else(|| {
            nutzerfehler(
                "Für dieses Konto wurde kein Entwürfe-Ordner gefunden — \
                 bitte einmal aktualisieren oder den Ordner beim Anbieter anlegen.",
            )
        })?
        .clone();

    let anhaenge = anhaenge_einlesen(&formular.anhaenge).await?;
    let neue = nachricht::NeueNachricht {
        von_name: String::new(),
        von_adresse: konto.email.clone(),
        an: adressliste(&formular.an),
        cc: adressliste(&formular.cc),
        betreff: formular.betreff.trim().to_string(),
        text: formular.text.clone(),
        html: html_aus_editor(formular.html.as_deref()),
        anhaenge,
        antwort: None,
    };
    let (_, rohbytes) =
        nachricht::baue_nachricht(&neue).map_err(|f| nutzerfehler(f.to_string()))?;

    let mut verbindung = verbindung_zum_konto(&konto).await?;
    verbindung
        .nachricht_ablegen(&entwuerfe.name, &rohbytes, "(\\Draft \\Seen)")
        .await?;

    // Beim Weiterbearbeiten: alten Stand entfernen (Fehler unkritisch).
    if let Some(alter_entwurf) = formular.entwurf_von {
        if let Err(fehler) = entwurf_entfernen(app, zustand, alter_entwurf).await {
            tracing::warn!("Alter Entwurf nicht entfernt: {fehler:#}");
        }
    }

    // Ordner direkt abgleichen, damit der Entwurf sofort auftaucht.
    if let Err(fehler) = ordner_synchronisieren(app, zustand, &mut verbindung, &entwuerfe).await {
        tracing::warn!("Entwürfe-Abgleich nach dem Speichern fehlgeschlagen: {fehler:#}");
    }
    verbindung.abmelden().await;
    tracing::info!(konto_id, "Entwurf gespeichert");
    Ok("Entwurf gespeichert.".to_string())
}

/// Entfernt einen Entwurf endgültig (nach dem Senden bzw. beim Ersetzen
/// durch einen neuen Stand).
async fn entwurf_entfernen(app: &AppHandle, zustand: &AppZustand, mail_id: i64) -> Result<()> {
    let (mail, ordner, konto) = mail_kontext(zustand, mail_id)?;
    let mut verbindung = verbindung_zum_konto(&konto).await?;
    verbindung.ordner_waehlen(&ordner.name).await?;
    verbindung.endgueltig_loeschen(mail.uid).await?;
    verbindung.abmelden().await;
    mit_db(zustand, |conn| db::mail_entfernen(conn, mail_id))?;
    let _ = app.emit(
        "mails:neu",
        MailsNeu {
            ordner_id: ordner.id,
        },
    );
    Ok(())
}

/// Zerlegt „a@b.c, d@e.f; g@h.i“ in einzelne Adressen.
fn adressliste(eingabe: &str) -> Vec<String> {
    eingabe
        .split([',', ';'])
        .map(str::trim)
        .filter(|teil| !teil.is_empty())
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------- Avatare --

/// Auffrischung der Avatar-Bilder (30 Tage) — danach wird neu geladen.
const AVATAR_HOECHSTALTER: i64 = 30 * 24 * 3600;

/// Liefert die `data:`-URI des Absender-Avatars oder `null`
/// (dann zeigt das Frontend farbige Initialen).
///
/// Hinweis: lädt bewusst externe Bilder (Gravatar/Favicon) — vom
/// Projektinhaber ausdrücklich so gewünscht. Ergebnisse werden gecacht.
#[tauri::command]
pub async fn absender_avatar(
    zustand: State<'_, AppZustand>,
    email: String,
) -> Result<Option<String>, String> {
    absender_avatar_intern(&zustand, email)
        .await
        .map_err(|f| als_meldung(&f))
}

async fn absender_avatar_intern(zustand: &AppZustand, email: String) -> Result<Option<String>> {
    if email.trim().is_empty() {
        return Ok(None);
    }
    // 1) Cache — Bild oder „bekannt kein Bild“ direkt zurückgeben.
    if let Some(gecacht) = mit_db(zustand, |conn| {
        db::avatar_aus_cache(conn, &email, AVATAR_HOECHSTALTER)
    })? {
        return Ok(gecacht);
    }
    // 2) Extern laden (blockiert die UI nicht — eigener Task).
    let client = avatar::client()?;
    let bild = avatar::hole_avatar(&client, &email).await;
    mit_db(zustand, |conn| {
        db::avatar_speichern(conn, &email, bild.as_deref())
    })?;
    Ok(bild)
}

// ----------------------------------------------------- Kalender (M4) --

/// Wie viele Termin-Objekte je `calendar-multiget` angefragt werden.
const MULTIGET_BATCH: usize = 50;

/// Schlüsselbund-Zugriffe für Kalender-Konten — wie bei den Mail-Konten
/// immer über `spawn_blocking` (zbus blockiert intern).
async fn kalender_passwort_holen(konto_id: i64) -> Result<String> {
    tauri::async_runtime::spawn_blocking(move || schluesselbund::kalender_passwort_holen(konto_id))
        .await
        .context("Schlüsselbund-Task abgebrochen")?
}

async fn kalender_passwort_speichern(konto_id: i64, passwort: String) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        schluesselbund::kalender_passwort_speichern(konto_id, &passwort)
    })
    .await
    .context("Schlüsselbund-Task abgebrochen")?
}

/// Eingaben des Kalender-Konto-Dialogs.
#[derive(serde::Deserialize)]
pub struct KalenderKontoFormular {
    pub name: String,
    pub server: String,
    pub benutzer: String,
    pub passwort: String,
}

#[tauri::command]
pub async fn kalender_konto_anlegen(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    formular: KalenderKontoFormular,
) -> Result<db_kalender::KalenderKonto, String> {
    let konto = kalender_konto_anlegen_intern(&zustand, formular)
        .await
        .map_err(|f| als_meldung(&f))?;
    // Termine im Hintergrund laden — die Oberfläche zeigt den Fortschritt
    // über das `kalender:aktualisiert`-Event.
    tauri::async_runtime::spawn(async move {
        let _ = kalender_sync_ausfuehren(&app).await;
    });
    Ok(konto)
}

async fn kalender_konto_anlegen_intern(
    zustand: &AppZustand,
    formular: KalenderKontoFormular,
) -> Result<db_kalender::KalenderKonto> {
    let name = formular.name.trim().to_string();
    let server = formular.server.trim().trim_end_matches('/').to_string();
    let benutzer = formular.benutzer.trim().to_string();
    if name.is_empty() || server.is_empty() || benutzer.is_empty() || formular.passwort.is_empty() {
        return Err(nutzerfehler("Bitte alle Felder ausfüllen."));
    }

    // Zugangsdaten prüfen und Kalender entdecken, bevor etwas gespeichert wird.
    let verbindung = CaldavVerbindung::neu(&server, &benutzer, &formular.passwort)?;
    let funde = verbindung.kalender_finden().await?;
    if funde.is_empty() {
        return Err(nutzerfehler(
            "Die Anmeldung hat geklappt, aber auf dem Server wurden keine \
             Kalender gefunden.",
        ));
    }

    let konto = mit_db(zustand, |conn| {
        db_kalender::konto_anlegen(conn, &name, &server, &benutzer)
    })?;
    if let Err(fehler) = kalender_passwort_speichern(konto.id, formular.passwort).await {
        // Ohne Passwort im Schlüsselbund ist das Konto nutzlos → zurückrollen.
        let _ = mit_db(zustand, |conn| db_kalender::konto_loeschen(conn, konto.id));
        return Err(fehler);
    }
    mit_db(zustand, |conn| {
        for fund in &funde {
            db_kalender::kalender_upsert(
                conn,
                konto.id,
                &fund.href,
                &fund.anzeige_name,
                &fund.farbe,
            )?;
        }
        Ok(())
    })?;
    tracing::info!(
        konto_id = konto.id,
        anzahl = funde.len(),
        "Kalender-Konto angelegt"
    );
    Ok(konto)
}

#[tauri::command]
pub async fn kalender_konto_loeschen(
    zustand: State<'_, AppZustand>,
    konto_id: i64,
) -> Result<(), String> {
    mit_db(&zustand, |conn| db_kalender::konto_loeschen(conn, konto_id))
        .map_err(|f| als_meldung(&f))?;
    tauri::async_runtime::spawn_blocking(move || {
        schluesselbund::kalender_passwort_loeschen(konto_id)
    })
    .await
    .map_err(|_| "Interner Fehler beim Aufräumen des Schlüsselbunds".to_string())?
    .map_err(|f| als_meldung(&f))?;
    tracing::info!(konto_id, "Kalender-Konto entfernt");
    Ok(())
}

#[tauri::command]
pub fn kalender_konten_liste(
    zustand: State<'_, AppZustand>,
) -> Result<Vec<db_kalender::KalenderKonto>, String> {
    mit_db(&zustand, db_kalender::konten_liste).map_err(|f| als_meldung(&f))
}

/// Ein Kalender, wie ihn die Oberfläche anzeigt (mit wirksamer Farbe).
#[derive(Serialize)]
pub struct KalenderAnsicht {
    pub id: i64,
    pub konto_id: i64,
    pub konto_name: String,
    pub anzeige_name: String,
    /// Wirksame Farbe (eigene Wahl vor Nextcloud-Farbe vor Standard).
    pub farbe: String,
    /// Wurde die Farbe in Nanomail überschrieben?
    pub farbe_ist_eigen: bool,
    pub sichtbar: bool,
}

impl From<db_kalender::Kalender> for KalenderAnsicht {
    fn from(kalender: db_kalender::Kalender) -> Self {
        Self {
            farbe: kalender.farbe().to_string(),
            farbe_ist_eigen: !kalender.farbe_eigen.is_empty(),
            id: kalender.id,
            konto_id: kalender.konto_id,
            konto_name: kalender.konto_name,
            anzeige_name: kalender.anzeige_name,
            sichtbar: kalender.sichtbar,
        }
    }
}

#[tauri::command]
pub fn kalender_liste(zustand: State<'_, AppZustand>) -> Result<Vec<KalenderAnsicht>, String> {
    mit_db(&zustand, db_kalender::kalender_liste)
        .map(|liste| liste.into_iter().map(KalenderAnsicht::from).collect())
        .map_err(|f| als_meldung(&f))
}

/// Setzt die eigene Kalender-Farbe (leer = zurück zur Nextcloud-Farbe).
#[tauri::command]
pub fn kalender_farbe_setzen(
    zustand: State<'_, AppZustand>,
    kalender_id: i64,
    farbe: String,
) -> Result<(), String> {
    let farbe = farbe.trim().to_lowercase();
    let gueltig = farbe.is_empty()
        || (farbe.len() == 7
            && farbe.starts_with('#')
            && farbe[1..].chars().all(|z| z.is_ascii_hexdigit()));
    if !gueltig {
        return Err("Die Farbe muss ein Hex-Wert wie #61afef sein.".to_string());
    }
    mit_db(&zustand, |conn| {
        db_kalender::farbe_setzen(conn, kalender_id, &farbe)
    })
    .map_err(|f| als_meldung(&f))
}

#[tauri::command]
pub fn kalender_sichtbar_setzen(
    zustand: State<'_, AppZustand>,
    kalender_id: i64,
    sichtbar: bool,
) -> Result<(), String> {
    mit_db(&zustand, |conn| {
        db_kalender::sichtbar_setzen(conn, kalender_id, sichtbar)
    })
    .map_err(|f| als_meldung(&f))
}

/// Ein anzeigefertiges Termin-Vorkommen fürs Monatsraster.
#[derive(Serialize)]
pub struct TerminAnzeige {
    pub kalender_id: i64,
    pub kalender_name: String,
    pub farbe: String,
    #[serde(flatten)]
    pub termin: termine::Termin,
}

/// Alle Termin-Vorkommen sichtbarer Kalender im Fenster `[von, bis)`
/// (UTC-Sekunden) — aus dem lokalen Cache, funktioniert auch offline.
#[tauri::command]
pub fn kalender_termine(
    zustand: State<'_, AppZustand>,
    von: i64,
    bis: i64,
) -> Result<Vec<TerminAnzeige>, String> {
    kalender_termine_intern(&zustand, von, bis).map_err(|f| als_meldung(&f))
}

fn kalender_termine_intern(zustand: &AppZustand, von: i64, bis: i64) -> Result<Vec<TerminAnzeige>> {
    let kalender = mit_db(zustand, db_kalender::kalender_liste)?;
    let quellen = mit_db(zustand, |conn| {
        db_kalender::termine_im_zeitraum(conn, von, bis)
    })?;

    let mut anzeige = Vec::new();
    for quelle in quellen {
        let Some(kal) = kalender.iter().find(|k| k.id == quelle.kalender_id) else {
            continue;
        };
        match termine::expandiere(&quelle.ics, von, bis) {
            Ok(vorkommen) => {
                for termin in vorkommen {
                    anzeige.push(TerminAnzeige {
                        kalender_id: kal.id,
                        kalender_name: kal.anzeige_name.clone(),
                        farbe: kal.farbe().to_string(),
                        termin,
                    });
                }
            }
            // Ein unlesbares Objekt darf die Ansicht nicht verhindern.
            Err(fehler) => tracing::warn!("Termin-Objekt übersprungen: {fehler:#}"),
        }
    }
    anzeige.sort_by_key(|t| t.termin.beginn);
    Ok(anzeige)
}

#[tauri::command]
pub async fn kalender_sync(app: AppHandle) -> Result<(), String> {
    kalender_sync_ausfuehren(&app).await
}

/// Gleicht alle Kalender-Konten ab (mit Doppelstart-Schutz). Wird vom
/// Command, beim App-Start und vom periodischen Sync genutzt.
pub async fn kalender_sync_ausfuehren(app: &AppHandle) -> Result<(), String> {
    let zustand = app.state::<AppZustand>();
    {
        let mut laeuft = zustand
            .kalender_sync_laeuft
            .lock()
            .map_err(|_| "Interner Fehler beim Kalender-Abgleich".to_string())?;
        if *laeuft {
            return Ok(()); // läuft bereits — kein Fehler
        }
        *laeuft = true;
    }

    let konten = mit_db(&zustand, db_kalender::konten_liste).unwrap_or_default();
    let mut erster_fehler: Option<String> = None;
    for konto in konten {
        if let Err(fehler) = kalender_konto_synchronisieren(app, &zustand, &konto).await {
            tracing::error!(
                konto_id = konto.id,
                "Kalender-Abgleich fehlgeschlagen: {fehler:#}"
            );
            erster_fehler
                .get_or_insert_with(|| format!("{}: {}", konto.name, als_meldung(&fehler)));
        }
    }

    if let Ok(mut laeuft) = zustand.kalender_sync_laeuft.lock() {
        *laeuft = false;
    }
    let _ = app.emit("kalender:aktualisiert", ());
    match erster_fehler {
        None => Ok(()),
        Some(meldung) => Err(meldung),
    }
}

/// Abgleich eines Kontos: Kalenderliste auffrischen, dann jeden Kalender
/// über sein Sync-Token abgleichen. Ein kaputter Kalender bricht nicht
/// den ganzen Abgleich ab.
async fn kalender_konto_synchronisieren(
    app: &AppHandle,
    zustand: &AppZustand,
    konto: &db_kalender::KalenderKonto,
) -> Result<()> {
    let passwort = kalender_passwort_holen(konto.id).await?;
    let verbindung = CaldavVerbindung::neu(&konto.server, &konto.benutzer, &passwort)?;

    // 1) Kalenderliste abgleichen (neue/umbenannte/gelöschte Kalender).
    let funde = verbindung.kalender_finden().await?;
    let konto_id = konto.id;
    let kalender_liste = mit_db(zustand, |conn| {
        let hrefs: Vec<String> = funde.iter().map(|f| f.href.clone()).collect();
        db_kalender::kalender_bereinigen(conn, konto_id, &hrefs)?;
        for fund in &funde {
            db_kalender::kalender_upsert(
                conn,
                konto_id,
                &fund.href,
                &fund.anzeige_name,
                &fund.farbe,
            )?;
        }
        db_kalender::kalender_liste(conn)
    })?;

    // 2) Termine je Kalender abgleichen.
    for kalender in kalender_liste.iter().filter(|k| k.konto_id == konto_id) {
        if let Err(fehler) = kalender_abgleichen(zustand, &verbindung, kalender).await {
            tracing::error!(kalender = %kalender.anzeige_name, "Kalender-Sync fehlgeschlagen: {fehler:#}");
        } else {
            let _ = app.emit("kalender:aktualisiert", ());
        }
    }
    Ok(())
}

/// Sync-Token-Abgleich eines einzelnen Kalenders; bei verfallenem Token
/// wird der Kalender einmal komplett neu geladen.
async fn kalender_abgleichen(
    zustand: &AppZustand,
    verbindung: &CaldavVerbindung,
    kalender: &db_kalender::Kalender,
) -> Result<()> {
    let ergebnis = match verbindung
        .abgleichen(&kalender.href, &kalender.sync_token)
        .await?
    {
        SyncAntwort::Ergebnis(ergebnis) => ergebnis,
        SyncAntwort::TokenUngueltig => {
            mit_db(zustand, |conn| {
                db_kalender::termine_leeren(conn, kalender.id)
            })?;
            match verbindung.abgleichen(&kalender.href, "").await? {
                SyncAntwort::Ergebnis(ergebnis) => ergebnis,
                SyncAntwort::TokenUngueltig => {
                    anyhow::bail!("Server lehnt auch den Erstabgleich ab")
                }
            }
        }
    };

    // Gelöschte Objekte aus dem Cache räumen.
    mit_db(zustand, |conn| {
        for href in &ergebnis.geloescht {
            db_kalender::termin_loeschen(conn, kalender.id, href)?;
        }
        Ok(())
    })?;

    // Nur Objekte laden, deren ETag sich geändert hat.
    let mut zu_laden: Vec<String> = Vec::new();
    for (href, etag) in &ergebnis.geaendert {
        let gecacht = mit_db(zustand, |conn| {
            db_kalender::termin_etag(conn, kalender.id, href)
        })?;
        if gecacht.as_deref() != Some(etag.as_str()) {
            zu_laden.push(href.clone());
        }
    }
    for batch in zu_laden.chunks(MULTIGET_BATCH) {
        let objekte = verbindung.objekte_laden(&kalender.href, batch).await?;
        mit_db(zustand, |conn| {
            for objekt in &objekte {
                termin_objekt_speichern(conn, kalender.id, objekt)?;
            }
            Ok(())
        })?;
    }

    mit_db(zustand, |conn| {
        db_kalender::sync_token_setzen(conn, kalender.id, &ergebnis.token)
    })?;
    tracing::debug!(
        kalender = %kalender.anzeige_name,
        neu = zu_laden.len(),
        geloescht = ergebnis.geloescht.len(),
        "Kalender abgeglichen"
    );
    Ok(())
}

/// Legt ein geladenes Termin-Objekt samt Eckdaten im Cache ab.
fn termin_objekt_speichern(
    conn: &rusqlite::Connection,
    kalender_id: i64,
    objekt: &xml::ObjektDaten,
) -> Result<()> {
    // Ein unlesbares ICS wird trotzdem gecacht (ohne Eckdaten), damit
    // der Abgleich es nicht bei jedem Lauf erneut lädt.
    let eckdaten = termine::metadaten(&objekt.ics).unwrap_or_else(|fehler| {
        tracing::warn!("Termin-Eckdaten nicht lesbar: {fehler:#}");
        termine::Metadaten {
            beginn: None,
            ende: None,
            hat_wiederholung: false,
        }
    });
    db_kalender::termin_upsert(
        conn,
        kalender_id,
        &objekt.href,
        &objekt.etag,
        &objekt.ics,
        eckdaten.beginn,
        eckdaten.ende,
        eckdaten.hat_wiederholung,
    )
}

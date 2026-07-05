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

use crate::db::{self, Konto, MailKopf, NeuerMailKopf, Ordner};
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
}

impl KontoFormular {
    fn bereinigt(mut self) -> Result<Self> {
        self.name = self.name.trim().to_string();
        self.email = self.email.trim().to_string();
        self.benutzer = self.benutzer.trim().to_string();
        self.imap_host = self.imap_host.trim().to_string();
        self.smtp_host = self.smtp_host.trim().to_string();
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
                        hat_anhang: false, // wird beim Öffnen der Mail erkannt
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
    offset: i64,
    limit: i64,
) -> Result<Vec<MailKopf>, String> {
    mit_db(&zustand, |conn| {
        db::mails_liste(conn, ordner_id, offset.max(0), limit.clamp(1, 500))
    })
    .map_err(|f| als_meldung(&f))
}

/// Anzeigefertige Mail für den Lesebereich.
#[derive(Serialize)]
pub struct MailAnsicht {
    pub kopf: MailKopf,
    pub text: String,
    /// Bereinigtes HTML — das Frontend zeigt es nur im Sandbox-iframe an.
    pub html: Option<String>,
    pub hatte_externe_bilder: bool,
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
                let inhalt = db::MailInhalt {
                    text: aufbereitet.text,
                    html_bereinigt: aufbereitet.html_bereinigt,
                    hatte_externe_bilder: aufbereitet.hatte_externe_bilder,
                };
                mit_db(zustand, |conn| {
                    db::inhalt_speichern(conn, mail_id, &inhalt)?;
                    db::mail_setze_hat_anhang(conn, mail_id, aufbereitet.hat_anhang)
                })?;
                (inhalt, aufbereitet.hat_anhang, true)
            }
        };

    if !mail.gelesen {
        mit_db(zustand, |conn| {
            db::mail_als_gelesen_markieren(conn, mail_id)
        })?;
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

    Ok(MailAnsicht {
        kopf: MailKopf {
            gelesen: true,
            hat_anhang,
            ..mail
        },
        text: inhalt.text,
        html: inhalt.html_bereinigt,
        hatte_externe_bilder: inhalt.hatte_externe_bilder,
    })
}

#[tauri::command]
pub async fn mail_bilder_laden(
    zustand: State<'_, AppZustand>,
    mail_id: i64,
) -> Result<String, String> {
    mail_bilder_laden_intern(&zustand, mail_id)
        .await
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
    /// Dateipfade der Anhänge (aus dem Datei-Dialog).
    pub anhaenge: Vec<String>,
    /// Mail-ID des Originals bei Antworten/Weiterleiten.
    pub antwort_auf: Option<i64>,
    pub weiterleiten: bool,
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

    // Anhänge von der Platte lesen (Pfade kommen aus dem Datei-Dialog).
    let mut anhaenge = Vec::new();
    let mut gesamt = 0usize;
    for pfad in &formular.anhaenge {
        let daten = tokio::fs::read(pfad)
            .await
            .map_err(|f| nutzerfehler(format!("Anhang „{pfad}“ ließ sich nicht lesen: {f}")))?;
        gesamt += daten.len();
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

    gesamt += anhaenge.iter().map(|a| a.daten.len()).sum::<usize>();
    if gesamt > MAX_ANHANG_BYTES {
        return Err(nutzerfehler(
            "Die Anhänge sind zusammen größer als 25 MB — das lehnen die meisten \
             Mail-Server ab. Bitte verkleinern.",
        ));
    }

    let neue = nachricht::NeueNachricht {
        von_name: String::new(), // M2: schlichte Absenderadresse, Name kommt mit Signaturen
        von_adresse: konto.email.clone(),
        an,
        cc,
        betreff: formular.betreff.trim().to_string(),
        text: formular.text.clone(),
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

async fn sent_ablage(
    app: &AppHandle,
    zustand: &AppZustand,
    konto: &Konto,
    gesendet: &Ordner,
    rohbytes: &[u8],
) -> Result<()> {
    let mut verbindung = verbindung_zum_konto(konto).await?;
    verbindung
        .nachricht_ablegen(&gesendet.name, rohbytes)
        .await?;
    // Ordner direkt abgleichen, damit die Kopie sofort in der App auftaucht.
    ordner_synchronisieren(app, zustand, &mut verbindung, gesendet).await?;
    verbindung.abmelden().await;
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

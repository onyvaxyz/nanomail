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
use tauri::{AppHandle, Emitter, State};

use crate::db::{self, Konto, MailKopf, NeuerMailKopf, Ordner};
use crate::imap::verbindung::ImapVerbindung;
use crate::imap::{parsen, sync};
use crate::{anzeige, schluesselbund};

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

/// Übersetzt technische Fehler in eine verständliche deutsche Meldung
/// und protokolliert die Details.
fn als_meldung(fehler: &anyhow::Error) -> String {
    let kette = format!("{fehler:#}");
    tracing::error!("{kette}");
    if kette.contains("Anmeldung abgelehnt") {
        "Anmeldung fehlgeschlagen — bitte Benutzername und (App-)Passwort prüfen.".into()
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

#[tauri::command]
pub async fn konto_anlegen(
    zustand: State<'_, AppZustand>,
    name: String,
    email: String,
    benutzer: String,
    passwort: String,
    imap_host: String,
    imap_port: u16,
) -> Result<Konto, String> {
    konto_anlegen_intern(
        &zustand, name, email, benutzer, passwort, imap_host, imap_port,
    )
    .await
    .map_err(|f| als_meldung(&f))
}

async fn konto_anlegen_intern(
    zustand: &AppZustand,
    name: String,
    email: String,
    benutzer: String,
    passwort: String,
    imap_host: String,
    imap_port: u16,
) -> Result<Konto> {
    let name = name.trim().to_string();
    let email = email.trim().to_string();
    let benutzer = benutzer.trim().to_string();
    let imap_host = imap_host.trim().to_string();
    if name.is_empty() || benutzer.is_empty() || passwort.is_empty() || imap_host.is_empty() {
        anyhow::bail!("Anmeldung abgelehnt: Pflichtfelder fehlen");
    }

    // Erst prüfen, ob die Zugangsdaten stimmen — dann speichern.
    let probe = ImapVerbindung::verbinden(&imap_host, imap_port, &benutzer, &passwort).await?;
    probe.abmelden().await;

    let konto = mit_db(zustand, |conn| {
        db::konto_anlegen(conn, &name, &email, &imap_host, imap_port, &benutzer)
    })?;

    if let Err(fehler) = passwort_speichern(konto.id, passwort).await {
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
pub async fn sync_starten(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    konto_id: i64,
) -> Result<(), String> {
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
    melde_sync(&app, konto_id, "laeuft", None);

    let ergebnis = konto_synchronisieren(&app, &zustand, konto_id).await;

    if let Ok(mut laufend) = zustand.sync_laeuft.lock() {
        laufend.remove(&konto_id);
    }
    match ergebnis {
        Ok(()) => {
            melde_sync(&app, konto_id, "fertig", None);
            Ok(())
        }
        Err(fehler) => {
            let meldung = als_meldung(&fehler);
            melde_sync(&app, konto_id, "fehler", Some(meldung.clone()));
            Err(meldung)
        }
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
            db::ordner_upsert(conn, konto_id, &eintrag.name, &eintrag.anzeige_name)?;
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
    let mail = mit_db(zustand, |conn| db::mail_holen(conn, mail_id))?
        .ok_or_else(|| anyhow!("Mail {mail_id} ist nicht (mehr) im Cache"))?;
    let ordner = mit_db(zustand, |conn| db::ordner_holen(conn, mail.ordner_id))?
        .ok_or_else(|| anyhow!("Ordner der Mail ist nicht (mehr) vorhanden"))?;
    let konto = konto_laden(zustand, ordner.konto_id)?;

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

/// Lädt die externen Bilder einer Mail herunter und liefert das HTML mit
/// eingebetteten `data:`-URIs. Wird bewusst nicht gecacht: Der Nutzer
/// entscheidet pro Anzeige, ob externe Inhalte geladen werden.
async fn mail_bilder_laden_intern(zustand: &AppZustand, mail_id: i64) -> Result<String> {
    let mail = mit_db(zustand, |conn| db::mail_holen(conn, mail_id))?
        .ok_or_else(|| anyhow!("Mail {mail_id} ist nicht (mehr) im Cache"))?;
    let ordner = mit_db(zustand, |conn| db::ordner_holen(conn, mail.ordner_id))?
        .ok_or_else(|| anyhow!("Ordner der Mail ist nicht (mehr) vorhanden"))?;
    let konto = konto_laden(zustand, ordner.konto_id)?;

    // Original frisch vom Server holen — unbereinigtes HTML wird nie gecacht.
    let mut verbindung = verbindung_zum_konto(&konto).await?;
    verbindung.ordner_waehlen(&ordner.name).await?;
    let roh = verbindung.nachricht_laden(mail.uid).await?;
    verbindung.abmelden().await;

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

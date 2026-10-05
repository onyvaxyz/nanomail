//! Abgleich, Live-Update (IDLE) und Terminerinnerungen.

use super::kalender::kalender_termine_intern;
use super::*;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use chrono::{Local, TimeZone};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::db::{self, kalender as db_kalender, NeuerMailKopf, Ordner};
use crate::imap::{idle, parsen, sync};
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

/// Prüft einmal pro Minute, ob ein Termin in den nächsten 30 Minuten
/// beginnt. Nanomail muss dafür laufen; der Kalender-Cache genügt offline.
pub async fn periodische_termin_erinnerungen(app: AppHandle) {
    loop {
        if let Err(fehler) = termin_erinnerungen_pruefen(&app) {
            tracing::warn!("Termin-Erinnerungen prüfen: {fehler:#}");
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

fn liegt_im_erinnerungsfenster(beginn: i64, jetzt: i64) -> bool {
    beginn > jetzt && beginn <= jetzt + TERMIN_ERINNERUNG_SEKUNDEN
}

fn termin_erinnerungen_pruefen(app: &AppHandle) -> Result<()> {
    let jetzt = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("Systemzeit für Termin-Erinnerung")?
        .as_secs() as i64;
    let zustand = app.state::<AppZustand>();
    mit_db(&zustand, |conn| {
        db_kalender::alte_erinnerungen_loeschen(conn, jetzt - 24 * 60 * 60)
    })?;
    let termine = kalender_termine_intern(&zustand, jetzt, jetzt + TERMIN_ERINNERUNG_SEKUNDEN + 1)?;

    for termin in termine
        .into_iter()
        .filter(|t| liegt_im_erinnerungsfenster(t.termin.beginn, jetzt))
    {
        let neu = mit_db(&zustand, |conn| {
            db_kalender::erinnerung_vormerken(
                conn,
                termin.kalender_id,
                &termin.href,
                termin.termin.beginn,
                jetzt,
            )
        })?;
        if !neu {
            continue;
        }

        let uhrzeit = Local
            .timestamp_opt(termin.termin.beginn, 0)
            .single()
            .map(|zeit| zeit.format("%H:%M").to_string())
            .unwrap_or_else(|| "bald".to_string());
        let titel = if termin.termin.titel.trim().is_empty() {
            "Termin".to_string()
        } else {
            termin.termin.titel.clone()
        };
        let mut text = format!("Beginn: {uhrzeit} Uhr");
        if !termin.termin.ort.trim().is_empty() {
            text.push_str(&format!(" · {}", termin.termin.ort.trim()));
        }
        if let Err(fehler) = app
            .notification()
            .builder()
            .title(format!("Terminerinnerung: {titel}"))
            .body(text)
            .show()
        {
            mit_db(&zustand, |conn| {
                db_kalender::erinnerung_zuruecknehmen(
                    conn,
                    termin.kalender_id,
                    &termin.href,
                    termin.termin.beginn,
                )
            })?;
            tracing::warn!("Systembenachrichtigung konnte nicht angezeigt werden: {fehler:#}");
        }
    }
    Ok(())
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

pub(crate) fn idle_stoppen(zustand: &AppZustand, konto_id: i64) {
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

pub(crate) async fn ordner_synchronisieren(
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
                        an: geparst.an,
                        cc: geparst.cc,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erinnerungsfenster_umfasst_die_naechsten_dreissig_minuten() {
        let jetzt = 10_000;
        assert!(!liegt_im_erinnerungsfenster(jetzt, jetzt));
        assert!(liegt_im_erinnerungsfenster(jetzt + 1, jetzt));
        assert!(liegt_im_erinnerungsfenster(
            jetzt + TERMIN_ERINNERUNG_SEKUNDEN,
            jetzt
        ));
        assert!(!liegt_im_erinnerungsfenster(
            jetzt + TERMIN_ERINNERUNG_SEKUNDEN + 1,
            jetzt
        ));
    }
}

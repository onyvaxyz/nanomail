//! Mails: Liste, Lesen, Suche, Loeschen, Anhaenge, Bilder.

use super::*;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use futures::stream::{self, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::caldav::verbindung::CaldavVerbindung;
use crate::caldav::{termine, xml};
use crate::db::{self, kalender as db_kalender, Konto, MailKopf, Ordner};
use crate::smtp::nachricht;
use crate::{anzeige, avatar};
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

/// Volltextsuche im geöffneten Ordner. Der lokale Index wird online durch
/// die Server-Suche ergänzt, damit auch ungeöffnete Mailtexte zählen.
/// Neueste Treffer zuerst.
#[tauri::command]
pub async fn mails_suchen(
    zustand: State<'_, AppZustand>,
    ordner_id: i64,
    eingabe: String,
) -> Result<Vec<db::SuchTreffer>, String> {
    let mut treffer = mit_db(&zustand, |conn| {
        db::mails_suchen(conn, ordner_id, &eingabe, 100)
    })
    .map_err(|f| als_meldung(&f))?;

    // Der lokale Index kann nur bereits geöffnete Mailtexte kennen. Die
    // ergänzende IMAP-Suche findet online auch noch nicht geladene Inhalte,
    // ohne sämtliche Nachrichten samt Anhängen herunterzuladen.
    let kontext = mit_db(&zustand, |conn| {
        let ordner = db::ordner_holen(conn, ordner_id)?
            .ok_or_else(|| anyhow!("Der ausgewählte Ordner ist nicht mehr vorhanden"))?;
        let konto = db::konto_holen(conn, ordner.konto_id)?
            .ok_or_else(|| anyhow!("Das Mail-Konto ist nicht mehr vorhanden"))?;
        Ok((ordner, konto))
    })
    .map_err(|f| als_meldung(&f))?;

    match verbindung_zum_konto(&kontext.1).await {
        Ok(mut verbindung) => {
            let server_treffer = async {
                verbindung.ordner_waehlen(&kontext.0.name).await?;
                verbindung.volltext_suchen(&eingabe).await
            }
            .await;
            verbindung.abmelden().await;
            match server_treffer {
                Ok(uids) => {
                    let weitere = mit_db(&zustand, |conn| {
                        db::mails_zu_uids(conn, ordner_id, &uids, 100)
                    })
                    .map_err(|f| als_meldung(&f))?;
                    for mail in weitere {
                        if !treffer
                            .iter()
                            .any(|vorhanden| vorhanden.kopf.id == mail.kopf.id)
                        {
                            treffer.push(mail);
                        }
                    }
                    treffer.sort_by(|a, b| {
                        b.kopf
                            .datum
                            .cmp(&a.kopf.datum)
                            .then_with(|| b.kopf.uid.cmp(&a.kopf.uid))
                    });
                    treffer.truncate(100);
                }
                Err(fehler) => tracing::warn!("Server-Volltextsuche fehlgeschlagen: {fehler:#}"),
            }
        }
        Err(fehler) => tracing::warn!("Server-Volltextsuche nicht verfügbar: {fehler:#}"),
    }

    Ok(treffer)
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
    /// Der Nutzer hat die Absender-Domain dauerhaft zum Bilderladen erlaubt.
    pub bilder_automatisch: bool,
    /// Anhänge für die Anhang-Leiste (M3.6).
    pub anhaenge: Vec<db::AnhangEintrag>,
    pub einladungen: Vec<termine::Einladung>,
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
    let bilder_automatisch = match avatar::domain(&mail.von_email) {
        Some(domain) => mit_db(zustand, |conn| db::bild_quelle_ist_erlaubt(conn, &domain))?,
        None => false,
    };

    let (inhalt, hat_anhang, server_flag_gesetzt) =
        match mit_db(zustand, |conn| db::inhalt_holen(conn, mail_id))? {
            Some(mut inhalt) => {
                // Bestehenden Cache einmal nachprüfen. Offline bleibt die Mail
                // lesbar und wird beim nächsten Öffnen erneut geprüft.
                if inhalt.kalender.is_none() {
                    if let Ok(roh) = roh_nachricht_laden(&konto, &ordner.name, mail.uid).await {
                        inhalt.kalender = Some(serde_json::to_string(
                            &anzeige::nachricht_aufbereiten(&roh).kalender,
                        )?);
                        mit_db(zustand, |conn| db::inhalt_speichern(conn, mail_id, &inhalt))?;
                    }
                }
                (inhalt, mail.hat_anhang, false)
            }
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
                    kalender: Some(serde_json::to_string(&aufbereitet.kalender)?),
                };
                mit_db(zustand, |conn| {
                    db::inhalt_speichern(conn, mail_id, &inhalt)?;
                    db::anhaenge_speichern(conn, mail_id, &anhang_paare)?;
                    db::mail_setze_hat_anhang(conn, mail_id, aufbereitet.hat_anhang)
                })?;
                (inhalt, aufbereitet.hat_anhang, true)
            }
        };

    let kalender: Vec<String> = serde_json::from_str(inhalt.kalender.as_deref().unwrap_or("[]"))?;
    let einladungen = kalender
        .iter()
        .enumerate()
        .flat_map(|(index, ics)| termine::einladungen(ics, &konto.email, index))
        .collect();

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
        bilder_automatisch,
        anhaenge,
        einladungen,
    })
}

#[tauri::command]
pub async fn mail_einladung_antworten(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    mail_id: i64,
    kalender_index: usize,
    ereignis_index: usize,
    zusage: bool,
) -> Result<String, String> {
    async {
        let (_, _, konto) = mail_kontext(&zustand, mail_id)?;
        let inhalt = mit_db(&zustand, |conn| db::inhalt_holen(conn, mail_id))?
            .context("Bitte die Einladung zuerst öffnen.")?;
        let kalender: Vec<String> =
            serde_json::from_str(inhalt.kalender.as_deref().unwrap_or("[]"))?;
        let ics = kalender.get(kalender_index).context("Einladung fehlt")?;
        let (organisator, antwort) =
            termine::einladung_antwort(ics, ereignis_index, &konto.email, zusage)?;
        let text = if zusage { "Zusage" } else { "Absage" };
        let eingabe = nachricht::KalenderEinladung {
            von_name: konto.anzeigename.clone(),
            von_adresse: konto.email.clone(),
            an: vec![organisator],
            betreff: text.into(),
            text: text.into(),
            ics: antwort,
        };
        let (fertig, roh) = nachricht::baue_kalender_antwort(&eingabe)?;
        let ordner = mit_db(&zustand, |conn| db::ordner_liste(conn, konto.id))?;
        smtp_senden(&konto, fertig).await?;
        if let Some(gesendet) = db::finde_gesendet_ordner(&ordner) {
            if sent_ablage(&app, &zustand, &konto, gesendet, &roh)
                .await
                .is_err()
            {
                return Ok(format!(
                    "{text} versendet; Ablage unter Gesendet fehlgeschlagen. Nicht erneut senden."
                ));
            }
        }
        Ok(format!("{text} versendet."))
    }
    .await
    .map_err(|f: anyhow::Error| als_meldung(&f))
}

/// Übernimmt eine Kalendereinladung aus einer Mail in einen CalDAV-Kalender
/// (Paket B): als eigenständige Kopie samt Serie und Ausnahmen, ohne
/// Antwort-Mail. Erneutes Übernehmen aktualisiert die eigene Kopie.
#[tauri::command]
pub async fn mail_einladung_uebernehmen(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    mail_id: i64,
    kalender_index: usize,
    ereignis_index: usize,
    kalender_id: i64,
) -> Result<String, String> {
    mail_einladung_uebernehmen_intern(
        &app,
        &zustand,
        mail_id,
        kalender_index,
        ereignis_index,
        kalender_id,
    )
    .await
    .map_err(|f| als_meldung(&f))
}

async fn mail_einladung_uebernehmen_intern(
    app: &AppHandle,
    zustand: &AppZustand,
    mail_id: i64,
    kalender_index: usize,
    ereignis_index: usize,
    kalender_id: i64,
) -> Result<String> {
    mail_kontext(zustand, mail_id)?;
    let inhalt = mit_db(zustand, |conn| db::inhalt_holen(conn, mail_id))?
        .context("Bitte die Einladung zuerst öffnen.")?;
    let kalender: Vec<String> = serde_json::from_str(inhalt.kalender.as_deref().unwrap_or("[]"))?;
    let ics = kalender.get(kalender_index).context("Einladung fehlt")?;
    let (uid, export) = termine::einladung_export_ics(ics, ereignis_index)?;
    let (ziel, _, verbindung) = caldav_verbindung_zum_kalender(zustand, kalender_id).await?;
    let datei = uid
        .chars()
        .map(|z| {
            if z.is_ascii_alphanumeric() || z == '-' {
                z
            } else {
                '-'
            }
        })
        .collect::<String>();
    let href = format!("{}{}.ics", ziel.href, datei);
    // Eigene frühere Übernahme aktualisieren, sonst neu anlegen. Liegt das
    // Objekt auf dem Server, aber nicht im Cache (z. B. andere App), gilt
    // der Server-Stand als Ausgangspunkt für die Aktualisierung.
    let etag_db = mit_db(zustand, |conn| {
        db_kalender::termin_etag(conn, ziel.id, &href)
    })?
    .filter(|etag| !etag.is_empty());
    let etag_alt = match etag_db {
        Some(etag) => Some(etag),
        None => verbindung
            .objekte_laden(&ziel.href, std::slice::from_ref(&href))
            .await
            .unwrap_or_default()
            .into_iter()
            .next()
            .map(|objekt| objekt.etag)
            .filter(|etag| !etag.is_empty()),
    };
    let etag_neu = verbindung
        .termin_speichern(&href, &export, etag_alt.as_deref())
        .await?;
    gespeichertes_objekt_cachen(app, zustand, &verbindung, &ziel, href, &export, etag_neu).await?;
    tracing::info!(kalender_id = ziel.id, "Einladung übernommen");
    Ok(format!("Im Kalender „{}“ übernommen.", ziel.anzeige_name))
}

/// Schreibt ein per PUT gespeichertes Objekt in den lokalen Cache und meldet
/// es der Oberfläche (gemeinsam für Termin-Speichern und Einladungs-Übernahme).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn gespeichertes_objekt_cachen(
    app: &AppHandle,
    zustand: &AppZustand,
    verbindung: &CaldavVerbindung,
    kalender: &db_kalender::Kalender,
    href: String,
    ics: &str,
    etag_neu: String,
) -> Result<()> {
    // Liefert der Server keinen ETag, wird das Objekt nachgeladen. Scheitert
    // auch das, gilt der Termin trotzdem als gespeichert — er liegt bereits
    // auf dem Server; den ETag holt der nächste Abgleich nach.
    let objekt = if etag_neu.is_empty() {
        verbindung
            .objekte_laden(&kalender.href, std::slice::from_ref(&href))
            .await
            .unwrap_or_else(|fehler| {
                tracing::warn!("Termin nach dem Speichern nicht nachgeladen: {fehler:#}");
                Vec::new()
            })
            .into_iter()
            .next()
            .unwrap_or_else(|| xml::ObjektDaten {
                href: href.clone(),
                etag: String::new(),
                ics: ics.to_string(),
            })
    } else {
        xml::ObjektDaten {
            href: href.clone(),
            etag: etag_neu,
            ics: ics.to_string(),
        }
    };
    mit_db(zustand, |conn| {
        termin_objekt_speichern(conn, kalender.id, &objekt)
    })?;
    let _ = app.emit("kalender:aktualisiert", ());
    Ok(())
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

/// Legt einen Anhang zum Öffnen mit dem Systemprogramm bereit (Paket D):
/// frisch vom Server geholt, sicher benamst im Zwischenlager, dann über
/// den System-Öffner gestartet. Anhänge liegen nie im lokalen Cache.
#[tauri::command]
pub async fn anhang_oeffnen(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    mail_id: i64,
    index: i64,
) -> Result<String, String> {
    anhang_oeffnen_intern(&app, &zustand, mail_id, index)
        .await
        .map_err(|f| als_meldung(&f))
}

async fn anhang_oeffnen_intern(
    app: &AppHandle,
    zustand: &AppZustand,
    mail_id: i64,
    index: i64,
) -> Result<String> {
    let (mail, ordner, konto) = mail_kontext(zustand, mail_id)?;
    let roh = roh_nachricht_laden(&konto, &ordner.name, mail.uid).await?;
    let (name, daten) =
        anzeige::anhang_daten(&roh, usize::try_from(index).unwrap_or(usize::MAX))
            .ok_or_else(|| nutzerfehler("Der Anhang wurde in der Mail nicht gefunden."))?;
    // Dateiname säubern: nur der reine Name, ohne Pfadanteile.
    let sicher = std::path::Path::new(&name)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("anhang.bin");
    let verzeichnis = std::env::temp_dir().join(format!("nanomail-anhang-{mail_id}-{index}"));
    std::fs::create_dir_all(&verzeichnis)
        .map_err(|f| nutzerfehler(format!("Die Datei ließ sich nicht bereitstellen: {f}")))?;
    let pfad = verzeichnis.join(sicher);
    tokio::fs::write(&pfad, daten)
        .await
        .map_err(|f| nutzerfehler(format!("Die Datei ließ sich nicht bereitstellen: {f}")))?;
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_path(pfad.to_string_lossy(), None::<&str>)
            .map_err(|f| {
                nutzerfehler(format!("Das Systemprogramm ließ sich nicht starten: {f}"))
            })?;
    }
    tracing::info!(mail_id, index, "Anhang geöffnet");
    Ok(format!("„{sicher}“ wird im Systemprogramm geöffnet …"))
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

/// Ergebnis des Sammel-Löschens aus der Mehrfachauswahl.
#[derive(Serialize)]
pub struct LoeschErgebnis {
    pub geloeschte: Vec<i64>,
    pub fehler: Vec<i64>,
}

/// Löscht mehrere Mails in einem Rutsch: Mails desselben Ordners teilen
/// sich eine Server-Verbindung und einen Server-Durchgang, statt dass das
/// Frontend jede Mail einzeln löschen lässt (das war bei vielen Mails
/// spürbar langsam: pro Mail verbinden, wählen, verschieben, abmelden).
#[tauri::command]
pub async fn mails_loeschen(
    app: AppHandle,
    zustand: State<'_, AppZustand>,
    mail_ids: Vec<i64>,
) -> Result<LoeschErgebnis, String> {
    mails_loeschen_intern(&app, &zustand, &mail_ids)
        .await
        .map_err(|f| als_meldung(&f))
}

async fn mails_loeschen_intern(
    app: &AppHandle,
    zustand: &AppZustand,
    mail_ids: &[i64],
) -> Result<LoeschErgebnis> {
    // Zusammengehörige Mails (gleiches Konto, gleicher Ordner) bilden
    // je eine Gruppe — pro Gruppe genügt eine Verbindung.
    struct Gruppe {
        konto: Konto,
        ordner: Ordner,
        papierkorb: Option<Ordner>,
        mails: Vec<MailKopf>,
    }
    let mut gruppen: Vec<Gruppe> = Vec::new();
    let mut fehler: Vec<i64> = Vec::new();
    for &mail_id in mail_ids {
        let (mail, ordner, konto) = match mail_kontext(zustand, mail_id) {
            Ok(kontext) => kontext,
            Err(_) => {
                fehler.push(mail_id);
                continue;
            }
        };
        match gruppen
            .iter_mut()
            .find(|gruppe| gruppe.konto.id == konto.id && gruppe.ordner.id == ordner.id)
        {
            Some(gruppe) => gruppe.mails.push(mail),
            None => {
                let alle_ordner = mit_db(zustand, |conn| db::ordner_liste(conn, konto.id))?;
                let papierkorb = db::finde_papierkorb_ordner(&alle_ordner)
                    .filter(|ziel| ziel.id != ordner.id)
                    .cloned();
                gruppen.push(Gruppe {
                    konto,
                    ordner,
                    papierkorb,
                    mails: vec![mail],
                });
            }
        }
    }

    let mut geloeschte: Vec<i64> = Vec::new();
    for gruppe in &gruppen {
        let uids: Vec<u32> = gruppe.mails.iter().map(|mail| mail.uid).collect();
        let ids: Vec<i64> = gruppe.mails.iter().map(|mail| mail.id).collect();
        let mut verbindung = match verbindung_zum_konto(&gruppe.konto).await {
            Ok(verbindung) => verbindung,
            Err(_) => {
                fehler.extend(ids);
                continue;
            }
        };
        let server_ok = async {
            verbindung.ordner_waehlen(&gruppe.ordner.name).await?;
            match &gruppe.papierkorb {
                Some(ziel) => verbindung.mehrere_verschieben(&uids, &ziel.name).await?,
                None => verbindung.mehrere_endgueltig_loeschen(&uids).await?,
            }
            if let Some(ziel) = &gruppe.papierkorb {
                if let Err(warnung) =
                    ordner_synchronisieren(app, zustand, &mut verbindung, ziel).await
                {
                    tracing::warn!(
                        "Papierkorb-Abgleich nach dem Löschen fehlgeschlagen: {warnung:#}"
                    );
                }
            }
            verbindung.abmelden().await;
            Result::<()>::Ok(())
        }
        .await;
        if server_ok.is_err() {
            fehler.extend(ids);
            continue;
        }
        // Cache sofort nachziehen, damit die Mails aus der Liste verschwinden.
        if mit_db(zustand, |conn| db::mails_ids_entfernen(conn, &ids)).is_err() {
            fehler.extend(ids);
            continue;
        }
        geloeschte.extend(ids);
        let _ = app.emit(
            "mails:neu",
            MailsNeu {
                ordner_id: gruppe.ordner.id,
            },
        );
    }

    tracing::info!(
        anzahl = geloeschte.len(),
        fehler = fehler.len(),
        "Mails per Sammel-Löschen gelöscht"
    );
    Ok(LoeschErgebnis { geloeschte, fehler })
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

/// Erlaubt externe Mail-Bilder dauerhaft für die Domain des Absenders.
/// Die Mail-Adresse selbst wird nicht zusätzlich gespeichert.
#[tauri::command]
pub fn mail_bild_quelle_erlauben(
    zustand: State<'_, AppZustand>,
    mail_id: i64,
) -> Result<(), String> {
    let mail = mit_db(&zustand, |conn| db::mail_holen(conn, mail_id))
        .map_err(|f| als_meldung(&f))?
        .ok_or_else(|| "Die Mail ist nicht mehr vorhanden.".to_string())?;
    let domain = avatar::domain(&mail.von_email)
        .ok_or_else(|| "Für diesen Absender ist keine gültige Domain erkennbar.".to_string())?;
    mit_db(&zustand, |conn| db::bild_quelle_erlauben(conn, &domain)).map_err(|f| als_meldung(&f))
}

/// Mail + Ordner + Konto zu einer Mail-ID aus dem Cache laden.
pub(crate) fn mail_kontext(
    zustand: &AppZustand,
    mail_id: i64,
) -> Result<(MailKopf, Ordner, Konto)> {
    let mail = mit_db(zustand, |conn| db::mail_holen(conn, mail_id))?
        .ok_or_else(|| anyhow!("Mail {mail_id} ist nicht (mehr) im Cache"))?;
    let ordner = mit_db(zustand, |conn| db::ordner_holen(conn, mail.ordner_id))?
        .ok_or_else(|| anyhow!("Ordner der Mail ist nicht (mehr) vorhanden"))?;
    let konto = konto_laden(zustand, ordner.konto_id)?;
    Ok((mail, ordner, konto))
}

/// Holt die Original-Rohbytes einer Mail frisch vom Server.
pub(crate) async fn roh_nachricht_laden(
    konto: &Konto,
    ordner_name: &str,
    uid: u32,
) -> Result<Vec<u8>> {
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

    let mut gesehen = HashSet::new();
    let sichere_urls: Vec<String> = urls
        .iter()
        .take(MAX_BILDER)
        .filter_map(|url| {
            if !url.starts_with("https://") {
                tracing::info!("Bild über unverschlüsseltes HTTP bleibt blockiert");
                return None;
            }
            gesehen.insert(url.as_str()).then(|| url.clone())
        })
        .collect();
    let aufgaben = sichere_urls.into_iter().map(|url| {
        let client = client.clone();
        async move {
            let ergebnis = bild_holen(&client, &url).await;
            (url, ergebnis)
        }
    });
    let mut ergebnisse = stream::iter(aufgaben).buffer_unordered(6);
    while let Some((url, ergebnis)) = ergebnisse.next().await {
        match ergebnis {
            Ok(daten_uri) => {
                geladene.insert(url, daten_uri);
            }
            Err(fehler) => tracing::warn!("Bild-Download fehlgeschlagen: {fehler:#}"),
        }
    }
    geladene
}

async fn bild_holen(client: &reqwest::Client, url: &str) -> Result<String> {
    let mut antwort = client.get(url).send().await.context("Bild anfragen")?;
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
    if antwort
        .content_length()
        .is_some_and(|groesse| groesse > MAX_BILD_BYTES as u64)
    {
        anyhow::bail!("Bild zu groß");
    }
    let mut bytes = Vec::new();
    while let Some(block) = antwort.chunk().await.context("Bild herunterladen")? {
        if bytes.len().saturating_add(block.len()) > MAX_BILD_BYTES {
            anyhow::bail!("Bild zu groß");
        }
        bytes.extend_from_slice(&block);
    }
    let daten = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{daten}"))
}

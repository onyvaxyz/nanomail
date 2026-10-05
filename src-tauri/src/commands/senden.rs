//! Senden, Antworten und Entwuerfe.

use super::*;
use std::collections::HashSet;

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use super::mails::{mail_kontext, roh_nachricht_laden};
use crate::anzeige;
use crate::db::{self, Konto, Ordner};
use crate::imap::parsen;
use crate::smtp::nachricht;
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
    pub cc: String,
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
    allen_antworten: bool,
) -> Result<Vorlage, String> {
    antwort_vorbereiten_intern(&zustand, mail_id, weiterleiten, allen_antworten)
        .await
        .map_err(|f| als_meldung(&f))
}

async fn antwort_vorbereiten_intern(
    zustand: &AppZustand,
    mail_id: i64,
    weiterleiten: bool,
    allen_antworten: bool,
) -> Result<Vorlage> {
    let (mail, ordner, konto) = mail_kontext(zustand, mail_id)?;
    let roh = roh_nachricht_laden(&konto, &ordner.name, mail.uid).await?;
    let daten = parsen::parse_fuer_antwort(&roh);

    if weiterleiten {
        Ok(Vorlage {
            an: String::new(),
            cc: String::new(),
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
        let (an, cc) = if allen_antworten {
            let eigene_adressen: HashSet<String> = mit_db(zustand, db::konten_liste)?
                .into_iter()
                .map(|konto| konto.email.trim().to_lowercase())
                .collect();
            antwort_alle_empfaenger(&daten.antwort_an, &daten.an, &daten.cc, &eigene_adressen)
        } else {
            (daten.antwort_an, String::new())
        };
        Ok(Vorlage {
            an,
            cc,
            betreff: nachricht::antwort_betreff(&daten.betreff),
            text: nachricht::zitat_block(daten.datum, &daten.von_anzeige, &daten.text),
        })
    }
}

/// Empfänger für „Allen antworten“: Absender zuerst, danach ursprüngliche
/// An-/Cc-Adressen; eigene Konten und Dubletten werden entfernt.
fn antwort_alle_empfaenger(
    antwort_an: &str,
    an: &str,
    cc: &str,
    eigene_adressen: &HashSet<String>,
) -> (String, String) {
    let mut empfaenger = Vec::<String>::new();
    for adresse in std::iter::once(antwort_an)
        .chain(an.split(','))
        .chain(cc.split(','))
        .map(str::trim)
        .filter(|adresse| !adresse.is_empty())
    {
        let normalisiert = adresse.to_lowercase();
        if eigene_adressen.contains(&normalisiert)
            || empfaenger
                .iter()
                .any(|vorhanden| vorhanden.eq_ignore_ascii_case(adresse))
        {
            continue;
        }
        empfaenger.push(adresse.to_string());
    }
    let an = empfaenger.first().cloned().unwrap_or_default();
    let cc = empfaenger
        .into_iter()
        .skip(1)
        .collect::<Vec<_>>()
        .join(", ");
    (an, cc)
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
        von_name: konto.anzeigename.clone(),
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
    smtp_senden(&konto, fertig).await?;

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

    // Kopie in den „Gesendet“-Ordner — läuft im Hintergrund weiter: die Mail
    // ist bereits verschickt, das Verfassen-Fenster soll sich sofort
    // schließen und nicht auf den Ordner-Abgleich warten (der bei großen
    // „Gesendet“-Ordnern oder einer parallel laufenden Live-Verbindung
    // spürbar dauern kann). Schlägt die Ablage fehl, bleibt die Mail
    // trotzdem gesendet — der nächste normale Sync holt die Kopie nach.
    let alle_ordner = mit_db(zustand, |conn| db::ordner_liste(conn, konto.id))?;
    match db::finde_gesendet_ordner(&alle_ordner).cloned() {
        Some(gesendet) => {
            let app_im_task = app.clone();
            let konto_im_task = konto.clone();
            tauri::async_runtime::spawn(async move {
                let zustand = app_im_task.state::<AppZustand>();
                if let Err(fehler) =
                    sent_ablage(&app_im_task, &zustand, &konto_im_task, &gesendet, &rohbytes).await
                {
                    tracing::error!("Gesendet-Ablage fehlgeschlagen: {fehler:#}");
                }
            });
        }
        None => tracing::warn!(konto_id, "Kein Gesendet-Ordner gefunden"),
    }
    Ok("Mail gesendet.".to_string())
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

pub(crate) async fn sent_ablage(
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
pub(crate) fn adressliste(eingabe: &str) -> Vec<String> {
    eingabe
        .split([',', ';'])
        .map(str::trim)
        .filter(|teil| !teil.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allen_antworten_entfernt_eigene_adressen_und_dubletten() {
        let eigene = HashSet::from([
            "ich@example.org".to_string(),
            "arbeit@example.org".to_string(),
        ]);
        let (an, cc) = antwort_alle_empfaenger(
            "anna@example.org",
            "ich@example.org, BERT@example.org, anna@example.org",
            "bert@example.org, arbeit@example.org, carla@example.org",
            &eigene,
        );
        assert_eq!(an, "anna@example.org");
        assert_eq!(cc, "BERT@example.org, carla@example.org");
    }

    #[test]
    fn allen_antworten_im_gesendet_ordner_nimmt_empfaenger_statt_mich() {
        let eigene = HashSet::from(["ich@example.org".to_string()]);
        let (an, cc) = antwort_alle_empfaenger(
            "ich@example.org",
            "kollege@example.org",
            "chef@example.org",
            &eigene,
        );
        assert_eq!(an, "kollege@example.org");
        assert_eq!(cc, "chef@example.org");
    }
}

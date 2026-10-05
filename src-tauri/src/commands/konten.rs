//! Konten, Ordnerliste und Microsoft-Anmeldung (M6).

use super::*;

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use tauri::{AppHandle, State};

use super::sync::{idle_starten, idle_stoppen};
use crate::db::{self, Konto, Ordner};
use crate::oauth::{self, TokenSatz};
use crate::schluesselbund;
use crate::smtp::versand;
// ---------------------------------------------------------------- Konten --

/// Eingaben des Konto-Dialogs (Anlegen und Bearbeiten).
#[derive(serde::Deserialize)]
pub struct KontoFormular {
    pub name: String,
    #[serde(default)]
    pub anzeigename: String,
    pub email: String,
    pub benutzer: String,
    /// Beim Bearbeiten leer lassen = Passwort unverändert.
    /// Bei Microsoft-Konten immer leer (Token statt Passwort).
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
    /// Anmeldeart: `passwort` oder `microsoft` (M6).
    #[serde(default)]
    pub auth_art: String,
    /// Fertige Microsoft-Anmeldesitzung (nur beim Anlegen/Erneuern).
    #[serde(default)]
    pub ms_sitzung: Option<String>,
}

impl KontoFormular {
    fn bereinigt(mut self) -> Result<Self> {
        self.name = self.name.trim().to_string();
        self.anzeigename = self.anzeigename.trim().to_string();
        self.email = self.email.trim().to_string();
        self.benutzer = self.benutzer.trim().to_string();
        self.imap_host = self.imap_host.trim().to_string();
        self.smtp_host = self.smtp_host.trim().to_string();
        self.farbe = self.farbe.trim().to_lowercase();
        self.auth_art = self.auth_art.trim().to_lowercase();
        if self.auth_art.is_empty() {
            self.auth_art = "passwort".to_string();
        }
        if self.auth_art != "passwort" && self.auth_art != "microsoft" {
            anyhow::bail!("Anmeldung abgelehnt: unbekannte Anmeldeart");
        }
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
            anzeigename: self.anzeigename.clone(),
            email: self.email.clone(),
            imap_host: self.imap_host.clone(),
            imap_port: self.imap_port,
            benutzer: self.benutzer.clone(),
            smtp_host: self.smtp_host.clone(),
            smtp_port: self.smtp_port,
            signatur: self.signatur.clone(),
            farbe: self.farbe.clone(),
            auth_art: self.auth_art.clone(),
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
    if formular.auth_art == "microsoft" {
        return konto_microsoft_anlegen(zustand, formular).await;
    }
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
    // Keyring-Einträge entfernen (blockiert intern → eigener Thread).
    // Beide Anmeldearten aufräumen — ein fehlender Eintrag ist kein Fehler.
    tauri::async_runtime::spawn_blocking(move || {
        let _ = schluesselbund::passwort_loeschen(konto_id);
        schluesselbund::microsoft_token_loeschen(konto_id)
    })
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
    let bisher = konto_laden(zustand, konto_id)?; // muss existieren
    if formular.auth_art == "microsoft" {
        return konto_microsoft_erneuern(zustand, konto_id, bisher, formular).await;
    }
    if bisher.auth_art == "microsoft" {
        anyhow::bail!(nutzerfehler(
            "Ein Microsoft-Konto kann nicht auf Passwort umgestellt werden — bitte das Konto entfernen und neu einrichten."
        ));
    }

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

// ------------------------------------------------- Microsoft-Anmeldung (M6) --
// Device-Code-Flow: Die App zeigt Code + URL, der Nutzer meldet sich im
// Browser an. Fertige Tokens liegen nur im Speicher, bis das Konto
// angelegt/erneuert ist — danach ausschließlich im Schlüsselbund.

/// Verwaiste Anmeldesitzungen verfallen nach 15 Minuten.
const MS_SITZUNG_HALTBARKEIT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

#[derive(Serialize)]
pub struct MsStartAntwort {
    sitzung: String,
    pruef_url: String,
    benutzer_code: String,
    laeuft_ab: u64,
    intervall: u64,
}

#[derive(Serialize)]
pub struct MsAbfrageAntwort {
    fertig: bool,
}

/// Beginnt die Microsoft-Anmeldung: liefert URL + Code für den Browser.
/// Das Frontend fragt danach per `ms_anmeldung_abfragen` nach.
#[tauri::command]
pub async fn ms_anmeldung_starten(
    zustand: State<'_, AppZustand>,
) -> Result<MsStartAntwort, String> {
    (|| async {
        let http = reqwest::Client::new();
        let anfrage = oauth::anmeldung_starten(&http)
            .await
            .map_err(|fehler| nutzerfehler(format!("{fehler:#}")))?;
        let sitzung = uuid::Uuid::new_v4().to_string();
        if let Ok(mut sitzungen) = zustand.ms_anmeldungen.lock() {
            sitzungen.retain(|_, s| s.begonnen.elapsed() < MS_SITZUNG_HALTBARKEIT);
            sitzungen.insert(
                sitzung.clone(),
                MsAnmeldung {
                    geraete_code: anfrage.geraete_code,
                    tokens: None,
                    begonnen: std::time::Instant::now(),
                },
            );
        }
        Ok(MsStartAntwort {
            sitzung,
            pruef_url: anfrage.pruef_url,
            benutzer_code: anfrage.benutzer_code,
            laeuft_ab: anfrage.laeuft_ab_sekunden,
            intervall: anfrage.intervall_sekunden,
        })
    })()
    .await
    .map_err(|f: anyhow::Error| als_meldung(&f))
}

/// Fragt einmal nach, ob die Browser-Anmeldung abgeschlossen ist.
/// Bei `fertig: false` später erneut aufrufen (Abstand: `intervall`).
#[tauri::command]
pub async fn ms_anmeldung_abfragen(
    zustand: State<'_, AppZustand>,
    sitzung: String,
) -> Result<MsAbfrageAntwort, String> {
    (|| async {
        let geraete_code = zustand
            .ms_anmeldungen
            .lock()
            .map_err(|_| anyhow!("Interner Anmeldefehler"))?
            .get(&sitzung)
            .map(|s| s.geraete_code.clone())
            .ok_or_else(|| {
                nutzerfehler("Die Anmeldesitzung ist abgelaufen — bitte erneut starten.")
            })?;
        let http = reqwest::Client::new();
        match oauth::anmeldung_abfragen(&http, &geraete_code, jetzt_unix())
            .await
            .map_err(|fehler| nutzerfehler(format!("{fehler:#}")))?
        {
            oauth::AbfrageStand::Wartet => Ok(MsAbfrageAntwort { fertig: false }),
            oauth::AbfrageStand::Fertig(tokens) => {
                if let Ok(mut sitzungen) = zustand.ms_anmeldungen.lock() {
                    if let Some(eintrag) = sitzungen.get_mut(&sitzung) {
                        eintrag.tokens = Some(tokens);
                    }
                }
                Ok(MsAbfrageAntwort { fertig: true })
            }
        }
    })()
    .await
    .map_err(|f: anyhow::Error| als_meldung(&f))
}

/// Holt die fertigen Tokens einer Sitzung ab (einmalig — danach ist die
/// Sitzung verbraucht).
fn ms_tokens_entnehmen(zustand: &AppZustand, sitzung: &str) -> Result<TokenSatz> {
    zustand
        .ms_anmeldungen
        .lock()
        .map_err(|_| anyhow!("Interner Anmeldefehler"))?
        .remove(sitzung)
        .and_then(|s| s.tokens)
        .ok_or_else(|| {
            nutzerfehler("Die Microsoft-Anmeldung ist nicht abgeschlossen — bitte zuerst im Browser anmelden.")
        })
}

/// Legt ein Microsoft-Konto an: prüft den Zugang per Token, speichert das
/// Konto und legt die Tokens in den Schlüsselbund (Rollback bei Fehlern).
async fn konto_microsoft_anlegen(zustand: &AppZustand, formular: KontoFormular) -> Result<Konto> {
    let sitzung = formular.ms_sitzung.clone().unwrap_or_default();
    if sitzung.is_empty() {
        anyhow::bail!(nutzerfehler(
            "Microsoft-Anmeldung fehlt — bitte zuerst „Mit Microsoft anmelden“ abschließen."
        ));
    }
    let tokens = ms_tokens_entnehmen(zustand, &sitzung)?;
    microsoft_zugang_pruefen(&formular, &tokens.zugang_token).await?;

    let konto = mit_db(zustand, |conn| {
        db::konto_anlegen(conn, &formular.als_daten())
    })?;
    let json = serde_json::to_string(&tokens).context("Microsoft-Token ablegen")?;
    let speichern = tauri::async_runtime::spawn_blocking(move || {
        schluesselbund::microsoft_token_speichern(konto.id, &json)
    })
    .await
    .context("Schlüsselbund-Task abgebrochen")?;
    if let Err(fehler) = speichern {
        // Ohne Tokens im Schlüsselbund ist das Konto nutzlos → zurückrollen.
        let _ = mit_db(zustand, |conn| {
            conn.execute("DELETE FROM konten WHERE id = ?1", [konto.id])
                .context("Konto zurückrollen")?;
            Ok(())
        });
        return Err(fehler).context("Microsoft-Token speichern")?;
    }
    tracing::info!(konto_id = konto.id, "Microsoft-Konto angelegt");
    Ok(konto)
}

/// Erneuert ein Microsoft-Konto: neue Sitzung ersetzt die Tokens,
/// ohne Sitzung werden die bestehenden (ggf. aufgefrischt) geprüft.
async fn konto_microsoft_erneuern(
    zustand: &AppZustand,
    konto_id: i64,
    bisher: Konto,
    formular: KontoFormular,
) -> Result<Konto> {
    let sitzung = formular.ms_sitzung.clone().unwrap_or_default();
    let tokens = if sitzung.is_empty() {
        if bisher.auth_art != "microsoft" {
            anyhow::bail!(nutzerfehler(
                "Für die Umstellung bitte einmalig „Mit Microsoft anmelden“ abschließen."
            ));
        }
        // Bestehende Tokens laden (frischt bei Bedarf auf) und prüfen.
        let zugang = microsoft_zugang_token(konto_id).await?;
        let json = tauri::async_runtime::spawn_blocking(move || {
            schluesselbund::microsoft_token_holen(konto_id)
        })
        .await
        .context("Schlüsselbund-Task abgebrochen")??;
        let mut satz: TokenSatz = serde_json::from_str(&json).context("Microsoft-Token lesen")?;
        satz.zugang_token = zugang;
        satz
    } else {
        ms_tokens_entnehmen(zustand, &sitzung)?
    };
    microsoft_zugang_pruefen(&formular, &tokens.zugang_token).await?;

    mit_db(zustand, |conn| {
        db::konto_aktualisieren(conn, konto_id, &formular.als_daten())
    })?;
    if !sitzung.is_empty() {
        let json = serde_json::to_string(&tokens).context("Microsoft-Token ablegen")?;
        tauri::async_runtime::spawn_blocking(move || {
            schluesselbund::microsoft_token_speichern(konto_id, &json)
        })
        .await
        .context("Schlüsselbund-Task abgebrochen")??;
    }
    tracing::info!(konto_id, "Microsoft-Konto aktualisiert");
    konto_laden(zustand, konto_id)
}

/// Prüft IMAP- und SMTP-Zugang eines Microsoft-Kontos per Token,
/// bevor irgendetwas gespeichert wird.
async fn microsoft_zugang_pruefen(formular: &KontoFormular, token: &str) -> Result<()> {
    ImapVerbindung::verbinden_mit_token(
        &formular.imap_host,
        formular.imap_port,
        &formular.benutzer,
        token,
    )
    .await
    .map_err(|fehler| {
        nutzerfehler(format!(
            "Microsoft-Anmeldung fehlgeschlagen ({fehler:#}) — Benutzername ist meist die vollständige E-Mail-Adresse."
        ))
    })?
    .abmelden()
    .await;
    versand::probe_mit_token(
        &formular.smtp_host,
        formular.smtp_port,
        &formular.benutzer,
        token,
    )
    .await
    .map_err(|fehler| {
        nutzerfehler(format!(
            "Versand-Server nicht erreichbar ({fehler:#}) — bitte Adresse und Port prüfen."
        ))
    })
}

#[tauri::command]
pub fn konten_liste(zustand: State<'_, AppZustand>) -> Result<Vec<Konto>, String> {
    mit_db(&zustand, db::konten_liste).map_err(|f| als_meldung(&f))
}

/// Speichert die per Ziehen geänderte Konto-Reihenfolge (Paket C):
/// `ids` in der gewünschten Reihenfolge (erste = oben in der Icon-Leiste).
#[tauri::command]
pub fn konten_reihenfolge(zustand: State<'_, AppZustand>, ids: Vec<i64>) -> Result<(), String> {
    mit_db(&zustand, |conn| db::konten_reihenfolge(conn, &ids)).map_err(|f| als_meldung(&f))
}

// ---------------------------------------------------------------- Ordner --

#[tauri::command]
pub fn ordner_liste(zustand: State<'_, AppZustand>, konto_id: i64) -> Result<Vec<Ordner>, String> {
    mit_db(&zustand, |conn| db::ordner_liste(conn, konto_id)).map_err(|f| als_meldung(&f))
}

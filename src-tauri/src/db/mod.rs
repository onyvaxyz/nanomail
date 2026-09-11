//! Lokale Datenhaltung mit SQLite/`rusqlite` (ab Meilenstein M1).
//!
//! SQLite ist ausschließlich **Cache** — Quelle der Wahrheit ist der
//! IMAP-Server. Passwörter liegen nie hier, sondern im GNOME Keyring.
//! Ablage nach XDG-Standard unter `~/.local/share/nanomail/nanomail.db`.

pub mod kalender;

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// Ein eingerichtetes Mail-Konto (ohne Passwort — das liegt im Keyring).
#[derive(Debug, Clone, Serialize)]
pub struct Konto {
    pub id: i64,
    pub name: String,
    /// Anzeigename im From:-Header ausgehender Mails (leer = nur Adresse).
    pub anzeigename: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub benutzer: String,
    /// Leer = SMTP noch nicht eingerichtet (Konto aus M1-Bestand).
    pub smtp_host: String,
    pub smtp_port: u16,
    /// Signatur, die beim Verfassen unter die Mail gesetzt wird (leer = keine).
    pub signatur: String,
    /// Akzentfarbe des Kontos als Hex-Wert, z. B. `#c678dd` (leer = Standard).
    pub farbe: String,
}

/// Ein IMAP-Ordner samt Cache-Stand und Zählern für die Anzeige.
#[derive(Debug, Clone, Serialize)]
pub struct Ordner {
    pub id: i64,
    pub konto_id: i64,
    /// IMAP-interner Name (z. B. `INBOX` oder `INBOX/Rechnungen`).
    pub name: String,
    pub anzeige_name: String,
    #[serde(skip)]
    pub uidvalidity: Option<u32>,
    /// Sonderrolle laut Server (IMAP SPECIAL-USE), z. B. `gesendet`.
    pub rolle: Option<String>,
    pub gesamt: i64,
    pub ungelesen: i64,
}

/// Kopfzeilen einer Mail für die Listenansicht.
#[derive(Debug, Clone, Serialize)]
pub struct MailKopf {
    pub id: i64,
    pub ordner_id: i64,
    pub uid: u32,
    pub betreff: String,
    /// Anzeigename des Absenders (Name, sonst Adresse).
    pub von: String,
    /// Reine Absenderadresse (für Avatare); kann leer sein.
    pub von_email: String,
    /// An-Empfänger (Adressen, kommagetrennt) — für Gesendet-Liste & Lesen.
    pub an: String,
    /// Cc-Empfänger (Adressen, kommagetrennt).
    pub cc: String,
    /// Unix-Sekunden (UTC); `None`, wenn die Mail kein lesbares Datum hat.
    pub datum: Option<i64>,
    pub gelesen: bool,
    /// IMAP \Answered — auf die Mail wurde geantwortet.
    pub beantwortet: bool,
    pub hat_anhang: bool,
}

/// Neu einzutragende Kopfzeilen (vor dem Insert gibt es noch keine `id`).
#[derive(Debug, Clone)]
pub struct NeuerMailKopf {
    pub uid: u32,
    pub betreff: String,
    pub von: String,
    pub von_email: String,
    pub an: String,
    pub cc: String,
    pub datum: Option<i64>,
    pub gelesen: bool,
    pub beantwortet: bool,
    pub hat_anhang: bool,
}

/// Gecachter, bereits aufbereiteter Mail-Inhalt.
/// Es wird ausschließlich bereinigtes HTML gespeichert — unbereinigtes
/// Original-HTML landet nie in der Datenbank.
#[derive(Debug, Clone)]
pub struct MailInhalt {
    pub text: String,
    /// Mit `ammonia` bereinigtes HTML — nur das darf angezeigt werden.
    pub html_bereinigt: Option<String>,
    pub hatte_externe_bilder: bool,
    /// JSON-Liste dekodierter ICS-Teile; None = älterer Cache, noch ungeprüft.
    pub kalender: Option<String>,
}

/// Öffnet (bzw. erzeugt) die Datenbank und bringt das Schema auf Stand.
pub fn oeffnen(pfad: &Path) -> Result<Connection> {
    if let Some(eltern) = pfad.parent() {
        std::fs::create_dir_all(eltern)
            .with_context(|| format!("Datenverzeichnis {} anlegen", eltern.display()))?;
    }
    let conn =
        Connection::open(pfad).with_context(|| format!("Datenbank {} öffnen", pfad.display()))?;
    einrichten(&conn)?;
    Ok(conn)
}

/// In-Memory-Datenbank für Tests.
#[cfg(test)]
pub fn oeffnen_im_speicher() -> Result<Connection> {
    let conn = Connection::open_in_memory().context("In-Memory-Datenbank öffnen")?;
    einrichten(&conn)?;
    Ok(conn)
}

fn einrichten(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("WAL-Modus aktivieren")?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .context("Fremdschlüssel aktivieren")?;
    migrieren(conn)
}

/// Führt alle noch fehlenden Migrationen aus (fortlaufende Versionsnummer).
fn migrieren(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)")
        .context("schema_version anlegen")?;
    let version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |z| z.get(0),
        )
        .context("Schema-Version lesen")?;

    if version < 1 {
        conn.execute_batch(
            r#"
            CREATE TABLE konten (
                id          INTEGER PRIMARY KEY,
                name        TEXT NOT NULL,
                email       TEXT NOT NULL,
                imap_host   TEXT NOT NULL,
                imap_port   INTEGER NOT NULL DEFAULT 993,
                benutzer    TEXT NOT NULL
            );
            CREATE TABLE ordner (
                id           INTEGER PRIMARY KEY,
                konto_id     INTEGER NOT NULL REFERENCES konten(id) ON DELETE CASCADE,
                name         TEXT NOT NULL,
                anzeige_name TEXT NOT NULL,
                uidvalidity  INTEGER,
                UNIQUE(konto_id, name)
            );
            CREATE TABLE mails (
                id         INTEGER PRIMARY KEY,
                ordner_id  INTEGER NOT NULL REFERENCES ordner(id) ON DELETE CASCADE,
                uid        INTEGER NOT NULL,
                betreff    TEXT NOT NULL DEFAULT '',
                von        TEXT NOT NULL DEFAULT '',
                datum      INTEGER,
                gelesen    INTEGER NOT NULL DEFAULT 0,
                hat_anhang INTEGER NOT NULL DEFAULT 0,
                UNIQUE(ordner_id, uid)
            );
            CREATE INDEX mails_liste_idx ON mails(ordner_id, datum DESC, uid DESC);
            CREATE TABLE mail_bodies (
                mail_id              INTEGER PRIMARY KEY REFERENCES mails(id) ON DELETE CASCADE,
                text                 TEXT NOT NULL DEFAULT '',
                html_bereinigt       TEXT,
                hatte_externe_bilder INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO schema_version (version) VALUES (1);
            "#,
        )
        .context("Migration 1 ausführen")?;
    }
    if version < 2 {
        conn.execute_batch(
            r#"
            ALTER TABLE konten ADD COLUMN smtp_host TEXT NOT NULL DEFAULT '';
            ALTER TABLE konten ADD COLUMN smtp_port INTEGER NOT NULL DEFAULT 465;
            ALTER TABLE ordner ADD COLUMN rolle TEXT;
            INSERT INTO schema_version (version) VALUES (2);
            "#,
        )
        .context("Migration 2 ausführen")?;
    }
    if version < 3 {
        conn.execute_batch(
            r#"
            CREATE TABLE absender_avatar (
                email     TEXT PRIMARY KEY,
                data_uri  TEXT,          -- NULL = kein Bild, Frontend zeigt Initialen
                geholt_am INTEGER NOT NULL
            );
            -- Absenderadresse getrennt vom Anzeigenamen (für Avatare).
            ALTER TABLE mails ADD COLUMN von_email TEXT NOT NULL DEFAULT '';
            INSERT INTO schema_version (version) VALUES (3);
            "#,
        )
        .context("Migration 3 ausführen")?;
    }
    if version < 4 {
        conn.execute_batch(
            r#"
            ALTER TABLE konten ADD COLUMN signatur TEXT NOT NULL DEFAULT '';
            ALTER TABLE konten ADD COLUMN farbe TEXT NOT NULL DEFAULT '';
            INSERT INTO schema_version (version) VALUES (4);
            "#,
        )
        .context("Migration 4 ausführen")?;
    }
    if version < 5 {
        conn.execute_batch(
            r#"
            -- Empfänger bisheriger Mails, für die Adress-Vorschläge beim
            -- Verfassen (M3.4). Kein Adressbuch — wird beim Senden befüllt.
            CREATE TABLE bekannte_adressen (
                email             TEXT PRIMARY KEY,
                name              TEXT NOT NULL DEFAULT '',
                zuletzt_verwendet INTEGER NOT NULL
            );
            INSERT INTO schema_version (version) VALUES (5);
            "#,
        )
        .context("Migration 5 ausführen")?;
    }
    if version < 6 {
        conn.execute_batch(
            r#"
            -- Volltext-Suchindex (M3.5): Betreff/Absender aller Mails plus
            -- Mailtext, sobald er im Cache liegt (mail_bodies). Trigger
            -- halten den Index automatisch aktuell; rowid = mails.id.
            CREATE VIRTUAL TABLE mails_fts USING fts5(betreff, von, von_email, text);
            INSERT INTO mails_fts (rowid, betreff, von, von_email, text)
                SELECT m.id, m.betreff, m.von, m.von_email, COALESCE(b.text, '')
                FROM mails m LEFT JOIN mail_bodies b ON b.mail_id = m.id;
            CREATE TRIGGER mails_fts_einfuegen AFTER INSERT ON mails BEGIN
                INSERT INTO mails_fts (rowid, betreff, von, von_email, text)
                VALUES (new.id, new.betreff, new.von, new.von_email, '');
            END;
            CREATE TRIGGER mails_fts_loeschen AFTER DELETE ON mails BEGIN
                DELETE FROM mails_fts WHERE rowid = old.id;
            END;
            CREATE TRIGGER mails_fts_text_einfuegen AFTER INSERT ON mail_bodies BEGIN
                UPDATE mails_fts SET text = new.text WHERE rowid = new.mail_id;
            END;
            CREATE TRIGGER mails_fts_text_aendern AFTER UPDATE ON mail_bodies BEGIN
                UPDATE mails_fts SET text = new.text WHERE rowid = new.mail_id;
            END;
            INSERT INTO schema_version (version) VALUES (6);
            "#,
        )
        .context("Migration 6 ausführen")?;
    }
    if version < 7 {
        conn.execute_batch(
            r#"
            -- Anhang-Namen und -Größen für die Anhang-Leiste (M3.6);
            -- wird beim ersten Öffnen einer Mail gefüllt. Die Inhalte
            -- selbst bleiben auf dem Server.
            CREATE TABLE mail_anhaenge (
                mail_id   INTEGER NOT NULL REFERENCES mails(id) ON DELETE CASCADE,
                idx       INTEGER NOT NULL,
                dateiname TEXT NOT NULL,
                groesse   INTEGER NOT NULL,
                PRIMARY KEY (mail_id, idx)
            );
            -- Das Anhang-Kennzeichen wurde bisher erst beim Öffnen einer
            -- Mail erkannt und ist im Bestand unvollständig. Der Cache
            -- wird einmalig geleert; der nächste Abgleich lädt alle
            -- Kopfzeilen mit korrektem Kennzeichen neu.
            DELETE FROM mails;
            INSERT INTO schema_version (version) VALUES (7);
            "#,
        )
        .context("Migration 7 ausführen")?;
    }
    if version < 8 {
        conn.execute_batch(
            r#"
            -- Kalender (M4): Nextcloud-CalDAV, getrennt von den Mail-Konten.
            -- SQLite ist auch hier nur Cache — Quelle der Wahrheit ist der
            -- CalDAV-Server; App-Passwörter liegen im Schlüsselbund.
            CREATE TABLE kalender_konten (
                id       INTEGER PRIMARY KEY,
                name     TEXT NOT NULL,
                server   TEXT NOT NULL, -- Basis-Adresse, z. B. https://cloud.example.com
                benutzer TEXT NOT NULL
            );
            CREATE TABLE kalender (
                id           INTEGER PRIMARY KEY,
                konto_id     INTEGER NOT NULL REFERENCES kalender_konten(id) ON DELETE CASCADE,
                href         TEXT NOT NULL,  -- Collection-Pfad auf dem Server
                anzeige_name TEXT NOT NULL,
                farbe_server TEXT NOT NULL DEFAULT '', -- Farbe laut Nextcloud
                farbe_eigen  TEXT NOT NULL DEFAULT '', -- Überschreibt die Server-Farbe
                sichtbar     INTEGER NOT NULL DEFAULT 1,
                sync_token   TEXT NOT NULL DEFAULT '',
                UNIQUE(konto_id, href)
            );
            -- Termin-Objekte als Roh-ICS (nötig für Wiederholungsregeln samt
            -- Ausnahmen); beginn/ende (UTC-Sekunden) dienen der schnellen
            -- Bereichsabfrage einfacher Termine.
            CREATE TABLE termine (
                id               INTEGER PRIMARY KEY,
                kalender_id      INTEGER NOT NULL REFERENCES kalender(id) ON DELETE CASCADE,
                href             TEXT NOT NULL,
                etag             TEXT NOT NULL DEFAULT '',
                ics              TEXT NOT NULL,
                beginn           INTEGER,
                ende             INTEGER,
                hat_wiederholung INTEGER NOT NULL DEFAULT 0,
                UNIQUE(kalender_id, href)
            );
            CREATE INDEX termine_bereich_idx ON termine(kalender_id, beginn, ende);
            INSERT INTO schema_version (version) VALUES (8);
            "#,
        )
        .context("Migration 8 ausführen")?;
    }
    if version < 9 {
        conn.execute_batch(
            r#"
            -- Beantwortet-Kennzeichen (IMAP \Answered) für die Markierung
            -- in der Mail-Liste; wird beim Sync vom Server übernommen.
            ALTER TABLE mails ADD COLUMN beantwortet INTEGER NOT NULL DEFAULT 0;
            INSERT INTO schema_version (version) VALUES (9);
            "#,
        )
        .context("Migration 9 ausführen")?;
    }
    if version < 10 {
        conn.execute_batch(
            r#"
            -- Anzeigename für ausgehende Mails; Passwörter bleiben weiterhin
            -- ausschließlich im Schlüsselbund.
            ALTER TABLE konten ADD COLUMN anzeigename TEXT NOT NULL DEFAULT '';
            INSERT INTO schema_version (version) VALUES (10);
            "#,
        )
        .context("Migration 10 ausführen")?;
    }
    if version < 11 {
        conn.execute_batch(
            r#"
            -- „Kein Bild"-Einträge einmalig verwerfen: Sie stammten teils aus
            -- vorübergehenden Ladefehlern oder wurden nur für die Absender-
            -- Subdomain geprüft. Nach dem Löschen werden Avatare mit dem
            -- verbesserten Favicon-Fallback (Basis-Domain) neu ermittelt.
            DELETE FROM absender_avatar WHERE data_uri IS NULL;
            INSERT INTO schema_version (version) VALUES (11);
            "#,
        )
        .context("Migration 11 ausführen")?;
    }
    if version < 12 {
        conn.execute_batch(
            r#"
            -- Empfänger (An/Cc) je Mail: für die Gesendet-Liste (an wen ging
            -- die Mail?) und die Cc-Anzeige beim Lesen. Der Cache wird einmalig
            -- geleert, damit der nächste Abgleich die Kopfzeilen mit den neuen
            -- Feldern neu lädt (Konten/Einstellungen bleiben erhalten).
            ALTER TABLE mails ADD COLUMN an TEXT NOT NULL DEFAULT '';
            ALTER TABLE mails ADD COLUMN cc TEXT NOT NULL DEFAULT '';
            DELETE FROM mails;
            INSERT INTO schema_version (version) VALUES (12);
            "#,
        )
        .context("Migration 12 ausführen")?;
    }
    if version < 13 {
        conn.execute_batch(
            r#"
            -- Bereits gezeigte Termin-Erinnerungen. Der konkrete Beginn ist
            -- Teil des Schlüssels, damit jedes Vorkommen einer Serie genau
            -- einmal erinnert wird.
            CREATE TABLE termin_erinnerungen (
                kalender_id      INTEGER NOT NULL REFERENCES kalender(id) ON DELETE CASCADE,
                href             TEXT NOT NULL,
                vorkommen_beginn INTEGER NOT NULL,
                erinnert_am      INTEGER NOT NULL,
                PRIMARY KEY (kalender_id, href, vorkommen_beginn)
            );
            INSERT INTO schema_version (version) VALUES (13);
            "#,
        )
        .context("Migration 13 ausführen")?;
    }
    if version < 14 {
        conn.execute_batch(
            r#"
            -- Domains, deren externe Mail-Bilder der Nutzer ausdrücklich
            -- dauerhaft erlaubt hat. Nur die Domain wird gespeichert.
            CREATE TABLE erlaubte_bild_quellen (
                domain TEXT PRIMARY KEY
            );
            -- Der Favicon-Fallback wechselte von DuckDuckGo zu Google.
            -- Bisher erfolglose Adressen sollen mit der neuen Quelle erneut
            -- geprüft werden; vorhandene echte Bilder bleiben erhalten.
            DELETE FROM absender_avatar WHERE data_uri IS NULL;
            INSERT INTO schema_version (version) VALUES (14);
            "#,
        )
        .context("Migration 14 ausführen")?;
    }
    if version < 15 {
        conn.execute_batch(
            "ALTER TABLE mail_bodies ADD COLUMN kalender TEXT;
            INSERT INTO schema_version (version) VALUES (15);",
        )
        .context("Migration 15 ausführen")?;
    }
    Ok(())
}

// ---------------------------------------------------------------- Konten --

/// Konto-Stammdaten ohne `id` (für Anlegen und Bearbeiten).
#[derive(Debug, Clone)]
pub struct KontoDaten {
    pub name: String,
    pub anzeigename: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub benutzer: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub signatur: String,
    pub farbe: String,
}

pub fn konto_anlegen(conn: &Connection, daten: &KontoDaten) -> Result<Konto> {
    conn.execute(
        "INSERT INTO konten (name, anzeigename, email, imap_host, imap_port, benutzer, smtp_host,
                             smtp_port, signatur, farbe)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            daten.name,
            daten.anzeigename,
            daten.email,
            daten.imap_host,
            daten.imap_port,
            daten.benutzer,
            daten.smtp_host,
            daten.smtp_port,
            daten.signatur,
            daten.farbe
        ],
    )
    .context("Konto speichern")?;
    let id = conn.last_insert_rowid();
    Ok(Konto {
        id,
        name: daten.name.clone(),
        anzeigename: daten.anzeigename.clone(),
        email: daten.email.clone(),
        imap_host: daten.imap_host.clone(),
        imap_port: daten.imap_port,
        benutzer: daten.benutzer.clone(),
        smtp_host: daten.smtp_host.clone(),
        smtp_port: daten.smtp_port,
        signatur: daten.signatur.clone(),
        farbe: daten.farbe.clone(),
    })
}

pub fn konto_aktualisieren(conn: &Connection, id: i64, daten: &KontoDaten) -> Result<()> {
    conn.execute(
        "UPDATE konten SET name = ?2, anzeigename = ?3, email = ?4, imap_host = ?5,
                           imap_port = ?6, benutzer = ?7, smtp_host = ?8,
                           smtp_port = ?9, signatur = ?10, farbe = ?11
         WHERE id = ?1",
        params![
            id,
            daten.name,
            daten.anzeigename,
            daten.email,
            daten.imap_host,
            daten.imap_port,
            daten.benutzer,
            daten.smtp_host,
            daten.smtp_port,
            daten.signatur,
            daten.farbe
        ],
    )
    .context("Konto aktualisieren")?;
    Ok(())
}

pub fn konten_liste(conn: &Connection) -> Result<Vec<Konto>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, anzeigename, email, imap_host, imap_port, benutzer, smtp_host,
                    smtp_port, signatur, farbe
             FROM konten ORDER BY id",
        )
        .context("Konten abfragen")?;
    let konten = stmt
        .query_map([], zeile_zu_konto)
        .context("Konten lesen")?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(konten)
}

pub fn konto_holen(conn: &Connection, id: i64) -> Result<Option<Konto>> {
    conn.query_row(
        "SELECT id, name, anzeigename, email, imap_host, imap_port, benutzer, smtp_host,
                smtp_port, signatur, farbe
         FROM konten WHERE id = ?1",
        params![id],
        zeile_zu_konto,
    )
    .optional()
    .context("Konto lesen")
}

fn zeile_zu_konto(zeile: &rusqlite::Row<'_>) -> rusqlite::Result<Konto> {
    Ok(Konto {
        id: zeile.get(0)?,
        name: zeile.get(1)?,
        anzeigename: zeile.get(2)?,
        email: zeile.get(3)?,
        imap_host: zeile.get(4)?,
        imap_port: zeile.get(5)?,
        benutzer: zeile.get(6)?,
        smtp_host: zeile.get(7)?,
        smtp_port: zeile.get(8)?,
        signatur: zeile.get(9)?,
        farbe: zeile.get(10)?,
    })
}

// ---------------------------------------------------------------- Ordner --

/// Legt einen Ordner an oder aktualisiert Anzeigename und Rolle.
/// Cache-Stand (uidvalidity) bleibt dabei unangetastet.
pub fn ordner_upsert(
    conn: &Connection,
    konto_id: i64,
    name: &str,
    anzeige_name: &str,
    rolle: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO ordner (konto_id, name, anzeige_name, rolle) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(konto_id, name) DO UPDATE SET
             anzeige_name = excluded.anzeige_name,
             rolle = excluded.rolle",
        params![konto_id, name, anzeige_name, rolle],
    )
    .context("Ordner speichern")?;
    let id = conn
        .query_row(
            "SELECT id FROM ordner WHERE konto_id = ?1 AND name = ?2",
            params![konto_id, name],
            |z| z.get(0),
        )
        .context("Ordner-ID lesen")?;
    Ok(id)
}

/// Entfernt Ordner, die es auf dem Server nicht mehr gibt.
pub fn ordner_bereinigen(conn: &Connection, konto_id: i64, server_namen: &[String]) -> Result<()> {
    let vorhandene: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, name FROM ordner WHERE konto_id = ?1")?;
        let zeilen = stmt
            .query_map(params![konto_id], |z| Ok((z.get(0)?, z.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        zeilen
    };
    for (id, name) in vorhandene {
        if !server_namen.contains(&name) {
            conn.execute("DELETE FROM ordner WHERE id = ?1", params![id])
                .context("verwaisten Ordner löschen")?;
        }
    }
    Ok(())
}

pub fn ordner_liste(conn: &Connection, konto_id: i64) -> Result<Vec<Ordner>> {
    let mut stmt = conn
        .prepare(
            "SELECT o.id, o.konto_id, o.name, o.anzeige_name, o.uidvalidity, o.rolle,
                    (SELECT COUNT(*) FROM mails m WHERE m.ordner_id = o.id),
                    (SELECT COUNT(*) FROM mails m WHERE m.ordner_id = o.id AND m.gelesen = 0)
             FROM ordner o WHERE o.konto_id = ?1
             ORDER BY (o.name != 'INBOX'), o.name",
        )
        .context("Ordner abfragen")?;
    let ordner = stmt
        .query_map(params![konto_id], zeile_zu_ordner)
        .context("Ordner lesen")?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ordner)
}

pub fn ordner_holen(conn: &Connection, id: i64) -> Result<Option<Ordner>> {
    conn.query_row(
        "SELECT o.id, o.konto_id, o.name, o.anzeige_name, o.uidvalidity, o.rolle,
                (SELECT COUNT(*) FROM mails m WHERE m.ordner_id = o.id),
                (SELECT COUNT(*) FROM mails m WHERE m.ordner_id = o.id AND m.gelesen = 0)
         FROM ordner o WHERE o.id = ?1",
        params![id],
        zeile_zu_ordner,
    )
    .optional()
    .context("Ordner lesen")
}

fn zeile_zu_ordner(zeile: &rusqlite::Row<'_>) -> rusqlite::Result<Ordner> {
    Ok(Ordner {
        id: zeile.get(0)?,
        konto_id: zeile.get(1)?,
        name: zeile.get(2)?,
        anzeige_name: zeile.get(3)?,
        uidvalidity: zeile.get(4)?,
        rolle: zeile.get(5)?,
        gesamt: zeile.get(6)?,
        ungelesen: zeile.get(7)?,
    })
}

/// Findet den „Gesendet“-Ordner: bevorzugt die Server-Rolle (SPECIAL-USE),
/// sonst über gängige Namen.
pub fn finde_gesendet_ordner(ordner: &[Ordner]) -> Option<&Ordner> {
    const NAMEN: [&str; 5] = [
        "gesendet",
        "sent",
        "sent items",
        "sent messages",
        "gesendete objekte",
    ];
    ordner
        .iter()
        .find(|o| o.rolle.as_deref() == Some("gesendet"))
        .or_else(|| {
            ordner.iter().find(|o| {
                let kurzname = o.name.rsplit(['/', '.']).next().unwrap_or(&o.name);
                NAMEN.contains(&kurzname.to_lowercase().as_str())
            })
        })
}

/// Findet den Entwürfe-Ordner: bevorzugt die Server-Rolle (SPECIAL-USE),
/// sonst über gängige Namen.
pub fn finde_entwuerfe_ordner(ordner: &[Ordner]) -> Option<&Ordner> {
    const NAMEN: [&str; 3] = ["drafts", "entwürfe", "entwuerfe"];
    ordner
        .iter()
        .find(|o| o.rolle.as_deref() == Some("entwuerfe"))
        .or_else(|| {
            ordner.iter().find(|o| {
                let kurzname = o.name.rsplit(['/', '.']).next().unwrap_or(&o.name);
                NAMEN.contains(&kurzname.to_lowercase().as_str())
            })
        })
}

/// Findet den Papierkorb: bevorzugt die Server-Rolle (SPECIAL-USE),
/// sonst über gängige Namen.
pub fn finde_papierkorb_ordner(ordner: &[Ordner]) -> Option<&Ordner> {
    const NAMEN: [&str; 6] = [
        "trash",
        "papierkorb",
        "deleted items",
        "deleted messages",
        "gelöscht",
        "gelöschte elemente",
    ];
    ordner
        .iter()
        .find(|o| o.rolle.as_deref() == Some("papierkorb"))
        .or_else(|| {
            ordner.iter().find(|o| {
                let kurzname = o.name.rsplit(['/', '.']).next().unwrap_or(&o.name);
                NAMEN.contains(&kurzname.to_lowercase().as_str())
            })
        })
}

/// UIDVALIDITY hat sich geändert: kompletten Ordner-Cache verwerfen.
pub fn ordner_cache_verwerfen(
    conn: &Connection,
    ordner_id: i64,
    neue_uidvalidity: u32,
) -> Result<()> {
    conn.execute("DELETE FROM mails WHERE ordner_id = ?1", params![ordner_id])
        .context("Mail-Cache leeren")?;
    conn.execute(
        "UPDATE ordner SET uidvalidity = ?2 WHERE id = ?1",
        params![ordner_id, neue_uidvalidity],
    )
    .context("Ordner-Stand zurücksetzen")?;
    Ok(())
}

pub fn ordner_setze_uidvalidity(conn: &Connection, ordner_id: i64, uidvalidity: u32) -> Result<()> {
    conn.execute(
        "UPDATE ordner SET uidvalidity = ?2 WHERE id = ?1",
        params![ordner_id, uidvalidity],
    )
    .context("UIDVALIDITY speichern")?;
    Ok(())
}

// ----------------------------------------------------------------- Mails --

pub fn mails_einfuegen(conn: &Connection, ordner_id: i64, koepfe: &[NeuerMailKopf]) -> Result<()> {
    let mut stmt = conn
        .prepare(
            "INSERT INTO mails (ordner_id, uid, betreff, von, von_email, an, cc, datum, gelesen, beantwortet, hat_anhang)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(ordner_id, uid) DO UPDATE
                 SET gelesen = excluded.gelesen, beantwortet = excluded.beantwortet",
        )
        .context("Mail-Insert vorbereiten")?;
    for kopf in koepfe {
        stmt.execute(params![
            ordner_id,
            kopf.uid,
            kopf.betreff,
            kopf.von,
            kopf.von_email,
            kopf.an,
            kopf.cc,
            kopf.datum,
            kopf.gelesen,
            kopf.beantwortet,
            kopf.hat_anhang
        ])
        .context("Mail-Kopf speichern")?;
    }
    Ok(())
}

pub fn mails_loeschen(conn: &Connection, ordner_id: i64, uids: &[u32]) -> Result<()> {
    let mut stmt = conn
        .prepare("DELETE FROM mails WHERE ordner_id = ?1 AND uid = ?2")
        .context("Mail-Delete vorbereiten")?;
    for uid in uids {
        stmt.execute(params![ordner_id, uid])
            .context("Mail löschen")?;
    }
    Ok(())
}

pub fn mails_flags_setzen(
    conn: &Connection,
    ordner_id: i64,
    aenderungen: &[(u32, bool, bool)],
) -> Result<()> {
    let mut stmt = conn
        .prepare(
            "UPDATE mails SET gelesen = ?3, beantwortet = ?4 WHERE ordner_id = ?1 AND uid = ?2",
        )
        .context("Flag-Update vorbereiten")?;
    for (uid, gelesen, beantwortet) in aenderungen {
        stmt.execute(params![ordner_id, uid, gelesen, beantwortet])
            .context("Flags speichern")?;
    }
    Ok(())
}

/// Alle gecachten UIDs eines Ordners mit Gelesen- und Beantwortet-Flag
/// (für den Sync-Abgleich).
pub fn mails_cache_stand(conn: &Connection, ordner_id: i64) -> Result<Vec<(u32, bool, bool)>> {
    let mut stmt = conn
        .prepare("SELECT uid, gelesen, beantwortet FROM mails WHERE ordner_id = ?1 ORDER BY uid")
        .context("Cache-Stand abfragen")?;
    let stand = stmt
        .query_map(params![ordner_id], |z| {
            Ok((z.get::<_, i64>(0)? as u32, z.get(1)?, z.get(2)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(stand)
}

pub fn mails_liste(
    conn: &Connection,
    ordner_id: i64,
    nur_ungelesen: bool,
    offset: i64,
    limit: i64,
) -> Result<Vec<MailKopf>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, ordner_id, uid, betreff, von, von_email, an, cc, datum, gelesen, beantwortet, hat_anhang
             FROM mails WHERE ordner_id = ?1 AND (?2 = 0 OR gelesen = 0)
             ORDER BY datum IS NULL, datum DESC, uid DESC
             LIMIT ?3 OFFSET ?4",
        )
        .context("Mail-Liste abfragen")?;
    let mails = stmt
        .query_map(
            params![ordner_id, nur_ungelesen, limit, offset],
            zeile_zu_mailkopf,
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(mails)
}

pub fn mail_holen(conn: &Connection, mail_id: i64) -> Result<Option<MailKopf>> {
    conn.query_row(
        "SELECT id, ordner_id, uid, betreff, von, von_email, an, cc, datum, gelesen, beantwortet, hat_anhang
         FROM mails WHERE id = ?1",
        params![mail_id],
        zeile_zu_mailkopf,
    )
    .optional()
    .context("Mail lesen")
}

fn zeile_zu_mailkopf(zeile: &rusqlite::Row<'_>) -> rusqlite::Result<MailKopf> {
    Ok(MailKopf {
        id: zeile.get(0)?,
        ordner_id: zeile.get(1)?,
        uid: zeile.get::<_, i64>(2)? as u32,
        betreff: zeile.get(3)?,
        von: zeile.get(4)?,
        von_email: zeile.get(5)?,
        an: zeile.get(6)?,
        cc: zeile.get(7)?,
        datum: zeile.get(8)?,
        gelesen: zeile.get(9)?,
        beantwortet: zeile.get(10)?,
        hat_anhang: zeile.get(11)?,
    })
}

/// Entfernt eine einzelne Mail aus dem Cache (nach dem Löschen auf dem Server).
pub fn mail_entfernen(conn: &Connection, mail_id: i64) -> Result<()> {
    conn.execute("DELETE FROM mails WHERE id = ?1", params![mail_id])
        .context("Mail aus dem Cache entfernen")?;
    Ok(())
}

/// Setzt das Gelesen-Flag im Cache — in beide Richtungen (Öffnen einer
/// Mail bzw. Kontextmenü „Als (un)gelesen markieren“).
pub fn mail_gelesen_setzen(conn: &Connection, mail_id: i64, gelesen: bool) -> Result<()> {
    conn.execute(
        "UPDATE mails SET gelesen = ?2 WHERE id = ?1",
        params![mail_id, gelesen],
    )
    .context("Gelesen-Flag speichern")?;
    Ok(())
}

/// Merkt im Cache, dass auf eine Mail geantwortet wurde (nach dem Senden
/// einer Antwort — das Server-Flag wird separat gesetzt).
pub fn mail_beantwortet_setzen(conn: &Connection, mail_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE mails SET beantwortet = 1 WHERE id = ?1",
        params![mail_id],
    )
    .context("Beantwortet-Flag speichern")?;
    Ok(())
}

/// Setzt das Anhang-Flag nachträglich (wird beim Laden des Bodies erkannt).
pub fn mail_setze_hat_anhang(conn: &Connection, mail_id: i64, hat_anhang: bool) -> Result<()> {
    conn.execute(
        "UPDATE mails SET hat_anhang = ?2 WHERE id = ?1",
        params![mail_id, hat_anhang],
    )
    .context("Anhang-Flag speichern")?;
    Ok(())
}

// --------------------------------------------------------------- Anhänge --

/// Ein Anhang für die Anhang-Leiste im Lesebereich.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnhangEintrag {
    /// Position innerhalb der Mail — Schlüssel fürs Speichern.
    pub index: i64,
    pub dateiname: String,
    pub groesse: i64,
}

/// Speichert die Anhang-Liste einer Mail (ersetzt einen alten Stand).
pub fn anhaenge_speichern(
    conn: &Connection,
    mail_id: i64,
    anhaenge: &[(String, usize)],
) -> Result<()> {
    conn.execute(
        "DELETE FROM mail_anhaenge WHERE mail_id = ?1",
        params![mail_id],
    )
    .context("Alte Anhang-Liste leeren")?;
    let mut stmt = conn
        .prepare(
            "INSERT INTO mail_anhaenge (mail_id, idx, dateiname, groesse)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .context("Anhang-Insert vorbereiten")?;
    for (index, (dateiname, groesse)) in anhaenge.iter().enumerate() {
        stmt.execute(params![mail_id, index as i64, dateiname, *groesse as i64])
            .context("Anhang speichern")?;
    }
    Ok(())
}

pub fn anhaenge_liste(conn: &Connection, mail_id: i64) -> Result<Vec<AnhangEintrag>> {
    let mut stmt = conn
        .prepare(
            "SELECT idx, dateiname, groesse FROM mail_anhaenge
             WHERE mail_id = ?1 ORDER BY idx",
        )
        .context("Anhänge abfragen")?;
    let anhaenge = stmt
        .query_map(params![mail_id], |z| {
            Ok(AnhangEintrag {
                index: z.get(0)?,
                dateiname: z.get(1)?,
                groesse: z.get(2)?,
            })
        })
        .context("Anhänge lesen")?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(anhaenge)
}

// ---------------------------------------------------------------- Bodies --

pub fn inhalt_holen(conn: &Connection, mail_id: i64) -> Result<Option<MailInhalt>> {
    conn.query_row(
        "SELECT text, html_bereinigt, hatte_externe_bilder, kalender
         FROM mail_bodies WHERE mail_id = ?1",
        params![mail_id],
        |z| {
            Ok(MailInhalt {
                text: z.get(0)?,
                html_bereinigt: z.get(1)?,
                hatte_externe_bilder: z.get(2)?,
                kalender: z.get(3)?,
            })
        },
    )
    .optional()
    .context("Mail-Inhalt lesen")
}

pub fn inhalt_speichern(conn: &Connection, mail_id: i64, inhalt: &MailInhalt) -> Result<()> {
    conn.execute(
        "INSERT INTO mail_bodies (mail_id, text, html_bereinigt, hatte_externe_bilder, kalender)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(mail_id) DO UPDATE SET
             text = excluded.text,
             html_bereinigt = excluded.html_bereinigt,
             hatte_externe_bilder = excluded.hatte_externe_bilder,
             kalender = excluded.kalender",
        params![
            mail_id,
            inhalt.text,
            inhalt.html_bereinigt,
            inhalt.hatte_externe_bilder,
            inhalt.kalender
        ],
    )
    .context("Mail-Inhalt speichern")?;
    Ok(())
}

// ------------------------------------------------------------- Suche --

/// Ein Treffer der Volltextsuche: Mail-Kopf plus Ordnername zur Anzeige.
/// `flatten` legt die Kopf-Felder im JSON direkt auf die oberste Ebene,
/// sodass das Frontend Treffer wie normale Listeneinträge behandelt.
#[derive(Debug, Clone, Serialize)]
pub struct SuchTreffer {
    #[serde(flatten)]
    pub kopf: MailKopf,
    pub ordner_name: String,
}

/// Baut aus der Nutzereingabe eine FTS5-Abfrage: jedes Wort wird zur
/// Präfix-Phrase („"wort"*“), alle Wörter müssen vorkommen. Die Anführungs-
/// zeichen entschärfen zugleich die FTS5-Sonderzeichen der Eingabe.
fn fts_abfrage(eingabe: &str) -> Option<String> {
    let woerter: Vec<String> = eingabe
        .split_whitespace()
        .map(|wort| format!("\"{}\"*", wort.replace('"', "\"\"")))
        .collect();
    if woerter.is_empty() {
        None
    } else {
        Some(woerter.join(" "))
    }
}

/// Volltextsuche in einem Ordner: Betreff, Absender und —
/// soweit im Cache — Mailtext. Neueste Treffer zuerst.
pub fn mails_suchen(
    conn: &Connection,
    ordner_id: i64,
    eingabe: &str,
    limit: i64,
) -> Result<Vec<SuchTreffer>> {
    let Some(abfrage) = fts_abfrage(eingabe) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn
        .prepare(
            "SELECT m.id, m.ordner_id, m.uid, m.betreff, m.von, m.von_email, m.an, m.cc, m.datum,
                    m.gelesen, m.beantwortet, m.hat_anhang, o.anzeige_name
             FROM mails_fts
             JOIN mails m ON m.id = mails_fts.rowid
             JOIN ordner o ON o.id = m.ordner_id
             WHERE mails_fts MATCH ?1 AND o.id = ?2
             ORDER BY m.datum IS NULL, m.datum DESC, m.uid DESC
             LIMIT ?3",
        )
        .context("Suche vorbereiten")?;
    let treffer = stmt
        .query_map(params![abfrage, ordner_id, limit], |zeile| {
            Ok(SuchTreffer {
                kopf: zeile_zu_mailkopf(zeile)?,
                ordner_name: zeile.get(12)?,
            })
        })
        .context("Suche ausführen")?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(treffer)
}

/// Liefert die lokal bekannten Köpfe zu UIDs, die der IMAP-Server bei einer
/// Volltextsuche gefunden hat.
pub fn mails_zu_uids(
    conn: &Connection,
    ordner_id: i64,
    uids: &[u32],
    limit: i64,
) -> Result<Vec<SuchTreffer>> {
    if uids.is_empty() {
        return Ok(Vec::new());
    }
    // UIDs sind vom Server gelieferte Zahlen und können deshalb sicher als
    // Liste eingesetzt werden. So greift auch bei großen Ordnern nicht
    // SQLites Grenze für die Anzahl gebundener Parameter.
    let uid_liste = uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT m.id, m.ordner_id, m.uid, m.betreff, m.von, m.von_email, m.an, m.cc, m.datum,
                m.gelesen, m.beantwortet, m.hat_anhang, o.anzeige_name
         FROM mails m
         JOIN ordner o ON o.id = m.ordner_id
         WHERE o.id = ?1 AND m.uid IN ({uid_liste})
         ORDER BY m.datum IS NULL, m.datum DESC, m.uid DESC
         LIMIT ?2"
    );
    let mut stmt = conn
        .prepare(&sql)
        .context("Server-Suchtreffer vorbereiten")?;
    let treffer = stmt
        .query_map(params![ordner_id, limit], |zeile| {
            Ok(SuchTreffer {
                kopf: zeile_zu_mailkopf(zeile)?,
                ordner_name: zeile.get(12)?,
            })
        })
        .context("Server-Suchtreffer lesen")?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(treffer)
}

// ---------------------------------------------------- Adress-Vorschläge --

/// Ein Vorschlag fürs Empfänger-Feld beim Verfassen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdressVorschlag {
    pub email: String,
    /// Anzeigename, sofern bekannt (sonst leer).
    pub name: String,
}

/// Merkt sich einen Empfänger für die Adress-Vorschläge. Nimmt rohe
/// Feld-Einträge wie „Anna Muster <anna@example.org>“ oder nur die
/// Adresse entgegen; ein leerer Name überschreibt keinen bekannten.
pub fn adresse_merken(conn: &Connection, eintrag: &str) -> Result<()> {
    let (name, email) = zerlege_adresseintrag(eintrag);
    if !email.contains('@') {
        return Ok(()); // kein brauchbarer Eintrag — still ignorieren
    }
    conn.execute(
        "INSERT INTO bekannte_adressen (email, name, zuletzt_verwendet) VALUES (?1, ?2, ?3)
         ON CONFLICT(email) DO UPDATE SET
             name = CASE WHEN excluded.name != '' THEN excluded.name ELSE name END,
             zuletzt_verwendet = excluded.zuletzt_verwendet",
        params![email.to_lowercase(), name, jetzt_sekunden()],
    )
    .context("Adresse merken")?;
    Ok(())
}

/// Zerlegt „Name <adresse>“ in beide Teile; ohne spitze Klammern ist
/// der ganze Eintrag die Adresse.
fn zerlege_adresseintrag(eintrag: &str) -> (String, String) {
    let eintrag = eintrag.trim();
    if let (Some(anfang), Some(ende)) = (eintrag.find('<'), eintrag.rfind('>')) {
        if anfang < ende {
            let name = eintrag[..anfang].trim().trim_matches('"').trim();
            let email = eintrag[anfang + 1..ende].trim();
            return (name.to_string(), email.to_string());
        }
    }
    (String::new(), eintrag.to_string())
}

/// Vorschläge fürs Empfänger-Feld: bereits angeschriebene Adressen plus
/// Absender aus dem Mail-Cache, gefiltert nach der Eingabe (Adresse oder
/// Name), zuletzt genutzte bzw. jüngste zuerst.
pub fn adress_vorschlaege(
    conn: &Connection,
    eingabe: &str,
    limit: usize,
) -> Result<Vec<AdressVorschlag>> {
    let eingabe = eingabe.trim();
    if eingabe.is_empty() {
        return Ok(Vec::new());
    }
    let muster = format!(
        "%{}%",
        eingabe
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );

    let mut quellen: Vec<(String, String, i64)> = Vec::new();
    let mut stmt = conn
        .prepare(
            "SELECT email, name, zuletzt_verwendet FROM bekannte_adressen
             WHERE email LIKE ?1 ESCAPE '\\' OR name LIKE ?1 ESCAPE '\\'
             ORDER BY zuletzt_verwendet DESC LIMIT 100",
        )
        .context("Bekannte Adressen abfragen")?;
    let zeilen = stmt
        .query_map(params![muster], |z| Ok((z.get(0)?, z.get(1)?, z.get(2)?)))
        .context("Bekannte Adressen lesen")?;
    for zeile in zeilen {
        quellen.push(zeile?);
    }
    let mut stmt = conn
        .prepare(
            "SELECT von_email, von, COALESCE(datum, 0) FROM mails
             WHERE von_email != ''
               AND (von_email LIKE ?1 ESCAPE '\\' OR von LIKE ?1 ESCAPE '\\')
             ORDER BY datum DESC LIMIT 100",
        )
        .context("Absender abfragen")?;
    let zeilen = stmt
        .query_map(params![muster], |z| Ok((z.get(0)?, z.get(1)?, z.get(2)?)))
        .context("Absender lesen")?;
    for zeile in zeilen {
        quellen.push(zeile?);
    }

    // Nach Adresse zusammenführen: höchste Priorität gewinnt, ein leerer
    // Name wird durch einen bekannten ergänzt.
    let mut vereint: std::collections::HashMap<String, (String, i64)> =
        std::collections::HashMap::new();
    for (email, name, prio) in quellen {
        let schluessel = email.to_lowercase();
        if !schluessel.contains('@') {
            continue;
        }
        let eintrag = vereint.entry(schluessel).or_insert((name.clone(), prio));
        eintrag.1 = eintrag.1.max(prio);
        if eintrag.0.is_empty() && !name.is_empty() {
            eintrag.0 = name;
        }
    }
    let mut liste: Vec<(i64, AdressVorschlag)> = vereint
        .into_iter()
        .map(|(email, (name, prio))| (prio, AdressVorschlag { email, name }))
        .collect();
    liste.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.email.cmp(&b.1.email)));
    Ok(liste
        .into_iter()
        .take(limit)
        .map(|(_, vorschlag)| vorschlag)
        .collect())
}

// ---------------------------------------------------------- Avatare --

/// Ein Cache-Treffer: `Some(None)` = „bekannt, kein Bild“ (Initialen),
/// `Some(Some(uri))` = Bild vorhanden, `None` = noch nicht abgefragt.
pub fn avatar_aus_cache(
    conn: &Connection,
    email: &str,
    hoechstalter_sekunden: i64,
) -> Result<Option<Option<String>>> {
    let jetzt = jetzt_sekunden();
    conn.query_row(
        "SELECT data_uri, geholt_am FROM absender_avatar WHERE email = ?1",
        params![email.to_lowercase()],
        |z| Ok((z.get::<_, Option<String>>(0)?, z.get::<_, i64>(1)?)),
    )
    .optional()
    .context("Avatar-Cache lesen")
    .map(|treffer| match treffer {
        Some((uri, geholt_am)) if jetzt - geholt_am <= hoechstalter_sekunden => Some(uri),
        _ => None, // nicht vorhanden oder zu alt → neu holen
    })
}

pub fn avatar_speichern(conn: &Connection, email: &str, data_uri: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO absender_avatar (email, data_uri, geholt_am) VALUES (?1, ?2, ?3)
         ON CONFLICT(email) DO UPDATE SET data_uri = excluded.data_uri, geholt_am = excluded.geholt_am",
        params![email.to_lowercase(), data_uri, jetzt_sekunden()],
    )
    .context("Avatar im Cache speichern")?;
    Ok(())
}

/// Speichert bzw. prüft eine Domain, von der externe Mail-Bilder ohne
/// erneute Rückfrage geladen werden dürfen.
pub fn bild_quelle_erlauben(conn: &Connection, domain: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO erlaubte_bild_quellen (domain) VALUES (?1)",
        params![domain.to_lowercase()],
    )
    .context("Erlaubte Bildquelle speichern")?;
    Ok(())
}

pub fn bild_quelle_ist_erlaubt(conn: &Connection, domain: &str) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM erlaubte_bild_quellen WHERE domain = ?1)",
        params![domain.to_lowercase()],
        |z| z.get(0),
    )
    .context("Erlaubte Bildquelle prüfen")
}

fn jetzt_sekunden() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beispiel_daten() -> KontoDaten {
        KontoDaten {
            name: "Test".into(),
            anzeigename: "Test Nutzer".into(),
            email: "test@example.org".into(),
            imap_host: "imap.example.org".into(),
            imap_port: 993,
            benutzer: "test".into(),
            smtp_host: "smtp.example.org".into(),
            smtp_port: 465,
            signatur: String::new(),
            farbe: String::new(),
        }
    }

    fn beispiel_konto(conn: &Connection) -> Konto {
        konto_anlegen(conn, &beispiel_daten()).unwrap()
    }

    #[test]
    fn migration_laeuft_zweimal_ohne_fehler() {
        let conn = oeffnen_im_speicher().unwrap();
        migrieren(&conn).unwrap();
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |z| z.get(0))
            .unwrap();
        assert_eq!(version, 15);
    }

    #[test]
    fn avatar_cache_speichert_und_verfaellt() {
        let conn = oeffnen_im_speicher().unwrap();
        // Noch nichts abgefragt.
        assert_eq!(avatar_aus_cache(&conn, "a@b.de", 3600).unwrap(), None);
        // Bild merken → Treffer.
        avatar_speichern(&conn, "A@B.de", Some("data:image/png;base64,xx")).unwrap();
        assert_eq!(
            avatar_aus_cache(&conn, "a@b.de", 3600).unwrap(),
            Some(Some("data:image/png;base64,xx".to_string()))
        );
        // „kein Bild“ merken (Initialen) → Treffer mit None.
        avatar_speichern(&conn, "c@d.de", None).unwrap();
        assert_eq!(avatar_aus_cache(&conn, "c@d.de", 3600).unwrap(), Some(None));
        // Zu altes Alter (0 s Toleranz) → gilt als abgelaufen.
        assert_eq!(avatar_aus_cache(&conn, "c@d.de", -1).unwrap(), None);
    }

    #[test]
    fn erlaubte_bild_quelle_wird_normalisiert_gespeichert() {
        let conn = oeffnen_im_speicher().unwrap();
        assert!(!bild_quelle_ist_erlaubt(&conn, "example.org").unwrap());
        bild_quelle_erlauben(&conn, "Example.ORG").unwrap();
        assert!(bild_quelle_ist_erlaubt(&conn, "example.org").unwrap());
    }

    #[test]
    fn migration_2_erhaelt_bestandsdaten_aus_version_1() {
        // Nachbau einer M1-Datenbank (Schema-Version 1), dann migrieren.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE schema_version (version INTEGER NOT NULL);
            CREATE TABLE konten (
                id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL,
                imap_host TEXT NOT NULL, imap_port INTEGER NOT NULL DEFAULT 993,
                benutzer TEXT NOT NULL
            );
            CREATE TABLE ordner (
                id INTEGER PRIMARY KEY,
                konto_id INTEGER NOT NULL REFERENCES konten(id) ON DELETE CASCADE,
                name TEXT NOT NULL, anzeige_name TEXT NOT NULL, uidvalidity INTEGER,
                UNIQUE(konto_id, name)
            );
            CREATE TABLE mails (
                id INTEGER PRIMARY KEY,
                ordner_id INTEGER NOT NULL REFERENCES ordner(id) ON DELETE CASCADE,
                uid INTEGER NOT NULL, betreff TEXT NOT NULL DEFAULT '',
                von TEXT NOT NULL DEFAULT '', datum INTEGER,
                gelesen INTEGER NOT NULL DEFAULT 0, hat_anhang INTEGER NOT NULL DEFAULT 0,
                UNIQUE(ordner_id, uid)
            );
            CREATE TABLE mail_bodies (
                mail_id INTEGER PRIMARY KEY REFERENCES mails(id) ON DELETE CASCADE,
                text TEXT NOT NULL DEFAULT '', html_bereinigt TEXT,
                hatte_externe_bilder INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO schema_version (version) VALUES (1);
            INSERT INTO konten (name, email, imap_host, benutzer)
                VALUES ('Bestand', 'alt@example.org', 'imap.alt.example', 'alt');
            INSERT INTO ordner (konto_id, name, anzeige_name) VALUES (1, 'INBOX', 'Posteingang');
            INSERT INTO mails (ordner_id, uid, betreff) VALUES (1, 7, 'Alte Mail');
            "#,
        )
        .unwrap();

        migrieren(&conn).unwrap();

        let konto = &konten_liste(&conn).unwrap()[0];
        assert_eq!(konto.email, "alt@example.org");
        assert_eq!(konto.smtp_host, ""); // leer = SMTP noch nicht eingerichtet
        assert_eq!(konto.smtp_port, 465);
        assert_eq!(konto.signatur, ""); // Migration 4: leere Vorgaben
        assert_eq!(konto.farbe, "");
        let ordner = &ordner_liste(&conn, konto.id).unwrap()[0];
        assert_eq!(ordner.rolle, None);
        // Konto und Ordner bleiben erhalten; den Mail-Cache leert
        // Migration 7 absichtlich (Anhang-Kennzeichen im Bestand war
        // unvollständig) — der nächste Sync lädt die Köpfe neu.
        assert_eq!(ordner.gesamt, 0);
        assert!(mails_liste(&conn, ordner.id, false, 0, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn gesendet_ordner_wird_ueber_rolle_oder_namen_gefunden() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        ordner_upsert(&conn, konto.id, "INBOX/Sent Items", "Sent Items", None).unwrap();
        let ordner = ordner_liste(&conn, konto.id).unwrap();
        // Ohne Rolle greift die Namensliste (auch bei Unterordner-Pfaden).
        assert_eq!(
            finde_gesendet_ordner(&ordner).unwrap().name,
            "INBOX/Sent Items"
        );

        // Mit Server-Rolle gewinnt diese.
        ordner_upsert(&conn, konto.id, "Ausgang", "Ausgang", Some("gesendet")).unwrap();
        let ordner = ordner_liste(&conn, konto.id).unwrap();
        assert_eq!(finde_gesendet_ordner(&ordner).unwrap().name, "Ausgang");

        // Gar kein Kandidat → None.
        let nur_inbox: Vec<Ordner> = ordner
            .iter()
            .filter(|o| o.name == "INBOX")
            .cloned()
            .collect();
        assert!(finde_gesendet_ordner(&nur_inbox).is_none());
    }

    #[test]
    fn papierkorb_wird_ueber_rolle_oder_namen_gefunden() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        ordner_upsert(&conn, konto.id, "INBOX/Trash", "Trash", None).unwrap();
        let ordner = ordner_liste(&conn, konto.id).unwrap();
        // Ohne Rolle greift die Namensliste (auch bei Unterordner-Pfaden).
        assert_eq!(
            finde_papierkorb_ordner(&ordner).unwrap().name,
            "INBOX/Trash"
        );

        // Mit Server-Rolle gewinnt diese.
        ordner_upsert(&conn, konto.id, "Muell", "Müll", Some("papierkorb")).unwrap();
        let ordner = ordner_liste(&conn, konto.id).unwrap();
        assert_eq!(finde_papierkorb_ordner(&ordner).unwrap().name, "Muell");

        // Gar kein Kandidat → None.
        let nur_inbox: Vec<Ordner> = ordner
            .iter()
            .filter(|o| o.name == "INBOX")
            .cloned()
            .collect();
        assert!(finde_papierkorb_ordner(&nur_inbox).is_none());
    }

    #[test]
    fn konto_speichert_signatur_und_farbe() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let mut daten = beispiel_daten();
        daten.signatur = "Viele Grüße\nPhilipp".into();
        daten.farbe = "#61afef".into();
        konto_aktualisieren(&conn, konto.id, &daten).unwrap();
        let neu = konto_holen(&conn, konto.id).unwrap().unwrap();
        assert_eq!(neu.signatur, "Viele Grüße\nPhilipp");
        assert_eq!(neu.farbe, "#61afef");
    }

    #[test]
    fn mail_entfernen_loescht_nur_die_eine() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        let koepfe: Vec<NeuerMailKopf> = (1..=2)
            .map(|i| NeuerMailKopf {
                uid: i,
                betreff: format!("Mail {i}"),
                von: String::new(),
                von_email: String::new(),
                an: String::new(),
                cc: String::new(),
                datum: None,
                gelesen: false,
                beantwortet: false,
                hat_anhang: false,
            })
            .collect();
        mails_einfuegen(&conn, id, &koepfe).unwrap();
        let erste = mails_liste(&conn, id, false, 0, 10).unwrap()[0].id;
        mail_entfernen(&conn, erste).unwrap();
        let rest = mails_liste(&conn, id, false, 0, 10).unwrap();
        assert_eq!(rest.len(), 1);
        assert_ne!(rest[0].id, erste);
    }

    #[test]
    fn konto_aktualisieren_aendert_smtp_daten() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let mut daten = beispiel_daten();
        daten.smtp_host = "mail.neu.example".into();
        daten.smtp_port = 587;
        konto_aktualisieren(&conn, konto.id, &daten).unwrap();
        let neu = konto_holen(&conn, konto.id).unwrap().unwrap();
        assert_eq!(neu.smtp_host, "mail.neu.example");
        assert_eq!(neu.smtp_port, 587);
        assert_eq!(neu.benutzer, "test");
    }

    #[test]
    fn konto_anlegen_und_lesen() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let liste = konten_liste(&conn).unwrap();
        assert_eq!(liste.len(), 1);
        assert_eq!(liste[0].email, "test@example.org");
        assert_eq!(
            konto_holen(&conn, konto.id).unwrap().unwrap().imap_port,
            993
        );
    }

    #[test]
    fn ordner_upsert_erhaelt_cache_stand() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        ordner_setze_uidvalidity(&conn, id, 42).unwrap();
        // Zweiter Upsert (z. B. nach erneutem LIST) darf den Stand nicht verlieren.
        let id2 = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        assert_eq!(id, id2);
        let ordner = ordner_holen(&conn, id).unwrap().unwrap();
        assert_eq!(ordner.uidvalidity, Some(42));
    }

    #[test]
    fn cache_verwerfen_leert_mails_und_setzt_zurueck() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        mails_einfuegen(
            &conn,
            id,
            &[NeuerMailKopf {
                uid: 1,
                betreff: "Hallo".into(),
                von: "a@b.c".into(),
                von_email: String::new(),
                an: String::new(),
                cc: String::new(),
                datum: Some(1_000),
                gelesen: false,
                beantwortet: false,
                hat_anhang: false,
            }],
        )
        .unwrap();
        ordner_cache_verwerfen(&conn, id, 99).unwrap();
        let ordner = ordner_holen(&conn, id).unwrap().unwrap();
        assert_eq!(ordner.gesamt, 0);
        assert_eq!(ordner.uidvalidity, Some(99));
    }

    #[test]
    fn mail_liste_neueste_zuerst_und_paginiert() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        let koepfe: Vec<NeuerMailKopf> = (1..=5)
            .map(|i| NeuerMailKopf {
                uid: i,
                betreff: format!("Mail {i}"),
                von: "a@b.c".into(),
                von_email: "a@b.c".into(),
                an: String::new(),
                cc: String::new(),
                datum: Some(i64::from(i) * 100),
                gelesen: i % 2 == 0,
                beantwortet: false,
                hat_anhang: false,
            })
            .collect();
        mails_einfuegen(&conn, id, &koepfe).unwrap();

        let seite1 = mails_liste(&conn, id, false, 0, 2).unwrap();
        assert_eq!(seite1[0].betreff, "Mail 5");
        assert_eq!(seite1[1].betreff, "Mail 4");
        let seite2 = mails_liste(&conn, id, false, 2, 2).unwrap();
        assert_eq!(seite2[0].betreff, "Mail 3");

        let ordner = ordner_holen(&conn, id).unwrap().unwrap();
        assert_eq!(ordner.gesamt, 5);
        assert_eq!(ordner.ungelesen, 3);
    }

    #[test]
    fn flags_und_loeschungen_wirken_auf_cache() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        mails_einfuegen(
            &conn,
            id,
            &[
                NeuerMailKopf {
                    uid: 1,
                    betreff: "eins".into(),
                    von: String::new(),
                    von_email: String::new(),
                    an: String::new(),
                    cc: String::new(),
                    datum: None,
                    gelesen: false,
                    beantwortet: false,
                    hat_anhang: false,
                },
                NeuerMailKopf {
                    uid: 2,
                    betreff: "zwei".into(),
                    von: String::new(),
                    von_email: String::new(),
                    an: String::new(),
                    cc: String::new(),
                    datum: None,
                    gelesen: false,
                    beantwortet: false,
                    hat_anhang: false,
                },
            ],
        )
        .unwrap();
        mails_flags_setzen(&conn, id, &[(1, true, true)]).unwrap();
        mails_loeschen(&conn, id, &[2]).unwrap();
        let stand = mails_cache_stand(&conn, id).unwrap();
        assert_eq!(stand, vec![(1, true, true)]);
    }

    #[test]
    fn inhalt_speichern_und_lesen() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        mails_einfuegen(
            &conn,
            id,
            &[NeuerMailKopf {
                uid: 1,
                betreff: "eins".into(),
                von: String::new(),
                von_email: String::new(),
                an: String::new(),
                cc: String::new(),
                datum: None,
                gelesen: false,
                beantwortet: false,
                hat_anhang: false,
            }],
        )
        .unwrap();
        let mail = &mails_liste(&conn, id, false, 0, 10).unwrap()[0];
        assert!(inhalt_holen(&conn, mail.id).unwrap().is_none());
        inhalt_speichern(
            &conn,
            mail.id,
            &MailInhalt {
                text: "Hallo".into(),
                html_bereinigt: Some("<p>Hallo</p>".into()),
                hatte_externe_bilder: true,
                kalender: Some("[]".into()),
            },
        )
        .unwrap();
        let inhalt = inhalt_holen(&conn, mail.id).unwrap().unwrap();
        assert_eq!(inhalt.text, "Hallo");
        assert!(inhalt.hatte_externe_bilder);
        assert_eq!(inhalt.kalender.as_deref(), Some("[]"));
    }

    #[test]
    fn anhaenge_speichern_und_lesen() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        mails_einfuegen(
            &conn,
            id,
            &[NeuerMailKopf {
                uid: 1,
                betreff: "Mit Anhang".into(),
                von: String::new(),
                von_email: String::new(),
                an: String::new(),
                cc: String::new(),
                datum: None,
                gelesen: false,
                beantwortet: false,
                hat_anhang: true,
            }],
        )
        .unwrap();
        let mail_id = mails_liste(&conn, id, false, 0, 10).unwrap()[0].id;

        assert!(anhaenge_liste(&conn, mail_id).unwrap().is_empty());
        anhaenge_speichern(
            &conn,
            mail_id,
            &[("doku.pdf".into(), 1000), ("foto.jpg".into(), 2000)],
        )
        .unwrap();
        let liste = anhaenge_liste(&conn, mail_id).unwrap();
        assert_eq!(liste.len(), 2);
        assert_eq!(liste[0].index, 0);
        assert_eq!(liste[0].dateiname, "doku.pdf");
        assert_eq!(liste[1].groesse, 2000);

        // Erneutes Speichern ersetzt den alten Stand.
        anhaenge_speichern(&conn, mail_id, &[("neu.txt".into(), 5)]).unwrap();
        assert_eq!(anhaenge_liste(&conn, mail_id).unwrap().len(), 1);

        // Löschen der Mail räumt die Anhänge mit ab.
        mail_entfernen(&conn, mail_id).unwrap();
        assert!(anhaenge_liste(&conn, mail_id).unwrap().is_empty());
    }

    #[test]
    fn entwuerfe_ordner_wird_ueber_rolle_oder_namen_gefunden() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        ordner_upsert(&conn, konto.id, "INBOX/Drafts", "Drafts", None).unwrap();
        let ordner = ordner_liste(&conn, konto.id).unwrap();
        // Ohne Rolle greift die Namensliste (auch bei Unterordner-Pfaden).
        assert_eq!(
            finde_entwuerfe_ordner(&ordner).unwrap().name,
            "INBOX/Drafts"
        );

        // Mit Server-Rolle gewinnt diese.
        ordner_upsert(&conn, konto.id, "Skizzen", "Skizzen", Some("entwuerfe")).unwrap();
        let ordner = ordner_liste(&conn, konto.id).unwrap();
        assert_eq!(finde_entwuerfe_ordner(&ordner).unwrap().name, "Skizzen");

        // Gar kein Kandidat → None.
        let nur_inbox: Vec<Ordner> = ordner
            .iter()
            .filter(|o| o.name == "INBOX")
            .cloned()
            .collect();
        assert!(finde_entwuerfe_ordner(&nur_inbox).is_none());
    }

    #[test]
    fn suche_findet_betreff_absender_und_text() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        mails_einfuegen(
            &conn,
            id,
            &[
                NeuerMailKopf {
                    uid: 1,
                    betreff: "Rechnung Oktober".into(),
                    von: "Buchhaltung".into(),
                    von_email: "rechnung@example.org".into(),
                    an: String::new(),
                    cc: String::new(),
                    datum: Some(1_000),
                    gelesen: true,
                    beantwortet: false,
                    hat_anhang: false,
                },
                NeuerMailKopf {
                    uid: 2,
                    betreff: "Urlaubsfotos".into(),
                    von: "Anna Muster".into(),
                    von_email: "anna@example.org".into(),
                    an: String::new(),
                    cc: String::new(),
                    datum: Some(2_000),
                    gelesen: true,
                    beantwortet: false,
                    hat_anhang: false,
                },
            ],
        )
        .unwrap();

        // Präfix im Betreff.
        let treffer = mails_suchen(&conn, id, "rechn", 50).unwrap();
        assert_eq!(treffer.len(), 1);
        assert_eq!(treffer[0].kopf.betreff, "Rechnung Oktober");
        assert_eq!(treffer[0].ordner_name, "Posteingang");

        // Absendername.
        let treffer = mails_suchen(&conn, id, "anna", 50).unwrap();
        assert_eq!(treffer.len(), 1);
        assert_eq!(treffer[0].kopf.betreff, "Urlaubsfotos");

        // Mailtext zählt, sobald der Inhalt im Cache liegt (Trigger).
        let mail_id = mails_liste(&conn, id, false, 0, 10).unwrap()[1].id;
        inhalt_speichern(
            &conn,
            mail_id,
            &MailInhalt {
                text: "Bitte um Überweisung bis Ende des Monats.".into(),
                html_bereinigt: None,
                hatte_externe_bilder: false,
                kalender: Some("[]".into()),
            },
        )
        .unwrap();
        let treffer = mails_suchen(&conn, id, "überweisung", 50).unwrap();
        assert_eq!(treffer.len(), 1);
        assert_eq!(treffer[0].kopf.betreff, "Rechnung Oktober");

        // Mehrere Wörter = alle müssen vorkommen.
        assert_eq!(
            mails_suchen(&conn, id, "rechnung urlaubsfotos", 50)
                .unwrap()
                .len(),
            0
        );
        // Anführungszeichen in der Eingabe stören die Abfrage nicht.
        assert_eq!(mails_suchen(&conn, id, "\"rechn", 50).unwrap().len(), 1);
        // Leere Eingabe liefert nichts.
        assert!(mails_suchen(&conn, id, "   ", 50).unwrap().is_empty());
    }

    #[test]
    fn suche_trennt_ordner_und_folgt_loeschungen() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto_a = beispiel_konto(&conn);
        let ordner_a = ordner_upsert(&conn, konto_a.id, "INBOX", "Posteingang", None).unwrap();
        let ordner_b = ordner_upsert(&conn, konto_a.id, "Sent", "Gesendet", None).unwrap();
        mails_einfuegen(
            &conn,
            ordner_a,
            &[NeuerMailKopf {
                uid: 1,
                betreff: "Geheimprojekt".into(),
                von: String::new(),
                von_email: String::new(),
                an: String::new(),
                cc: String::new(),
                datum: Some(1_000),
                gelesen: true,
                beantwortet: false,
                hat_anhang: false,
            }],
        )
        .unwrap();

        // Nur der geöffnete Ordner findet die Mail.
        assert_eq!(
            mails_suchen(&conn, ordner_a, "geheim", 50).unwrap().len(),
            1
        );
        assert!(mails_suchen(&conn, ordner_b, "geheim", 50)
            .unwrap()
            .is_empty());

        // Nach dem Löschen verschwindet sie auch aus dem Suchindex.
        let mail_id = mails_liste(&conn, ordner_a, false, 0, 10).unwrap()[0].id;
        mail_entfernen(&conn, mail_id).unwrap();
        assert!(mails_suchen(&conn, ordner_a, "geheim", 50)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn server_suchtreffer_werden_nach_uid_und_ordner_zugeordnet() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let inbox = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        let gesendet = ordner_upsert(&conn, konto.id, "Sent", "Gesendet", None).unwrap();
        for ordner in [inbox, gesendet] {
            mails_einfuegen(
                &conn,
                ordner,
                &[NeuerMailKopf {
                    uid: 7,
                    betreff: format!("Mail in {ordner}"),
                    von: String::new(),
                    von_email: String::new(),
                    an: String::new(),
                    cc: String::new(),
                    datum: Some(ordner),
                    gelesen: true,
                    beantwortet: false,
                    hat_anhang: false,
                }],
            )
            .unwrap();
        }

        let treffer = mails_zu_uids(&conn, gesendet, &[7], 100).unwrap();
        assert_eq!(treffer.len(), 1);
        assert_eq!(treffer[0].kopf.ordner_id, gesendet);
        assert_eq!(treffer[0].ordner_name, "Gesendet");
        assert!(mails_zu_uids(&conn, gesendet, &[], 100).unwrap().is_empty());
    }

    #[test]
    fn mails_liste_filtert_ungelesene() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        let koepfe: Vec<NeuerMailKopf> = (1..=4)
            .map(|i| NeuerMailKopf {
                uid: i,
                betreff: format!("Mail {i}"),
                von: String::new(),
                von_email: String::new(),
                an: String::new(),
                cc: String::new(),
                datum: Some(i64::from(i)),
                gelesen: i % 2 == 0,
                beantwortet: false,
                hat_anhang: false,
            })
            .collect();
        mails_einfuegen(&conn, id, &koepfe).unwrap();

        let ungelesen = mails_liste(&conn, id, true, 0, 10).unwrap();
        assert_eq!(ungelesen.len(), 2);
        assert!(ungelesen.iter().all(|m| !m.gelesen));
        // Ohne Filter kommen weiterhin alle.
        assert_eq!(mails_liste(&conn, id, false, 0, 10).unwrap().len(), 4);
    }

    #[test]
    fn gelesen_setzen_wirkt_in_beide_richtungen() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        mails_einfuegen(
            &conn,
            id,
            &[NeuerMailKopf {
                uid: 1,
                betreff: "eins".into(),
                von: String::new(),
                von_email: String::new(),
                an: String::new(),
                cc: String::new(),
                datum: None,
                gelesen: false,
                beantwortet: false,
                hat_anhang: false,
            }],
        )
        .unwrap();
        let mail_id = mails_liste(&conn, id, false, 0, 10).unwrap()[0].id;

        mail_gelesen_setzen(&conn, mail_id, true).unwrap();
        assert!(mail_holen(&conn, mail_id).unwrap().unwrap().gelesen);
        mail_gelesen_setzen(&conn, mail_id, false).unwrap();
        assert!(!mail_holen(&conn, mail_id).unwrap().unwrap().gelesen);
        assert_eq!(ordner_holen(&conn, id).unwrap().unwrap().ungelesen, 1);
    }

    #[test]
    fn adresse_merken_zerlegt_eintraege_und_ergaenzt_namen() {
        let conn = oeffnen_im_speicher().unwrap();
        // Nur-Adresse, dann derselbe Empfänger mit Anzeigename.
        adresse_merken(&conn, "anna@example.org").unwrap();
        adresse_merken(&conn, "\"Anna Muster\" <Anna@example.org>").unwrap();
        // Unbrauchbares wird still ignoriert.
        adresse_merken(&conn, "kein-eintrag").unwrap();

        let treffer = adress_vorschlaege(&conn, "anna", 10).unwrap();
        assert_eq!(
            treffer,
            vec![AdressVorschlag {
                email: "anna@example.org".into(),
                name: "Anna Muster".into(),
            }]
        );
        // Späterer Eintrag ohne Namen löscht den bekannten Namen nicht.
        adresse_merken(&conn, "anna@example.org").unwrap();
        assert_eq!(adress_vorschlaege(&conn, "anna", 10).unwrap(), treffer);
    }

    #[test]
    fn adress_vorschlaege_nutzen_auch_absender_aus_dem_cache() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang", None).unwrap();
        mails_einfuegen(
            &conn,
            id,
            &[NeuerMailKopf {
                uid: 1,
                betreff: "Hallo".into(),
                von: "Bert Beispiel".into(),
                von_email: "bert@example.org".into(),
                an: String::new(),
                cc: String::new(),
                datum: Some(1_000),
                gelesen: true,
                beantwortet: false,
                hat_anhang: false,
            }],
        )
        .unwrap();
        // Treffer über den Namen, obwohl die Adresse nie gemerkt wurde …
        let treffer = adress_vorschlaege(&conn, "bert", 10).unwrap();
        assert_eq!(treffer[0].email, "bert@example.org");
        assert_eq!(treffer[0].name, "Bert Beispiel");
        // … und keine Dublette, wenn dieselbe Adresse auch gemerkt ist.
        adresse_merken(&conn, "bert@example.org").unwrap();
        assert_eq!(adress_vorschlaege(&conn, "bert", 10).unwrap().len(), 1);
        // LIKE-Sonderzeichen in der Eingabe wirken nicht als Platzhalter.
        assert!(adress_vorschlaege(&conn, "b%rt", 10).unwrap().is_empty());
        // Leere Eingabe liefert nichts.
        assert!(adress_vorschlaege(&conn, "  ", 10).unwrap().is_empty());
    }
}

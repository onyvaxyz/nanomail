//! Lokale Datenhaltung mit SQLite/`rusqlite` (ab Meilenstein M1).
//!
//! SQLite ist ausschließlich **Cache** — Quelle der Wahrheit ist der
//! IMAP-Server. Passwörter liegen nie hier, sondern im GNOME Keyring.
//! Ablage nach XDG-Standard unter `~/.local/share/nanomail/nanomail.db`.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// Ein eingerichtetes Mail-Konto (ohne Passwort — das liegt im Keyring).
#[derive(Debug, Clone, Serialize)]
pub struct Konto {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub benutzer: String,
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
    pub von: String,
    /// Unix-Sekunden (UTC); `None`, wenn die Mail kein lesbares Datum hat.
    pub datum: Option<i64>,
    pub gelesen: bool,
    pub hat_anhang: bool,
}

/// Neu einzutragende Kopfzeilen (vor dem Insert gibt es noch keine `id`).
#[derive(Debug, Clone)]
pub struct NeuerMailKopf {
    pub uid: u32,
    pub betreff: String,
    pub von: String,
    pub datum: Option<i64>,
    pub gelesen: bool,
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
    Ok(())
}

// ---------------------------------------------------------------- Konten --

pub fn konto_anlegen(
    conn: &Connection,
    name: &str,
    email: &str,
    imap_host: &str,
    imap_port: u16,
    benutzer: &str,
) -> Result<Konto> {
    conn.execute(
        "INSERT INTO konten (name, email, imap_host, imap_port, benutzer) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![name, email, imap_host, imap_port, benutzer],
    )
    .context("Konto speichern")?;
    let id = conn.last_insert_rowid();
    Ok(Konto {
        id,
        name: name.into(),
        email: email.into(),
        imap_host: imap_host.into(),
        imap_port,
        benutzer: benutzer.into(),
    })
}

pub fn konten_liste(conn: &Connection) -> Result<Vec<Konto>> {
    let mut stmt = conn
        .prepare("SELECT id, name, email, imap_host, imap_port, benutzer FROM konten ORDER BY id")
        .context("Konten abfragen")?;
    let konten = stmt
        .query_map([], zeile_zu_konto)
        .context("Konten lesen")?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(konten)
}

pub fn konto_holen(conn: &Connection, id: i64) -> Result<Option<Konto>> {
    conn.query_row(
        "SELECT id, name, email, imap_host, imap_port, benutzer FROM konten WHERE id = ?1",
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
        email: zeile.get(2)?,
        imap_host: zeile.get(3)?,
        imap_port: zeile.get(4)?,
        benutzer: zeile.get(5)?,
    })
}

// ---------------------------------------------------------------- Ordner --

/// Legt einen Ordner an oder aktualisiert den Anzeigenamen.
/// Cache-Stand (uidvalidity) bleibt dabei unangetastet.
pub fn ordner_upsert(
    conn: &Connection,
    konto_id: i64,
    name: &str,
    anzeige_name: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO ordner (konto_id, name, anzeige_name) VALUES (?1, ?2, ?3)
         ON CONFLICT(konto_id, name) DO UPDATE SET anzeige_name = excluded.anzeige_name",
        params![konto_id, name, anzeige_name],
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
            "SELECT o.id, o.konto_id, o.name, o.anzeige_name, o.uidvalidity,
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
        "SELECT o.id, o.konto_id, o.name, o.anzeige_name, o.uidvalidity,
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
        gesamt: zeile.get(5)?,
        ungelesen: zeile.get(6)?,
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
            "INSERT INTO mails (ordner_id, uid, betreff, von, datum, gelesen, hat_anhang)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(ordner_id, uid) DO UPDATE SET gelesen = excluded.gelesen",
        )
        .context("Mail-Insert vorbereiten")?;
    for kopf in koepfe {
        stmt.execute(params![
            ordner_id,
            kopf.uid,
            kopf.betreff,
            kopf.von,
            kopf.datum,
            kopf.gelesen,
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
    aenderungen: &[(u32, bool)],
) -> Result<()> {
    let mut stmt = conn
        .prepare("UPDATE mails SET gelesen = ?3 WHERE ordner_id = ?1 AND uid = ?2")
        .context("Flag-Update vorbereiten")?;
    for (uid, gelesen) in aenderungen {
        stmt.execute(params![ordner_id, uid, gelesen])
            .context("Gelesen-Flag speichern")?;
    }
    Ok(())
}

/// Alle gecachten UIDs eines Ordners mit Gelesen-Flag (für den Sync-Abgleich).
pub fn mails_cache_stand(conn: &Connection, ordner_id: i64) -> Result<Vec<(u32, bool)>> {
    let mut stmt = conn
        .prepare("SELECT uid, gelesen FROM mails WHERE ordner_id = ?1 ORDER BY uid")
        .context("Cache-Stand abfragen")?;
    let stand = stmt
        .query_map(params![ordner_id], |z| {
            Ok((z.get::<_, i64>(0)? as u32, z.get(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(stand)
}

pub fn mails_liste(
    conn: &Connection,
    ordner_id: i64,
    offset: i64,
    limit: i64,
) -> Result<Vec<MailKopf>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, ordner_id, uid, betreff, von, datum, gelesen, hat_anhang
             FROM mails WHERE ordner_id = ?1
             ORDER BY datum IS NULL, datum DESC, uid DESC
             LIMIT ?2 OFFSET ?3",
        )
        .context("Mail-Liste abfragen")?;
    let mails = stmt
        .query_map(params![ordner_id, limit, offset], zeile_zu_mailkopf)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(mails)
}

pub fn mail_holen(conn: &Connection, mail_id: i64) -> Result<Option<MailKopf>> {
    conn.query_row(
        "SELECT id, ordner_id, uid, betreff, von, datum, gelesen, hat_anhang
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
        datum: zeile.get(5)?,
        gelesen: zeile.get(6)?,
        hat_anhang: zeile.get(7)?,
    })
}

pub fn mail_als_gelesen_markieren(conn: &Connection, mail_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE mails SET gelesen = 1 WHERE id = ?1",
        params![mail_id],
    )
    .context("Mail als gelesen markieren")?;
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

// ---------------------------------------------------------------- Bodies --

pub fn inhalt_holen(conn: &Connection, mail_id: i64) -> Result<Option<MailInhalt>> {
    conn.query_row(
        "SELECT text, html_bereinigt, hatte_externe_bilder
         FROM mail_bodies WHERE mail_id = ?1",
        params![mail_id],
        |z| {
            Ok(MailInhalt {
                text: z.get(0)?,
                html_bereinigt: z.get(1)?,
                hatte_externe_bilder: z.get(2)?,
            })
        },
    )
    .optional()
    .context("Mail-Inhalt lesen")
}

pub fn inhalt_speichern(conn: &Connection, mail_id: i64, inhalt: &MailInhalt) -> Result<()> {
    conn.execute(
        "INSERT INTO mail_bodies (mail_id, text, html_bereinigt, hatte_externe_bilder)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(mail_id) DO UPDATE SET
             text = excluded.text,
             html_bereinigt = excluded.html_bereinigt,
             hatte_externe_bilder = excluded.hatte_externe_bilder",
        params![
            mail_id,
            inhalt.text,
            inhalt.html_bereinigt,
            inhalt.hatte_externe_bilder
        ],
    )
    .context("Mail-Inhalt speichern")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beispiel_konto(conn: &Connection) -> Konto {
        konto_anlegen(
            conn,
            "Test",
            "test@example.org",
            "imap.example.org",
            993,
            "test",
        )
        .unwrap()
    }

    #[test]
    fn migration_laeuft_zweimal_ohne_fehler() {
        let conn = oeffnen_im_speicher().unwrap();
        migrieren(&conn).unwrap();
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |z| z.get(0))
            .unwrap();
        assert_eq!(version, 1);
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
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang").unwrap();
        ordner_setze_uidvalidity(&conn, id, 42).unwrap();
        // Zweiter Upsert (z. B. nach erneutem LIST) darf den Stand nicht verlieren.
        let id2 = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang").unwrap();
        assert_eq!(id, id2);
        let ordner = ordner_holen(&conn, id).unwrap().unwrap();
        assert_eq!(ordner.uidvalidity, Some(42));
    }

    #[test]
    fn cache_verwerfen_leert_mails_und_setzt_zurueck() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang").unwrap();
        mails_einfuegen(
            &conn,
            id,
            &[NeuerMailKopf {
                uid: 1,
                betreff: "Hallo".into(),
                von: "a@b.c".into(),
                datum: Some(1_000),
                gelesen: false,
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
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang").unwrap();
        let koepfe: Vec<NeuerMailKopf> = (1..=5)
            .map(|i| NeuerMailKopf {
                uid: i,
                betreff: format!("Mail {i}"),
                von: "a@b.c".into(),
                datum: Some(i64::from(i) * 100),
                gelesen: i % 2 == 0,
                hat_anhang: false,
            })
            .collect();
        mails_einfuegen(&conn, id, &koepfe).unwrap();

        let seite1 = mails_liste(&conn, id, 0, 2).unwrap();
        assert_eq!(seite1[0].betreff, "Mail 5");
        assert_eq!(seite1[1].betreff, "Mail 4");
        let seite2 = mails_liste(&conn, id, 2, 2).unwrap();
        assert_eq!(seite2[0].betreff, "Mail 3");

        let ordner = ordner_holen(&conn, id).unwrap().unwrap();
        assert_eq!(ordner.gesamt, 5);
        assert_eq!(ordner.ungelesen, 3);
    }

    #[test]
    fn flags_und_loeschungen_wirken_auf_cache() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang").unwrap();
        mails_einfuegen(
            &conn,
            id,
            &[
                NeuerMailKopf {
                    uid: 1,
                    betreff: "eins".into(),
                    von: String::new(),
                    datum: None,
                    gelesen: false,
                    hat_anhang: false,
                },
                NeuerMailKopf {
                    uid: 2,
                    betreff: "zwei".into(),
                    von: String::new(),
                    datum: None,
                    gelesen: false,
                    hat_anhang: false,
                },
            ],
        )
        .unwrap();
        mails_flags_setzen(&conn, id, &[(1, true)]).unwrap();
        mails_loeschen(&conn, id, &[2]).unwrap();
        let stand = mails_cache_stand(&conn, id).unwrap();
        assert_eq!(stand, vec![(1, true)]);
    }

    #[test]
    fn inhalt_speichern_und_lesen() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = ordner_upsert(&conn, konto.id, "INBOX", "Posteingang").unwrap();
        mails_einfuegen(
            &conn,
            id,
            &[NeuerMailKopf {
                uid: 1,
                betreff: "eins".into(),
                von: String::new(),
                datum: None,
                gelesen: false,
                hat_anhang: false,
            }],
        )
        .unwrap();
        let mail = &mails_liste(&conn, id, 0, 10).unwrap()[0];
        assert!(inhalt_holen(&conn, mail.id).unwrap().is_none());
        inhalt_speichern(
            &conn,
            mail.id,
            &MailInhalt {
                text: "Hallo".into(),
                html_bereinigt: Some("<p>Hallo</p>".into()),
                hatte_externe_bilder: true,
            },
        )
        .unwrap();
        let inhalt = inhalt_holen(&conn, mail.id).unwrap().unwrap();
        assert_eq!(inhalt.text, "Hallo");
        assert!(inhalt.hatte_externe_bilder);
    }
}

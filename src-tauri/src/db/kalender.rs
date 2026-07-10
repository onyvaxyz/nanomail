//! Kalender-Cache in SQLite (Meilenstein M4).
//!
//! Spiegel der CalDAV-Server: Kalender-Konten, deren Kalender und die
//! Termin-Objekte als Roh-ICS. Quelle der Wahrheit bleibt der Server;
//! App-Passwörter liegen ausschließlich im Schlüsselbund.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// Ein eingerichtetes Kalender-Konto (ohne Passwort — das liegt im Keyring).
#[derive(Debug, Clone, Serialize)]
pub struct KalenderKonto {
    pub id: i64,
    pub name: String,
    /// Basis-Adresse des Nextcloud-Servers, z. B. `https://cloud.example.com`.
    pub server: String,
    pub benutzer: String,
}

/// Ein Kalender innerhalb eines Kontos, wie ihn die Oberfläche braucht.
#[derive(Debug, Clone, Serialize)]
pub struct Kalender {
    pub id: i64,
    pub konto_id: i64,
    pub konto_name: String,
    #[serde(skip)]
    pub href: String,
    pub anzeige_name: String,
    /// Farbe laut Nextcloud (leer = Server nennt keine).
    pub farbe_server: String,
    /// In Nanomail gewählte Farbe — überschreibt die Server-Farbe (leer = keine).
    pub farbe_eigen: String,
    pub sichtbar: bool,
    #[serde(skip)]
    pub sync_token: String,
}

impl Kalender {
    /// Wirksame Farbe: eigene Wahl vor Server-Farbe vor Standard-Violett.
    pub fn farbe(&self) -> &str {
        if !self.farbe_eigen.is_empty() {
            &self.farbe_eigen
        } else if !self.farbe_server.is_empty() {
            &self.farbe_server
        } else {
            "#a371f7"
        }
    }
}

/// Ein gecachtes Termin-Objekt als Quelle für die Termin-Expansion.
#[derive(Debug, Clone)]
pub struct TerminQuelle {
    pub kalender_id: i64,
    pub href: String,
    pub etag: String,
    pub ics: String,
}

// ---------------------------------------------------------------- Konten --

pub fn konto_anlegen(
    conn: &Connection,
    name: &str,
    server: &str,
    benutzer: &str,
) -> Result<KalenderKonto> {
    conn.execute(
        "INSERT INTO kalender_konten (name, server, benutzer) VALUES (?1, ?2, ?3)",
        params![name, server, benutzer],
    )
    .context("Kalender-Konto speichern")?;
    Ok(KalenderKonto {
        id: conn.last_insert_rowid(),
        name: name.to_string(),
        server: server.to_string(),
        benutzer: benutzer.to_string(),
    })
}

pub fn konten_liste(conn: &Connection) -> Result<Vec<KalenderKonto>> {
    let mut stmt = conn
        .prepare("SELECT id, name, server, benutzer FROM kalender_konten ORDER BY id")
        .context("Kalender-Konten abfragen")?;
    let konten = stmt
        .query_map([], zeile_zu_konto)
        .context("Kalender-Konten lesen")?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(konten)
}

pub fn konto_holen(conn: &Connection, id: i64) -> Result<Option<KalenderKonto>> {
    conn.query_row(
        "SELECT id, name, server, benutzer FROM kalender_konten WHERE id = ?1",
        params![id],
        zeile_zu_konto,
    )
    .optional()
    .context("Kalender-Konto lesen")
}

pub fn konto_loeschen(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM kalender_konten WHERE id = ?1", params![id])
        .context("Kalender-Konto löschen")?;
    Ok(())
}

fn zeile_zu_konto(zeile: &rusqlite::Row<'_>) -> rusqlite::Result<KalenderKonto> {
    Ok(KalenderKonto {
        id: zeile.get(0)?,
        name: zeile.get(1)?,
        server: zeile.get(2)?,
        benutzer: zeile.get(3)?,
    })
}

// -------------------------------------------------------------- Kalender --

/// Legt einen Kalender an oder aktualisiert Anzeigename und Server-Farbe.
/// Eigene Farbe, Sichtbarkeit und Sync-Token bleiben dabei erhalten.
pub fn kalender_upsert(
    conn: &Connection,
    konto_id: i64,
    href: &str,
    anzeige_name: &str,
    farbe_server: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO kalender (konto_id, href, anzeige_name, farbe_server)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(konto_id, href) DO UPDATE SET
             anzeige_name = excluded.anzeige_name,
             farbe_server = excluded.farbe_server",
        params![konto_id, href, anzeige_name, farbe_server],
    )
    .context("Kalender speichern")?;
    let id = conn
        .query_row(
            "SELECT id FROM kalender WHERE konto_id = ?1 AND href = ?2",
            params![konto_id, href],
            |z| z.get(0),
        )
        .context("Kalender-ID lesen")?;
    Ok(id)
}

/// Entfernt Kalender, die es auf dem Server nicht mehr gibt.
pub fn kalender_bereinigen(
    conn: &Connection,
    konto_id: i64,
    server_hrefs: &[String],
) -> Result<()> {
    let vorhandene: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, href FROM kalender WHERE konto_id = ?1")?;
        let zeilen = stmt
            .query_map(params![konto_id], |z| Ok((z.get(0)?, z.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        zeilen
    };
    for (id, href) in vorhandene {
        if !server_hrefs.contains(&href) {
            conn.execute("DELETE FROM kalender WHERE id = ?1", params![id])
                .context("verwaisten Kalender löschen")?;
        }
    }
    Ok(())
}

pub fn kalender_liste(conn: &Connection) -> Result<Vec<Kalender>> {
    let mut stmt = conn
        .prepare(
            "SELECT k.id, k.konto_id, o.name, k.href, k.anzeige_name,
                    k.farbe_server, k.farbe_eigen, k.sichtbar, k.sync_token
             FROM kalender k JOIN kalender_konten o ON o.id = k.konto_id
             ORDER BY o.id, k.anzeige_name",
        )
        .context("Kalender abfragen")?;
    let kalender = stmt
        .query_map([], zeile_zu_kalender)
        .context("Kalender lesen")?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(kalender)
}

pub fn kalender_holen(conn: &Connection, id: i64) -> Result<Option<Kalender>> {
    conn.query_row(
        "SELECT k.id, k.konto_id, o.name, k.href, k.anzeige_name,
                k.farbe_server, k.farbe_eigen, k.sichtbar, k.sync_token
         FROM kalender k JOIN kalender_konten o ON o.id = k.konto_id
         WHERE k.id = ?1",
        params![id],
        zeile_zu_kalender,
    )
    .optional()
    .context("Kalender lesen")
}

fn zeile_zu_kalender(zeile: &rusqlite::Row<'_>) -> rusqlite::Result<Kalender> {
    Ok(Kalender {
        id: zeile.get(0)?,
        konto_id: zeile.get(1)?,
        konto_name: zeile.get(2)?,
        href: zeile.get(3)?,
        anzeige_name: zeile.get(4)?,
        farbe_server: zeile.get(5)?,
        farbe_eigen: zeile.get(6)?,
        sichtbar: zeile.get(7)?,
        sync_token: zeile.get(8)?,
    })
}

pub fn sync_token_setzen(conn: &Connection, kalender_id: i64, token: &str) -> Result<()> {
    conn.execute(
        "UPDATE kalender SET sync_token = ?2 WHERE id = ?1",
        params![kalender_id, token],
    )
    .context("Sync-Token speichern")?;
    Ok(())
}

pub fn farbe_setzen(conn: &Connection, kalender_id: i64, farbe: &str) -> Result<()> {
    conn.execute(
        "UPDATE kalender SET farbe_eigen = ?2 WHERE id = ?1",
        params![kalender_id, farbe],
    )
    .context("Kalender-Farbe speichern")?;
    Ok(())
}

pub fn sichtbar_setzen(conn: &Connection, kalender_id: i64, sichtbar: bool) -> Result<()> {
    conn.execute(
        "UPDATE kalender SET sichtbar = ?2 WHERE id = ?1",
        params![kalender_id, sichtbar],
    )
    .context("Kalender-Sichtbarkeit speichern")?;
    Ok(())
}

// --------------------------------------------------------------- Termine --

/// Gecachter ETag eines Termin-Objekts — `None`, wenn es noch fehlt.
pub fn termin_etag(conn: &Connection, kalender_id: i64, href: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT etag FROM termine WHERE kalender_id = ?1 AND href = ?2",
        params![kalender_id, href],
        |z| z.get(0),
    )
    .optional()
    .context("Termin-ETag lesen")
}

pub fn termin_ics(conn: &Connection, kalender_id: i64, href: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT ics FROM termine WHERE kalender_id = ?1 AND href = ?2",
        params![kalender_id, href],
        |z| z.get(0),
    )
    .optional()
    .context("Termin-ICS lesen")
}

/// Legt ein Termin-Objekt an oder ersetzt den alten Stand.
#[allow(clippy::too_many_arguments)]
pub fn termin_upsert(
    conn: &Connection,
    kalender_id: i64,
    href: &str,
    etag: &str,
    ics: &str,
    beginn: Option<i64>,
    ende: Option<i64>,
    hat_wiederholung: bool,
) -> Result<()> {
    conn.execute(
        "INSERT INTO termine (kalender_id, href, etag, ics, beginn, ende, hat_wiederholung)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(kalender_id, href) DO UPDATE SET
             etag = excluded.etag,
             ics = excluded.ics,
             beginn = excluded.beginn,
             ende = excluded.ende,
             hat_wiederholung = excluded.hat_wiederholung",
        params![kalender_id, href, etag, ics, beginn, ende, hat_wiederholung],
    )
    .context("Termin speichern")?;
    Ok(())
}

pub fn termin_loeschen(conn: &Connection, kalender_id: i64, href: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM termine WHERE kalender_id = ?1 AND href = ?2",
        params![kalender_id, href],
    )
    .context("Termin löschen")?;
    Ok(())
}

/// Leert den Termin-Cache eines Kalenders (ungültiges Sync-Token → Neuaufbau).
pub fn termine_leeren(conn: &Connection, kalender_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM termine WHERE kalender_id = ?1",
        params![kalender_id],
    )
    .context("Termin-Cache leeren")?;
    conn.execute(
        "UPDATE kalender SET sync_token = '' WHERE id = ?1",
        params![kalender_id],
    )
    .context("Sync-Token zurücksetzen")?;
    Ok(())
}

/// Termin-Quellen sichtbarer Kalender fürs Zeitfenster: einfache Termine
/// nur bei Überschneidung, Wiederholungstermine immer (die Regel wird
/// erst bei der Expansion aufs Fenster angewendet).
pub fn termine_im_zeitraum(conn: &Connection, von: i64, bis: i64) -> Result<Vec<TerminQuelle>> {
    let mut stmt = conn
        .prepare(
            "SELECT t.kalender_id, t.href, t.etag, t.ics
             FROM termine t JOIN kalender k ON k.id = t.kalender_id
             WHERE k.sichtbar = 1
               AND (t.hat_wiederholung = 1
                    OR (COALESCE(t.beginn, 0) < ?2 AND COALESCE(t.ende, t.beginn, ?2) > ?1))",
        )
        .context("Termine abfragen")?;
    let termine = stmt
        .query_map(params![von, bis], |z| {
            Ok(TerminQuelle {
                kalender_id: z.get(0)?,
                href: z.get(1)?,
                etag: z.get(2)?,
                ics: z.get(3)?,
            })
        })
        .context("Termine lesen")?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(termine)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::oeffnen_im_speicher;

    fn beispiel_konto(conn: &Connection) -> KalenderKonto {
        konto_anlegen(conn, "Nextcloud", "https://cloud.example.com", "philipp").unwrap()
    }

    #[test]
    fn konto_anlegen_und_loeschen_raeumt_kalender_ab() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = kalender_upsert(&conn, konto.id, "/cal/privat/", "Privat", "#00ff00").unwrap();
        termin_upsert(&conn, id, "a.ics", "e1", "ICS", Some(10), Some(20), false).unwrap();

        assert_eq!(konten_liste(&conn).unwrap().len(), 1);
        konto_loeschen(&conn, konto.id).unwrap();
        assert!(konten_liste(&conn).unwrap().is_empty());
        assert!(kalender_liste(&conn).unwrap().is_empty());
        assert!(termine_im_zeitraum(&conn, 0, 100).unwrap().is_empty());
    }

    #[test]
    fn kalender_upsert_erhaelt_eigene_farbe_und_token() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = kalender_upsert(&conn, konto.id, "/cal/privat/", "Privat", "#00ff00").unwrap();
        farbe_setzen(&conn, id, "#ff0000").unwrap();
        sync_token_setzen(&conn, id, "token-1").unwrap();
        sichtbar_setzen(&conn, id, false).unwrap();

        // Erneute Discovery (neuer Name/Server-Farbe) darf nichts davon verlieren.
        let id2 =
            kalender_upsert(&conn, konto.id, "/cal/privat/", "Privat neu", "#0000ff").unwrap();
        assert_eq!(id, id2);
        let kalender = kalender_holen(&conn, id).unwrap().unwrap();
        assert_eq!(kalender.anzeige_name, "Privat neu");
        assert_eq!(kalender.farbe_server, "#0000ff");
        assert_eq!(kalender.farbe_eigen, "#ff0000");
        assert_eq!(kalender.farbe(), "#ff0000");
        assert_eq!(kalender.sync_token, "token-1");
        assert!(!kalender.sichtbar);
    }

    #[test]
    fn wirksame_farbe_faellt_auf_server_und_standard_zurueck() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = kalender_upsert(&conn, konto.id, "/cal/a/", "A", "#00ff00").unwrap();
        let kalender = kalender_holen(&conn, id).unwrap().unwrap();
        assert_eq!(kalender.farbe(), "#00ff00");

        let id = kalender_upsert(&conn, konto.id, "/cal/b/", "B", "").unwrap();
        let kalender = kalender_holen(&conn, id).unwrap().unwrap();
        assert_eq!(kalender.farbe(), "#a371f7");
    }

    #[test]
    fn kalender_bereinigen_entfernt_verwaiste() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        kalender_upsert(&conn, konto.id, "/cal/a/", "A", "").unwrap();
        kalender_upsert(&conn, konto.id, "/cal/b/", "B", "").unwrap();
        kalender_bereinigen(&conn, konto.id, &["/cal/a/".to_string()]).unwrap();
        let liste = kalender_liste(&conn).unwrap();
        assert_eq!(liste.len(), 1);
        assert_eq!(liste[0].href, "/cal/a/");
    }

    #[test]
    fn termine_im_zeitraum_filtert_und_nimmt_wiederholungen_immer() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = kalender_upsert(&conn, konto.id, "/cal/a/", "A", "").unwrap();
        // Im Fenster, außerhalb, Wiederholung (alter Beginn, zählt trotzdem).
        termin_upsert(&conn, id, "in.ics", "e", "IN", Some(100), Some(200), false).unwrap();
        termin_upsert(
            &conn,
            id,
            "out.ics",
            "e",
            "OUT",
            Some(900),
            Some(950),
            false,
        )
        .unwrap();
        termin_upsert(&conn, id, "rr.ics", "e", "RR", Some(1), Some(2), true).unwrap();

        let quellen = termine_im_zeitraum(&conn, 150, 300).unwrap();
        let mut ics: Vec<&str> = quellen.iter().map(|q| q.ics.as_str()).collect();
        ics.sort_unstable();
        assert_eq!(ics, vec!["IN", "RR"]);

        // Unsichtbare Kalender liefern nichts.
        sichtbar_setzen(&conn, id, false).unwrap();
        assert!(termine_im_zeitraum(&conn, 150, 300).unwrap().is_empty());
    }

    #[test]
    fn termin_etag_und_loeschen() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = kalender_upsert(&conn, konto.id, "/cal/a/", "A", "").unwrap();
        assert_eq!(termin_etag(&conn, id, "a.ics").unwrap(), None);
        termin_upsert(&conn, id, "a.ics", "etag-1", "ICS", None, None, false).unwrap();
        assert_eq!(
            termin_etag(&conn, id, "a.ics").unwrap(),
            Some("etag-1".to_string())
        );
        termin_loeschen(&conn, id, "a.ics").unwrap();
        assert_eq!(termin_etag(&conn, id, "a.ics").unwrap(), None);
    }

    #[test]
    fn termine_leeren_setzt_token_zurueck() {
        let conn = oeffnen_im_speicher().unwrap();
        let konto = beispiel_konto(&conn);
        let id = kalender_upsert(&conn, konto.id, "/cal/a/", "A", "").unwrap();
        sync_token_setzen(&conn, id, "token").unwrap();
        termin_upsert(&conn, id, "a.ics", "e", "ICS", Some(1), Some(2), false).unwrap();
        termine_leeren(&conn, id).unwrap();
        assert!(termine_im_zeitraum(&conn, 0, 100).unwrap().is_empty());
        assert_eq!(kalender_holen(&conn, id).unwrap().unwrap().sync_token, "");
    }
}

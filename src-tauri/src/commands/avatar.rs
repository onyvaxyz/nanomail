//! Absender-/Konto-Avatare.

use super::*;

use anyhow::Result;
use tauri::State;

use crate::avatar;
use crate::db::{self};
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
    let bild = match avatar::hole_avatar(&client, &email).await {
        Ok(bild) => bild,
        // Vorübergehender Fehler (Netz/Server): nichts cachen —
        // beim nächsten Anzeigen wird erneut versucht.
        Err(_) => return Ok(None),
    };
    mit_db(zustand, |conn| {
        db::avatar_speichern(conn, &email, bild.as_deref())
    })?;
    Ok(bild)
}

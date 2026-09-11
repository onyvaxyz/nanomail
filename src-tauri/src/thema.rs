//! Omarchy-Palette ausschließlich lesen, niemals Shell-Kommandos ausführen.
use std::collections::BTreeMap;

#[derive(serde::Serialize)]
pub struct Thema {
    hell: bool,
    farben: BTreeMap<String, String>,
}

fn palette(text: &str, light_mode: bool) -> Option<Thema> {
    let werte: BTreeMap<String, String> = text
        .lines()
        .filter_map(|zeile| {
            let (key, value) = zeile.split_once('=')?;
            let value = value.trim();
            let quote = value.chars().next()?;
            if quote != '"' && quote != '\'' {
                return None;
            }
            let (value, rest) = value[1..].split_once(quote)?;
            if !rest.trim().is_empty() && !rest.trim().starts_with('#') {
                return None;
            }
            Some((key.trim().to_string(), value.to_string()))
        })
        .collect();
    let mut farben = BTreeMap::new();
    for (key, token) in [
        ("background", "--bg-editor"),
        ("background", "--bg-panel"),
        ("background", "--bg-surface"),
        ("dark_background", "--bg-window"),
        ("lighter_background", "--bg-hover"),
        ("selection", "--bg-aktiv"),
        ("selection", "--bg-auswahl"),
        ("foreground", "--text"),
        ("foreground", "--text-hell"),
        ("foreground", "--text-muted"),
        ("foreground", "--text-leise"),
        ("accent", "--omarchy-akzent"),
        ("red", "--rot"),
        ("green", "--grun"),
        ("blue", "--blau"),
        ("yellow", "--gelb"),
    ] {
        if let Some(farbe) = werte.get(key).filter(|v| {
            v.len() == 7 && v.starts_with('#') && v[1..].bytes().all(|b| b.is_ascii_hexdigit())
        }) {
            farben.insert(token.to_string(), farbe.clone());
        }
    }
    let bg = farben.get("--bg-editor")?.clone();
    farben.entry("--bg-window".into()).or_insert(bg.clone());
    farben.insert("--eingabe-fokus".into(), bg.clone());
    let hell = match werte
        .get("mode")
        .or_else(|| werte.get("theme_type"))
        .map(String::as_str)
    {
        Some("light") => true,
        Some("dark") => false,
        _ => {
            light_mode
                || u32::from_str_radix(&bg[1..], 16).is_ok_and(|rgb| {
                    let r = (rgb >> 16) & 255;
                    let g = (rgb >> 8) & 255;
                    let b = rgb & 255;
                    r * 299 + g * 587 + b * 114 > 128000
                })
        }
    };
    Some(Thema { hell, farben })
}

#[tauri::command]
pub async fn omarchy_thema() -> Option<Thema> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let dirs = directories::BaseDirs::new()?;
    let state = dirs
        .state_dir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| dirs.home_dir().join(".local/state"));
    for basis in [state, dirs.config_dir().to_path_buf()] {
        let theme = basis.join("omarchy/current/theme");
        if let Ok(text) = tokio::fs::read_to_string(theme.join("colors.toml")).await {
            if let Some(theme) = palette(&text, theme.join("light.mode").is_file()) {
                return Some(theme);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_validiert_farben_und_modus() {
        let t = palette(
            "background = '#ffffff'\nmode = 'dark'\naccent = '#012345'\nred = 'url(evil)'",
            true,
        )
        .unwrap();
        assert!(!t.hell);
        assert_eq!(t.farben["--omarchy-akzent"], "#012345");
        assert!(!t.farben.contains_key("--rot"));
        assert!(palette("background = '#ffffff'", false).unwrap().hell);
        assert!(palette("background = '#101010'", true).unwrap().hell);
        assert!(palette("background = 'invalid'", false).is_none());
        assert!(palette("background = '#ffffff", false).is_none());
        assert!(palette("background = '#ffffff' garbage", false).is_none());
        assert!(
            palette("background = '#ffffff' # Kommentar", false)
                .unwrap()
                .hell
        );
    }
}

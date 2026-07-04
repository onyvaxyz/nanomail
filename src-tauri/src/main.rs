// Verhindert ein zusätzliches Konsolenfenster (nur für Windows relevant,
// Tauri-Standard-Zeile).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    nanomail_lib::run()
}

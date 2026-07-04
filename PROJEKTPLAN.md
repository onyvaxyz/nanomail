# Nanomail — Projektplan

Lebendes Übersichtsdokument. Wird nach jedem Meilenstein aktualisiert.
Regel: **Ein Meilenstein nach dem anderen, jeder wird von Philipp getestet
und freigegeben, bevor der nächste beginnt.**

Stand: 2026-07-04

## Meilensteine

| Nr. | Meilenstein | Inhalt | Status |
|---|---|---|---|
| M0 | Projektgerüst | Tauri-App-Skelett, Grundlayout, Logging, CI, `.deb`-Build, Doku/Skills | 🟡 Wartet auf Freigabe |
| M1 | Erstes Konto lesend | Infomaniak-IMAP: Ordner & Mails anzeigen (HTML bereinigt, Bilder blockiert), SQLite-Cache, Passwort im Keyring | ⚪ Offen |
| M2 | Senden | Verfassen, Antworten, Weiterleiten, Anhänge, Ablage im „Gesendet“-Ordner | ⚪ Offen |
| M3 | Multi-Account | Die beiden OpenXchange-Konten (generische Kontoverwaltung) | ⚪ Offen |
| M4 | Kalender lesend | 3× Nextcloud-CalDAV, Wiederholungstermine, Zeitzonen, Offline-Cache | ⚪ Offen |
| M5 | Kalender schreibend *(optional)* | Termine erstellen/bearbeiten/löschen, Konfliktbehandlung | ⚪ Offen |
| M6 | Microsoft (zuletzt) | Erst Mini-Auth-Test (klärt Kontotyp & Tenant-Regeln), dann Device-Code-Flow, Token-Refresh, IMAP-Anbindung | ⚪ Offen |

Status-Legende: ⚪ Offen · 🔵 In Arbeit · 🟡 Wartet auf Freigabe · 🟢 Freigegeben

## Offene Entscheidungen

| Entscheidung | Wann klären |
|---|---|
| Microsoft-Konto: privat oder Firmen-Tenant (mit/ohne Admin-Zugriff)? | Zu Beginn von M6 per Mini-Auth-Test |
| Kalender auch schreiben (M5) oder nur lesen? | Nach Abnahme von M4 |

## Getroffene Entscheidungen

| Datum | Entscheidung |
|---|---|
| 2026-07-04 | Microsoft-OAuth als letzter Meilenstein (M6) |
| 2026-07-04 | Jeder Meilenstein einzeln freigabepflichtig, kleine Schritte |
| 2026-07-04 | Zugangsdaten nur im GNOME Keyring, nie im Klartext |
| 2026-07-04 | HTML-Mails werden vor Anzeige bereinigt, externe Bilder blockiert |
| 2026-07-04 | Microsoft-Anmeldung per Device-Code-Flow (einfacher als Redirect) |
| 2026-07-04 | Frontend ohne Build-Schritt (Vanilla HTML/CSS/JS, direkt editierbar) |

## So testest du den aktuellen Stand (M0)

1. Auf einem Ubuntu-Rechner das `.deb` bauen (oder bauen lassen):
   `cd src-tauri && cargo tauri build`
   → Ergebnis liegt unter `src-tauri/target/release/bundle/deb/`
2. Installieren: `sudo dpkg -i nanomail_0.1.0_amd64.deb`
3. „Nanomail“ im Startmenü suchen und starten
4. Erwartung: Fenster mit drei Spalten (Konten / Posteingang / Vorschau,
   noch leer) und unten in der Statusleiste grün:
   „✓ Backend verbunden — Nanomail 0.1.0 (M0 — Projektgerüst)“
5. Log-Datei existiert unter `~/.local/share/nanomail/logs/`

Wenn das passt: M0 freigeben → M1 beginnt.

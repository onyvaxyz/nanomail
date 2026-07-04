# Nanomail — Projektplan

Lebendes Übersichtsdokument. Wird nach jedem Meilenstein aktualisiert.
Regel: **Ein Meilenstein nach dem anderen, jeder wird von Philipp getestet
und freigegeben, bevor der nächste beginnt.**

Stand: 2026-07-04

## Meilensteine

| Nr. | Meilenstein | Inhalt | Status |
|---|---|---|---|
| M0 | Projektgerüst | Tauri-App-Skelett, Grundlayout, Logging, CI, `.deb`-Build, Doku/Skills | 🟢 Freigegeben |
| M1 | Erstes Konto lesend | Infomaniak-IMAP: Ordner & Mails anzeigen (HTML bereinigt, Bilder blockiert), SQLite-Cache, Passwort im Keyring | 🟡 Wartet auf Freigabe |
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
| 2026-07-04 | M1: Mail gilt als gelesen, sobald sie geöffnet wird (Flag wird zum Server übertragen) |
| 2026-07-04 | M1: „Bilder laden“ lädt externe Bilder über das Backend und bettet sie ein — die Original-Mail wird dafür frisch vom Server geholt, unbereinigtes HTML wird nie gespeichert |
| 2026-07-04 | M1: Neue Mails per „Aktualisieren“-Knopf und Sync beim Start; automatisches Live-Update (IMAP IDLE) kommt mit M3 |

## So testest du den aktuellen Stand (M1)

1. Bauen und installieren wie gehabt:
   `cd src-tauri && cargo tauri build`, dann
   `sudo dpkg -i src-tauri/target/release/bundle/deb/Nanomail_0.1.0_amd64.deb`
2. „Nanomail“ starten → ein Dialog fragt nach deinem Mail-Konto.
   Für Infomaniak sind Server (`mail.infomaniak.com`) und Port (993)
   vorausgefüllt. **Empfehlung:** In deinem Infomaniak-Konto ein
   App-Passwort erstellen und das hier verwenden.
3. Nach „Verbindung prüfen & speichern“ gleicht die App dein Postfach ab.
   Erwartung:
   - Links erscheinen deine Ordner mit Ungelesen-Zählern
   - In der Mitte die Mails (neueste oben, ungelesene fett)
   - Klick auf eine Mail zeigt sie rechts an und markiert sie als gelesen
     (auch am Handy/Webmail sichtbar)
   - Bei HTML-Mails mit externen Bildern erscheint eine gelbe Leiste
     „Externe Bilder wurden blockiert“ mit „Bilder laden“-Knopf
4. Das Passwort liegt danach im Ubuntu-Schlüsselbund
   (nachprüfbar mit dem Programm „Passwörter und Verschlüsselung“,
   Eintrag „nanomail“) — nirgendwo sonst.
5. Bei Problemen: Log-Datei unter `~/.local/share/nanomail/logs/` mitschicken.

Wenn das passt: M1 freigeben → M2 (Senden) beginnt.

## Was in M1 bewusst noch fehlt

Senden/Antworten (M2), weitere Konten (M3), automatisches Live-Update
neuer Mails (M3), Mail-Suche, Löschen/Verschieben, Anhänge öffnen
(nur 📎-Kennzeichnung). Kalender ab M4.

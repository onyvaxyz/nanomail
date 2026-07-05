# Nanomail — Projektplan

Lebendes Übersichtsdokument. Wird nach jedem Meilenstein aktualisiert.
Regel: **Ein Meilenstein nach dem anderen, jeder wird von Philipp getestet
und freigegeben, bevor der nächste beginnt.**

Stand: 2026-07-04

## Meilensteine

| Nr. | Meilenstein | Inhalt | Status |
|---|---|---|---|
| M0 | Projektgerüst | Tauri-App-Skelett, Grundlayout, Logging, CI, `.deb`-Build, Doku/Skills | 🟢 Freigegeben |
| M1 | Erstes Konto lesend | IMAP: Ordner & Mails anzeigen (HTML bereinigt, Bilder blockiert), SQLite-Cache, Passwort im Keyring | 🟢 Freigegeben |
| M2 | Senden | Verfassen, Antworten, Weiterleiten, Anhänge, Ablage im „Gesendet“-Ordner | 🟡 Wartet auf Freigabe |
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
| 2026-07-05 | M1 mit OpenXchange-Konto abgenommen; Infomaniak-Anmeldung scheiterte serverseitig → Fehlermeldung zeigt jetzt die Serverantwort + Hinweis auf App-Passwort/vollständige Adresse |
| 2026-07-05 | M2: Nur-Text-Mails verfassen (HTML-Verfassen, Entwürfe, Signaturen bewusst später) |
| 2026-07-05 | M2: Weiterleiten übernimmt die Original-Anhänge automatisch |
| 2026-07-05 | M2: „Gesendet“-Ordner wird über die Server-Kennzeichnung erkannt (Fallback: gängige Namen) |

## So testest du den aktuellen Stand (M2)

1. Bauen und installieren wie gehabt:
   `cd src-tauri && cargo tauri build`, dann
   `sudo dpkg -i src-tauri/target/release/bundle/deb/Nanomail_0.1.0_amd64.deb`
2. **Dein bestehendes Konto bleibt erhalten** (die Datenbank wird beim
   ersten Start automatisch erweitert). Neu: Klick auf das ⚙-Zahnrad
   neben dem Kontonamen öffnet „Konto bearbeiten“ — dort einmalig den
   **Versand-Server (SMTP)** eintragen (bei OpenXchange/Infomaniak meist
   derselbe Servername, Port 465). Passwortfeld leer lassen = Passwort
   bleibt unverändert.
3. **Senden testen** (am einfachsten an dich selbst):
   - „✉ Verfassen“ → Mail an deine eigene Adresse → Senden.
     Erwartung: Status „✓ Mail gesendet.“, die Mail kommt an und liegt
     zusätzlich im „Gesendet“-Ordner.
   - Eine empfangene Mail öffnen → „↩ Antworten“: Empfänger und
     „Re:“-Betreff sind vorausgefüllt, darunter das Zitat.
   - Eine Mail mit Anhang öffnen → „↪ Weiterleiten“: Der Anhang wird
     automatisch mitgeschickt.
   - „📎 Datei anhängen“ im Verfassen-Fenster hängt eigene Dateien an.
4. **Infomaniak nochmal probieren:** Die Fehlermeldung zeigt jetzt die
   konkrete Serverantwort. Benutzername = vollständige E-Mail-Adresse;
   bei aktivierter Zwei-Faktor-Anmeldung im Infomaniak-Manager unter
   Sicherheit → Anwendungspasswörter ein App-Passwort erstellen.
5. Bei Problemen: Log-Datei unter `~/.local/share/nanomail/logs/` mitschicken.

Wenn das passt: M2 freigeben → M3 (die beiden OpenXchange-Konten
gleichzeitig, automatisches Live-Update) beginnt.

## Was in M2 bewusst noch fehlt

Mehrere Konten gleichzeitig + Live-Update neuer Mails (M3), HTML-Mails
verfassen, Entwürfe, Signaturen, Mail-Suche, Löschen/Verschieben,
Anhänge aus Mails öffnen/speichern. Kalender ab M4.

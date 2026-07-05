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
| M2 | Senden | Verfassen, Antworten, Weiterleiten, Anhänge, Ablage im „Gesendet“-Ordner | 🟢 Freigegeben |
| M3 | Multi-Account + Live-Update + Zed-Look | Mehrere Konten gleichzeitig, automatisches Live-Update (IMAP IDLE), Oberfläche im Zed-Stil mit Phosphor-Icons | 🟡 Wartet auf Freigabe |
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
| 2026-07-05 | Infomaniak-Abbruch („close_notify“) = Server trennt vor Anmeldung, typisch nach mehreren Fehlversuchen → eigene Fehlermeldung, kein Code-Fehler |
| 2026-07-05 | M3: Live-Update über IMAP IDLE (Posteingang sofort), plus 5-Minuten-Vollsync als Sicherheitsnetz für alle Ordner |
| 2026-07-05 | M3: Oberfläche im Zed-Look (Farbschema „One Dark“), Symbole Phosphor Thin — lokal mitgeliefert, kein Nachladen aus dem Netz |

## So testest du den aktuellen Stand (M3)

1. Bauen und installieren wie gehabt:
   `cd src-tauri && cargo tauri build`, dann
   `sudo dpkg -i src-tauri/target/release/bundle/deb/Nanomail_0.1.0_amd64.deb`
2. **Neue Optik:** Die Oberfläche ist jetzt dunkel im Stil des
   Zed-Editors, die Symbole sind dünne Phosphor-Icons. Dein bestehendes
   Konto bleibt erhalten.
3. **Zweites Konto:** Links unten „＋ Konto hinzufügen“ → dein zweites
   OpenXchange-Konto eintragen. Danach stehen beide Konten untereinander
   in der Seitenleiste, jedes mit eigenen Ordnern und Ungelesen-Zählern.
4. **Live-Update testen:** App geöffnet lassen und dir (oder von einem
   anderen Gerät) eine Mail an eines der Konten schicken. Erwartung: Sie
   erscheint nach wenigen Sekunden **von selbst** im Posteingang — ohne
   „Aktualisieren“ zu drücken.
5. **Senden mit Absenderwahl:** Bei „Verfassen“ gibt es jetzt ein
   „Von“-Feld — damit wählst du, über welches Konto gesendet wird.
6. **Konto entfernen:** Über das ⚙-Zahnrad → „Konto entfernen“
   (mit Rückfrage). Löscht nur lokal; auf dem Server ändert sich nichts.
7. **Infomaniak-Hinweis:** Deine Fehlermeldung („close_notify“) bedeutet,
   dass der Server die Verbindung *vor* der Anmeldung trennt — das ist
   meist eine **vorübergehende Sperre nach mehreren Fehlversuchen**.
   30–60 Minuten warten, im Infomaniak-Manager unter Sicherheit nach
   blockierten Geräten schauen, dann mit App-Passwort + vollständiger
   Adresse erneut versuchen. Die App zeigt dafür jetzt eine eigene,
   verständliche Meldung.

Wenn das passt: M3 freigeben → M4 (Kalender lesend, Nextcloud) beginnt.

## Was in M3 bewusst noch fehlt

Microsoft (M6, zuletzt), Kalender (ab M4), HTML-Mails verfassen,
Entwürfe, Signaturen, Mail-Suche, Löschen/Verschieben von Mails,
Anhänge aus Mails öffnen/speichern.

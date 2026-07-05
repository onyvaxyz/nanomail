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
| M3 | Multi-Account + Live-Update + Zed-Look | Mehrere Konten gleichzeitig, automatisches Live-Update (IMAP IDLE), Oberfläche im Zed-Stil mit Phosphor-Icons | 🟢 Freigegeben |
| M3.1 | Design-Feinschliff | Neu gestalteter Posteingang, runde Absender-Avatare (Gravatar/Favicon/Initialen), Verfassen im eigenen Fenster | 🟡 Wartet auf Freigabe |
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
| 2026-07-05 | M3.1: Verfassen/Antworten/Weiterleiten öffnen ein eigenes Fenster (native App statt Webseite); Lesen bleibt in der Vorschau |
| 2026-07-05 | **M3.1: Absender-Avatare laden bewusst externe Bilder (Gravatar → Favicon → Initialen).** Vom Projektinhaber ausdrücklich gewählte Ausnahme vom „keine externen Ladevorgänge“-Prinzip; Ergebnisse werden gecacht. Mail-Inhalte bleiben weiterhin geschützt (externe Bilder in Mails weiter blockiert). |

## So testest du den aktuellen Stand (M3.1)

1. Bauen und installieren wie gehabt:
   `cd src-tauri && cargo tauri build`, dann
   `sudo dpkg -i src-tauri/target/release/bundle/deb/Nanomail_0.1.0_amd64.deb`
   (Dein bestehendes Konto bleibt erhalten.)
2. **Neuer Posteingang:** aufgeräumte Liste mit rundem Absender-Bild
   (Gravatar/Favicon, sonst farbige Initialen), Betreff, Zeit, Punkt bei
   ungelesenen Mails. Oben der große „Neue Mail“-Knopf, links die Konten
   mit Ordner-Symbolen.
3. **Verfassen im eigenen Fenster:** „Neue Mail“ (bzw. „Antworten“/
   „Weiterleiten“ im Lesebereich) öffnet jetzt ein **separates Fenster** —
   wie bei einer echten Desktop-App. Mehrere gleichzeitig sind möglich.
   Nach dem Senden schließt es sich, das Hauptfenster meldet den Erfolg.
4. **Avatare:** Die runden Bilder werden aus dem Netz geladen (Gravatar,
   sonst Favicon der Absender-Domain), sonst zeigt Nanomail Initialen.
   *Hinweis:* Das ist die einzige Stelle, an der die App bewusst externe
   Bilder lädt — so von dir gewünscht. Mail-Inhalte selbst bleiben
   geschützt (Tracking-Pixel & Co. weiter blockiert).
5. **Zweites Konto / Live-Update / Konto entfernen** wie in M3 (unverändert).
6. **Infomaniak:** Falls wieder „Verbindung getrennt“ — das ist die
   vorübergehende Server-Sperre; kurz warten und mit App-Passwort erneut.

Wenn das passt: M3.1 freigeben → M4 (Kalender lesend, Nextcloud) beginnt.

## Was bewusst noch fehlt

Vorschau-/Schnipseltext in der Mail-Liste, Lesen im eigenen Fenster,
Microsoft (M6, zuletzt), Kalender (ab M4), HTML-Mails verfassen,
Entwürfe, Signaturen, Mail-Suche, Löschen/Verschieben von Mails,
Anhänge aus Mails öffnen/speichern.

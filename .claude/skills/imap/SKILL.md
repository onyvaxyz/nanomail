---
name: imap
description: Konventionen für IMAP-Abruf und Mail-Sync in Nanomail. Verwenden bei jeder Arbeit an src-tauri/src/imap/ — Verbindungsaufbau, Ordner-/Mail-Abruf, Abgleich mit dem SQLite-Cache, Umgang mit UIDVALIDITY und neuen Mails.
---

# IMAP-Konventionen (Nanomail)

## Konten

| Konto | Server-Typ | Auth |
|---|---|---|
| Infomaniak | Standard-IMAP | App-Passwort aus Keyring |
| OpenXchange 1 + 2 | Standard-IMAP | App-Passwort aus Keyring |
| Microsoft (M6) | IMAP + XOAUTH2 | OAuth-Token aus Keyring |

Konten generisch modellieren (ein `Konto`-Struct mit Host/Port/Auth-Variante),
keine provider-spezifischen Codepfade außer bei der Authentifizierung.

## Sync-Regeln (der eigentliche Kern)

- Quelle der Wahrheit ist der Server; SQLite ist Cache.
- Pro Ordner `UIDVALIDITY` speichern. Ändert sie sich → lokalen
  Ordner-Cache verwerfen und neu synchronisieren (niemals UIDs mischen).
- Inkrementeller Abgleich über UID-Bereiche (`UID FETCH <letzte>+1:*`),
  Flag-Änderungen und Löschungen per UID-Vergleich erkennen.
- Erstsync großer Postfächer: neueste Mails zuerst, in Batches, Header vor
  Bodies — die UI muss früh etwas anzeigen können.
- Neue Mails: IMAP IDLE pro Konto; Fallback Polling, Intervall
  konfigurierbar.
- Bodies lazy laden (erst beim Öffnen), danach cachen.

## Technik

- `async-imap` + `tokio`; immer TLS (Port 993), niemals STARTTLS-Downgrade
  akzeptieren.
- Sync-Entscheidungslogik (was fehlt/ist neu/wurde gelöscht) als reine
  Funktionen ohne Netzwerk implementieren und unit-testen; die
  Netzwerk-Schicht bleibt dünn.
- Fehler pro Konto isolieren: Ein nicht erreichbares Konto darf die anderen
  nicht blockieren; Status je Konto ans Frontend melden.
- Nie Mail-Inhalte oder Zugangsdaten loggen (siehe CLAUDE.md).

---
name: oauth-ms
description: Konventionen für den Microsoft-OAuth2-Flow in Nanomail (Meilenstein M6, letzter Schritt). Verwenden bei jeder Arbeit an src-tauri/src/oauth/ — Device-Code-Flow, Token-Refresh, Keyring-Ablage, XOAUTH2 für IMAP.
---

# Microsoft-OAuth2-Konventionen (Nanomail)

## Reihenfolge (verbindlich)

Microsoft ist **M6, der letzte Meilenstein**. Bevor irgendetwas gebaut wird:
**Mini-Auth-Test** — kleiner Wegwerf-Ablauf, der nur prüft, ob sich mit dem
Konto ein Token holen lässt. Der klärt:
- privates Konto (outlook.com) vs. Firmen-Tenant
- ob der Tenant fremde App-Registrierungen/Device-Code erlaubt

Erst wenn der Test gelingt, wird der volle Layer gebaut.

## Flow

- **Device-Code-Flow** (`https://login.microsoftonline.com/<tenant>/oauth2/v2.0/devicecode`),
  kein Loopback-Redirect: Die App zeigt Code + URL an, Philipp meldet sich
  im Browser an. Robust, kein lokaler HTTP-Server nötig.
- Scopes: `https://outlook.office365.com/IMAP.AccessAsUser.All offline_access`
  (SMTP-Scope erst ergänzen, wenn Versand über Microsoft gebraucht wird).
- `oauth2`-Crate verwenden; Tenant-Wert (`common`, `consumers` oder
  Tenant-ID) konfigurierbar halten, bis der Mini-Auth-Test ihn festlegt.

## Token-Handling

- Access- und Refresh-Token **nur im GNOME Keyring** (ein Eintrag pro Konto),
  nie in SQLite, Config-Dateien oder Logs.
- Refresh proaktiv vor Ablauf (Puffer ~5 Minuten); schlägt der Refresh fehl
  → Konto als „Neuanmeldung nötig“ markieren und im Frontend anzeigen,
  nicht endlos wiederholen.
- IMAP-Anmeldung via `AUTHENTICATE XOAUTH2`
  (Base64 von `user=<mail>\x01auth=Bearer <token>\x01\x01`).

## Fehlerbilder, auf die geprüft werden muss

- `authorization_pending` / `slow_down` beim Device-Code-Polling korrekt
  behandeln (Intervall respektieren).
- AADSTS-Fehlercodes verständlich übersetzen (z. B. „Admin-Zustimmung
  erforderlich“) statt roh anzuzeigen.

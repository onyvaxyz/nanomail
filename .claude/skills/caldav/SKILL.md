---
name: caldav
description: Konventionen für die CalDAV-Anbindung (3× Nextcloud) in Nanomail. Verwenden bei jeder Arbeit an src-tauri/src/caldav/ — XML-Schicht, Discovery, Sync-Token-Abgleich, ICS/RRULE/Zeitzonen, ETag-Konflikte.
---

# CalDAV-Konventionen (Nanomail)

## Grundsatz

Kein fertiger CalDAV-Crate — eigene, schmale XML-Schicht auf `reqwest`.
Nur die Requests implementieren, die Nanomail wirklich braucht.

## Ablauf

1. **Discovery** (einmalig pro Konto): `PROPFIND` auf
   `remote.php/dav/calendars/<user>/`, Kalender mit `displayname`,
   `calendar-color` und `sync-token` einsammeln.
2. **Abgleich**: `sync-collection` REPORT mit gespeichertem Sync-Token.
   Antwort liefert geänderte/gelöschte Objekte (href + etag).
   Bei ungültigem Token (HTTP 403/`valid-sync-token`-Fehler): kompletten
   Kalender neu laden.
3. **Objekte laden**: `calendar-multiget` REPORT für geänderte hrefs.
4. **Schreiben (M5)**: `PUT` mit `If-Match: <etag>`; bei 412 den Konflikt
   ans Frontend melden (Server-Stand anzeigen, nicht stumm überschreiben).

## XML

- Erzeugung über kleine Builder-Funktionen mit Tests (erwartetes XML als
  String-Vergleich); Parsing mit `quick-xml`, namespace-tolerant
  (`d:`/`D:`/default-ns nicht hart verdrahten).
- Jede Request/Response-Paarung als reine Funktion testbar ohne Netzwerk.

## ICS / Termine

- Parsing mit `icalendar`, Wiederholungen mit `rrule` expandieren —
  immer nur für das angefragte Zeitfenster, nie unbegrenzt.
- Zeitzonen: intern alles in UTC speichern, Umrechnung in die lokale Zone
  erst bei der Anzeige. VTIMEZONE-Definitionen aus dem ICS respektieren.
- Ausnahmen (EXDATE, RECURRENCE-ID-Überschreibungen) explizit behandeln
  und testen — hier liegen die klassischen Kalender-Bugs.

## Nextcloud-Spezifika

- App-Passwort aus dem Keyring, Basic Auth über HTTPS.
- Drei Kalender = drei Collections unter einem Konto; Farben aus
  `calendar-color` übernehmen.

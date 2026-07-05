---
name: frontend
description: Konventionen für das Nanomail-Frontend (ui/) und die Tauri-Command-Schnittstelle. Verwenden bei jeder Arbeit an HTML/CSS/JS, neuen Tauri-Commands oder der Anzeige von Mail-Inhalten.
---

# Frontend-Konventionen (Nanomail)

## Grundsatz

Vanilla HTML/CSS/JS in `ui/`, **kein Build-Schritt, keine npm-Abhängigkeiten
zur Laufzeit**. Philipp passt Optik selbst über `ui/styles.css` an — deshalb:
- Alle Farben/Maße als CSS-Variablen in `:root` am Dateianfang
- Klassennamen und Kommentare auf Deutsch, selbsterklärend
- Keine Inline-Styles im HTML, kein CSS in JS

## Optik (ab M3)

- Farbschema „Zed One Dark“ (dunkel), Werte als CSS-Variablen.
- Symbole: **Phosphor Icons Thin**, lokal unter `ui/phosphor/`
  (`<i class="ph-thin ph-<name>"></i>`). Nichts wird aus dem Netz geladen
  (CSP bleibt dicht) — neue Icons nur aus der lokalen Schrift verwenden.
- Mail-HTML-Anzeige bleibt bewusst hell (Mails sind für Weiß gestaltet).

## Rollenverteilung

Frontend = Darstellung. Keine Geschäftslogik in JS: kein Parsing, keine
Sync-Entscheidungen, keine Zugangsdaten. Alles Fachliche liefert das
Backend fertig aufbereitet über Commands/Events.

## Tauri-Commands

- Zugriff über `window.__TAURI__.core.invoke` (withGlobalTauri aktiv).
- Namensschema: `bereich_aktion` in snake_case, z. B. `mails_liste`,
  `mail_lesen`, `mail_senden`, `konten_status`, `kalender_termine`.
- Antworten sind `serde`-serialisierte Structs; Fehler kommen als
  verständliche deutsche Meldung (String) an und werden dem Nutzer in der
  Statusleiste oder im betroffenen Bereich angezeigt — niemals nur
  `console.error`.
- Push-Richtung (neue Mail eingetroffen, Sync-Status) über Tauri-Events
  (`listen`), Namensschema `bereich:ereignis`, z. B. `mails:neu`.

## Mail-Inhalte anzeigen (sicherheitskritisch)

- HTML kommt **ausschließlich vom Backend bereits mit `ammonia` bereinigt**.
  Das Frontend rendert nie rohes Mail-HTML.
- Anzeige in einer `<iframe sandbox>` ohne Skriptrechte; externe Bilder sind
  durch die CSP blockiert und werden erst nach Klick auf „Bilder laden“
  (Backend lädt und liefert als data:-URI) angezeigt.
- Nutzertexte immer über `textContent` setzen, nie `innerHTML` mit
  Fremddaten.

## Große Listen

Mail-Listen können zehntausende Einträge haben: nie alle DOM-Knoten auf
einmal erzeugen — seitenweise laden (Backend paginiert) und beim Scrollen
nachladen.

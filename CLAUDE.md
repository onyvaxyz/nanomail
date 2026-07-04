# Nanomail — Projektkontext für Claude Code

Eigener Mail- (IMAP/SMTP) & Kalender-Client (CalDAV) für Ubuntu.
Rust-Backend + Tauri 2 + Vanilla-HTML/CSS/JS-Frontend, Auslieferung als `.deb`.

**Der Projektinhaber ist kein Entwickler und kann Code nicht selbst prüfen.**
Daraus folgen die wichtigsten Regeln dieses Projekts:

1. **Kleine Schritte.** Es wird immer nur der aktuelle Meilenstein aus
   `PROJEKTPLAN.md` umgesetzt — nie mehrere auf einmal, nie vorgreifen.
2. **Freigabe-Workflow.** Nach jedem Meilenstein: Stopp, verständliche
   Zusammenfassung (deutsch, ohne Fachjargon), Testanleitung für den
   Projektinhaber. Erst nach seiner Freigabe beginnt der nächste Meilenstein.
3. **`PROJEKTPLAN.md` ist das lebende Übersichtsdokument.** Nach jedem
   abgeschlossenen Meilenstein Status und ggf. Entscheidungen dort
   aktualisieren.
4. **Qualität ist automatisiert abgesichert:** Tests für Kernlogik, CI muss
   grün sein (`test`, `clippy -D warnings`, `fmt`), Logging für Fehlerberichte.

## Tech-Stack

| Layer | Technologie |
|---|---|
| Backend | Rust (Edition 2021), Tauri 2 |
| Frontend | Vanilla HTML/CSS/JS in `ui/`, kein Build-Schritt, `withGlobalTauri` |
| IMAP | `async-imap` (ab M1) |
| SMTP | `lettre` (ab M2) |
| MIME | `mail-parser` (ab M1) |
| CalDAV | `reqwest` + eigene XML-Schicht (ab M4) |
| ICS | `icalendar`, RRULE via `rrule` (ab M4) |
| OAuth Microsoft | `oauth2`, Device-Code-Flow (M6, letzter Schritt) |
| Datenhaltung | SQLite via `rusqlite`, FTS5 für Suche (ab M1) |
| Logging | `tracing` → Datei unter `~/.local/share/nanomail/logs/` |

## Modulstruktur

```
ui/                  Frontend (HTML/CSS/JS) — reine Darstellung
src-tauri/src/
  lib.rs             Tauri-Setup, Command-Registrierung
  logging.rs         tracing-Initialisierung (Datei + Terminal)
  imap/              Mail-Abruf & Sync (M1)
  smtp/              Versand inkl. Sent-Ordner-Ablage (M2)
  caldav/            Nextcloud-Kalender, XML-Schicht (M4)
  oauth/             Microsoft Device-Code-Flow (M6)
  db/                SQLite-Cache, Migrationen, FTS5 (M1)
```

Backend = Datenlogik, Frontend = Darstellung. Keine Geschäftslogik in JS.
Kommunikation ausschließlich über Tauri-Commands/Events
(Namensschema: siehe `.claude/skills/frontend/SKILL.md`).

## Verbindliche Konventionen

- **Zugangsdaten:** Passwörter/Tokens ausschließlich im GNOME Keyring
  (`keyring`-Crate). Niemals im Klartext in Dateien, SQLite oder Logs.
- **HTML-Mails:** Vor Anzeige mit `ammonia` sanitizen. Externe Bilder
  standardmäßig blockieren („Bilder laden“-Button). Kein Skript aus
  Mail-Inhalten darf je ausgeführt werden.
- **Logging:** Nur technische Abläufe/Fehler loggen — nie Passwörter, Tokens,
  Mail-Inhalte oder Betreffzeilen.
- **Dateiablage:** XDG-Standard — Daten `~/.local/share/nanomail/`,
  Konfiguration `~/.config/nanomail/` (über `directories`-Crate).
- **Fehlerbehandlung:** Kein `unwrap()`/`expect()` außerhalb von Tests und
  `main`/`run`-Setup. Fehler mit Kontext bis zum Command-Rand durchreichen
  und dem Frontend als verständliche deutsche Meldung liefern.
- **Sprache:** UI-Texte, Doku und Commit-Beschreibungen deutsch;
  Rust-Bezeichner idiomatisch (englisch ist ok, deutsch ist ok — konsistent
  je Modul bleiben).
- **Tests:** Kernlogik (Sync-Entscheidungen, Parsing, XML-Erzeugung) ohne
  Netzwerk testbar halten; Netzwerk-Schicht dünn und getrennt.

## Build & Prüfung

```bash
cd src-tauri
cargo fmt --check          # Formatierung
cargo clippy -- -D warnings
cargo test
cargo tauri dev            # Entwicklung (braucht tauri-cli)
cargo tauri build          # erzeugt .deb unter target/release/bundle/deb/
```

CI (`.github/workflows/ci.yml`) führt fmt/clippy/test bei jedem Push aus
und muss grün sein, bevor ein Meilenstein als fertig gilt.

# Nanomail

Eigener Mail- (IMAP/SMTP) & Kalender-Client (CalDAV) für Ubuntu.
Rust + Tauri 2, Frontend in Vanilla HTML/CSS/JS, Auslieferung als `.deb`.

- **[PROJEKTPLAN.md](PROJEKTPLAN.md)** — Meilensteine, Status, Entscheidungen
  und Testanleitung für den aktuellen Stand
- **[CLAUDE.md](CLAUDE.md)** — Tech-Stack, Modulstruktur und verbindliche
  Konventionen für die Entwicklung

## Bauen

```bash
cd src-tauri
cargo tauri build   # erzeugt .deb unter target/release/bundle/deb/
```

Benötigte Systempakete (Ubuntu):

```bash
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
```

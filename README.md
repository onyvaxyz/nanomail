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

## Neuinstallation

Nanomail wird über die `.deb`-Datei installiert bzw. neu installiert:

```bash
cd src-tauri
cargo tauri build   # baut eine neue .deb unter target/release/bundle/deb/
sudo dpkg -i target/release/bundle/deb/Nanomail_0.1.0_amd64.deb
```

`dpkg -i` installiert automatisch über eine bestehende Version drüber —
ein vorheriges `apt remove` ist nicht nötig. Persönliche Daten (Mails,
Kalender-Cache, Zugangsdaten im Schlüsselbund) bleiben dabei erhalten, da sie
unter `~/.local/share/nanomail/` bzw. `~/.config/nanomail/` liegen und nicht
Teil des Pakets sind.

Fehlen nach `dpkg -i` noch Abhängigkeiten, meldet dpkg das mit einem Fehler;
in dem Fall einmalig:

```bash
sudo apt-get install -f
```

Ob Nanomail installiert ist und welche Version:

```bash
dpkg -l | grep nanomail
```

Vollständig deinstallieren:

```bash
sudo apt-get remove nanomail
```

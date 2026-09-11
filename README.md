# Nanomail

Eigener Mail- (IMAP/SMTP) & Kalender-Client (CalDAV) für Linux.
Rust + Tauri 2, Frontend in Vanilla HTML/CSS/JS; Arch-/Omarchy-Paket und `.deb`.

- **[PROJEKTPLAN.md](PROJEKTPLAN.md)** — Meilensteine, Status, Entscheidungen
  und Testanleitung für den aktuellen Stand
- **[CLAUDE.md](CLAUDE.md)** — Tech-Stack, Modulstruktur und verbindliche
  Konventionen für die Entwicklung

## Arch Linux / Omarchy

Im aktuellen Checkout einen überprüfbaren Quellschnappschuss erzeugen und
**auf einem aktuellen Arch Linux x86_64** als normaler Benutzer bauen:

```bash
sudo pacman -Syu --needed base-devel
bash packaging/arch/prepare-source.sh
cd .amp/arch-build
less PKGBUILD
makepkg -si
```

Das Skript kopiert die PKGBUILD-Vorlage und ersetzt deren Prüfsummenplatzhalter
durch die tatsächliche SHA-256 des lokalen Quellarchivs. Es nimmt auch noch
nicht veröffentlichte Änderungen mit, aber keine Zugangsdaten oder vorherigen
Builds. Nur diesen erzeugten PKGBUILD mit `makepkg` verwenden.
`makepkg -si` installiert Build-Abhängigkeiten, baut mit Tauri und gesperrtem
Cargo.lock, führt Rust-Tests aus und installiert `nanomail-0.1.0-1-x86_64.pkg.tar.zst`.
Die ersten Downloads und der Rust-Build brauchen Internet, Zeit und mehrere GB Platz.
Node.js ist für den Paketbau nicht erforderlich.

Ein bereits gebautes **Arch-Paket** installieren/aktualisieren:

```bash
sudo pacman -U ./nanomail-0.1.0-1-x86_64.pkg.tar.zst
nanomail
# Paketinhalt prüfen / deinstallieren (persönliche Daten bleiben erhalten):
pacman -Ql nanomail
sudo pacman -R nanomail
```

Der Programmstarter und Icons werden systemweit installiert. Für Zugangsdaten
muss in der Desktop-Sitzung ein entsperrter Secret-Service-Anbieter laufen,
z. B. GNOME Keyring; bestehende Omarchy-Schlüsselbunde nicht ersetzen.
Bei fehlenden Wayland-Dateidialogen `xdg-desktop-portal-gtk` zusätzlich zum
vorhandenen Hyprland-Portal installieren und die Sitzung neu anmelden.
Die Theme-Auswahl „Omarchy“ liest die lokale Omarchy-Palette automatisch.

Ein Debian-/Ubuntu-Build ist **kein Arch-Binärpaket**. Bei Bedarf den erzeugten
PKGBUILD samt Quellarchiv in einem sauberen Arch-Chroot mit `extra-x86_64-build`
aus `devtools` bauen. Die MIT-Angabe stammt aus Cargo.toml; ein eigener
vollständiger Lizenztext fehlt bisher im Repository und muss vor öffentlicher
Paketveröffentlichung vom Projektinhaber ergänzt werden. Die vorhandene
Phosphor-Lizenz wird mitinstalliert.

## Bauen unter Debian / Ubuntu

```bash
cd src-tauri
cargo tauri build   # erzeugt .deb unter target/release/bundle/deb/
```

Benötigte Systempakete (Ubuntu):

```bash
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
```

## Neuinstallation unter Debian / Ubuntu

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

## UI-Prüfungen und lokale Vorschau

Das Frontend benötigt weiterhin keinen Build und keine Laufzeitpakete.
Nur die Browserprüfungen verwenden Node.js und Playwright:

```bash
npm ci
npx playwright install --with-deps chromium webkit
npm run check
npm test
```

`node tests/preview.cjs` startet eine Vorschau auf Port 4173 mit simuliertem
Tauri-Backend und ausschließlich Beispieldaten. Die Seiten `index.html`,
`verfassen.html`, `verfassen.html?antwortAuf=1` und
`mail.html?mailId=1&kontoId=1` sind unabhängig aufrufbar.
Es werden keine echten Mails versendet und keine Konten verändert.

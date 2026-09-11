# Nanomail — Projektplan

Lebendes Übersichtsdokument. Wird nach jedem Meilenstein aktualisiert.
Regel: **Ein Meilenstein nach dem anderen, jeder wird von Philipp getestet
und freigegeben, bevor der nächste beginnt.**

Stand: 2026-09-11

## Arch-/Omarchy-Paketierung (September 2026)

- `packaging/arch/prepare-source.sh` erzeugt einen lokalen Quellschnappschuss
  einschließlich unveröffentlichter Änderungen und einen PKGBUILD mit echter
  SHA-256. Keine vorgebauten Debian-Binaries und keine persönlichen Daten.
- Arch-Paketbau mit `cargo-tauri`, gesperrtem Cargo.lock und Rust-Tests;
  Installation nach `/usr/bin`, Desktop-Starter und vier Hicolor-Icongrößen.
  Build-/Runtime-Abhängigkeiten sind getrennt; Secret Service und das optionale
  Wayland-Dateidialog-Portal sind dokumentiert. Bestehende Debian-Unterstützung bleibt.
- Im Orb mit offiziellem Arch-Bootstrap 2026.09.01 und signaturgeprüften,
  aktuellen Arch-Paketen in einem isolierten Bubblewrap-Dateisystem gebaut,
  nicht gegen Debian-/Ubuntu-Bibliotheken. Rust 1.98.1, glibc 2.44,
  WebKitGTK 2.52.6; Ausgabe `nanomail-0.1.0-1-x86_64.pkg.tar.zst`.
- 120 Rust-Tests unter Arch bestanden; Paketinhalt, Desktop-Datei, dynamische
  Bibliotheken, Pacman-Installation und nativer Start mit leerem Testprofil
  geprüft. Ein Screenshot des gerenderten Arch-Programms wurde inspiziert.
  JavaScript-Prüfung, Chromium/WebKit-Tests, Rust-fmt und Clippy erneut grün.
- Einschränkungen: Xvfb statt echter Omarchy-/Hyprland-Sitzung; kein eingerichteter
  Secret Service oder Live-Mailkonto. `namcap` erkennt den dynamisch benutzten
  `xdg-open`-Fallback nicht; `xdg-utils` bleibt daher eine Laufzeitabhängigkeit.
  Makepkg meldet eingebettete Rust-Quellpfade unter `/build` (keine persönlichen
  Pfade). Der eigene vollständige MIT-Lizenztext fehlt bereits im Repository;
  vor einer öffentlichen Veröffentlichung muss der Projektinhaber ihn ergänzen.

Installation und lokaler Nachbau: siehe README, Abschnitt „Arch Linux / Omarchy“.
Kein Push, Release, Deployment oder Pull Request; keine Freigabe weiterer Meilensteine.

## Beauftragte Nachbesserungen zu M5.1 (September 2026)

Lokal umgesetzt; keine Veröffentlichung und keine Freigabe von M6:

- Buttons wählen Schwarz/Weiß anhand des sichtbaren Hintergrunds einschließlich
  Kontofarbe, Transparenz und Hover. Fenster verwenden den gemeinsamen
  8-Pixel-Außenabstand von `#app` (im Projekt eine ID, keine Klasse `.app`).
- Antworten begannen bisher mit freiem Text vor dem ersten Absatz. Das ist in
  WebKit reproduziert und korrigiert. Neue Mail, Antworten und Allen antworten
  verwenden echte Absätze: Enter = Absatz, Shift+Enter = Zeilenumbruch.
  Die Adressfelder An/Cc bleiben einzeilige Empfängerfelder; Enter übernimmt
  dort gegebenenfalls einen Adressvorschlag, es gibt dort keine Absatzformatierung.
- Signaturen erscheinen jetzt ausdrücklich auch bei Antworten, mit einer
  Leerzeile davor. Das ersetzt den früheren Wunsch „nur bei Erstnachricht“.
  Textmails und Textentwürfe bewahren Absatzgrenzen als Leerzeilen und einzelne
  Zeilenumbrüche als einfache neue Zeilen.
- Datei- und Speicherdialoge beginnen auf dem plattformgerechten Desktop;
  fehlt dieser, im persönlichen Ordner. Dateinamen können den Startordner nicht verlassen.
- Eingehende ICS-/MIME-Kalenderteile werden erkannt, im Cache gespeichert und
  in beiden Mailansichten interaktiv angezeigt. Zu-/Absagen senden nach Rückfrage
  eine iTIP-REPLY-Mail an den Organisator. Das Konto muss als Teilnehmer genannt
  sein. Ohne Organisator bzw. bei CANCEL/REPLY werden keine Antwortknöpfe angeboten.
  Keine automatische Übernahme in CalDAV und keine automatische Verarbeitung
  fremder Zu-/Absagen. Ältere Cacheeinträge werden beim Öffnen einmal nachgeladen;
  bei fehlendem Netz bleibt die bisherige Mailansicht verfügbar.
- Termindetails bleiben beim Markieren, Kopieren und Fensterwechsel offen.
  HTTP(S)-Links sind klickbar; Escape/Schließen gibt den Fokus zurück, ein neuer
  Klick außerhalb schließt. Lange Inhalte bleiben scrollbar.
- Omarchy ergänzt Hell/Dunkel/System. Unter Linux werden
  `$XDG_STATE_HOME/omarchy/current/theme/colors.toml` (Standard `~/.local/state`)
  sowie der ältere Pfad unter `$XDG_CONFIG_HOME` gelesen. `mode`, `theme_type`,
  `light.mode` und Hintergrundhelligkeit bestimmen Hell/Dunkel; gültige
  Hex-Farben speisen die vorhandenen Tokens. Prüfung alle drei Sekunden und
  beim Fensterfokus, ohne Shellbefehle oder Schreibzugriffe. Ohne Palette bzw.
  außerhalb Linux gilt die Systemeinstellung einschließlich normaler Kontofarbe.
  Quellen: [Omarchy Theme-Set](https://github.com/basecamp/omarchy/blob/master/bin/omarchy-theme-set),
  [Farbschema](https://github.com/basecamp/omarchy/blob/master/themes/tokyo-night/colors.toml).

Prüfung: Rust-Kerntests, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo check`, vollständiger Tauri-`.deb`-Build sowie Browserprüfungen in Chromium
und WebKit. Letztere testen echte Tastatureingaben, Rückgängig, Text-Roundtrips,
Kontrastwechsel, Themewechsel/Fallback, Maus-Textselektion und Einladungsaktionen.
Screenshots für neue Mail, Antwort, Einladung und Kalender wurden inspiziert.
Browser-Review verwendet ausschließlich Testdaten, keinen echten Mailversand.
Ein echter Omarchy-Desktop und Live-IMAP/SMTP/CalDAV waren im Orb nicht eingerichtet.

Zum Abnehmen: Neue Mail und Antwort mit Enter/Shift+Enter verfassen, Signaturabstand
prüfen, einen Entwurf wieder öffnen; bei einer echten Einladung bewusst Zu-/Absage
testen; Termintext über den Popoverrand hinaus markieren; Theme und Desktop-Dialog
auf dem Zielrechner ausprobieren. Danach M5/M5.1 freigeben, nicht automatisch M6 starten.

## Meilensteine

| Nr. | Meilenstein | Inhalt | Status |
|---|---|---|---|
| M0 | Projektgerüst | Tauri-App-Skelett, Grundlayout, Logging, CI, `.deb`-Build, Doku/Skills | 🟢 Freigegeben |
| M1 | Erstes Konto lesend | IMAP: Ordner & Mails anzeigen (HTML bereinigt, Bilder blockiert), SQLite-Cache, Passwort im Keyring | 🟢 Freigegeben |
| M2 | Senden | Verfassen, Antworten, Weiterleiten, Anhänge, Ablage im „Gesendet“-Ordner | 🟢 Freigegeben |
| M3 | Multi-Account + Live-Update + Zed-Look | Mehrere Konten gleichzeitig, automatisches Live-Update (IMAP IDLE), Oberfläche im Zed-Stil mit Phosphor-Icons | 🟢 Freigegeben |
| M3.1 | Design-Feinschliff | Neu gestalteter Posteingang, runde Absender-Avatare (Gravatar/Favicon/Initialen), Verfassen im eigenen Fenster | 🟢 Freigegeben |
| M3.2 | Redesign (Zed One Dark, Violett) | Layout-Umbau nach Vorlage (Icon-Leiste statt Konten-Baum, Ordner als Reiter), Konto-Icons nach Gravatar/Favicon/Initialen-Schema, Avatar-Bugfix (Kachelung behoben), lokal mitgelieferte Schriften | 🟢 Freigegeben |
| M3.3 | Bedienkomfort & Personalisierung | Mail löschen (Entf-Taste + Papierkorb-Symbol), Signatur je Konto, Formatier-Leiste beim Verfassen (fett, kursiv, Listen, Links), Anhänge per Hineinziehen, Fensterleiste im App-Design, wählbare Konto-Farbe für die ganze Oberfläche | 🟢 Freigegeben |
| M3.4 | Bedienkomfort II | Rechtsklick-Menü „Als (un)gelesen markieren“, Ungelesen-Filter in der Mail-Liste, Doppelklick auf die Fensterleiste maximiert, Fenstergröße per Rand-Ziehen, Adress-Vorschläge beim Verfassen (aus bisherigen Empfängern und Absendern, kein Adressbuch) | 🟢 Freigegeben |
| M3.5 | Suche & einheitliche Lesefläche | Volltextsuche über alle Ordner des Kontos (Suchfeld oben), Lesebereich standardmäßig im App-Stil mit „Originalansicht“-Umschalter für HTML-Mails | 🟢 Freigegeben |
| M3.6 | Anhänge & Entwürfe | Anhang-Leiste unten im Lesebereich (jeder Anhang einzeln sichtbar, Klick speichert), zuverlässiges Anhang-Kennzeichen schon beim Abgleich, Entwürfe (speichern, weiterbearbeiten, nach Versand automatisch entfernt), Symbolleiste aufgeräumt (Archivieren/Stern/Drei-Punkte entfernt) | 🟢 Freigegeben |
| M4 | Kalender lesend | Nextcloud-CalDAV (mehrere Konten), Monatsansicht, Farbe je Kalender, Wiederholungstermine, Zeitzonen, Offline-Cache | 🟢 Freigegeben |
| M4.1 | Zwischenschritt Mail-Komfort | „Beantwortet“-Markierung in der Mail-Liste (Pfeil-Symbol; wird beim Antworten gesetzt und vom Server übernommen), „Löschen“ im Rechtsklick-Menü — damit lassen sich auch Entwürfe löschen | 🟢 Freigegeben |
| M5 | Kalender schreibend *(optional)* | Termine erstellen/bearbeiten/löschen, Personen per E-Mail einladen, Konfliktbehandlung | 🟡 Wartet auf Freigabe |
| M5.1 | Komfort- & Fehlerbehebungsrunde | Neuer Termin nach Wiederholungstermin, Links im Browser, Ungelesen-Zähler, Favicon-Fallback, Avatar-Hintergrund, Signatur nur bei Erstnachricht, Tab bei Empfängern, Emoji-Auswahl | 🟡 Wartet auf Freigabe |
| M6 | Microsoft (zuletzt) | Erst Mini-Auth-Test (klärt Kontotyp & Tenant-Regeln), dann Device-Code-Flow, Token-Refresh, IMAP-Anbindung | ⚪ Offen |

Status-Legende: ⚪ Offen · 🔵 In Arbeit · 🟡 Wartet auf Freigabe · 🟢 Freigegeben

## Offene Entscheidungen

| Entscheidung | Wann klären |
|---|---|
| Microsoft-Konto: privat oder Firmen-Tenant (mit/ohne Admin-Zugriff)? | Zu Beginn von M6 per Mini-Auth-Test |
| Kalender auch schreiben (M5) oder nur lesen? | Nach Abnahme von M4 |
| Git/Versionierung: Dieser Ordner ist eine Experimentier-Kopie ohne Repo-Anbindung — Versionsverwaltung wird am Projektende sauber aufgesetzt. | Am Projektende |

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
| 2026-07-06 | M3.3: Löschen verschiebt in den Papierkorb (wiederherstellbar); nur im Papierkorb selbst wird nach Rückfrage endgültig gelöscht. Ordner ohne Server-Kennzeichnung werden zusätzlich am Namen erkannt (z. B. „Trash“). |
| 2026-07-06 | M3.3: Verfassen erzeugt jetzt Text **und** formatiertes HTML (multipart/alternative); das HTML wird vor dem Versand mit derselben Schutzschicht bereinigt wie beim Anzeigen. Die Signatur wird pro Konto gespeichert und beim Verfassen automatisch eingefügt (bei Antworten über dem Zitat, Trennlinie „-- “). |
| 2026-07-06 | M3.3: Jedes Konto kann eine eigene Farbe bekommen (Konto-Dialog). Sie färbt Auswahl, Ordner-Reiter, Ungelesen-Punkte, Knöpfe usw.; die Icon-Leiste zeigt jedes Konto dauerhaft in seiner Farbe. Standard bleibt das Zed-Violett. |
| 2026-07-06 | M3.3: Beide Fenster nutzen eine eigene Fensterleiste im App-Design (Systemrahmen abgeschaltet), mit Minimieren/Maximieren/Schließen; Verschieben per Ziehen an der Leiste. |
| 2026-07-07 | M3.1–M3.3 von Philipp freigegeben. |
| 2026-07-07 | M3.5 von Philipp freigegeben. |
| 2026-07-07 | M3.6: Das Anhang-Kennzeichen (Büroklammer) kommt jetzt schon beim Abgleich aus der Struktur-Auskunft des Servers (BODYSTRUCTURE) — bisher wurde es erst beim Öffnen einer Mail erkannt, daher fehlte es oft. Dafür wird der lokale Mail-Cache einmalig neu aufgebaut (Konten, Einstellungen und Adress-Vorschläge bleiben erhalten; der erste Start lädt die Listen neu). |
| 2026-07-07 | M3.6: Anhänge werden weiterhin nie lokal zwischengespeichert. Die neue Anhang-Leiste zeigt Name und Größe (beim ersten Öffnen der Mail gemerkt); „Speichern“ holt den Inhalt frisch vom Server und legt ihn am gewählten Ort ab. |
| 2026-07-07 | M3.6: Entwürfe liegen im Entwürfe-Ordner des Servers (auch in anderen Programmen sichtbar). Klick auf eine Mail im Entwürfe-Ordner öffnet sie im Verfassen-Fenster; erneutes Speichern ersetzt den alten Stand, nach dem Senden wird der Entwurf automatisch entfernt. Ein zwischengespeicherter Antwort-Entwurf verliert den Gesprächsbezug (Threading) — bewusst einfach gehalten. |
| 2026-07-07 | M3.6: Symbole ohne Funktion (Archivieren, Stern, Drei-Punkte) entfernt — sie kommen erst wieder, wenn es die Funktion dahinter gibt. Nebenbei behoben: Die 25-MB-Grenze für Anhänge zählte Datei-Anhänge doppelt und schlug zu früh an. |
| 2026-07-07 | M3.4 von Philipp freigegeben (inklusive Nachbesserung: Doppelklick ohne Zurückspringen, Größe per Rand-Ziehen). |
| 2026-07-07 | M3.6 von Philipp freigegeben. |
| 2026-07-07 | M4: Die drei Nextcloud-Kalender liegen unter **mehreren Nextcloud-Konten** — die App verwaltet Kalender-Konten getrennt von Mail-Konten (Server-Adresse, Benutzername, App-Passwort im Schlüsselbund). |
| 2026-07-07 | M4: Kalender-Farben kommen zunächst aus Nextcloud und lassen sich in Nanomail je Kalender überschreiben. |
| 2026-07-07 | M4: Nur Monatsansicht — Wochen-/Tagesansicht bewusst weggelassen (wird nicht genutzt). |
| 2026-07-07 | M4 umgesetzt: eigene schmale CalDAV-Schicht (Kalender-Suche, Abgleich über Sync-Token — bei verfallenem Token wird der Kalender einmal neu geladen). Termine liegen als Original-Daten im lokalen Cache (offline nutzbar); Wiederholungen werden immer nur für den angezeigten Monat ausgerechnet, Ausnahmen (ausgefallene und verschobene Einzeltermine) und Zeitzonen (intern UTC, Anzeige in lokaler Zeit, Sommer-/Winterzeit) werden berücksichtigt. Abgleich beim Start, beim Öffnen der Kalender-Ansicht, per Knopf und alle 5 Minuten mit. |
| 2026-07-07 | M4: Kalender-Verbindung nur über https; App-Passwort liegt im Schlüsselbund (`kalender:<id>`), nie im Klartext. Reine Aufgaben-Listen (VTODO) werden bei der Kalender-Suche übersprungen. |
| 2026-07-07 | M3.5 Suche: Volltextindex (SQLite FTS5) über Betreff und Absender aller Mails; der Mailtext zählt mit, sobald er lokal im Cache liegt (wird beim Öffnen einer Mail gefüllt). Gesucht wird je Konto über alle Ordner; Treffer tragen ein Ordner-Etikett. Wortanfänge genügen („rech“ findet „Rechnung“). |
| 2026-07-07 | M3.5 Lesefläche: Alle Mails erscheinen standardmäßig im App-Stil (dunkel, App-Schrift); dafür entfernt eine zweite, strengere Bereinigungsstufe das mitgebrachte Design der Mail. Der Paletten-Knopf über der Mail schaltet pro Mail auf die helle Originalansicht des Absenders um. „Bilder laden“ wirkt in beiden Ansichten. Die Schutzregeln (bereinigtes HTML, Sandbox, blockierte externe Bilder) bleiben unverändert. |
| 2026-07-07 | M3.4-Nachbesserung: Das Doppelklick-Maximieren erledigt die Titelleiste (Tauri-Drag-Region) bereits von selbst — der in M3.4 zusätzlich eingebaute Doppelklick-Code schaltete dadurch doppelt um, das Fenster sprang sofort zurück. Der Zusatz-Code wurde entfernt. Außerdem lässt sich die Fenstergröße jetzt per Ziehen an allen Rändern und Ecken ändern (unsichtbare Anfasser in `ui/fenster.js` — ohne Systemrahmen bietet GNOME sonst keine Ränder an). |
| 2026-07-07 | Neue Wünsche in zwei Schritte geteilt: M3.4 (Ungelesen-Markierung/-Filter, Doppelklick-Maximieren, Adress-Vorschläge) zuerst, danach M3.5 (Suche + einheitliche Lesefläche). |
| 2026-07-07 | Lesefläche (M3.5): Alle Mails erscheinen künftig standardmäßig im App-Stil; ein Knopf „Originalansicht“ zeigt HTML-Mails bei Bedarf im Design des Absenders. |
| 2026-07-08 | M4 von Philipp freigegeben („Kalender funktioniert super“). |
| 2026-07-08 | M4.1 (eingeschoben): Die „Beantwortet“-Markierung nutzt das offizielle Mail-Kennzeichen (IMAP `\Answered`) — Antworten aus anderen Programmen (z. B. Handy) erscheinen dadurch ebenfalls markiert, und Nanomail-Antworten sind in anderen Programmen sichtbar. Die Markierung wird beim Abgleich vom Server übernommen; der Server bleibt die Quelle der Wahrheit. |
| 2026-07-08 | M4.1: „Löschen“ im Rechtsklick-Menü folgt denselben Regeln wie der Löschen-Knopf (Papierkorb; im Papierkorb endgültig nach Rückfrage) und funktioniert für alle Mails — insbesondere Entwürfe, die sich nicht im Lesebereich öffnen lassen und bisher nicht löschbar waren. |
| 2026-07-08 | M4.1 von Philipp freigegeben. M5 startet als nächster Schritt; Termine sollen auch Personen per E-Mail einladen können. |
| 2026-07-08 | M5 umgesetzt: In der Kalenderansicht können einfache Termine erstellt, bearbeitet und gelöscht werden. „Ort“ ist bewusst ein freies Textfeld (Raum, Adresse, Telefon oder Link). Eingeladene Personen werden als E-Mail-Adressen im Termin gespeichert (`ATTENDEE` im Kalenderformat); der Teilnahmestatus aus Nextcloud/Thunderbird wird angezeigt (z. B. bestätigt/nicht bestätigt). Weil Nextcloud in der getesteten Konfiguration keine Einladungs-Mail verschickt, kann Nanomail zusätzlich optional selbst eine Kalender-Einladungs-Mail (`METHOD:REQUEST`, nur in der Mail — nicht im CalDAV-Objekt) über ein auswählbares Mailkonto senden. Bei späteren Änderungen an einem Termin mit Teilnehmern ist die Änderungs-Mail standardmäßig aktiviert, aber abwählbar. Das Teilnehmerfeld nutzt die bekannten Adress-Vorschläge wie beim Mail-Verfassen. Änderungen und Löschungen nutzen den CalDAV-Konfliktschutz (`If-Match`) — wurde der Termin inzwischen anderswo geändert, fordert Nanomail zum Aktualisieren auf statt stumm zu überschreiben. Zusätzlich kann im Mail-Konto ein Anzeigename für ausgehende Mails gepflegt werden. Wiederholungstermine bleiben lesbar und können als ganze Serie gelöscht werden; Bearbeiten von Serien ist noch gesperrt, damit Wiederholungsregeln nicht versehentlich verloren gehen. |
| 2026-07-10 | M5-Qualitätsprüfung (Code-Review mit mehreren Prüf-Perspektiven, da die Umsetzung extern erfolgte): 10 bestätigte bzw. plausible Probleme gefunden und behoben. Die wichtigsten: (1) Ein Anzeigename mit Komma/Klammern hätte jeden Mailversand des Kontos blockiert — Absender wird jetzt strukturiert gebaut statt als Text geparst. (2) Löschen eines einzelnen Serien-Vorkommens löscht die ganze Serie — die Rückfrage warnt jetzt ausdrücklich davor. (3) Der Organisator in Einladungs-Mails ist jetzt die Adresse des gewählten Versand-Mailkontos (vorher der Nextcloud-Anmeldename, der meist keine E-Mail-Adresse ist — Einladungen wären ohne Organisator formal ungültig gewesen und hätten beim Empfänger keine Zusagen-Knöpfe gezeigt). Außerdem: strengere Prüfung von Teilnehmeradressen (Sonderzeichen hätten das Kalenderformat beschädigt), Kalender-Auswahl beim Bearbeiten gesperrt (Verschieben wird noch nicht unterstützt), verständliche Meldung bei abgelehnter Anmeldung auch beim Speichern/Löschen, mehrere Randfälle bei Server-Versionskennungen (ETags) und Groß-/Kleinschreibung von Teilnehmern. |
| 2026-07-10 | Einladungs-Mail und Kalender-Objekt bekommen bewusst zwei verschiedene Fassungen: Der Mail-Anbieter lehnte Nanomails Einladungs-Mail ab („550 Reject for policy reason“), sobald sie den Organisator enthielt — vermutlich ein Schutz gegen Kalender-Spoofing, der Einladungen nur aus dem anbietereigenen Kalender zulässt. Die verschickte Mail nutzt deshalb wieder exakt das Format ohne Organisator, das nachweislich durchkam und bei den Empfängern funktionierte (Bestätigen-Knöpfe inklusive). Das in Nextcloud gespeicherte Objekt behält den Organisator (saubere Anzeige) samt „Server bleibt still“-Schalter. |
| 2026-07-16 | Infomaniak-Konto ließ sich nicht hinzufügen („peer closed connection without sending TLS close_notify“), obwohl die Einstellungen stimmten: Nanomail schickte die Anmeldung sofort nach dem Verbindungsaufbau los, ohne die Begrüßung des Servers abzuwarten. Die meisten Anbieter verzeihen das — Infomaniak verwirft solche zu frühen Anfragen in gut der Hälfte der Fälle und trennt dann kommentarlos (live am Server nachgemessen: sofort gesendet 3 von 5 Fehlversuchen, nach Begrüßung 5 von 5 erfolgreich). Nanomail wartet jetzt — wie Thunderbird — erst auf die Server-Begrüßung. Die IP-Sperren im Infomaniak-Manager waren Folge der vielen Einrichtungsversuche, nicht die Ursache. |
| 2026-07-16 | Helles/dunkles Design (Wunsch von Philipp): Neuer Umschalt-Knopf in der Icon-Leiste unter dem Kalender-Symbol. Er wechselt reihum Dunkel → Hell → Systemeinstellung (Ubuntu) und zeigt als Symbol immer den nächsten Schritt (Mond/Sonne/Bildschirm). Das helle Design nutzt bewusst #EBEBEC statt Weiß als Hintergrund. Die Wahl wird gespeichert und beim nächsten Start übernommen; Erststandard ist „Systemeinstellung“ — wechselt Ubuntu selbst zwischen hell/dunkel, zieht Nanomail live mit. Gilt für Haupt- und Verfassen-Fenster sowie die Mail-Anzeige in der App-Ansicht; die „Originalansicht des Absenders“ bleibt wie bisher hell. |
| 2026-07-10 | Einladungen laufen jetzt komplett „nativ“ über Nanomail (Wunsch von Philipp): Durch die Organisator-Korrektur hatte Nextcloud begonnen, zusätzlich eigene Einladungs-Mails mit Web-Link zu verschicken — zwei „Akzeptieren“-Wege verwirrten. Nanomail markiert Teilnehmer jetzt mit dem offiziellen Schalter `SCHEDULE-AGENT=CLIENT` (RFC 6638): Der Kalender-Server verschickt nichts mehr, nur noch Nanomails eigene Einladungs-Mail. Folge: Zu-/Absagen der Empfänger kommen als normale Mail an und aktualisieren den Teilnehmerstatus im Kalender (noch) nicht automatisch — die Verarbeitung solcher Antwort-Mails wäre ein eigener späterer Schritt. |
| 2026-07-07 | M3.4: Adress-Vorschläge ohne Adressbuch — beim Senden werden Empfänger gemerkt, zusätzlich zählen Absender aus dem Mail-Cache. Vorschläge erscheinen beim Tippen im An-/CC-Feld. |
| 2026-07-07 | M3.4: „Als ungelesen markieren“ ändert das Flag sofort in der App und überträgt es im Hintergrund zum Server (wie beim Lesen); klappt das nicht (offline), korrigiert es der nächste Abgleich. |
| 2026-07-05 | **M3.2: Komplettes Redesign nach eigener Vorlage** (Zed One Dark, Violett-Akzent, Schriften JetBrains Mono/Inter). Löst den Kachel-Fehler bei Absender-Avataren (Ursache: eine CSS-Kurzschreibweise in JS überschrieb versehentlich die Bild-Darstellung). Konto-Icons in der neuen Icon-Leiste nutzen ab jetzt ebenfalls Gravatar → Favicon → Initialen — die bestehende Ausnahme vom „keine externen Ladevorgänge“-Prinzip gilt damit für Absender- **und** Konto-Avatare. Schriften/Symbole werden weiterhin nur lokal mitgeliefert, nicht aus dem Netz geladen. Löschen/Archivieren/Markieren sind als Symbole schon sichtbar, aber noch ohne Funktion (kommt später). |
| 2026-07-20 | **M5.1 Nachtrag 2:** (a) Im Gesendet-Ordner zeigt die Liste jetzt den Empfänger („An: …") statt meiner eigenen Adresse. (b) Beim Lesen werden An- und (falls vorhanden) Cc-Empfänger im Kopf angezeigt — man sieht jetzt, ob jemand in Kopie stand. (c) Das Datum steht im Lesekopf unter den Buttons (rechts), damit ein langer Absender keine schiefen Umbrüche mehr erzeugt. Für (a)/(b) werden An/Cc je Mail schon beim Abgleich gespeichert (zwei neue Cache-Spalten); der Mail-Cache wird dafür einmalig neu aufgebaut (Konten/Einstellungen bleiben, die Listen laden beim nächsten Start neu). Zum dev-Fenster: `GDK_BACKEND=x11 cargo tauri dev` wurde auf diesem Rechner als funktionierend verifiziert (Fenster mit Inhalt); wichtig ist, die installierte Nanomail-App vorher zu schließen (sonst ist der lokale Cache gesperrt). |
| 2026-07-20 | **M5.1 Nachtrag:** (a) Beim Antworten/Weiterleiten wurde als Absender fälschlich das erste Konto vorgewählt statt des Kontos, in dessen Ordner man sich befindet — es wird jetzt die aktive Konto-ID ans Verfassen-Fenster übergeben. (b) Strg + Mausrad zoomt jetzt das ganze Fenster (Mail wird mitvergrößert); zusätzlich ist der native Zoom über Strg + Plus/Minus aktiv (`zoomHotkeysEnabled`). Direkt über dem Mail-Inhalt fängt das Sicherheits-iframe das Mausrad ab — dort wirkt Strg + Plus/Minus. (c) Hinweis: `cargo tauri dev` zeigt auf diesem System ein leeres Fenster (WebKitGTK-Darstellungsfehler im Debug-Build unter Wayland); die installierte .deb rendert korrekt. Für dev hilft i. d. R. `GDK_BACKEND=x11 cargo tauri dev`. |
| 2026-07-20 | **M5.1 (Komfort- & Fehlerbehebungsrunde, 9 Wünsche von Philipp):** (1) Nach dem Öffnen eines Wiederholungstermins ließ sich kein neuer Termin mehr anlegen („Termin lokal nicht mehr vorhanden“) — das versteckte Termin-Kennzeichen blieb im Formular hängen; es wird beim Anlegen jetzt sauber geleert. (2) Links in Mails öffnen jetzt im Standard-Browser des Systems (Backend fängt den Klick ab; Mail-Inhalte bleiben unverändert streng geschützt). (3) Der Ungelesen-Zähler am Konto-Symbol zählt nur noch den Posteingang statt aller Ordner (Spam/Papierkorb blähten die Zahl auf). (4) Favicon-Fallback greift jetzt auch bei Absender-Subdomains (z. B. `notify.docker.com` → `docker.com`); zusätzlich werden bisher als „kein Bild“ gemerkte Einträge einmalig verworfen und neu ermittelt. (5) Bei Gravatar/Favicon hat der Avatar-Kreis keinen farbigen Hintergrund mehr — nur Initialen behalten die Farbe. (6) Die Signatur erscheint nur noch bei einer neuen Erstnachricht, nicht beim Antworten/Weiterleiten. (7) Adress-Vorschläge lassen sich auch mit Tab übernehmen (nicht nur Enter). (8) Tab springt von den Empfängern direkt in den Mailinhalt; die Formatier-Knöpfe werden übersprungen. (9) Neue Emoji-Auswahl im Verfassen-Fenster (Smiley-Knopf oder „Super + .“). |
| 2026-07-21 | **M5.1 Nachtrag 3 (wartet auf Praxistest):** (a) Die Suche arbeitet jetzt nur im gerade geöffneten Ordner, sodass „Gesendet“ keine Posteingangstreffer mehr zeigt. (b) Online ergänzt eine direkte IMAP-Volltextsuche den lokalen Index und findet dadurch auch ungeöffnete Mails, ohne alle Nachrichten samt Anhängen herunterzuladen; offline bleibt der lokale Bestand durchsuchbar. (c) Im Editor erzeugt Enter einen Absatz mit sichtbarem Abstand und Umschalt+Enter eine einfache neue Zeile ohne Zusatzabstand. (d) Der Fensterzoom reagiert auf Strg+Mausrad und Umschalt+Mausrad. (e) Nanomail zeigt 30 Minuten vor einem sichtbaren Kalendertermin eine Ubuntu-Systembenachrichtigung mit Titel, Uhrzeit und optionalem Ort. Die Prüfung läuft minütlich, berücksichtigt Serien/Zeitzonen und merkt sich gezeigte Erinnerungen dauerhaft, damit sie nicht doppelt erscheinen. Nanomail muss dafür laufen; ein eigener Hintergrunddienst bei geschlossener App ist bewusst nicht hinzugefügt. |
| 2026-07-24 | **M5.1 Nachtrag 4 (wartet auf Praxistest):** Die Oberfläche wurde auf Wunsch in einem ruhigen, von Amp inspirierten Stil neu gestaltet: warme Grün-Neutraltöne, Ubuntu-/Systemschrift, feinere Trennlinien, zurückhaltende Rundungen und eine flachere Mail-Liste. Hell-, Dunkel- und Systemmodus bleiben erhalten; Funktionen und gespeicherte Daten sind unverändert. |
| 2026-07-24 | **M5.1 Nachtrag 5 (wartet auf Praxistest):** Design-Nachbesserung nach Sichtprüfung: Der gesamte helle Inhaltsbereich nutzt einheitlich `#F9FDF6` und liegt leicht eingerückt mit abgerundeten Ecken auf der gemeinsamen Fläche von Fensterkopf und Statusleiste. Jede Mail besitzt wieder einen eigenen feinen Rahmen. Die bisherige Bedeutung leer gespeicherter Kontofarben bleibt erhalten, damit insbesondere das Hauptkonto wieder sein früheres Standard-Violett statt des neuen Grüns verwendet; ausdrücklich gewählte eigene Farben bleiben unverändert. |
| 2026-07-24 | **M5.1 Nachtrag 6 (wartet auf Praxistest):** Weitere Bedienungsrunde: „Allen antworten“ mit Doppelpfeil und bereinigter Empfängerliste (eigene Konten/Dubletten werden entfernt), zuverlässige Absatz-/Zeilenwechsel auch in neuen Mails sowie Strg+Z, eigenes Lesefenster per Doppelklick, dauerhaft erlaubbare externe Bilder je Absender-Domain, reine größere Ordner-Symbole, gleichzeitiger Status je Mailkonto und Google statt DuckDuckGo als Favicon-Fallback. Ohne Funktionsverlust beschleunigt: Ordnerstände werden je Konto parallel geladen, doppelte Avatar-Abfragen zusammengeführt und externe Mailbilder begrenzt parallel geladen. Die bekannten Beispieldomains `artischock.net`, `gra.ch`, `rizag.ch` und `woistroci.de` liefern bei DuckDuckGo 404, bei Google dagegen ein Bild; Microsoft 365 ist nicht die Ursache. |

## So testest du den aktuellen Stand (M5)

1. Bauen und installieren wie gehabt:
   `cd src-tauri && cargo tauri build`, dann
   `sudo dpkg -i src-tauri/target/release/bundle/deb/Nanomail_0.1.0_amd64.deb`
   Mail-Konten und Einstellungen bleiben erhalten.
2. **Termin erstellen:** Kalender öffnen → oben auf „Termin“ klicken
   (oder im Monatsraster auf einen Tag doppelklicken). Titel, Zeit, Ort
   und optional Teilnehmer-E-Mail eintragen → speichern. Der Termin sollte
   sofort in Nanomail und nach kurzer Zeit auch in Nextcloud erscheinen.
3. **Ort testen:** Beim Ort z. B. einen Link oder freien Text eintragen.
   Beim Anklicken des Termins muss dieser Text unverändert angezeigt werden.
4. **Adressvorschläge:** Bei „Teilnehmer“ mindestens zwei Buchstaben einer
   bekannten Adresse tippen. Es sollten Vorschläge wie beim Mail-Verfassen
   erscheinen; Auswahl per Klick oder Pfeiltasten/Enter.
5. **Person einladen:** Bei „Teilnehmer“ eine E-Mail-Adresse eintragen und
   „Einladungs-Mail mit Nanomail senden“ anhaken. Erst dann erscheint die
   Auswahl des Mailkontos für den Versand. Das gewünschte Mailkonto
   auswählen und speichern. Danach prüfen:
   - Der Termin erscheint in Nanomail und Nextcloud.
   - Die Person steht in Nextcloud als Teilnehmer.
   - Beim Empfänger kommt **genau eine** Kalender-Einladung per Mail an
     (die von Nanomail) — keine zusätzliche Nextcloud-Mail mit Web-Link.
6. **Termin ändern und Update senden:** Den neuen Termin anklicken →
   „Bearbeiten“, z. B. die Uhrzeit ändern. Wenn Teilnehmer eingetragen sind,
   ist „Änderungs-Mail mit Nanomail senden“ automatisch angehakt. Speichern
   und prüfen: Die Änderung erscheint in Nanomail/Nextcloud, und beim
   Empfänger kommt eine Aktualisierungs-Mail an.
7. **Teilnehmerstatus:** Beim Anklicken des Termins wird je Teilnehmer der
   in Nextcloud gespeicherte Status angezeigt (z. B. „Bestätigt“ oder
   „Nicht bestätigt“). Hinweis: Zu-/Absagen der Empfänger kommen als
   normale Mail bei dir an; der Status im Kalender aktualisiert sich
   dadurch (noch) nicht automatisch.
8. **Mail-Anzeigename:** Mail-Konto bearbeiten → „Anzeigename beim Senden“
   eintragen → speichern. Eine Testmail senden; beim Empfänger sollte der
   Name vor der Adresse erscheinen. Auch mit Komma testen (z. B.
   „Nachname, Vorname“) — der Versand muss trotzdem funktionieren.
9. **Termin bearbeiten ohne Mail:** Soll keine Aktualisierung verschickt
   werden, beim Bearbeiten das Häkchen „Änderungs-Mail …“ entfernen und
   speichern. Die Änderung sollte trotzdem in Nanomail und Nextcloud sichtbar
   werden.
10. **Termin löschen:** Den Termin anklicken → „Löschen“ und bestätigen.
   Er sollte aus Nanomail und aus Nextcloud verschwinden. Beim Löschen
   eines Wiederholungstermins muss die Rückfrage ausdrücklich warnen,
   dass die gesamte Serie gelöscht wird.
11. **Konfliktschutz (optional):** Einen Termin in Nanomail öffnen, dann den
   gleichen Termin in Nextcloud ändern und erst danach in Nanomail speichern.
   Nanomail sollte nicht überschreiben, sondern zum Aktualisieren auffordern.
12. **Suche je Ordner:** Im Ordner „Gesendet“ nach einem Begriff suchen, der
    auch im Posteingang vorkommt. Es dürfen nur Treffer aus „Gesendet“ erscheinen.
13. **Ungeöffnete Mail durchsuchen:** Bei bestehender Internetverbindung nach
    einem Begriff im Text einer noch nie in Nanomail geöffneten Mail suchen.
    Auch diese Mail muss gefunden werden; die Serversuche kann kurz dauern.
14. **Editor:** Beim Verfassen Enter drücken (neuer Absatz) und danach
    Umschalt+Enter (nur eine neue Zeile innerhalb des Absatzes).
15. **Zoom:** Strg (alternativ Umschalt) halten und über Mail-Liste oder
    Seitenleiste am Mausrad drehen. Das Fenster muss größer/kleiner werden.
    Über dem geschützten Mail-Inhalt weiterhin Strg+Plus/Minus verwenden.
16. **Terminerinnerung:** Einen Termin auf ungefähr 30 Minuten ab jetzt setzen
    und Nanomail geöffnet lassen. Spätestens nach einer Minute muss Ubuntu eine
    Systembenachrichtigung mit Termintitel, Beginn und ggf. Ort zeigen. Sie darf
    bei den folgenden Prüfungen nicht erneut erscheinen.
17. **Neues Design:** Hell- und Dunkelmodus über den Knopf links durchschalten.
    Im hellen Modus muss der Inhaltsbereich einheitlich sehr hellgrün sein und
    mit sichtbaren Rundungen auf der Fläche von Kopf und Statusleiste liegen.
    Jede Mail braucht einen feinen eigenen Rahmen. Danach alle Konten anklicken:
    Die jeweils eingestellte Kontofarbe muss Knöpfe und Auswahl markieren.
18. **Allen antworten:** Eine Mail mit mehreren An-/Cc-Empfängern öffnen und
    den neuen Doppelpfeil anklicken. Absender und weitere Personen müssen
    eingetragen sein, die eigenen Mailadressen aber nicht. Dasselbe im
    Gesendet-Ordner prüfen: Dort darf Nanomail nicht an die eigene Adresse
    antworten.
19. **Editor und Rückgängig:** Eine komplett neue Mail öffnen. Enter muss
    einen Absatz mit Abstand erzeugen, Umschalt+Enter nur eine neue Zeile.
    Text eingeben und mit Strg+Z rückgängig machen.
20. **Eigenes Lesefenster:** Eine Mail im Posteingang doppelt anklicken. Es
    muss genau ein eigenes Fenster mit der Mail erscheinen; Antworten,
    Allen antworten, Weiterleiten und Anhänge dort kurz prüfen.
21. **Bilder dauerhaft erlauben:** Bei einer Mail mit blockierten Bildern
    „Von dieser Quelle immer laden“ wählen. Eine weitere Mail derselben
    Absender-Domain öffnen — ihre Bilder sollen nun automatisch erscheinen.
    Mails anderer Domains bleiben blockiert.
22. **Ordner und Status:** Oben in der Mail-Liste müssen Posteingang,
    Entwürfe usw. nur noch als größere Symbole erscheinen (Name beim
    Darüberfahren). „Aktualisieren“ drücken: Unten muss für jedes Mailkonto
    getrennt stehen, ob es aktualisiert wird, aktuell ist oder einen Fehler
    hatte.
23. **Avatare:** Im Konto `onyva.xyz` Absender von `artischock.net`, `gra.ch`,
    `rizag.ch` und `woistroci.de` erneut ansehen. Nach dem ersten Laden
    sollten die Google-Favicons erscheinen; vorhandene echte Gravatars haben
    weiterhin Vorrang.

Wenn das passt: M5 freigeben → danach folgt M6 (Microsoft, zuletzt).

## Was bewusst noch fehlt

Vorschau-/Schnipseltext in der Mail-Liste,
Microsoft (M6, zuletzt), Bearbeiten von Wiederholungstermin-Serien,
automatische Übernahme von Zu-/Absage-Mails in den Teilnehmerstatus,
Wochen-/Tagesansicht im Kalender (bewusst weggelassen),
Archivieren/Markieren/Verschieben in beliebige Ordner,
Anhänge direkt aus der Mail öffnen (Speichern geht bereits).

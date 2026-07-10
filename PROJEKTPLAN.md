# Nanomail — Projektplan

Lebendes Übersichtsdokument. Wird nach jedem Meilenstein aktualisiert.
Regel: **Ein Meilenstein nach dem anderen, jeder wird von Philipp getestet
und freigegeben, bevor der nächste beginnt.**

Stand: 2026-07-08

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
| 2026-07-07 | M3.4: Adress-Vorschläge ohne Adressbuch — beim Senden werden Empfänger gemerkt, zusätzlich zählen Absender aus dem Mail-Cache. Vorschläge erscheinen beim Tippen im An-/CC-Feld. |
| 2026-07-07 | M3.4: „Als ungelesen markieren“ ändert das Flag sofort in der App und überträgt es im Hintergrund zum Server (wie beim Lesen); klappt das nicht (offline), korrigiert es der nächste Abgleich. |
| 2026-07-05 | **M3.2: Komplettes Redesign nach eigener Vorlage** (Zed One Dark, Violett-Akzent, Schriften JetBrains Mono/Inter). Löst den Kachel-Fehler bei Absender-Avataren (Ursache: eine CSS-Kurzschreibweise in JS überschrieb versehentlich die Bild-Darstellung). Konto-Icons in der neuen Icon-Leiste nutzen ab jetzt ebenfalls Gravatar → Favicon → Initialen — die bestehende Ausnahme vom „keine externen Ladevorgänge“-Prinzip gilt damit für Absender- **und** Konto-Avatare. Schriften/Symbole werden weiterhin nur lokal mitgeliefert, nicht aus dem Netz geladen. Löschen/Archivieren/Markieren sind als Symbole schon sichtbar, aber noch ohne Funktion (kommt später). |

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
   - Beim Empfänger kommt eine Kalender-Einladung per Mail an.
6. **Termin ändern und Update senden:** Den neuen Termin anklicken →
   „Bearbeiten“, z. B. die Uhrzeit ändern. Wenn Teilnehmer eingetragen sind,
   ist „Änderungs-Mail mit Nanomail senden“ automatisch angehakt. Speichern
   und prüfen: Die Änderung erscheint in Nanomail/Nextcloud, und beim
   Empfänger kommt eine Aktualisierungs-Mail an.
7. **Teilnehmerstatus:** Wenn der Empfänger in seinem Kalenderprogramm zu-
   oder absagt, Kalender in Nanomail erneut abgleichen. Beim Anklicken des
   Termins sollte der Status sichtbar sein (z. B. „Bestätigt“ oder
   „Nicht bestätigt“).
8. **Mail-Anzeigename:** Mail-Konto bearbeiten → „Anzeigename beim Senden“
   eintragen → speichern. Eine Testmail senden; beim Empfänger sollte der
   Name vor der Adresse erscheinen.
9. **Termin bearbeiten ohne Mail:** Soll keine Aktualisierung verschickt
   werden, beim Bearbeiten das Häkchen „Änderungs-Mail …“ entfernen und
   speichern. Die Änderung sollte trotzdem in Nanomail und Nextcloud sichtbar
   werden.
10. **Termin löschen:** Den Termin anklicken → „Löschen“ und bestätigen.
   Er sollte aus Nanomail und aus Nextcloud verschwinden.
11. **Konfliktschutz (optional):** Einen Termin in Nanomail öffnen, dann den
   gleichen Termin in Nextcloud ändern und erst danach in Nanomail speichern.
   Nanomail sollte nicht überschreiben, sondern zum Aktualisieren auffordern.

Wenn das passt: M5 freigeben → danach folgt M6 (Microsoft, zuletzt).

## Was bewusst noch fehlt

Vorschau-/Schnipseltext in der Mail-Liste, Lesen im eigenen Fenster,
Microsoft (M6, zuletzt), Bearbeiten von Wiederholungstermin-Serien,
Wochen-/Tagesansicht im Kalender (bewusst weggelassen),
Archivieren/Markieren/Verschieben in beliebige Ordner,
Anhänge direkt aus der Mail öffnen (Speichern geht bereits).

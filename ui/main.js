// Nanomail — Hauptfenster (M3.1).
// Reine Darstellung: alle Daten kommen fertig aufbereitet aus dem
// Rust-Backend. Verfassen läuft in einem eigenen Fenster (verfassen.html).

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { WebviewWindow, getCurrentWebviewWindow } = window.__TAURI__.webviewWindow;

const SEITENGROESSE = 50;

const zustand = {
  konten: [],
  ordnerJeKonto: new Map(), // kontoId -> Ordner[]
  aktiverOrdnerId: null,
  aktivesKontoId: null,
  aktiveMailId: null,
  offset: 0,
  alleGeladen: false,
  laedtNach: false,
  nurUngelesen: false,
  suchbegriff: "",
  /// Ordner der geöffneten Mail (kann bei Suchtreffern vom aktiven
  /// Ordner abweichen — wichtig für die Papierkorb-Rückfrage).
  aktiveMailOrdnerId: null,
  /// Inhalt des Lesebereichs: beide HTML-Fassungen + gewählte Ansicht.
  lese: { html: null, schlicht: null, modus: "app" },
  kontoDialog: { modus: "anlegen", kontoId: null },
};

// Avatar-Cache im Frontend: email -> dataUri | null (null = Initialen).
const avatarCache = new Map();
// Noch laufende Abfragen zusammenführen, damit dieselbe Adresse in einer
// langen Liste nicht gleichzeitig mehrfach extern gesucht wird.
const avatarAnfragen = new Map();
// Ein Eintrag je Konto verhindert, dass sich parallele Sync-Meldungen
// gegenseitig überschreiben.
const syncStatus = new Map();
// Kennzeichnet die jüngste Mail-Leseanfrage. Langsame ältere Antworten
// dürfen weder die Ansicht überschreiben noch Bilder einer neuen Mail laden.
let leseAnfrage = 0;
let ordnerLadeAnfrage = null;

// ---------------------------------------------------------- DOM-Kürzel --

const el = (id) => document.getElementById(id);
const datumFormat = new Intl.DateTimeFormat("de-DE", {
  day: "2-digit", month: "2-digit", year: "2-digit",
  hour: "2-digit", minute: "2-digit",
});
const uhrzeitFormat = new Intl.DateTimeFormat("de-DE", { hour: "2-digit", minute: "2-digit" });
const tagFormat = new Intl.DateTimeFormat("de-DE", { day: "2-digit", month: "2-digit" });

/// Kompakte Zeitangabe: heute = Uhrzeit, sonst Tag.Monat, älter = mit Jahr.
function zeitKompakt(sekunden) {
  if (!sekunden) return "";
  const d = new Date(sekunden * 1000);
  const jetzt = new Date();
  if (d.toDateString() === jetzt.toDateString()) return uhrzeitFormat.format(d);
  if (d.getFullYear() === jetzt.getFullYear()) return tagFormat.format(d);
  return datumFormat.format(d).slice(0, 8);
}

function status(text, klasse = "") {
  el("backend-status").innerHTML = "";
  const span = document.createElement("span");
  if (klasse) span.className = klasse;
  span.textContent = text;
  el("backend-status").appendChild(span);
}

function zeige(id, sichtbar) {
  el(id).classList.toggle("versteckt", !sichtbar);
}

function icon(name) {
  const i = document.createElement("i");
  i.className = `ph-light ph-${name}`;
  return i;
}

// ------------------------------------------------------ Fensterleiste --

const aktuellesFenster = getCurrentWebviewWindow();
el("fenster-minimieren").addEventListener("click", () => aktuellesFenster.minimize());
el("fenster-maximieren").addEventListener("click", () => aktuellesFenster.toggleMaximize());
el("fenster-schliessen").addEventListener("click", () => aktuellesFenster.close());
// Doppelklick-Maximieren übernimmt die Tauri-Drag-Region der Titelleiste
// selbst — hier keinen eigenen dblclick-Handler ergänzen, sonst wird
// doppelt umgeschaltet und das Fenster springt sofort zurück.
// Größenändern per Rand-Ziehen: siehe fenster.js.

// Strg oder Umschalt + Mausrad zoomt das ganze Fenster (die Mail wird mit
// vergrößert). Beide Varianten bleiben aktiv, damit die Bedienung unabhängig
// von der gewohnten Desktop-Konvention funktioniert.
// Hinweis: Direkt über dem Mail-Inhalt fängt das abgeschottete Sicherheits-
// Fenster das Rad ab — dort zoomt zusätzlich Strg + Plus/Minus (nativ).
let zoomFaktor = 1;
document.addEventListener(
  "wheel",
  (ereignis) => {
    if (!ereignis.ctrlKey && !ereignis.shiftKey) return;
    ereignis.preventDefault();
    const schritt = ereignis.deltaY < 0 ? 0.1 : -0.1;
    zoomFaktor = Math.min(3, Math.max(0.3, Math.round((zoomFaktor + schritt) * 10) / 10));
    aktuellesFenster.setZoom(zoomFaktor).catch(() => {});
  },
  { passive: false, capture: true },
);

// ------------------------------------------------------- Konto-Farben --

// Leere Farbe in älteren Konten bedeutet weiterhin das damalige
// Standard-Violett. So bleibt eine bereits gewählte Kontofarbe erhalten.
const STANDARD_FARBE = "#c678dd";

function kontoFarbe(konto) {
  return konto && konto.farbe ? konto.farbe : STANDARD_FARBE;
}

/// Zieht die Farbe des aktiven Kontos als Akzent durch die ganze
/// Oberfläche (alle Akzent-Töne leiten sich per color-mix von --akzent ab).
function akzentAnwenden() {
  const konto = zustand.konten.find((k) => k.id === zustand.aktivesKontoId);
  document.documentElement.style.setProperty("--akzent", kontoFarbe(konto));
}

// ------------------------------------------------------------- Avatare --

/// Initialen aus Anzeigename/Adresse (max. 2 Buchstaben).
function initialen(name, email) {
  const quelle = (name || email || "?").trim();
  const teile = quelle.split(/[\s@._-]+/).filter(Boolean);
  if (teile.length === 0) return "?";
  if (teile.length === 1) return teile[0].slice(0, 2).toUpperCase();
  return (teile[0][0] + teile[1][0]).toUpperCase();
}

/// Deterministische, ruhige Farbe aus einer Zeichenkette.
function avatarFarbe(text) {
  let hash = 0;
  for (const zeichen of text || "?") hash = (hash * 31 + zeichen.charCodeAt(0)) & 0xffffff;
  const hue = hash % 360;
  return `hsl(${hue}, 42%, 45%)`;
}

/// Baut ein Avatar-Element: sofort Initialen, dann ggf. echtes Bild nachladen.
/// Nutzt ein <img>-Kind statt background-image, damit ein Bild nie gekachelt
/// dargestellt werden kann (siehe M3.2: alte Kurzschreibweise `style.background =`
/// setzte background-repeat/-size implizit zurück und überstimmte die CSS-Regeln).
function avatarElement(name, email, extraKlasse = "") {
  const kreis = document.createElement("div");
  kreis.className = `avatar ${extraKlasse}`.trim();
  kreis.style.background = avatarFarbe(email || name);
  const kuerzel = document.createElement("span");
  kuerzel.textContent = initialen(name, email);
  kreis.appendChild(kuerzel);

  if (email) avatarLaden(email, kreis);
  return kreis;
}

async function avatarLaden(email, kreis) {
  const schluessel = email.toLowerCase();
  if (avatarCache.has(schluessel)) {
    bildSetzen(kreis, avatarCache.get(schluessel));
    return;
  }
  try {
    let anfrage = avatarAnfragen.get(schluessel);
    if (!anfrage) {
      anfrage = invoke("absender_avatar", { email }).finally(() => avatarAnfragen.delete(schluessel));
      avatarAnfragen.set(schluessel, anfrage);
    }
    const uri = await anfrage;
    avatarCache.set(schluessel, uri || null);
    bildSetzen(kreis, uri || null);
  } catch {
    avatarCache.set(schluessel, null); // bei Fehler bleiben Initialen
  }
}

function bildSetzen(kreis, uri) {
  if (!uri) return; // Initialen behalten
  const bild = document.createElement("img");
  bild.src = uri;
  bild.alt = "";
  // Echtes Bild (Gravatar/Favicon) steht ohne farbigen Kreis; die
  // Hintergrundfarbe bleibt nur unter den Initialen.
  kreis.style.background = "transparent";
  kreis.replaceChildren(bild);
}

// ------------------------------------------------------------- Start --

async function start() {
  try {
    zustand.konten = await invoke("konten_liste");
    if (zustand.konten.length === 0) {
      kontoDialogOeffnen("anlegen", null);
      status("Bitte zuerst ein Mail-Konto einrichten.");
      return;
    }
    zeige("verfassen-knopf", true);
    zeige("aktualisieren-knopf", true);
    zeige("konto-hinzufuegen-knopf", true);
    await kontenAnzeigen();
    erstenOrdnerOeffnen();
    synchronisieren();
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
}

function erstenOrdnerOeffnen() {
  if (zustand.aktiverOrdnerId) return;
  for (const konto of zustand.konten) {
    const ordner = zustand.ordnerJeKonto.get(konto.id) || [];
    if (ordner.length > 0) {
      ordnerOeffnen(konto.id, ordner[0].id);
      return;
    }
  }
}

// ------------------------------------------------------------ Konten --

/// Passendes Ordner-Icon nach Rolle bzw. Name.
function ordnerIconName(ordner) {
  switch (ordner.rolle) {
    case "gesendet": return "paper-plane-tilt";
    case "entwuerfe": return "note-pencil";
    case "papierkorb": return "trash";
    case "spam": return "warning-octagon";
    case "archiv": return "archive";
    default:
      return ordner.name.toUpperCase() === "INBOX" ? "tray" : "folder";
  }
}

async function kontenAnzeigen() {
  if (!ordnerLadeAnfrage) {
    ordnerLadeAnfrage = Promise.all(zustand.konten.map(async (konto) => {
      try {
        zustand.ordnerJeKonto.set(konto.id, await invoke("ordner_liste", { kontoId: konto.id }));
      } catch (fehler) {
        status(`✗ ${konto.name}: ${fehler}`, "fehler");
      }
    })).finally(() => {
      ordnerLadeAnfrage = null;
    });
  }
  await ordnerLadeAnfrage;
  kontenLeisteAnzeigen();
  ordnerPillenAnzeigen();
}

/// Icon-Leiste (Spalte 1): ein rundes Konto-Icon je Konto, mit
/// Ungelesen-Abzeichen (Summe über alle Ordner) und Aktiv-Markierung.
function kontenLeisteAnzeigen() {
  const bereich = el("konten-bereich");
  bereich.innerHTML = "";
  for (const konto of zustand.konten) {
    const knopf = document.createElement("button");
    knopf.className = "konto-icon";
    knopf.type = "button";
    knopf.style.setProperty("--konto-farbe", kontoFarbe(konto));
    if (konto.id === zustand.aktivesKontoId) knopf.classList.add("aktiv");
    knopf.title = `${konto.name} · ${konto.email}`;
    knopf.appendChild(avatarElement(konto.name, konto.email, "avatar-konto"));

    // Abzeichen zeigt nur die ungelesenen Mails im Posteingang — nicht die
    // Summe aller Ordner (Spam/Papierkorb würden die Zahl aufblähen).
    const eingang = (zustand.ordnerJeKonto.get(konto.id) || []).find(
      (ordner) => ordner.name.toUpperCase() === "INBOX",
    );
    const ungelesenGesamt = eingang ? eingang.ungelesen || 0 : 0;
    if (ungelesenGesamt > 0) {
      const abzeichen = document.createElement("span");
      abzeichen.className = "konto-icon-abzeichen";
      abzeichen.textContent = ungelesenGesamt > 99 ? "99+" : String(ungelesenGesamt);
      knopf.appendChild(abzeichen);
    }

    knopf.addEventListener("click", () => kontoAuswaehlen(konto.id));
    bereich.appendChild(knopf);
  }
}

/// Ordner-Reiter + Konto-Kontext-Zeile (Kopf der Mail-Liste), nur für das
/// gerade ausgewählte Konto.
function ordnerPillenAnzeigen() {
  const konto = zustand.konten.find((k) => k.id === zustand.aktivesKontoId);
  const ordnerListe = zustand.aktivesKontoId
    ? zustand.ordnerJeKonto.get(zustand.aktivesKontoId) || []
    : [];

  const reihenfolge = { "entwuerfe": 1, "gesendet": 2, "spam": 3, "papierkorb": 4 };
  const sortierteOrdner = [...ordnerListe].sort((a, b) => {
    const istEingang = (o) => o.name.toUpperCase() === "INBOX";
    const ra = istEingang(a) ? 0 : reihenfolge[a.rolle] ?? 9;
    const rb = istEingang(b) ? 0 : reihenfolge[b.rolle] ?? 9;
    return ra - rb;
  });

  const pillen = el("ordner-pillen");
  pillen.innerHTML = "";
  for (const ordner of sortierteOrdner) {
    const pille = document.createElement("li");
    pille.className = "ordner-pille";
    if (ordner.id === zustand.aktiverOrdnerId) pille.classList.add("aktiv");
    pille.appendChild(icon(ordnerIconName(ordner)));
    pille.title = ordner.anzeige_name;
    pille.setAttribute("aria-label", ordner.anzeige_name);
    pille.addEventListener("click", () => ordnerOeffnen(zustand.aktivesKontoId, ordner.id));
    pillen.appendChild(pille);
  }

  el("mailliste-konto-adresse").textContent = konto ? konto.email : "";
  const aktiverOrdner = ordnerListe.find((o) => o.id === zustand.aktiverOrdnerId);
  el("ordner-anzahl").textContent = aktiverOrdner && aktiverOrdner.gesamt ? `${aktiverOrdner.gesamt}` : "";
}

/// Wechselt das ausgewählte Konto und öffnet dessen ersten Ordner.
function kontoAuswaehlen(kontoId) {
  if (kontoId === zustand.aktivesKontoId) return;
  const ordner = zustand.ordnerJeKonto.get(kontoId) || [];
  if (ordner.length > 0) ordnerOeffnen(kontoId, ordner[0].id);
}

function ordnerOeffnen(kontoId, ordnerId) {
  // Ein Ordnerwechsel beendet eine laufende Suche.
  zustand.suchbegriff = "";
  el("suche-feld").value = "";
  zustand.aktivesKontoId = kontoId;
  zustand.aktiverOrdnerId = ordnerId;
  zustand.offset = 0;
  zustand.alleGeladen = false;
  el("mail-eintraege").innerHTML = "";
  akzentAnwenden();
  kontenAnzeigen(); // aktiv-Markierung (Icon-Leiste + Ordner-Reiter)
  naechsteSeiteLaden();
}

// --------------------------------------------------------- Mail-Liste --

function leerText() {
  return zustand.nurUngelesen
    ? "Keine ungelesenen Mails in diesem Ordner."
    : "Dieser Ordner ist leer.";
}

function leerzustand(behaelter, iconName, text) {
  behaelter.innerHTML = "";
  const box = document.createElement("div");
  box.className = "leerzustand";
  box.appendChild(icon(iconName));
  const p = document.createElement("p");
  p.textContent = text;
  box.appendChild(p);
  behaelter.appendChild(box);
}

async function naechsteSeiteLaden() {
  if (zustand.suchbegriff) return; // Suchansicht blättert nicht nach
  if (zustand.alleGeladen || zustand.laedtNach || !zustand.aktiverOrdnerId) return;
  zustand.laedtNach = true;
  try {
    const mails = await invoke("mails_liste", {
      ordnerId: zustand.aktiverOrdnerId,
      nurUngelesen: zustand.nurUngelesen,
      offset: zustand.offset,
      limit: SEITENGROESSE,
    });
    if (mails.length < SEITENGROESSE) zustand.alleGeladen = true;

    const behaelter = el("mail-eintraege");
    if (zustand.offset === 0 && mails.length === 0) {
      leerzustand(behaelter, "tray", leerText());
    } else {
      if (zustand.offset === 0) behaelter.innerHTML = "";
      for (const mail of mails) behaelter.appendChild(mailEintrag(mail));
    }
    zustand.offset += mails.length;
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  } finally {
    zustand.laedtNach = false;
  }
}

/// Eine Empfängerzeile („An: …" / „Cc: …") für den Lesekopf.
function empfaengerZeile(feld, adressen) {
  const zeile = document.createElement("span");
  zeile.className = "meta-empfaenger";
  const name = document.createElement("span");
  name.className = "meta-feldname";
  name.textContent = `${feld}:`;
  zeile.append(name, document.createTextNode(" " + adressen));
  return zeile;
}

/// Ordner (über alle Konten) zu einer Ordner-ID — auch für Suchtreffer.
function ordnerZuId(ordnerId) {
  for (const liste of zustand.ordnerJeKonto.values()) {
    const treffer = liste.find((o) => o.id === ordnerId);
    if (treffer) return treffer;
  }
  return null;
}

/// Erste Adresse einer kommagetrennten Empfängerliste (für Avatare).
function ersteAdresse(liste) {
  return (liste || "").split(",")[0].trim();
}

/// Kurzform einer Empfängerliste: erste Adresse, sonst „ + N".
function empfaengerKurz(liste) {
  const teile = (liste || "").split(",").map((t) => t.trim()).filter(Boolean);
  if (teile.length === 0) return "(kein Empfänger)";
  return teile.length === 1 ? teile[0] : `${teile[0]} +${teile.length - 1}`;
}

function mailEintrag(mail, ordnerName = null) {
  const eintrag = document.createElement("div");
  eintrag.className = "mail-eintrag";
  if (!mail.gelesen) eintrag.classList.add("ungelesen");
  if (mail.id === zustand.aktiveMailId) eintrag.classList.add("aktiv");
  eintrag.dataset.mailId = mail.id;
  eintrag.dataset.gelesen = mail.gelesen ? "1" : "0";

  // Im Gesendet-Ordner zeigt die Liste den Empfänger statt des Absenders
  // (der bin ja immer ich).
  const istGesendet = ordnerZuId(mail.ordner_id)?.rolle === "gesendet";
  const anzeigeName = istGesendet ? empfaengerKurz(mail.an) : mail.von || mail.von_email || "(unbekannt)";
  const avatarEmail = istGesendet ? ersteAdresse(mail.an) : mail.von_email;

  const avatarWrap = document.createElement("div");
  avatarWrap.className = "mail-avatar-wrap";
  avatarWrap.appendChild(avatarElement(anzeigeName, avatarEmail));
  eintrag.appendChild(avatarWrap);

  const text = document.createElement("div");
  text.className = "mail-text-block";

  const zeile1 = document.createElement("div");
  zeile1.className = "mail-zeile-oben";
  const von = document.createElement("span");
  von.className = "mail-von";
  von.textContent = istGesendet ? `An: ${anzeigeName}` : anzeigeName;
  const zeit = document.createElement("span");
  zeit.className = "mail-zeit";
  zeit.textContent = zeitKompakt(mail.datum);
  zeile1.append(von, zeit);

  const zeile2 = document.createElement("div");
  zeile2.className = "mail-zeile-unten";
  const betreff = document.createElement("span");
  betreff.className = "mail-betreff";
  betreff.textContent = mail.betreff || "(kein Betreff)";
  zeile2.appendChild(betreff);
  if (ordnerName) {
    // Suchtreffer zeigen, in welchem Ordner die Mail liegt.
    const tag = document.createElement("span");
    tag.className = "mail-ordner-tag";
    tag.textContent = ordnerName;
    zeile2.appendChild(tag);
  }
  if (mail.beantwortet) {
    const beantwortet = icon("arrow-bend-up-left");
    beantwortet.title = "Beantwortet";
    zeile2.appendChild(beantwortet);
  }
  if (mail.hat_anhang) zeile2.appendChild(icon("paperclip"));

  text.append(zeile1, zeile2);
  eintrag.appendChild(text);
  let klickTimer = null;
  eintrag.addEventListener("click", () => {
    clearTimeout(klickTimer);
    klickTimer = setTimeout(() => mailAnklicken(mail), 220);
  });
  eintrag.addEventListener("dblclick", () => {
    clearTimeout(klickTimer);
    mailFensterOeffnen(mail);
  });
  eintrag.addEventListener("contextmenu", (ereignis) => kontextmenuZeigen(ereignis, mail));
  return eintrag;
}

/// Klick auf einen Listeneintrag: Entwürfe öffnen sich im
/// Verfassen-Fenster zum Weiterbearbeiten, alles andere im Lesebereich.
function mailAnklicken(mail) {
  const ordnerListe = zustand.ordnerJeKonto.get(zustand.aktivesKontoId) || [];
  const ordner = ordnerListe.find((o) => o.id === mail.ordner_id);
  if (ordner?.rolle === "entwuerfe") {
    verfassenFensterOeffnen(
      `?entwurfId=${mail.id}&kontoId=${zustand.aktivesKontoId}`,
      "Entwurf bearbeiten",
    );
    return;
  }
  mailOeffnen(mail.id);
}

/// Öffnet eine Mail per Doppelklick in einem eigenen, schlanken Lesefenster.
function mailFensterOeffnen(mail) {
  const ordner = ordnerZuId(mail.ordner_id);
  if (ordner?.rolle === "entwuerfe") {
    mailAnklicken(mail);
    return;
  }
  fensterZaehler += 1;
  new WebviewWindow(`mail-${Date.now()}-${fensterZaehler}`, {
    url: `mail.html?mailId=${mail.id}&kontoId=${ordner?.konto_id || zustand.aktivesKontoId || ""}`,
    title: mail.betreff || "Mail",
    width: 840,
    height: 760,
    minWidth: 520,
    minHeight: 420,
    decorations: false,
  });
}

// --------------------------------------------------------------- Suche --
// Volltextsuche im aktiven Ordner (Betreff, Absender
// und — soweit lokal vorhanden — Mailtext). Tippen startet die Suche
// leicht verzögert; Leeren oder Escape kehrt zur Ordneransicht zurück.

let sucheVerzoegerung = null;

el("suche-feld").addEventListener("input", () => {
  clearTimeout(sucheVerzoegerung);
  sucheVerzoegerung = setTimeout(() => {
    const eingabe = el("suche-feld").value.trim();
    if (eingabe.length < 2) {
      sucheBeenden();
    } else {
      zustand.suchbegriff = eingabe;
      sucheAusfuehren();
    }
  }, 250);
});

el("suche-feld").addEventListener("keydown", (ereignis) => {
  if (ereignis.key === "Escape") {
    el("suche-feld").value = "";
    sucheBeenden();
  }
});

function sucheBeenden() {
  if (!zustand.suchbegriff) return;
  zustand.suchbegriff = "";
  ordnerPillenAnzeigen(); // Ordner-Zähler ersetzt die Treffer-Zahl
  listeNeuLaden();
}

async function sucheAusfuehren() {
  if (!zustand.suchbegriff || !zustand.aktiverOrdnerId) return;
  const suchbegriff = zustand.suchbegriff;
  const ordnerId = zustand.aktiverOrdnerId;
  try {
    const treffer = await invoke("mails_suchen", {
      ordnerId,
      eingabe: suchbegriff,
    });
    // Eine ältere, langsamere Server-Suche darf neuere Ergebnisse nicht
    // überschreiben, wenn inzwischen weitergetippt/umgeschaltet wurde.
    if (suchbegriff !== zustand.suchbegriff || ordnerId !== zustand.aktiverOrdnerId) return;
    const behaelter = el("mail-eintraege");
    behaelter.innerHTML = "";
    if (treffer.length === 0) {
      leerzustand(behaelter, "magnifying-glass", "Keine Treffer.");
    } else {
      for (const mail of treffer) behaelter.appendChild(mailEintrag(mail, mail.ordner_name));
    }
    el("ordner-anzahl").textContent =
      treffer.length === 1 ? "1 Treffer" : `${treffer.length} Treffer`;
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
}

// -------------------------------------------------- Ungelesen-Filter --

el("ungelesen-filter-knopf").addEventListener("click", () => {
  // Der Filter arbeitet auf der Ordneransicht — eine laufende Suche endet.
  zustand.suchbegriff = "";
  el("suche-feld").value = "";
  zustand.nurUngelesen = !zustand.nurUngelesen;
  el("ungelesen-filter-knopf").classList.toggle("aktiv", zustand.nurUngelesen);
  zustand.offset = 0;
  zustand.alleGeladen = false;
  el("mail-eintraege").innerHTML = "";
  naechsteSeiteLaden();
});

// -------------------------------------------------------- Kontextmenü --

/// Rechtsklick auf einen Listeneintrag: „Als (un)gelesen markieren“
/// und „Löschen“ (einziger Weg, Entwürfe loszuwerden — sie öffnen sich
/// nicht im Lesebereich).
function kontextmenuZeigen(ereignis, mail) {
  ereignis.preventDefault();
  const eintrag = ereignis.currentTarget;
  const gelesen = eintrag.dataset.gelesen === "1";

  const menu = el("kontextmenu");
  menu.innerHTML = "";
  const knopf = document.createElement("button");
  knopf.type = "button";
  knopf.appendChild(icon(gelesen ? "envelope-simple" : "envelope-simple-open"));
  knopf.append(gelesen ? "Als ungelesen markieren" : "Als gelesen markieren");
  knopf.addEventListener("click", async () => {
    kontextmenuSchliessen();
    try {
      await invoke("mail_gelesen_setzen", { mailId: mail.id, gelesen: !gelesen });
      await listeNeuLaden();
      kontenAnzeigen(); // Ungelesen-Zähler der Konto-Icons auffrischen
    } catch (fehler) {
      status(`✗ ${fehler}`, "fehler");
    }
  });
  menu.appendChild(knopf);

  const loeschKnopf = document.createElement("button");
  loeschKnopf.type = "button";
  loeschKnopf.appendChild(icon("trash"));
  loeschKnopf.append("Löschen");
  loeschKnopf.addEventListener("click", () => {
    kontextmenuSchliessen();
    mailAusListeLoeschen(mail);
  });
  menu.appendChild(loeschKnopf);

  // Am Zeiger öffnen, aber nie über den Fensterrand hinausragen.
  menu.classList.remove("versteckt");
  const kasten = menu.getBoundingClientRect();
  menu.style.left = `${Math.min(ereignis.clientX, window.innerWidth - kasten.width - 8)}px`;
  menu.style.top = `${Math.min(ereignis.clientY, window.innerHeight - kasten.height - 8)}px`;
}

function kontextmenuSchliessen() {
  el("kontextmenu").classList.add("versteckt");
}

document.addEventListener("click", kontextmenuSchliessen);
window.addEventListener("blur", kontextmenuSchliessen);
document.addEventListener("keydown", (ereignis) => {
  if (ereignis.key === "Escape") kontextmenuSchliessen();
});

async function listeNeuLaden() {
  // In der Suchansicht heißt „neu laden“: Suche erneut ausführen
  // (z. B. nach Löschen, Markieren oder neuen Mails).
  if (zustand.suchbegriff) return sucheAusfuehren();
  if (!zustand.aktiverOrdnerId) return;
  const anzahl = Math.max(zustand.offset, SEITENGROESSE);
  zustand.alleGeladen = false;
  try {
    const mails = await invoke("mails_liste", {
      ordnerId: zustand.aktiverOrdnerId,
      nurUngelesen: zustand.nurUngelesen,
      offset: 0,
      limit: anzahl,
    });
    zustand.offset = mails.length;
    if (mails.length < anzahl) zustand.alleGeladen = true;
    const behaelter = el("mail-eintraege");
    if (mails.length === 0) {
      leerzustand(behaelter, "tray", leerText());
    } else {
      behaelter.innerHTML = "";
      for (const mail of mails) behaelter.appendChild(mailEintrag(mail));
    }
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
}

// --------------------------------------------------------- Lesebereich --

async function mailOeffnen(mailId) {
  const anfrage = ++leseAnfrage;
  zustand.aktiveMailId = mailId;
  markiereAktivenEintrag(mailId);
  status("Lade Mail …");
  try {
    const ansicht = await invoke("mail_lesen", { mailId });
    if (anfrage !== leseAnfrage || zustand.aktiveMailId !== mailId) return;
    zustand.aktiveMailOrdnerId = ansicht.kopf.ordner_id;
    zeige("lese-platzhalter", false);
    zeige("lese-kopf", true);

    const avatar = avatarElement(ansicht.kopf.von, ansicht.kopf.von_email, "avatar-gross");
    el("lese-avatar").replaceWith(avatar);
    avatar.id = "lese-avatar";

    el("mail-titel").textContent = ansicht.kopf.betreff || "(kein Betreff)";
    const meta = el("mail-meta");
    meta.innerHTML = "";
    const von = document.createElement("span");
    von.className = "meta-von";
    von.textContent = ansicht.kopf.von || "(unbekannt)";
    const adresse = document.createElement("span");
    adresse.className = "meta-adresse";
    adresse.textContent = ansicht.kopf.von_email ? `<${ansicht.kopf.von_email}>` : "";
    meta.append(von, adresse);
    // Empfänger: An immer, Cc nur wenn vorhanden (damit man sieht, ob jemand
    // in Kopie stand).
    if (ansicht.kopf.an) meta.appendChild(empfaengerZeile("An", ansicht.kopf.an));
    if (ansicht.kopf.cc) meta.appendChild(empfaengerZeile("Cc", ansicht.kopf.cc));
    // Datum steht unter den Buttons (verhindert Kopf-Umbrüche).
    el("lese-datum").textContent = ansicht.kopf.datum
      ? datumFormat.format(new Date(ansicht.kopf.datum * 1000))
      : "";

    zeige("bilder-leiste", ansicht.hatte_externe_bilder && !ansicht.bilder_automatisch);

    // Beide HTML-Fassungen merken; Standard ist die App-Ansicht.
    zustand.lese = { html: ansicht.html, schlicht: ansicht.html_schlicht, modus: "app" };
    if (ansicht.html) {
      ansichtKnopfAktualisieren(true);
      htmlAnzeigen();
    } else {
      ansichtKnopfAktualisieren(false);
      zeige("mail-html", false);
      el("mail-text").textContent = ansicht.text;
      zeige("mail-text", true);
    }
    anhangLeisteAnzeigen(mailId, ansicht.anhaenge || []);
    if (ansicht.hatte_externe_bilder && ansicht.bilder_automatisch) {
      void bilderLaden(mailId, anfrage);
    }

    const eintrag = document.querySelector(`.mail-eintrag[data-mail-id="${mailId}"]`);
    if (eintrag) {
      eintrag.classList.remove("ungelesen");
      eintrag.dataset.gelesen = "1";
    }
    kontenAnzeigen();
    status("Bereit.");
  } catch (fehler) {
    if (anfrage === leseAnfrage) status(`✗ ${fehler}`, "fehler");
  }
}

// Stile fürs Sandbox-iframe: Standard ist die App-Ansicht (folgt dem
// gewählten Design, siehe thema.js), per Umschalter gibt es die
// Originalansicht des Absenders (immer hell). CSS-Variablen erreichen das
// iframe nicht, deshalb stehen die Farbwerte beider Designs hier noch einmal.
function leseStilApp() {
  const hell = document.documentElement.dataset.thema === "hell";
  const [hintergrund, text, link, linie, zitat] = hell
    ? ["#f9fdf6", "#0b0d0b", "#286bbd", "#878b8633", "#595959"]
    : ["#0b0d0b", "#f6fff5", "#6ca0e0", "#878b8633", "#9ca49c"];
  return (
    `body{background:${hintergrund};color:${text};font-family:system-ui,sans-serif;` +
    "font-size:15px;line-height:1.75;max-width:72ch;margin:0 auto;" +
    "padding:40px 36px;overflow-wrap:break-word}" +
    `a{color:${link}}img{max-width:100%;height:auto}` +
    `blockquote{border-left:3px solid ${linie};margin:10px 0;padding:2px 14px;color:${zitat}}` +
    `hr{border:none;border-top:1px solid ${linie}}` +
    "pre{white-space:pre-wrap}" +
    "table{border-collapse:collapse;max-width:100%}td,th{padding:2px 8px;vertical-align:top}"
  );
}
const LESE_STIL_ORIGINAL =
  "body{font-family:system-ui,sans-serif;font-size:14px;margin:16px;" +
  "line-height:1.5;overflow-wrap:break-word;background:#ffffff;color:#1a1a1a}" +
  "a{color:#1a56c4}";

/// Zeigt den gemerkten HTML-Inhalt in der gewählten Ansicht an.
function htmlAnzeigen() {
  zeige("mail-text", false);
  const rahmen = el("mail-html");
  const original = zustand.lese.modus === "original";
  rahmen.classList.toggle("original", original);
  const inhalt = original ? zustand.lese.html : zustand.lese.schlicht || zustand.lese.html;
  rahmen.srcdoc =
    `<style>${original ? LESE_STIL_ORIGINAL : leseStilApp()}</style>` + inhalt;
  zeige("mail-html", true);
}

// Beim Designwechsel die offene Mail in der App-Ansicht neu einfärben.
window.addEventListener("thema:gewechselt", () => {
  if (zustand.lese.html && zustand.lese.modus === "app") htmlAnzeigen();
});

/// Blendet den Ansicht-Umschalter ein/aus und spiegelt den Modus wider.
function ansichtKnopfAktualisieren(sichtbar) {
  const knopf = el("ansicht-knopf");
  zeige("ansicht-knopf", sichtbar);
  const original = zustand.lese.modus === "original";
  knopf.classList.toggle("aktiv", original);
  knopf.title = original ? "Zur App-Ansicht wechseln" : "Originalansicht des Absenders";
}

el("ansicht-knopf").addEventListener("click", () => {
  if (!zustand.lese.html) return;
  zustand.lese.modus = zustand.lese.modus === "app" ? "original" : "app";
  ansichtKnopfAktualisieren(true);
  htmlAnzeigen();
});

/// Setzt den Lesebereich auf den Platzhalter zurück.
function lesebereichLeeren() {
  leseAnfrage += 1;
  zustand.lese = { html: null, schlicht: null, modus: "app" };
  zustand.aktiveMailOrdnerId = null;
  zeige("lese-kopf", false);
  zeige("bilder-leiste", false);
  zeige("mail-html", false);
  zeige("mail-text", false);
  zeige("anhang-leiste", false);
  zeige("lese-platzhalter", true);
}

// ------------------------------------------------------------ Anhänge --

/// Lesbare Größenangabe für die Anhang-Knöpfe.
function groesseText(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1).replace(".", ",")} MB`;
}

/// Zeigt die Anhänge der geöffneten Mail als Knöpfe in der Leiste unten;
/// ein Klick öffnet den Speichern-Dialog.
function anhangLeisteAnzeigen(mailId, anhaenge) {
  const leiste = el("anhang-leiste");
  leiste.innerHTML = "";
  zeige("anhang-leiste", anhaenge.length > 0);
  for (const anhang of anhaenge) {
    const knopf = document.createElement("button");
    knopf.type = "button";
    knopf.className = "anhang-knopf";
    knopf.title = `„${anhang.dateiname}“ speichern`;
    knopf.appendChild(icon("paperclip"));
    const name = document.createElement("span");
    name.className = "anhang-name";
    name.textContent = anhang.dateiname;
    knopf.appendChild(name);
    const groesse = document.createElement("span");
    groesse.className = "anhang-groesse";
    groesse.textContent = groesseText(anhang.groesse);
    knopf.appendChild(groesse);
    knopf.addEventListener("click", () => anhangSpeichern(mailId, anhang, knopf));
    leiste.appendChild(knopf);
  }
}

async function anhangSpeichern(mailId, anhang, knopf) {
  try {
    const ziel = await window.__TAURI__.dialog.save({
      title: "Anhang speichern",
      defaultPath: anhang.dateiname,
    });
    if (!ziel) return; // Dialog abgebrochen
    knopf.disabled = true;
    status("Speichere Anhang …");
    await invoke("anhang_speichern", { mailId, index: anhang.index, zielPfad: ziel });
    status(`✓ Anhang gespeichert: ${anhang.dateiname}`, "ok");
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  } finally {
    knopf.disabled = false;
  }
}

// ------------------------------------------------------- Mail löschen --

/// Löscht die geöffnete Mail (Papierkorb; dort: endgültig nach Rückfrage)
/// und wählt danach die nächste Mail in der Liste aus.
async function aktiveMailLoeschen() {
  const mailId = zustand.aktiveMailId;
  if (!mailId) return;

  // Rolle über den Ordner der Mail selbst bestimmen — bei Suchtreffern
  // kann er vom gerade aktiven Ordner abweichen.
  const ordnerListe = zustand.ordnerJeKonto.get(zustand.aktivesKontoId) || [];
  const ordner = ordnerListe.find((o) => o.id === zustand.aktiveMailOrdnerId);
  const endgueltig = ordner?.rolle === "papierkorb";
  if (endgueltig) {
    const sicher = confirm(
      "Diese Mail endgültig löschen?\n\n" +
        "Sie liegt im Papierkorb und kann danach nicht wiederhergestellt werden.",
    );
    if (!sicher) return;
  }

  const knopf = el("loeschen-knopf");
  knopf.disabled = true;
  status("Lösche Mail …");
  try {
    await invoke("mail_loeschen", { mailId });
    const eintrag = document.querySelector(`.mail-eintrag[data-mail-id="${mailId}"]`);
    const naechsteId =
      eintrag?.nextElementSibling?.dataset.mailId ||
      eintrag?.previousElementSibling?.dataset.mailId ||
      null;
    eintrag?.remove();
    zustand.offset = Math.max(0, zustand.offset - 1);
    zustand.aktiveMailId = null;
    lesebereichLeeren();
    status(endgueltig ? "✓ Mail endgültig gelöscht." : "✓ Mail in den Papierkorb verschoben.", "ok");
    kontenAnzeigen();
    if (naechsteId) {
      mailOeffnen(Number(naechsteId));
    } else if (el("mail-eintraege").children.length === 0) {
      leerzustand(el("mail-eintraege"), "tray", "Dieser Ordner ist leer.");
    }
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  } finally {
    knopf.disabled = false;
  }
}

el("loeschen-knopf").addEventListener("click", aktiveMailLoeschen);

/// Löscht eine Mail direkt aus der Liste (Kontextmenü) — gleiche Regeln
/// wie beim Löschen-Knopf: Papierkorb heißt endgültig, mit Rückfrage.
async function mailAusListeLoeschen(mail) {
  const ordnerListe = zustand.ordnerJeKonto.get(zustand.aktivesKontoId) || [];
  const rolle = ordnerListe.find((o) => o.id === mail.ordner_id)?.rolle;
  const endgueltig = rolle === "papierkorb";
  if (endgueltig) {
    const sicher = confirm(
      "Diese Mail endgültig löschen?\n\n" +
        "Sie liegt im Papierkorb und kann danach nicht wiederhergestellt werden.",
    );
    if (!sicher) return;
  }

  status("Lösche Mail …");
  try {
    await invoke("mail_loeschen", { mailId: mail.id });
    if (zustand.aktiveMailId === mail.id) {
      zustand.aktiveMailId = null;
      lesebereichLeeren();
    }
    status(endgueltig ? "✓ Mail endgültig gelöscht." : "✓ Mail in den Papierkorb verschoben.", "ok");
    await listeNeuLaden();
    kontenAnzeigen();
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
}

// Entf-Taste löscht die geöffnete Mail — aber nie beim Tippen in Feldern
// oder bei geöffnetem Dialog.
document.addEventListener("keydown", (ereignis) => {
  if (ereignis.key !== "Delete") return;
  if (el("konto-dialog").open) return;
  const ziel = ereignis.target;
  if (ziel instanceof Element && ziel.closest("input, textarea, select, [contenteditable]")) {
    return;
  }
  aktiveMailLoeschen();
});

function markiereAktivenEintrag(mailId) {
  document.querySelectorAll(".mail-eintrag.aktiv").forEach((e) => e.classList.remove("aktiv"));
  const eintrag = document.querySelector(`.mail-eintrag[data-mail-id="${mailId}"]`);
  if (eintrag) eintrag.classList.add("aktiv");
}

async function bilderLaden(mailId = zustand.aktiveMailId, anfrage = leseAnfrage) {
  if (!mailId) return;
  const knopf = el("bilder-laden-knopf");
  knopf.disabled = true;
  knopf.textContent = "Lade Bilder …";
  try {
    const ansicht = await invoke("mail_bilder_laden", { mailId });
    if (anfrage !== leseAnfrage || zustand.aktiveMailId !== mailId) return;
    zustand.lese.html = ansicht.html;
    zustand.lese.schlicht = ansicht.html_schlicht;
    htmlAnzeigen();
    zeige("bilder-leiste", false);
  } catch (fehler) {
    if (anfrage === leseAnfrage) status(`✗ ${fehler}`, "fehler");
  } finally {
    if (anfrage === leseAnfrage) {
      knopf.disabled = false;
      knopf.textContent = "Bilder laden";
    }
  }
}

el("bilder-laden-knopf").addEventListener("click", () => bilderLaden());
el("bilder-immer-knopf").addEventListener("click", async () => {
  const mailId = zustand.aktiveMailId;
  const anfrage = leseAnfrage;
  if (!mailId) return;
  try {
    await invoke("mail_bild_quelle_erlauben", { mailId });
    if (anfrage !== leseAnfrage || zustand.aktiveMailId !== mailId) return;
    await bilderLaden(mailId, anfrage);
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
});

// -------------------------------------------------------------- Sync --

async function synchronisieren() {
  await Promise.allSettled(
    zustand.konten.map((konto) => invoke("sync_starten", { kontoId: konto.id })),
  );
  await kontenAnzeigen();
  erstenOrdnerOeffnen();
  if (zustand.aktiverOrdnerId) listeNeuLaden();
}

el("aktualisieren-knopf").addEventListener("click", synchronisieren);

function kontoName(kontoId) {
  return zustand.konten.find((k) => k.id === kontoId)?.name || `Konto ${kontoId}`;
}

function syncStatusAnzeigen() {
  const bereich = el("backend-status");
  bereich.innerHTML = "";
  for (const konto of zustand.konten) {
    const eintrag = syncStatus.get(konto.id);
    if (!eintrag) continue;
    const span = document.createElement("span");
    span.className = `sync-konto ${eintrag.status === "fertig" ? "ok" : eintrag.status === "fehler" ? "fehler" : ""}`;
    const zustandsText = eintrag.status === "laeuft"
      ? "wird aktualisiert …"
      : eintrag.status === "fertig"
        ? "aktuell"
        : eintrag.meldung || "Fehler";
    span.textContent = `${konto.name}: ${zustandsText}`;
    bereich.appendChild(span);
  }
}

listen("sync:status", (ereignis) => {
  const { status: s, meldung, konto_id } = ereignis.payload;
  syncStatus.set(konto_id, { status: s, meldung });
  syncStatusAnzeigen();
});

listen("mails:neu", (ereignis) => {
  if (ereignis.payload.ordner_id === zustand.aktiverOrdnerId) listeNeuLaden();
  kontenAnzeigen();
});

listen("ordner:aktualisiert", async () => {
  await kontenAnzeigen();
  erstenOrdnerOeffnen();
});

// Nach dem Senden aus einem Verfassen-Fenster: Ansicht auffrischen.
listen("mail:gesendet", () => {
  status("✓ Mail gesendet.", "ok");
  kontenAnzeigen();
  if (zustand.aktiverOrdnerId) listeNeuLaden();
});

el("mail-eintraege").addEventListener("scroll", (ereignis) => {
  kontextmenuSchliessen();
  const ziel = ereignis.target;
  if (ziel.scrollTop + ziel.clientHeight >= ziel.scrollHeight - 200) {
    naechsteSeiteLaden();
  }
});

// -------------------------------------------------- Verfassen-Fenster --

let fensterZaehler = 0;

function verfassenFensterOeffnen(query, titel) {
  fensterZaehler += 1;
  const label = `verfassen-${Date.now()}-${fensterZaehler}`;
  new WebviewWindow(label, {
    url: `verfassen.html${query}`,
    title: titel,
    width: 680,
    height: 780,
    minWidth: 480,
    minHeight: 420,
    decorations: false,
  });
}

el("verfassen-knopf").addEventListener("click", () => {
  const konto = zustand.aktivesKontoId ?? zustand.konten[0]?.id;
  verfassenFensterOeffnen(konto ? `?kontoId=${konto}` : "", "Neue Mail");
});

el("antworten-knopf").addEventListener("click", () => {
  if (zustand.aktiveMailId) {
    // Als Absender das Konto des gerade geöffneten Ordners vorwählen.
    const konto = zustand.aktivesKontoId ? `&kontoId=${zustand.aktivesKontoId}` : "";
    verfassenFensterOeffnen(`?antwortAuf=${zustand.aktiveMailId}&weiterleiten=0${konto}`, "Antworten");
  }
});

el("allen-antworten-knopf").addEventListener("click", () => {
  if (zustand.aktiveMailId) {
    const konto = zustand.aktivesKontoId ? `&kontoId=${zustand.aktivesKontoId}` : "";
    verfassenFensterOeffnen(
      `?antwortAuf=${zustand.aktiveMailId}&weiterleiten=0&allenAntworten=1${konto}`,
      "Allen antworten",
    );
  }
});

el("weiterleiten-knopf").addEventListener("click", () => {
  if (zustand.aktiveMailId) {
    const konto = zustand.aktivesKontoId ? `&kontoId=${zustand.aktivesKontoId}` : "";
    verfassenFensterOeffnen(`?antwortAuf=${zustand.aktiveMailId}&weiterleiten=1${konto}`, "Weiterleiten");
  }
});

// ------------------------------------------------------ Konto-Dialog --

function kontoDialogOeffnen(modus, kontoId) {
  zustand.kontoDialog = { modus, kontoId };
  const formular = el("konto-formular");
  const passwortFeld = formular.elements.passwort;
  const konto = zustand.konten.find((k) => k.id === kontoId);

  if (modus === "bearbeiten" && konto) {
    el("konto-dialog-titel").textContent = "Konto bearbeiten";
    formular.elements.name.value = konto.name;
    formular.elements.anzeigename.value = konto.anzeigename || "";
    formular.elements.email.value = konto.email;
    formular.elements.benutzer.value = konto.benutzer;
    formular.elements.imap_host.value = konto.imap_host;
    formular.elements.imap_port.value = konto.imap_port;
    formular.elements.smtp_host.value = konto.smtp_host || "mail.infomaniak.com";
    formular.elements.smtp_port.value = konto.smtp_port || 465;
    formular.elements.signatur.value = konto.signatur || "";
    formular.elements.farbe.value = konto.farbe || STANDARD_FARBE;
    passwortFeld.value = "";
    passwortFeld.placeholder = "leer lassen = Passwort unverändert";
    passwortFeld.required = false;
    zeige("konto-entfernen-knopf", true);
    zeige("konto-abbrechen-knopf", true);
  } else {
    el("konto-dialog-titel").textContent = "Mail-Konto einrichten";
    formular.reset();
    formular.elements.imap_host.value = "mail.infomaniak.com";
    formular.elements.imap_port.value = 993;
    formular.elements.smtp_host.value = "mail.infomaniak.com";
    formular.elements.smtp_port.value = 465;
    passwortFeld.placeholder = "";
    passwortFeld.required = true;
    zeige("konto-entfernen-knopf", false);
    zeige("konto-abbrechen-knopf", zustand.konten.length > 0);
  }
  zeige("dialog-fehler", false);
  el("konto-dialog").showModal();
}

el("konto-hinzufuegen-knopf").addEventListener("click", () => kontoDialogOeffnen("anlegen", null));
el("konto-bearbeiten-knopf").addEventListener("click", () => {
  if (zustand.aktivesKontoId) kontoDialogOeffnen("bearbeiten", zustand.aktivesKontoId);
});
el("konto-abbrechen-knopf").addEventListener("click", () => el("konto-dialog").close());
el("farbe-standard-knopf").addEventListener("click", () => {
  el("konto-formular").elements.farbe.value = STANDARD_FARBE;
});

el("konto-entfernen-knopf").addEventListener("click", async () => {
  const konto = zustand.konten.find((k) => k.id === zustand.kontoDialog.kontoId);
  if (!konto) return;
  const sicher = confirm(
    `Konto „${konto.name}“ wirklich entfernen?\n\n` +
      "Der lokale Cache und das gespeicherte Passwort werden gelöscht. " +
      "Auf dem Mail-Server ändert sich nichts.",
  );
  if (!sicher) return;
  try {
    await invoke("konto_loeschen", { kontoId: konto.id });
    el("konto-dialog").close();
    zustand.aktiverOrdnerId = null;
    zustand.aktivesKontoId = null;
    zustand.aktiveMailId = null;
    el("mail-eintraege").innerHTML = "";
    lesebereichLeeren();
    status(`Konto „${konto.name}“ entfernt.`);
    await start();
  } catch (fehler) {
    const fehlerfeld = el("dialog-fehler");
    fehlerfeld.textContent = String(fehler);
    zeige("dialog-fehler", true);
  }
});

el("konto-formular").addEventListener("submit", async (ereignis) => {
  ereignis.preventDefault();
  const daten = new FormData(ereignis.target);
  const knopf = el("konto-speichern-knopf");
  knopf.disabled = true;
  knopf.textContent = "Prüfe Verbindung …";
  zeige("dialog-fehler", false);
  const formular = {
    name: daten.get("name"),
    anzeigename: daten.get("anzeigename"),
    email: daten.get("email"),
    benutzer: daten.get("benutzer"),
    passwort: daten.get("passwort"),
    imap_host: daten.get("imap_host"),
    imap_port: Number(daten.get("imap_port")),
    smtp_host: daten.get("smtp_host"),
    smtp_port: Number(daten.get("smtp_port")),
    signatur: daten.get("signatur"),
    // Standard-Violett wird als „leer“ gespeichert (= Vorgabe der App).
    farbe: daten.get("farbe") === STANDARD_FARBE ? "" : daten.get("farbe"),
  };
  try {
    if (zustand.kontoDialog.modus === "bearbeiten") {
      await invoke("konto_bearbeiten", { kontoId: zustand.kontoDialog.kontoId, formular });
    } else {
      await invoke("konto_anlegen", { formular });
    }
    el("konto-dialog").close();
    await start();
  } catch (fehler) {
    const fehlerfeld = el("dialog-fehler");
    fehlerfeld.textContent = String(fehler);
    zeige("dialog-fehler", true);
  } finally {
    knopf.disabled = false;
    knopf.textContent = "Verbindung prüfen & speichern";
  }
});

start();

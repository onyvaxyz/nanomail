// Nanomail — Hauptfenster (M3.1).
// Reine Darstellung: alle Daten kommen fertig aufbereitet aus dem
// Rust-Backend. Verfassen läuft in einem eigenen Fenster (verfassen.html).

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { WebviewWindow } = window.__TAURI__.webviewWindow;

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
  kontoDialog: { modus: "anlegen", kontoId: null },
};

// Avatar-Cache im Frontend: email -> dataUri | null (null = Initialen).
const avatarCache = new Map();

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
  i.className = `ph-thin ph-${name}`;
  return i;
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
    const uri = await invoke("absender_avatar", { email });
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
  for (const konto of zustand.konten) {
    try {
      zustand.ordnerJeKonto.set(konto.id, await invoke("ordner_liste", { kontoId: konto.id }));
    } catch (fehler) {
      status(`✗ ${konto.name}: ${fehler}`, "fehler");
    }
  }
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
    if (konto.id === zustand.aktivesKontoId) knopf.classList.add("aktiv");
    knopf.title = `${konto.name} · ${konto.email}`;
    knopf.appendChild(avatarElement(konto.name, konto.email, "avatar-konto"));

    const ungelesenGesamt = (zustand.ordnerJeKonto.get(konto.id) || []).reduce(
      (summe, ordner) => summe + (ordner.ungelesen || 0),
      0,
    );
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

  const pillen = el("ordner-pillen");
  pillen.innerHTML = "";
  for (const ordner of ordnerListe) {
    const pille = document.createElement("li");
    pille.className = "ordner-pille";
    if (ordner.id === zustand.aktiverOrdnerId) pille.classList.add("aktiv");
    pille.appendChild(icon(ordnerIconName(ordner)));
    const name = document.createElement("span");
    name.textContent = ordner.anzeige_name;
    pille.appendChild(name);
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
  zustand.aktivesKontoId = kontoId;
  zustand.aktiverOrdnerId = ordnerId;
  zustand.offset = 0;
  zustand.alleGeladen = false;
  el("mail-eintraege").innerHTML = "";
  kontenAnzeigen(); // aktiv-Markierung (Icon-Leiste + Ordner-Reiter)
  naechsteSeiteLaden();
}

// --------------------------------------------------------- Mail-Liste --

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
  if (zustand.alleGeladen || zustand.laedtNach || !zustand.aktiverOrdnerId) return;
  zustand.laedtNach = true;
  try {
    const mails = await invoke("mails_liste", {
      ordnerId: zustand.aktiverOrdnerId,
      offset: zustand.offset,
      limit: SEITENGROESSE,
    });
    if (mails.length < SEITENGROESSE) zustand.alleGeladen = true;

    const behaelter = el("mail-eintraege");
    if (zustand.offset === 0 && mails.length === 0) {
      leerzustand(behaelter, "tray", "Dieser Ordner ist leer.");
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

function mailEintrag(mail) {
  const eintrag = document.createElement("div");
  eintrag.className = "mail-eintrag";
  if (!mail.gelesen) eintrag.classList.add("ungelesen");
  if (mail.id === zustand.aktiveMailId) eintrag.classList.add("aktiv");
  eintrag.dataset.mailId = mail.id;

  const avatarWrap = document.createElement("div");
  avatarWrap.className = "mail-avatar-wrap";
  avatarWrap.appendChild(avatarElement(mail.von, mail.von_email));
  if (!mail.gelesen) {
    const punkt = document.createElement("span");
    punkt.className = "ungelesen-punkt";
    avatarWrap.appendChild(punkt);
  }
  eintrag.appendChild(avatarWrap);

  const text = document.createElement("div");
  text.className = "mail-text-block";

  const zeile1 = document.createElement("div");
  zeile1.className = "mail-zeile-oben";
  const von = document.createElement("span");
  von.className = "mail-von";
  von.textContent = mail.von || mail.von_email || "(unbekannt)";
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
  if (mail.hat_anhang) zeile2.appendChild(icon("paperclip"));

  text.append(zeile1, zeile2);
  eintrag.appendChild(text);
  eintrag.addEventListener("click", () => mailOeffnen(mail.id));
  return eintrag;
}

async function listeNeuLaden() {
  if (!zustand.aktiverOrdnerId) return;
  const anzahl = Math.max(zustand.offset, SEITENGROESSE);
  zustand.alleGeladen = false;
  try {
    const mails = await invoke("mails_liste", {
      ordnerId: zustand.aktiverOrdnerId,
      offset: 0,
      limit: anzahl,
    });
    zustand.offset = mails.length;
    if (mails.length < anzahl) zustand.alleGeladen = true;
    const behaelter = el("mail-eintraege");
    if (mails.length === 0) {
      leerzustand(behaelter, "tray", "Dieser Ordner ist leer.");
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
  zustand.aktiveMailId = mailId;
  markiereAktivenEintrag(mailId);
  status("Lade Mail …");
  try {
    const ansicht = await invoke("mail_lesen", { mailId });
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
    const datum = document.createElement("span");
    datum.className = "meta-datum";
    datum.textContent = ansicht.kopf.datum
      ? datumFormat.format(new Date(ansicht.kopf.datum * 1000))
      : "";
    meta.append(von, adresse, datum);

    zeige("bilder-leiste", ansicht.hatte_externe_bilder);

    if (ansicht.html) {
      htmlAnzeigen(ansicht.html);
    } else {
      zeige("mail-html", false);
      el("mail-text").textContent = ansicht.text;
      zeige("mail-text", true);
    }

    const eintrag = document.querySelector(`.mail-eintrag[data-mail-id="${mailId}"]`);
    if (eintrag) {
      eintrag.classList.remove("ungelesen");
      eintrag.querySelector(".ungelesen-punkt")?.remove();
    }
    kontenAnzeigen();
    status("Bereit.");
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
}

function htmlAnzeigen(html) {
  zeige("mail-text", false);
  const rahmen = el("mail-html");
  rahmen.srcdoc =
    "<style>body{font-family:system-ui,sans-serif;font-size:14px;margin:16px;" +
    "line-height:1.5;overflow-wrap:break-word;background:#ffffff;color:#1a1a1a}" +
    "a{color:#1a56c4}</style>" + html;
  zeige("mail-html", true);
}

function markiereAktivenEintrag(mailId) {
  document.querySelectorAll(".mail-eintrag.aktiv").forEach((e) => e.classList.remove("aktiv"));
  const eintrag = document.querySelector(`.mail-eintrag[data-mail-id="${mailId}"]`);
  if (eintrag) eintrag.classList.add("aktiv");
}

el("bilder-laden-knopf").addEventListener("click", async () => {
  if (!zustand.aktiveMailId) return;
  const knopf = el("bilder-laden-knopf");
  knopf.disabled = true;
  knopf.textContent = "Lade Bilder …";
  try {
    const html = await invoke("mail_bilder_laden", { mailId: zustand.aktiveMailId });
    htmlAnzeigen(html);
    zeige("bilder-leiste", false);
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  } finally {
    knopf.disabled = false;
    knopf.textContent = "Bilder laden";
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

listen("sync:status", (ereignis) => {
  const { status: s, meldung, konto_id } = ereignis.payload;
  if (s === "laeuft") status(`${kontoName(konto_id)} wird abgeglichen …`);
  else if (s === "fertig") status("✓ Postfach ist aktuell.", "ok");
  else if (s === "fehler") status(`✗ ${kontoName(konto_id)}: ${meldung}`, "fehler");
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
    height: 620,
    minWidth: 480,
    minHeight: 420,
  });
}

el("verfassen-knopf").addEventListener("click", () => {
  const konto = zustand.aktivesKontoId ?? zustand.konten[0]?.id;
  verfassenFensterOeffnen(konto ? `?kontoId=${konto}` : "", "Neue Mail");
});

el("antworten-knopf").addEventListener("click", () => {
  if (zustand.aktiveMailId) {
    verfassenFensterOeffnen(`?antwortAuf=${zustand.aktiveMailId}&weiterleiten=0`, "Antworten");
  }
});

el("weiterleiten-knopf").addEventListener("click", () => {
  if (zustand.aktiveMailId) {
    verfassenFensterOeffnen(`?antwortAuf=${zustand.aktiveMailId}&weiterleiten=1`, "Weiterleiten");
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
    formular.elements.email.value = konto.email;
    formular.elements.benutzer.value = konto.benutzer;
    formular.elements.imap_host.value = konto.imap_host;
    formular.elements.imap_port.value = konto.imap_port;
    formular.elements.smtp_host.value = konto.smtp_host || "mail.infomaniak.com";
    formular.elements.smtp_port.value = konto.smtp_port || 465;
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
    zeige("lese-kopf", false);
    zeige("mail-html", false);
    zeige("mail-text", false);
    zeige("lese-platzhalter", true);
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
    email: daten.get("email"),
    benutzer: daten.get("benutzer"),
    passwort: daten.get("passwort"),
    imap_host: daten.get("imap_host"),
    imap_port: Number(daten.get("imap_port")),
    smtp_host: daten.get("smtp_host"),
    smtp_port: Number(daten.get("smtp_port")),
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

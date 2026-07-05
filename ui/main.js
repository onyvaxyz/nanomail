// Nanomail — Frontend-Logik (M2).
// Reine Darstellung: alle Daten kommen fertig aufbereitet aus dem
// Rust-Backend (Tauri-Commands/-Events). Kein Parsing, keine
// Sync-Entscheidungen, keine Zugangsdaten in dieser Datei.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const dateiDialog = window.__TAURI__.dialog.open;

const SEITENGROESSE = 50;

// Zentraler UI-Zustand
const zustand = {
  konto: null,
  ordner: [],
  aktiverOrdnerId: null,
  aktiveMailId: null,
  offset: 0,
  alleGeladen: false,
  laedtNach: false,
  kontoDialogModus: "anlegen", // "anlegen" | "bearbeiten"
  verfassen: { anhaenge: [], antwortAuf: null, weiterleiten: false },
};

// ---------------------------------------------------------- DOM-Kürzel --

const el = (id) => document.getElementById(id);
const datumFormat = new Intl.DateTimeFormat("de-DE", {
  day: "2-digit", month: "2-digit", year: "2-digit",
  hour: "2-digit", minute: "2-digit",
});

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

// ------------------------------------------------------------- Start --

async function start() {
  try {
    const konten = await invoke("konten_liste");
    if (konten.length === 0) {
      kontoDialogOeffnen("anlegen");
      status("Bitte zuerst ein Mail-Konto einrichten.");
      return;
    }
    zustand.konto = konten[0]; // M2: genau ein Konto
    await kontoAnzeigen();
    synchronisieren();
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
}

async function kontoAnzeigen() {
  zeige("aktualisieren-knopf", true);
  zeige("verfassen-knopf", true);
  await ordnerNeuLaden();
  if (!zustand.aktiverOrdnerId && zustand.ordner.length > 0) {
    ordnerOeffnen(zustand.ordner[0].id);
  }
}

// ------------------------------------------------------------ Ordner --

async function ordnerNeuLaden() {
  zustand.ordner = await invoke("ordner_liste", { kontoId: zustand.konto.id });
  const bereich = el("konto-bereich");
  bereich.innerHTML = "";

  const kopfzeile = document.createElement("div");
  kopfzeile.className = "konto-zeile";
  const kontoName = document.createElement("span");
  kontoName.className = "konto-name";
  kontoName.textContent = zustand.konto.name;
  const bearbeiten = document.createElement("button");
  bearbeiten.className = "zahnrad-knopf";
  bearbeiten.title = "Konto bearbeiten";
  bearbeiten.textContent = "⚙";
  bearbeiten.addEventListener("click", () => kontoDialogOeffnen("bearbeiten"));
  kopfzeile.append(kontoName, bearbeiten);
  bereich.appendChild(kopfzeile);

  const liste = document.createElement("ul");
  liste.className = "ordner-liste";
  for (const ordner of zustand.ordner) {
    const eintrag = document.createElement("li");
    eintrag.className = "ordner-eintrag";
    if (ordner.id === zustand.aktiverOrdnerId) eintrag.classList.add("aktiv");

    const name = document.createElement("span");
    name.textContent = ordner.anzeige_name;
    eintrag.appendChild(name);

    if (ordner.ungelesen > 0) {
      const zaehler = document.createElement("span");
      zaehler.className = "ungelesen-zaehler";
      zaehler.textContent = ordner.ungelesen;
      eintrag.appendChild(zaehler);
    }
    eintrag.addEventListener("click", () => ordnerOeffnen(ordner.id));
    liste.appendChild(eintrag);
  }
  bereich.appendChild(liste);
}

function ordnerOeffnen(ordnerId) {
  zustand.aktiverOrdnerId = ordnerId;
  zustand.offset = 0;
  zustand.alleGeladen = false;
  const ordner = zustand.ordner.find((o) => o.id === ordnerId);
  el("ordner-titel").textContent = ordner ? ordner.anzeige_name : "";
  el("mail-eintraege").innerHTML = "";
  ordnerNeuLaden(); // aktiv-Markierung aktualisieren
  naechsteSeiteLaden();
}

// --------------------------------------------------------- Mail-Liste --

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
    if (zustand.offset === 0) behaelter.innerHTML = "";
    zustand.offset += mails.length;

    if (zustand.offset === 0 && mails.length === 0) {
      const leer = document.createElement("div");
      leer.className = "platzhalter";
      leer.textContent = "Dieser Ordner ist leer.";
      behaelter.appendChild(leer);
    }
    for (const mail of mails) behaelter.appendChild(mailEintrag(mail));
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

  const zeile1 = document.createElement("div");
  zeile1.className = "mail-zeile-oben";
  const von = document.createElement("span");
  von.className = "mail-von";
  von.textContent = mail.von || "(unbekannt)";
  const datum = document.createElement("span");
  datum.className = "mail-datum";
  datum.textContent = mail.datum ? datumFormat.format(new Date(mail.datum * 1000)) : "";
  zeile1.append(von, datum);

  const zeile2 = document.createElement("div");
  zeile2.className = "mail-betreff";
  zeile2.textContent = mail.betreff || "(kein Betreff)";
  if (mail.hat_anhang) zeile2.textContent = "📎 " + zeile2.textContent;

  eintrag.append(zeile1, zeile2);
  eintrag.addEventListener("click", () => mailOeffnen(mail.id));
  return eintrag;
}

async function listeNeuLaden() {
  const anzahl = Math.max(zustand.offset, SEITENGROESSE);
  zustand.offset = 0;
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
    behaelter.innerHTML = "";
    for (const mail of mails) behaelter.appendChild(mailEintrag(mail));
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
    el("mail-titel").textContent = ansicht.kopf.betreff || "(kein Betreff)";

    const meta = el("mail-meta-text");
    meta.innerHTML = "";
    const von = document.createElement("span");
    von.textContent = `Von: ${ansicht.kopf.von || "(unbekannt)"}`;
    const datum = document.createElement("span");
    datum.textContent = ansicht.kopf.datum
      ? " — " + datumFormat.format(new Date(ansicht.kopf.datum * 1000))
      : "";
    meta.append(von, datum);
    zeige("mail-meta", true);

    zeige("bilder-leiste", ansicht.hatte_externe_bilder);

    if (ansicht.html) {
      htmlAnzeigen(ansicht.html);
    } else {
      zeige("mail-html", false);
      const textfeld = el("mail-text");
      textfeld.textContent = ansicht.text;
      zeige("mail-text", true);
    }

    const eintrag = document.querySelector(`.mail-eintrag[data-mail-id="${mailId}"]`);
    if (eintrag) eintrag.classList.remove("ungelesen");
    ordnerNeuLaden();
    status("Bereit.");
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
}

function htmlAnzeigen(html) {
  zeige("mail-text", false);
  const rahmen = el("mail-html");
  rahmen.srcdoc =
    "<style>body{font-family:system-ui,sans-serif;font-size:14px;margin:12px;" +
    "overflow-wrap:break-word}</style>" + html;
  zeige("mail-html", true);
}

function markiereAktivenEintrag(mailId) {
  document.querySelectorAll(".mail-eintrag.aktiv").forEach((e) => e.classList.remove("aktiv"));
  const eintrag = document.querySelector(`.mail-eintrag[data-mail-id="${mailId}"]`);
  if (eintrag) eintrag.classList.add("aktiv");
}

// ------------------------------------------------------- Bilder laden --

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
  if (!zustand.konto) return;
  try {
    await invoke("sync_starten", { kontoId: zustand.konto.id });
    await ordnerNeuLaden();
    if (!zustand.aktiverOrdnerId && zustand.ordner.length > 0) {
      ordnerOeffnen(zustand.ordner[0].id);
    } else if (zustand.aktiverOrdnerId) {
      listeNeuLaden();
    }
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
}

el("aktualisieren-knopf").addEventListener("click", synchronisieren);

listen("sync:status", (ereignis) => {
  const { status: s, meldung } = ereignis.payload;
  if (s === "laeuft") status("Postfach wird abgeglichen …");
  else if (s === "fertig") status("✓ Postfach ist aktuell.", "ok");
  else if (s === "fehler") status(`✗ ${meldung}`, "fehler");
});

listen("mails:neu", (ereignis) => {
  if (ereignis.payload.ordner_id === zustand.aktiverOrdnerId) listeNeuLaden();
  ordnerNeuLaden();
});

listen("ordner:aktualisiert", async () => {
  await ordnerNeuLaden();
  if (!zustand.aktiverOrdnerId && zustand.ordner.length > 0) {
    ordnerOeffnen(zustand.ordner[0].id);
  }
});

el("mail-eintraege").addEventListener("scroll", (ereignis) => {
  const ziel = ereignis.target;
  if (ziel.scrollTop + ziel.clientHeight >= ziel.scrollHeight - 200) {
    naechsteSeiteLaden();
  }
});

// ------------------------------------------------------ Konto-Dialog --

function kontoDialogOeffnen(modus) {
  zustand.kontoDialogModus = modus;
  const formular = el("konto-formular");
  const passwortFeld = formular.elements.passwort;

  if (modus === "bearbeiten" && zustand.konto) {
    el("konto-dialog-titel").textContent = "Konto bearbeiten";
    el("konto-speichern-knopf").textContent = "Verbindung prüfen & speichern";
    formular.elements.name.value = zustand.konto.name;
    formular.elements.email.value = zustand.konto.email;
    formular.elements.benutzer.value = zustand.konto.benutzer;
    formular.elements.imap_host.value = zustand.konto.imap_host;
    formular.elements.imap_port.value = zustand.konto.imap_port;
    formular.elements.smtp_host.value = zustand.konto.smtp_host || "mail.infomaniak.com";
    formular.elements.smtp_port.value = zustand.konto.smtp_port || 465;
    passwortFeld.value = "";
    passwortFeld.placeholder = "leer lassen = Passwort unverändert";
    passwortFeld.required = false;
    zeige("konto-abbrechen-knopf", true);
  } else {
    el("konto-dialog-titel").textContent = "Mail-Konto einrichten";
    passwortFeld.placeholder = "";
    passwortFeld.required = true;
    zeige("konto-abbrechen-knopf", false);
  }
  zeige("dialog-fehler", false);
  el("konto-dialog").showModal();
}

el("konto-abbrechen-knopf").addEventListener("click", () => el("konto-dialog").close());

el("konto-formular").addEventListener("submit", async (ereignis) => {
  ereignis.preventDefault();
  const daten = new FormData(ereignis.target);
  const knopf = el("konto-speichern-knopf");
  knopf.disabled = true;
  knopf.textContent = "Prüfe Verbindung …";
  zeige("dialog-fehler", false);
  // Feldnamen entsprechen dem Rust-Struct KontoFormular (snake_case).
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
    const konto =
      zustand.kontoDialogModus === "bearbeiten"
        ? await invoke("konto_bearbeiten", { kontoId: zustand.konto.id, formular })
        : await invoke("konto_anlegen", { formular });
    el("konto-dialog").close();
    zustand.konto = konto;
    await kontoAnzeigen();
    synchronisieren();
  } catch (fehler) {
    const fehlerfeld = el("dialog-fehler");
    fehlerfeld.textContent = String(fehler);
    zeige("dialog-fehler", true);
  } finally {
    knopf.disabled = false;
    knopf.textContent = "Verbindung prüfen & speichern";
  }
});

// -------------------------------------------------- Verfassen-Dialog --

function verfassenOeffnen(vorlage, antwortAuf, weiterleiten) {
  const formular = el("verfassen-formular");
  formular.elements.an.value = vorlage?.an || "";
  formular.elements.cc.value = "";
  formular.elements.betreff.value = vorlage?.betreff || "";
  formular.elements.text.value = vorlage?.text || "";
  zustand.verfassen = { anhaenge: [], antwortAuf: antwortAuf ?? null, weiterleiten: !!weiterleiten };
  el("verfassen-titel").textContent = weiterleiten
    ? "Weiterleiten"
    : antwortAuf
      ? "Antworten"
      : "Neue Mail";
  anhangListeZeichnen();
  zeige("verfassen-fehler", false);
  el("verfassen-dialog").showModal();
  formular.elements[antwortAuf && !weiterleiten ? "text" : "an"].focus();
  if (antwortAuf && !weiterleiten) formular.elements.text.setSelectionRange(0, 0);
}

el("verfassen-knopf").addEventListener("click", () => verfassenOeffnen(null, null, false));

async function antwortStarten(weiterleiten) {
  if (!zustand.aktiveMailId) return;
  status(weiterleiten ? "Bereite Weiterleitung vor …" : "Bereite Antwort vor …");
  try {
    const vorlage = await invoke("antwort_vorbereiten", {
      mailId: zustand.aktiveMailId,
      weiterleiten,
    });
    status("Bereit.");
    verfassenOeffnen(vorlage, zustand.aktiveMailId, weiterleiten);
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
}

el("antworten-knopf").addEventListener("click", () => antwortStarten(false));
el("weiterleiten-knopf").addEventListener("click", () => antwortStarten(true));

function anhangListeZeichnen() {
  const liste = el("anhang-liste");
  liste.innerHTML = "";
  zustand.verfassen.anhaenge.forEach((pfad, index) => {
    const eintrag = document.createElement("li");
    const name = document.createElement("span");
    name.textContent = pfad.split("/").pop();
    const entfernen = document.createElement("button");
    entfernen.type = "button";
    entfernen.className = "anhang-entfernen";
    entfernen.textContent = "✕";
    entfernen.title = "Anhang entfernen";
    entfernen.addEventListener("click", () => {
      zustand.verfassen.anhaenge.splice(index, 1);
      anhangListeZeichnen();
    });
    eintrag.append(name, entfernen);
    liste.appendChild(eintrag);
  });
}

el("anhang-knopf").addEventListener("click", async () => {
  try {
    const auswahl = await dateiDialog({ multiple: true, title: "Dateien anhängen" });
    if (!auswahl) return;
    const pfade = Array.isArray(auswahl) ? auswahl : [auswahl];
    zustand.verfassen.anhaenge.push(...pfade);
    anhangListeZeichnen();
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
});

el("verfassen-abbrechen-knopf").addEventListener("click", () => el("verfassen-dialog").close());

el("verfassen-formular").addEventListener("submit", async (ereignis) => {
  ereignis.preventDefault();
  const daten = new FormData(ereignis.target);
  const knopf = el("senden-knopf");
  knopf.disabled = true;
  knopf.textContent = "Sende …";
  zeige("verfassen-fehler", false);
  try {
    // Feldnamen entsprechen dem Rust-Struct SendeFormular (snake_case).
    const meldung = await invoke("mail_senden", {
      kontoId: zustand.konto.id,
      formular: {
        an: daten.get("an"),
        cc: daten.get("cc"),
        betreff: daten.get("betreff"),
        text: daten.get("text"),
        anhaenge: zustand.verfassen.anhaenge,
        antwort_auf: zustand.verfassen.antwortAuf,
        weiterleiten: zustand.verfassen.weiterleiten,
      },
    });
    el("verfassen-dialog").close();
    status(`✓ ${meldung}`, "ok");
    ordnerNeuLaden();
  } catch (fehler) {
    const fehlerfeld = el("verfassen-fehler");
    fehlerfeld.textContent = String(fehler);
    zeige("verfassen-fehler", true);
  } finally {
    knopf.disabled = false;
    knopf.textContent = "Senden";
  }
});

start();

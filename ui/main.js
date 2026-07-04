// Nanomail — Frontend-Logik (M1).
// Reine Darstellung: alle Daten kommen fertig aufbereitet aus dem
// Rust-Backend (Tauri-Commands/-Events). Kein Parsing, keine
// Sync-Entscheidungen, keine Zugangsdaten in dieser Datei.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

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
      el("konto-dialog").showModal();
      status("Bitte zuerst ein Mail-Konto einrichten.");
      return;
    }
    zustand.konto = konten[0]; // M1: genau ein Konto
    await kontoAnzeigen();
    synchronisieren();
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
}

async function kontoAnzeigen() {
  zeige("aktualisieren-knopf", true);
  await ordnerNeuLaden();
  // Posteingang (erster Ordner) automatisch öffnen
  if (!zustand.aktiverOrdnerId && zustand.ordner.length > 0) {
    ordnerOeffnen(zustand.ordner[0].id);
  }
}

// ------------------------------------------------------------ Ordner --

async function ordnerNeuLaden() {
  zustand.ordner = await invoke("ordner_liste", { kontoId: zustand.konto.id });
  const bereich = el("konto-bereich");
  bereich.innerHTML = "";

  const kontoName = document.createElement("div");
  kontoName.className = "konto-name";
  kontoName.textContent = zustand.konto.name;
  bereich.appendChild(kontoName);

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
    zustand.offset += mails.length;

    const behaelter = el("mail-eintraege");
    if (zustand.offset === 0 && mails.length === 0) {
      behaelter.innerHTML = "";
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
  // Liste von vorn neu aufbauen (z. B. nach Sync-Ereignis)
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

    const meta = el("mail-meta");
    meta.innerHTML = "";
    const von = document.createElement("span");
    von.textContent = `Von: ${ansicht.kopf.von || "(unbekannt)"}`;
    const datum = document.createElement("span");
    datum.textContent = ansicht.kopf.datum
      ? datumFormat.format(new Date(ansicht.kopf.datum * 1000))
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

    // Gelesen-Status in Liste und Ordner-Zählern nachziehen
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
  // Basis-Stil in den Sandbox-Rahmen einbetten; das HTML selbst ist
  // bereits vom Backend bereinigt.
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
    // Sicherheitsnetz zusätzlich zu den Sync-Ereignissen: nach Abschluss
    // Ordner und Liste auf jeden Fall aktualisieren.
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

// Nachladen beim Scrollen ans Listenende
el("mail-eintraege").addEventListener("scroll", (ereignis) => {
  const ziel = ereignis.target;
  if (ziel.scrollTop + ziel.clientHeight >= ziel.scrollHeight - 200) {
    naechsteSeiteLaden();
  }
});

// ------------------------------------------------------ Konto-Dialog --

el("konto-formular").addEventListener("submit", async (ereignis) => {
  ereignis.preventDefault();
  const formular = new FormData(ereignis.target);
  const knopf = el("konto-speichern-knopf");
  const fehlerfeld = el("dialog-fehler");
  knopf.disabled = true;
  knopf.textContent = "Prüfe Verbindung …";
  zeige("dialog-fehler", false);
  try {
    const konto = await invoke("konto_anlegen", {
      name: formular.get("name"),
      email: formular.get("email"),
      benutzer: formular.get("benutzer"),
      passwort: formular.get("passwort"),
      imapHost: formular.get("imapHost"),
      imapPort: Number(formular.get("imapPort")),
    });
    el("konto-dialog").close();
    zustand.konto = konto;
    await kontoAnzeigen();
    synchronisieren();
  } catch (fehler) {
    fehlerfeld.textContent = String(fehler);
    zeige("dialog-fehler", true);
  } finally {
    knopf.disabled = false;
    knopf.textContent = "Verbindung prüfen & speichern";
  }
});

start();

// Nanomail — Kalender-Ansicht (M4).
// Reine Darstellung: Termine kommen fertig expandiert (inkl. Wiederholungen
// und Zeitzonen) aus dem Backend. Nutzt die Helfer aus main.js
// (el, status, icon, zeige — Skripte teilen sich den globalen Bereich).

const kalZustand = {
  aktiv: false,
  /// Erster Tag des angezeigten Monats (lokale Zeit).
  monat: new Date(new Date().getFullYear(), new Date().getMonth(), 1),
  kalender: [],
  /// Kalender-ID, deren Farbe gerade im Farbwähler geändert wird.
  farbwahlKalenderId: null,
};

const kalMonatFormat = new Intl.DateTimeFormat("de-DE", { month: "long", year: "numeric" });
const kalZeitFormat = new Intl.DateTimeFormat("de-DE", { hour: "2-digit", minute: "2-digit" });
const kalDatumLang = new Intl.DateTimeFormat("de-DE", {
  weekday: "long", day: "numeric", month: "long", year: "numeric",
});
const kalDatumKurz = new Intl.DateTimeFormat("de-DE", {
  weekday: "short", day: "2-digit", month: "2-digit", year: "numeric",
});

// ------------------------------------------------------ Ansicht wechseln --

function kalenderAnsichtZeigen(aktiv) {
  if (kalZustand.aktiv === aktiv) return;
  kalZustand.aktiv = aktiv;
  zeige("mailliste", !aktiv);
  zeige("lesebereich", !aktiv);
  zeige("kalenderbereich", aktiv);
  el("kalender-knopf").classList.toggle("aktiv", aktiv);
  terminPopoverSchliessen();
  if (aktiv) {
    kalenderLaden();
    // Beim Öffnen im Hintergrund abgleichen (Anzeige kommt aus dem Cache).
    window.__TAURI__.core.invoke("kalender_sync").catch(() => {});
  }
}

el("kalender-knopf").addEventListener("click", () => kalenderAnsichtZeigen(true));
// Klick auf ein Mail-Konto-Icon wechselt zurück zur Mail-Ansicht.
el("konten-bereich").addEventListener("click", () => kalenderAnsichtZeigen(false));

// ------------------------------------------------------------ Monatslauf --

el("kal-zurueck-knopf").addEventListener("click", () => monatVerschieben(-1));
el("kal-vor-knopf").addEventListener("click", () => monatVerschieben(1));
el("kal-heute-knopf").addEventListener("click", () => {
  const jetzt = new Date();
  kalZustand.monat = new Date(jetzt.getFullYear(), jetzt.getMonth(), 1);
  monatAnzeigen();
});

function monatVerschieben(schritt) {
  kalZustand.monat = new Date(
    kalZustand.monat.getFullYear(),
    kalZustand.monat.getMonth() + schritt,
    1,
  );
  monatAnzeigen();
}

/// Die 42 Tageszellen (6 Wochen) des Monatsrasters, beginnend am Montag
/// vor bzw. am Monatsersten — jeweils lokale Mitternacht.
function rasterTage() {
  const erster = kalZustand.monat;
  const start = new Date(erster);
  start.setDate(1 - ((erster.getDay() + 6) % 7)); // Montag = Wochenstart
  const tage = [];
  for (let i = 0; i < 42; i += 1) {
    const tag = new Date(start);
    tag.setDate(start.getDate() + i);
    tage.push(tag);
  }
  return tage;
}

function tagSchluessel(datum) {
  return `${datum.getFullYear()}-${datum.getMonth()}-${datum.getDate()}`;
}

// --------------------------------------------------------------- Laden --

async function kalenderLaden() {
  try {
    kalZustand.kalender = await window.__TAURI__.core.invoke("kalender_liste");
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
    kalZustand.kalender = [];
  }
  const leer = kalZustand.kalender.length === 0;
  zeige("kal-leerzustand", leer);
  zeige("kal-raster", !leer);
  zeige("kal-wochentage", !leer);
  kalenderLeisteAnzeigen();
  if (leer) {
    el("kal-monat-titel").textContent = kalMonatFormat.format(kalZustand.monat);
  } else {
    monatAnzeigen();
  }
}

async function monatAnzeigen() {
  el("kal-monat-titel").textContent = kalMonatFormat.format(kalZustand.monat);
  wochentageAnzeigen();

  const tage = rasterTage();
  const von = Math.floor(tage[0].getTime() / 1000);
  const ende = new Date(tage[41]);
  ende.setDate(ende.getDate() + 1);
  const bis = Math.floor(ende.getTime() / 1000);

  let termine = [];
  try {
    termine = await window.__TAURI__.core.invoke("kalender_termine", { von, bis });
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }

  // Termine den Tageszellen zuordnen (mehrtägige jedem berührten Tag).
  const jeTag = new Map();
  for (const termin of termine) {
    const tag = new Date(termin.beginn * 1000);
    tag.setHours(0, 0, 0, 0);
    for (let i = 0; i < 42 && tag.getTime() / 1000 < termin.ende; i += 1) {
      const schluessel = tagSchluessel(tag);
      if (!jeTag.has(schluessel)) jeTag.set(schluessel, []);
      jeTag.get(schluessel).push(termin);
      tag.setDate(tag.getDate() + 1);
    }
  }

  const raster = el("kal-raster");
  raster.innerHTML = "";
  const heute = tagSchluessel(new Date());
  for (const tag of tage) {
    raster.appendChild(tagesZelle(tag, jeTag.get(tagSchluessel(tag)) || [], heute));
  }
}

function wochentageAnzeigen() {
  const leiste = el("kal-wochentage");
  if (leiste.childElementCount > 0) return; // ändert sich nie
  for (const name of ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"]) {
    const feld = document.createElement("span");
    feld.textContent = name;
    leiste.appendChild(feld);
  }
}

function tagesZelle(tag, termine, heuteSchluessel) {
  const zelle = document.createElement("div");
  zelle.className = "kal-zelle";
  if (tag.getMonth() !== kalZustand.monat.getMonth()) zelle.classList.add("nebenmonat");

  const nummer = document.createElement("span");
  nummer.className = "kal-tagnummer";
  if (tagSchluessel(tag) === heuteSchluessel) nummer.classList.add("heute");
  nummer.textContent = tag.getDate();
  zelle.appendChild(nummer);

  // Ganztägige zuerst, danach nach Uhrzeit.
  const sortiert = [...termine].sort(
    (a, b) => (b.ganztags ? 1 : 0) - (a.ganztags ? 1 : 0) || a.beginn - b.beginn,
  );
  for (const termin of sortiert) {
    zelle.appendChild(terminChip(termin, tag));
  }
  return zelle;
}

function terminChip(termin, tag) {
  const chip = document.createElement("button");
  chip.type = "button";
  chip.className = "kal-termin";
  if (termin.ganztags) chip.classList.add("ganztags");
  chip.style.setProperty("--termin-farbe", termin.farbe);

  // Uhrzeit nur am ersten Tag eines Termins anzeigen.
  const beginntHeute = new Date(termin.beginn * 1000).toDateString() === tag.toDateString();
  if (!termin.ganztags && beginntHeute) {
    const zeit = document.createElement("span");
    zeit.className = "kal-termin-zeit";
    zeit.textContent = kalZeitFormat.format(new Date(termin.beginn * 1000));
    chip.appendChild(zeit);
  }
  const titel = document.createElement("span");
  titel.className = "kal-termin-titel";
  titel.textContent = termin.titel;
  chip.appendChild(titel);

  chip.addEventListener("click", (ereignis) => {
    ereignis.stopPropagation();
    terminPopoverZeigen(ereignis, termin);
  });
  return chip;
}

// ------------------------------------------------------- Termin-Details --

function terminZeitText(termin) {
  const beginn = new Date(termin.beginn * 1000);
  if (termin.ganztags) {
    // Ende ist exklusiv — letzter Tag = Ende minus ein Tag.
    const letzter = new Date((termin.ende - 24 * 3600) * 1000);
    if (beginn.toDateString() === letzter.toDateString()) {
      return `${kalDatumLang.format(beginn)} · ganztägig`;
    }
    return `${kalDatumKurz.format(beginn)} – ${kalDatumKurz.format(letzter)} · ganztägig`;
  }
  const ende = new Date(termin.ende * 1000);
  if (beginn.toDateString() === ende.toDateString()) {
    return `${kalDatumLang.format(beginn)} · ${kalZeitFormat.format(beginn)}–${kalZeitFormat.format(ende)} Uhr`;
  }
  return `${kalDatumKurz.format(beginn)}, ${kalZeitFormat.format(beginn)} Uhr – ${kalDatumKurz.format(ende)}, ${kalZeitFormat.format(ende)} Uhr`;
}

function terminPopoverZeigen(ereignis, termin) {
  const popover = el("termin-popover");
  popover.innerHTML = "";

  const kopf = document.createElement("div");
  kopf.className = "popover-kopf";
  const punkt = document.createElement("span");
  punkt.className = "popover-farbpunkt";
  punkt.style.background = termin.farbe;
  const titel = document.createElement("strong");
  titel.textContent = termin.titel;
  kopf.append(punkt, titel);
  popover.appendChild(kopf);

  const zeit = document.createElement("div");
  zeit.className = "popover-zeile";
  zeit.appendChild(icon("clock"));
  zeit.append(terminZeitText(termin));
  popover.appendChild(zeit);

  if (termin.ort) {
    const ort = document.createElement("div");
    ort.className = "popover-zeile";
    ort.appendChild(icon("map-pin"));
    ort.append(termin.ort);
    popover.appendChild(ort);
  }
  if (termin.beschreibung) {
    const beschreibung = document.createElement("div");
    beschreibung.className = "popover-beschreibung";
    beschreibung.textContent = termin.beschreibung;
    popover.appendChild(beschreibung);
  }
  const kalender = document.createElement("div");
  kalender.className = "popover-zeile leise";
  kalender.appendChild(icon("calendar-blank"));
  kalender.append(termin.kalender_name);
  popover.appendChild(kalender);

  // Am Klickpunkt öffnen, ohne über den Fensterrand zu ragen.
  popover.classList.remove("versteckt");
  const kasten = popover.getBoundingClientRect();
  popover.style.left = `${Math.min(ereignis.clientX, window.innerWidth - kasten.width - 12)}px`;
  popover.style.top = `${Math.min(ereignis.clientY, window.innerHeight - kasten.height - 12)}px`;
}

function terminPopoverSchliessen() {
  el("termin-popover").classList.add("versteckt");
}

document.addEventListener("click", terminPopoverSchliessen);
window.addEventListener("blur", terminPopoverSchliessen);
document.addEventListener("keydown", (ereignis) => {
  if (ereignis.key === "Escape") terminPopoverSchliessen();
});

// ------------------------------------------------------ Kalender-Leiste --

function kalenderLeisteAnzeigen() {
  const leiste = el("kal-kalender-leiste");
  leiste.innerHTML = "";
  for (const kalender of kalZustand.kalender) {
    const chip = document.createElement("div");
    chip.className = "kal-chip";
    if (!kalender.sichtbar) chip.classList.add("aus");
    chip.style.setProperty("--kal-farbe", kalender.farbe);
    chip.title = `${kalender.anzeige_name} (${kalender.konto_name})`;

    const punkt = document.createElement("button");
    punkt.type = "button";
    punkt.className = "kal-chip-farbe";
    punkt.title = "Farbe ändern";
    punkt.addEventListener("click", (ereignis) => {
      ereignis.stopPropagation();
      farbwahlOeffnen(kalender);
    });
    chip.appendChild(punkt);

    const name = document.createElement("span");
    name.textContent = kalender.anzeige_name;
    chip.appendChild(name);

    chip.addEventListener("click", () => sichtbarkeitUmschalten(kalender));
    chip.addEventListener("contextmenu", (ereignis) => kalChipMenue(ereignis, kalender));
    leiste.appendChild(chip);
  }
}

async function sichtbarkeitUmschalten(kalender) {
  try {
    await window.__TAURI__.core.invoke("kalender_sichtbar_setzen", {
      kalenderId: kalender.id,
      sichtbar: !kalender.sichtbar,
    });
    await kalenderLaden();
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
}

function farbwahlOeffnen(kalender) {
  kalZustand.farbwahlKalenderId = kalender.id;
  const eingabe = el("kal-farbe-input");
  eingabe.value = kalender.farbe;
  eingabe.click();
}

el("kal-farbe-input").addEventListener("change", async (ereignis) => {
  if (!kalZustand.farbwahlKalenderId) return;
  try {
    await window.__TAURI__.core.invoke("kalender_farbe_setzen", {
      kalenderId: kalZustand.farbwahlKalenderId,
      farbe: ereignis.target.value,
    });
    await kalenderLaden();
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  } finally {
    kalZustand.farbwahlKalenderId = null;
  }
});

/// Rechtsklick auf einen Kalender-Chip: Standardfarbe & Konto entfernen.
function kalChipMenue(ereignis, kalender) {
  ereignis.preventDefault();
  ereignis.stopPropagation();
  const menu = el("kontextmenu");
  menu.innerHTML = "";

  if (kalender.farbe_ist_eigen) {
    const zuruecksetzen = document.createElement("button");
    zuruecksetzen.type = "button";
    zuruecksetzen.appendChild(icon("arrow-counter-clockwise"));
    zuruecksetzen.append("Farbe aus Nextcloud verwenden");
    zuruecksetzen.addEventListener("click", async () => {
      menu.classList.add("versteckt");
      try {
        await window.__TAURI__.core.invoke("kalender_farbe_setzen", {
          kalenderId: kalender.id,
          farbe: "",
        });
        await kalenderLaden();
      } catch (fehler) {
        status(`✗ ${fehler}`, "fehler");
      }
    });
    menu.appendChild(zuruecksetzen);
  }

  const entfernen = document.createElement("button");
  entfernen.type = "button";
  entfernen.appendChild(icon("trash"));
  entfernen.append(`Konto „${kalender.konto_name}“ entfernen`);
  entfernen.addEventListener("click", async () => {
    menu.classList.add("versteckt");
    await kalenderKontoEntfernen(kalender.konto_id, kalender.konto_name);
  });
  menu.appendChild(entfernen);

  menu.classList.remove("versteckt");
  const kasten = menu.getBoundingClientRect();
  menu.style.left = `${Math.min(ereignis.clientX, window.innerWidth - kasten.width - 8)}px`;
  menu.style.top = `${Math.min(ereignis.clientY, window.innerHeight - kasten.height - 8)}px`;
}

async function kalenderKontoEntfernen(kontoId, kontoName) {
  const sicher = confirm(
    `Kalender-Konto „${kontoName}“ wirklich entfernen?\n\n` +
      "Die lokal gespeicherten Termine und das App-Passwort werden gelöscht. " +
      "In Nextcloud ändert sich nichts.",
  );
  if (!sicher) return;
  try {
    await window.__TAURI__.core.invoke("kalender_konto_loeschen", { kontoId });
    status(`Kalender-Konto „${kontoName}“ entfernt.`);
    await kalenderLaden();
    kalKontenListeAnzeigen();
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
}

// -------------------------------------------------- Konto-Dialog --

function kalenderDialogOeffnen() {
  el("kalender-formular").reset();
  zeige("kal-dialog-fehler", false);
  kalKontenListeAnzeigen();
  el("kalender-dialog").showModal();
}

el("kal-konto-hinzufuegen-knopf").addEventListener("click", kalenderDialogOeffnen);
el("kal-leer-hinzufuegen-knopf").addEventListener("click", kalenderDialogOeffnen);
el("kal-abbrechen-knopf").addEventListener("click", () => el("kalender-dialog").close());

/// Bereits eingerichtete Konten im Dialog (mit Entfernen-Knopf).
async function kalKontenListeAnzeigen() {
  const behaelter = el("kal-konten-liste");
  behaelter.innerHTML = "";
  let konten = [];
  try {
    konten = await window.__TAURI__.core.invoke("kalender_konten_liste");
  } catch {
    return;
  }
  for (const konto of konten) {
    const zeile = document.createElement("div");
    zeile.className = "kal-konto-zeile";
    const name = document.createElement("span");
    name.textContent = `${konto.name} · ${konto.benutzer}`;
    zeile.appendChild(name);
    const entfernen = document.createElement("button");
    entfernen.type = "button";
    entfernen.className = "icon-knopf klein";
    entfernen.title = "Konto entfernen";
    entfernen.appendChild(icon("trash"));
    entfernen.addEventListener("click", () => kalenderKontoEntfernen(konto.id, konto.name));
    zeile.appendChild(entfernen);
    behaelter.appendChild(zeile);
  }
}

el("kalender-formular").addEventListener("submit", async (ereignis) => {
  ereignis.preventDefault();
  const daten = new FormData(ereignis.target);
  const knopf = el("kal-speichern-knopf");
  knopf.disabled = true;
  knopf.textContent = "Verbinde …";
  zeige("kal-dialog-fehler", false);
  try {
    await window.__TAURI__.core.invoke("kalender_konto_anlegen", {
      formular: {
        name: daten.get("name"),
        server: daten.get("server"),
        benutzer: daten.get("benutzer"),
        passwort: daten.get("passwort"),
      },
    });
    el("kalender-dialog").close();
    status("✓ Kalender-Konto verbunden — Termine werden geladen …", "ok");
    await kalenderLaden();
  } catch (fehler) {
    const fehlerfeld = el("kal-dialog-fehler");
    fehlerfeld.textContent = String(fehler);
    zeige("kal-dialog-fehler", true);
  } finally {
    knopf.disabled = false;
    knopf.textContent = "Verbinden & Kalender suchen";
  }
});

// ------------------------------------------------------------ Abgleich --

el("kal-sync-knopf").addEventListener("click", async () => {
  const knopf = el("kal-sync-knopf");
  knopf.disabled = true;
  status("Kalender werden abgeglichen …");
  try {
    await window.__TAURI__.core.invoke("kalender_sync");
    status("✓ Kalender sind aktuell.", "ok");
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  } finally {
    knopf.disabled = false;
  }
});

// Backend meldet abgeschlossene Abgleiche — Ansicht auffrischen.
window.__TAURI__.event.listen("kalender:aktualisiert", () => {
  if (kalZustand.aktiv) kalenderLaden();
});

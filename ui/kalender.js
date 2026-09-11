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
  /// Termin, der gerade im Dialog bearbeitet wird (null = neu).
  bearbeiteterTermin: null,
  mailKonten: [],
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
el("kal-termin-neu-knopf").addEventListener("click", () => terminDialogOeffnen());

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
  zelle.title = "Doppelklick: neuen Termin an diesem Tag erstellen";
  zelle.addEventListener("dblclick", () => terminDialogOeffnen(null, tag));

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

let popoverAusloeser = null;
function terminPopoverZeigen(ereignis, termin) {
  const popover = el("termin-popover");
  popoverAusloeser = ereignis.currentTarget;
  popover.setAttribute("role", "dialog");
  popover.setAttribute("aria-label", "Termindetails");
  popover.tabIndex = -1;
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
    textMitLinks(ort, termin.ort);
    popover.appendChild(ort);
  }
  if (termin.beschreibung) {
    const beschreibung = document.createElement("div");
    beschreibung.className = "popover-beschreibung";
    textMitLinks(beschreibung, termin.beschreibung);
    popover.appendChild(beschreibung);
  }
  if (termin.url) {
    const link = document.createElement("div");
    link.className = "popover-zeile";
    textMitLinks(link, termin.url);
    popover.appendChild(link);
  }
  if (termin.teilnehmer && termin.teilnehmer.length > 0) {
    const teilnehmer = document.createElement("div");
    teilnehmer.className = "popover-zeile";
    teilnehmer.appendChild(icon("users"));
    teilnehmer.append(termin.teilnehmer.map(teilnehmerText).join(", "));
    popover.appendChild(teilnehmer);
  }
  const kalender = document.createElement("div");
  kalender.className = "popover-zeile leise";
  kalender.appendChild(icon("calendar-blank"));
  kalender.append(termin.kalender_name);
  popover.appendChild(kalender);

  const aktionen = document.createElement("div");
  aktionen.className = "popover-aktionen";
  const schliessen = document.createElement("button");
  schliessen.type = "button";
  schliessen.className = "knopf-sekundaer";
  schliessen.textContent = "Schließen";
  schliessen.addEventListener("click", () => terminPopoverSchliessen(true));
  const bearbeiten = document.createElement("button");
  bearbeiten.type = "button";
  bearbeiten.className = "knopf-sekundaer";
  bearbeiten.appendChild(icon("pencil-simple-line"));
  bearbeiten.append("Bearbeiten");
  bearbeiten.addEventListener("click", (event) => {
    event.stopPropagation();
    terminPopoverSchliessen();
    terminDialogOeffnen(termin);
  });
  const loeschen = document.createElement("button");
  loeschen.type = "button";
  loeschen.className = "knopf-gefahr";
  loeschen.appendChild(icon("trash"));
  loeschen.append("Löschen");
  loeschen.addEventListener("click", async (event) => {
    event.stopPropagation();
    terminPopoverSchliessen();
    await terminLoeschen(termin);
  });
  aktionen.append(bearbeiten, loeschen, schliessen);
  popover.appendChild(aktionen);

  // Am Klickpunkt öffnen, ohne über den Fensterrand zu ragen.
  popover.classList.remove("versteckt");
  const kasten = popover.getBoundingClientRect();
  const anker = popoverAusloeser.getBoundingClientRect();
  popover.style.left = `${Math.max(12, Math.min(ereignis.clientX || anker.left, window.innerWidth - kasten.width - 12))}px`;
  popover.style.top = `${Math.max(12, Math.min(ereignis.clientY || anker.bottom, window.innerHeight - kasten.height - 12))}px`;
  popover.focus();
}

// ------------------------------------------------------- Termin-Dialog --

function sichtbareKalender() {
  const sichtbare = kalZustand.kalender.filter((kalender) => kalender.sichtbar);
  return sichtbare.length > 0 ? sichtbare : kalZustand.kalender;
}

async function terminDialogOeffnen(termin = null, tag = null) {
  if (kalZustand.kalender.length === 0) {
    status("Bitte zuerst ein Kalender-Konto hinzufügen.", "fehler");
    return;
  }
  kalZustand.bearbeiteterTermin = termin;
  const formular = el("termin-formular");
  formular.reset();
  zeige("termin-dialog-fehler", false);
  zeige("termin-loeschen-dialog-knopf", Boolean(termin));
  el("termin-dialog-titel").textContent = termin ? "Termin bearbeiten" : "Termin erstellen";

  const auswahl = formular.elements.kalender_id;
  auswahl.innerHTML = "";
  for (const kalender of kalZustand.kalender) {
    const option = document.createElement("option");
    option.value = kalender.id;
    option.textContent = `${kalender.anzeige_name} · ${kalender.konto_name}`;
    auswahl.appendChild(option);
  }
  // Verschieben zwischen Kalendern wird noch nicht unterstützt — beim
  // Bearbeiten bleibt die Kalender-Auswahl deshalb gesperrt.
  auswahl.disabled = Boolean(termin);
  await mailKontenFuerEinladungLaden();

  if (termin) {
    formular.elements.href.value = termin.href || "";
    formular.elements.etag.value = termin.etag || "";
    formular.elements.kalender_id.value = termin.kalender_id;
    formular.elements.titel.value = termin.titel || "";
    formular.elements.ort.value = termin.ort || "";
    formular.elements.teilnehmer.value = teilnehmerAdressen(termin).join(", ");
    formular.elements.einladung_senden.checked =
      teilnehmerAdressen(termin).length > 0 && kalZustand.mailKonten.length > 0;
    el("termin-einladung-text").textContent = "Änderungs-Mail mit Nanomail senden";
    formular.elements.beschreibung.value = termin.beschreibung || "";
    formular.elements.ganztags.checked = Boolean(termin.ganztags);
    zeitfelderSetzen(new Date(termin.beginn * 1000), new Date(termin.ende * 1000), termin.ganztags);
  } else {
    // reset() leert die versteckten Felder nicht zuverlässig — sonst bleibt
    // nach dem Öffnen eines vorhandenen Termins ein href hängen und der neue
    // Termin gilt beim Speichern als „lokal nicht mehr vorhanden".
    formular.elements.href.value = "";
    formular.elements.etag.value = "";
    const kalender = sichtbareKalender()[0];
    formular.elements.kalender_id.value = kalender.id;
    const beginn = tag ? new Date(tag) : new Date();
    beginn.setMinutes(0, 0, 0);
    if (!tag) beginn.setHours(beginn.getHours() + 1);
    const ende = new Date(beginn);
    ende.setHours(ende.getHours() + 1);
    zeitfelderSetzen(beginn, ende, false);
    formular.elements.einladung_senden.checked = false;
    el("termin-einladung-text").textContent = "Einladungs-Mail mit Nanomail senden";
  }
  einladungsFelderAktualisieren();
  ganztagsFelderAktualisieren();
  el("termin-dialog").showModal();
}

async function mailKontenFuerEinladungLaden() {
  const formular = el("termin-formular");
  const auswahl = formular.elements.einladung_konto_id;
  auswahl.innerHTML = "";
  try {
    kalZustand.mailKonten = await window.__TAURI__.core.invoke("konten_liste");
  } catch {
    kalZustand.mailKonten = [];
  }
  for (const konto of kalZustand.mailKonten) {
    const option = document.createElement("option");
    option.value = konto.id;
    option.textContent = `${konto.name} <${konto.email}>`;
    auswahl.appendChild(option);
  }
  formular.elements.einladung_senden.disabled = kalZustand.mailKonten.length === 0;
  zeige("termin-einladung-konto-label", false);
}

function teilnehmerAdressen(termin) {
  return (termin.teilnehmer || []).map((eintrag) => {
    if (typeof eintrag === "string") return eintrag;
    return eintrag.email || "";
  }).filter(Boolean);
}

function teilnehmerText(eintrag) {
  if (typeof eintrag === "string") return `${eintrag} (Nicht bestätigt)`;
  return `${eintrag.email} (${teilnehmerStatusText(eintrag.status)})`;
}

function teilnehmerStatusText(status) {
  switch (status) {
    case "accepted": return "Bestätigt";
    case "declined": return "Abgelehnt";
    case "tentative": return "Vorläufig";
    case "needs_action": return "Nicht bestätigt";
    default: return "Unbekannt";
  }
}

function zwei(zahl) {
  return String(zahl).padStart(2, "0");
}

function datumInput(datum) {
  return `${datum.getFullYear()}-${zwei(datum.getMonth() + 1)}-${zwei(datum.getDate())}`;
}

function zeitInput(datum) {
  return `${zwei(datum.getHours())}:${zwei(datum.getMinutes())}`;
}

function zeitfelderSetzen(beginn, ende, ganztags) {
  const formular = el("termin-formular");
  formular.elements.beginn_datum.value = datumInput(beginn);
  formular.elements.ende_datum.value = datumInput(ganztags ? endeMinusEinTag(ende) : ende);
  formular.elements.beginn_zeit.value = zeitInput(beginn);
  formular.elements.ende_zeit.value = zeitInput(ende);
}

function endeMinusEinTag(ende) {
  const datum = new Date(ende);
  datum.setDate(datum.getDate() - 1);
  return datum;
}

function ganztagsFelderAktualisieren() {
  const formular = el("termin-formular");
  const ganztags = formular.elements.ganztags.checked;
  formular.elements.beginn_zeit.disabled = ganztags;
  formular.elements.ende_zeit.disabled = ganztags;
}

function einladungsFelderAktualisieren() {
  const formular = el("termin-formular");
  const sichtbar = formular.elements.einladung_senden.checked && kalZustand.mailKonten.length > 0;
  zeige("termin-einladung-konto-label", sichtbar);
  formular.elements.einladung_konto_id.disabled = !sichtbar;
}

el("termin-formular").elements.ganztags.addEventListener("change", ganztagsFelderAktualisieren);
el("termin-formular").elements.einladung_senden.addEventListener(
  "change",
  einladungsFelderAktualisieren,
);
el("termin-abbrechen-knopf").addEventListener("click", () => el("termin-dialog").close());
el("termin-loeschen-dialog-knopf").addEventListener("click", async () => {
  if (kalZustand.bearbeiteterTermin) await terminLoeschen(kalZustand.bearbeiteterTermin);
});

el("termin-formular").addEventListener("submit", async (ereignis) => {
  ereignis.preventDefault();
  const formular = ereignis.target;
  const fehlerfeld = el("termin-dialog-fehler");
  const knopf = el("termin-speichern-knopf");
  knopf.disabled = true;
  zeige("termin-dialog-fehler", false);
  try {
    const meldung = await window.__TAURI__.core.invoke("kalender_termin_speichern", {
      formular: terminFormularDaten(formular),
    });
    el("termin-dialog").close();
    status(`✓ ${meldung}`, String(meldung).includes("aber") ? "fehler" : "ok");
    await kalenderLaden();
  } catch (fehler) {
    fehlerfeld.textContent = String(fehler);
    zeige("termin-dialog-fehler", true);
  } finally {
    knopf.disabled = false;
  }
});

function terminFormularDaten(formular) {
  const daten = new FormData(formular);
  const ganztags = daten.get("ganztags") === "on";
  const einladungSenden = daten.get("einladung_senden") === "on";
  const beginn = datumZeitAusFormular(
    String(daten.get("beginn_datum") || ""),
    ganztags ? "00:00" : String(daten.get("beginn_zeit") || ""),
  );
  let ende = datumZeitAusFormular(
    String(daten.get("ende_datum") || ""),
    ganztags ? "00:00" : String(daten.get("ende_zeit") || ""),
  );
  if (ganztags) ende.setDate(ende.getDate() + 1); // CalDAV-Ende ist exklusiv.
  return {
    // Direkt vom Element lesen: gesperrte Felder fehlen in FormData.
    kalender_id: Number(formular.elements.kalender_id.value),
    href: String(daten.get("href") || "") || null,
    etag: String(daten.get("etag") || "") || null,
    titel: String(daten.get("titel") || ""),
    ort: String(daten.get("ort") || ""),
    beschreibung: String(daten.get("beschreibung") || ""),
    teilnehmer: String(daten.get("teilnehmer") || ""),
    beginn: Math.floor(beginn.getTime() / 1000),
    ende: Math.floor(ende.getTime() / 1000),
    ganztags,
    einladung_senden: einladungSenden,
    einladung_konto_id: einladungSenden
      ? Number(daten.get("einladung_konto_id"))
      : null,
  };
}

function datumZeitAusFormular(datum, zeit) {
  const [jahr, monat, tag] = datum.split("-").map(Number);
  const [stunde, minute] = zeit.split(":").map(Number);
  return new Date(jahr, monat - 1, tag, stunde, minute, 0, 0);
}

async function terminLoeschen(termin) {
  if (!termin) return;
  const frage = termin.serie
    ? `„${termin.titel}“ gehört zu einer Wiederholungsserie. Es wird die GESAMTE Serie ` +
      `mit allen Terminen gelöscht. Wirklich fortfahren?`
    : `Termin „${termin.titel}“ wirklich löschen?`;
  if (!confirm(frage)) return;
  try {
    await window.__TAURI__.core.invoke("kalender_termin_loeschen", {
      kalenderId: termin.kalender_id,
      href: termin.href,
      etag: termin.etag,
    });
    el("termin-dialog").close();
    status("✓ Termin gelöscht.", "ok");
    await kalenderLaden();
  } catch (fehler) {
    status(`✗ ${fehler}`, "fehler");
  }
}

// ------------------------------------------ Teilnehmer-Adressvorschläge --

const kalAdressVorschlaege = { feld: null, eintraege: [], index: -1 };
let kalAdressAnfrage = 0;

function kalAdressvorschlaegeVerbergen() {
  el("kal-adress-vorschlaege").classList.add("versteckt");
  kalAdressVorschlaege.eintraege = [];
  kalAdressVorschlaege.index = -1;
}

function kalLetzterAdressteil(wert) {
  const teile = wert.split(/[,;]/);
  return teile[teile.length - 1].trim();
}

function kalAdressvorschlagUebernehmen(email) {
  const feld = kalAdressVorschlaege.feld;
  const teile = feld.value.split(/[,;]/);
  teile[teile.length - 1] = " " + email;
  feld.value = teile.join(",").trimStart();
  kalAdressvorschlaegeVerbergen();
  feld.focus();
}

function kalAdressvorschlaegeZeichnen() {
  const liste = el("kal-adress-vorschlaege");
  liste.innerHTML = "";
  kalAdressVorschlaege.eintraege.forEach((eintrag, index) => {
    const zeile = document.createElement("button");
    zeile.type = "button";
    zeile.className = "vorschlag" + (index === kalAdressVorschlaege.index ? " aktiv" : "");
    if (eintrag.name) {
      const name = document.createElement("span");
      name.className = "vorschlag-name";
      name.textContent = eintrag.name;
      zeile.appendChild(name);
    }
    const adresse = document.createElement("span");
    adresse.className = "vorschlag-adresse";
    adresse.textContent = eintrag.email;
    zeile.appendChild(adresse);
    zeile.addEventListener("mousedown", (ereignis) => {
      ereignis.preventDefault();
      kalAdressvorschlagUebernehmen(eintrag.email);
    });
    liste.appendChild(zeile);
  });
  const kasten = kalAdressVorschlaege.feld.getBoundingClientRect();
  const dialogKasten = el("termin-dialog").getBoundingClientRect();
  liste.style.left = `${kasten.left - dialogKasten.left}px`;
  liste.style.top = `${kasten.bottom - dialogKasten.top + 4}px`;
  liste.style.width = `${kasten.width}px`;
  liste.classList.toggle("versteckt", kalAdressVorschlaege.eintraege.length === 0);
}

async function kalAdressvorschlaegeAktualisieren(feld) {
  const eingabe = kalLetzterAdressteil(feld.value);
  if (eingabe.length < 2) {
    kalAdressvorschlaegeVerbergen();
    return;
  }
  const anfrage = ++kalAdressAnfrage;
  try {
    const treffer = await window.__TAURI__.core.invoke("adress_vorschlaege", { eingabe });
    if (anfrage !== kalAdressAnfrage || document.activeElement !== feld) return;
    kalAdressVorschlaege.feld = feld;
    kalAdressVorschlaege.eintraege = treffer;
    kalAdressVorschlaege.index = -1;
    kalAdressvorschlaegeZeichnen();
  } catch {
    kalAdressvorschlaegeVerbergen();
  }
}

function kalAdressvorschlaegeAnbinden(feld) {
  feld.addEventListener("input", () => kalAdressvorschlaegeAktualisieren(feld));
  feld.addEventListener("blur", kalAdressvorschlaegeVerbergen);
  feld.addEventListener("keydown", (ereignis) => {
    if (kalAdressVorschlaege.eintraege.length === 0) return;
    if (ereignis.key === "ArrowDown" || ereignis.key === "ArrowUp") {
      ereignis.preventDefault();
      const schritt = ereignis.key === "ArrowDown" ? 1 : -1;
      const anzahl = kalAdressVorschlaege.eintraege.length;
      kalAdressVorschlaege.index = (kalAdressVorschlaege.index + schritt + anzahl) % anzahl;
      kalAdressvorschlaegeZeichnen();
    } else if (ereignis.key === "Enter" && kalAdressVorschlaege.index >= 0) {
      ereignis.preventDefault();
      kalAdressvorschlagUebernehmen(kalAdressVorschlaege.eintraege[kalAdressVorschlaege.index].email);
    } else if (ereignis.key === "Escape") {
      kalAdressvorschlaegeVerbergen();
    }
  });
}

kalAdressvorschlaegeAnbinden(el("termin-formular").elements.teilnehmer);

function terminPopoverSchliessen(fokusZurueck = false) {
  el("termin-popover").classList.add("versteckt");
  if (fokusZurueck) popoverAusloeser?.focus();
}

// Nur der Beginn einer Interaktion außerhalb schließt. Eine innen begonnene
// Textselektion darf auch außerhalb enden; Fensterwechsel zum Kopieren bleibt offen.
document.addEventListener("pointerdown", (ereignis) => {
  if (!el("termin-popover").contains(ereignis.target)) terminPopoverSchliessen();
});
document.addEventListener("focusin", (ereignis) => {
  if (!el("termin-popover").contains(ereignis.target) && ereignis.target !== popoverAusloeser) terminPopoverSchliessen();
});
document.addEventListener("keydown", (ereignis) => {
  if (ereignis.key === "Escape" && !el("termin-popover").classList.contains("versteckt")) terminPopoverSchliessen(true);
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

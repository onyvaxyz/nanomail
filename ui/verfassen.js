// Nanomail — Verfassen-Fenster (M3.1, Editor + Signaturen ab M3.3).
// Eigenständiges Fenster: baut sich aus Query-Parametern auf, nutzt die
// bestehenden Backend-Commands und schließt sich nach dem Senden selbst.

const { invoke } = window.__TAURI__.core;
const { emit } = window.__TAURI__.event;
const dateiDialog = window.__TAURI__.dialog.open;
const aktuellesFenster = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();

const el = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);

const STANDARD_FARBE = "#c678dd";

const zustand = {
  konten: [],
  anhaenge: [],
  antwortAuf: params.get("antwortAuf") ? Number(params.get("antwortAuf")) : null,
  weiterleiten: params.get("weiterleiten") === "1",
  vorgabeKontoId: params.get("kontoId") ? Number(params.get("kontoId")) : null,
  /// Mail-ID des Entwurfs, der hier weiterbearbeitet wird (null = neu).
  entwurfVon: params.get("entwurfId") ? Number(params.get("entwurfId")) : null,
};

function icon(name) {
  const i = document.createElement("i");
  i.className = `ph-light ph-${name}`;
  return i;
}

function fehler(text) {
  const feld = el("verfassen-fehler");
  feld.textContent = text;
  feld.classList.remove("versteckt");
}

// ------------------------------------------------------ Fensterleiste --

el("fenster-minimieren").addEventListener("click", () => aktuellesFenster.minimize());
el("fenster-maximieren").addEventListener("click", () => aktuellesFenster.toggleMaximize());
el("fenster-schliessen").addEventListener("click", () => aktuellesFenster.close());
// Doppelklick-Maximieren übernimmt die Tauri-Drag-Region der Titelleiste
// selbst — hier keinen eigenen dblclick-Handler ergänzen, sonst wird
// doppelt umgeschaltet und das Fenster springt sofort zurück.
// Größenändern per Rand-Ziehen: siehe fenster.js.

const fensterTitel = zustand.entwurfVon
  ? "Entwurf bearbeiten"
  : zustand.antwortAuf
    ? (zustand.weiterleiten ? "Weiterleiten" : "Antworten")
    : "Neue Mail";
el("fenster-titel").textContent = fensterTitel;
document.title = `${fensterTitel} — Nanomail`;

// ------------------------------------------------------------ Editor --

/// Fügt mehrzeiligen Text als Text-Knoten + <br> ein (nie als HTML —
/// so kann aus Vorlagen-Text niemals Markup entstehen).
function textAlsZeilen(ziel, text) {
  const zeilen = (text || "").split("\n");
  zeilen.forEach((zeile, index) => {
    ziel.appendChild(document.createTextNode(zeile));
    if (index < zeilen.length - 1) ziel.appendChild(document.createElement("br"));
  });
}

function aktuellesKonto() {
  return zustand.konten.find((k) => k.id === Number(el("von-auswahl").value));
}

/// Wendet Farbe und Signatur des gewählten Von-Kontos an.
function kontoAnwenden() {
  const konto = aktuellesKonto();
  document.documentElement.style.setProperty(
    "--akzent",
    konto && konto.farbe ? konto.farbe : STANDARD_FARBE,
  );
  signaturSetzen(konto);
}

/// Signatur als eigener Block im Editor: bei Antworten über dem Zitat,
/// sonst am Ende. Wechselt das Von-Konto, wird der Block ersetzt.
function signaturSetzen(konto) {
  // Ein geladener Entwurf bringt seinen Text (samt ggf. Signatur) schon
  // mit — nichts doppelt einfügen.
  if (zustand.entwurfVon) return;
  // Signatur nur bei einer neuen Erstnachricht, nicht beim Antworten oder
  // Weiterleiten.
  if (zustand.antwortAuf) {
    document.getElementById("signatur-block")?.remove();
    return;
  }
  const editor = el("verfassen-editor");
  let block = document.getElementById("signatur-block");
  if (!konto || !konto.signatur || !konto.signatur.trim()) {
    block?.remove();
    return;
  }
  if (!block) {
    block = document.createElement("div");
    block.id = "signatur-block";
    editor.insertBefore(block, document.getElementById("zitat-block"));
  }
  block.innerHTML = "";
  block.appendChild(document.createElement("br"));
  block.appendChild(document.createTextNode("-- "));
  block.appendChild(document.createElement("br"));
  textAlsZeilen(block, konto.signatur.trim());
}

// Formatleiste: mousedown abfangen, damit die Auswahl im Editor bleibt.
document.querySelectorAll(".editor-leiste [data-befehl]").forEach((knopf) => {
  knopf.addEventListener("mousedown", (ereignis) => ereignis.preventDefault());
  knopf.addEventListener("click", () => {
    document.execCommand(knopf.dataset.befehl, false, null);
    el("verfassen-editor").focus();
  });
});

el("link-knopf").addEventListener("mousedown", (ereignis) => ereignis.preventDefault());
el("link-knopf").addEventListener("click", () => {
  const eingabe = prompt("Link-Adresse:", "https://");
  if (!eingabe) return;
  const url = eingabe.trim();
  if (!/^https?:\/\/./.test(url)) {
    fehler("Bitte eine vollständige Link-Adresse angeben (beginnend mit https://).");
    return;
  }
  el("verfassen-editor").focus();
  const auswahl = window.getSelection();
  if (auswahl.isCollapsed) {
    // Nichts markiert: Link samt sichtbarem Text einfügen.
    const a = document.createElement("a");
    a.href = url;
    a.textContent = url;
    document.execCommand("insertHTML", false, a.outerHTML);
  } else {
    document.execCommand("createLink", false, url);
  }
});

// ------------------------------------------------------------- Emoji --
// Kleine eigene Emoji-Auswahl: per Knopf oder Tastenkürzel „Super + ." (die
// Windows-Taste) zu öffnen, Klick fügt das Emoji an der Schreibmarke ein.

const EMOJIS =
  "😀 😃 😄 😁 😆 😅 😂 🙂 🙃 😉 😊 😍 😘 😗 😎 🤩 🥳 🤗 🤔 😐 😴 😪 " +
  "😑 🙄 😳 😢 😭 😤 😠 😡 🥺 😱 😬 🤯 😇 🤠 🤓 🥰 😋 😜 🤪 😏 😌 " +
  "👍 👎 👌 🙏 👏 🙌 💪 👀 🎉 🎊 ✨ ⭐ 🔥 💯 ✅ ❌ ❓ ❗ ⚠️ 💡 " +
  "❤️ 🧡 💛 💚 💙 💜 🖤 🤍 💔 💕 🌟 ☀️ 🌈 ☕ 🍀 🎁 📅 📌 📎 ✉️"
    .split(/\s+/)
    .filter(Boolean);

let emojiAufgebaut = false;

function emojiAuswahlAufbauen() {
  if (emojiAufgebaut) return;
  emojiAufgebaut = true;
  const auswahl = el("emoji-auswahl");
  for (const emoji of EMOJIS) {
    const knopf = document.createElement("button");
    knopf.type = "button";
    knopf.tabIndex = -1;
    knopf.className = "emoji-zelle";
    knopf.textContent = emoji;
    // mousedown abfangen: die Schreibmarke im Editor bleibt erhalten.
    knopf.addEventListener("mousedown", (ereignis) => ereignis.preventDefault());
    knopf.addEventListener("click", () => {
      document.execCommand("insertText", false, emoji);
      emojiAuswahlSchliessen();
      el("verfassen-editor").focus();
    });
    auswahl.appendChild(knopf);
  }
}

function emojiAuswahlSichtbar() {
  return !el("emoji-auswahl").classList.contains("versteckt");
}

function emojiAuswahlSchliessen() {
  el("emoji-auswahl").classList.add("versteckt");
}

function emojiAuswahlUmschalten() {
  const auswahl = el("emoji-auswahl");
  if (emojiAuswahlSichtbar()) {
    emojiAuswahlSchliessen();
    return;
  }
  emojiAuswahlAufbauen();
  // Über dem Emoji-Knopf ausrichten.
  const kasten = el("emoji-knopf").getBoundingClientRect();
  auswahl.classList.remove("versteckt");
  auswahl.style.left = `${Math.max(8, kasten.left)}px`;
  auswahl.style.top = `${kasten.bottom + 4}px`;
}

el("emoji-knopf").addEventListener("mousedown", (ereignis) => ereignis.preventDefault());
el("emoji-knopf").addEventListener("click", emojiAuswahlUmschalten);

// „Super + ." (Windows-Taste + Punkt) öffnet die Auswahl.
document.addEventListener("keydown", (ereignis) => {
  if (ereignis.key === "." && (ereignis.metaKey || ereignis.getModifierState?.("Super"))) {
    ereignis.preventDefault();
    emojiAuswahlUmschalten();
  } else if (ereignis.key === "Escape" && emojiAuswahlSichtbar()) {
    emojiAuswahlSchliessen();
  }
});

// Klick außerhalb schließt die Auswahl.
document.addEventListener("mousedown", (ereignis) => {
  if (
    emojiAuswahlSichtbar() &&
    !el("emoji-auswahl").contains(ereignis.target) &&
    ereignis.target.closest("#emoji-knopf") === null
  ) {
    emojiAuswahlSchliessen();
  }
});

// ------------------------------------------------------------- Aufbau --

async function aufbauen() {
  const formular = el("verfassen-formular");
  try {
    zustand.konten = await invoke("konten_liste");
    const auswahl = el("von-auswahl");
    for (const konto of zustand.konten) {
      const option = document.createElement("option");
      option.value = konto.id;
      option.textContent = `${konto.name} <${konto.email}>`;
      if (konto.id === zustand.vorgabeKontoId) option.selected = true;
      auswahl.appendChild(option);
    }
    // „Von“ nur zeigen, wenn es mehr als ein Konto gibt.
    el("von-label").classList.toggle("versteckt", zustand.konten.length <= 1);
    if (zustand.konten.length === 0) {
      fehler("Kein Konto vorhanden.");
      return;
    }
    auswahl.addEventListener("change", kontoAnwenden);

    // Entwurf weiterbearbeiten: gespeicherten Stand in die Felder laden.
    if (zustand.entwurfVon) {
      const entwurf = await invoke("entwurf_laden", { mailId: zustand.entwurfVon });
      formular.elements.an.value = entwurf.an || "";
      formular.elements.cc.value = entwurf.cc || "";
      formular.elements.betreff.value = entwurf.betreff || "";
      const editor = el("verfassen-editor");
      if (entwurf.html) {
        // Vom Backend bereinigt — Formatierung bleibt erhalten.
        editor.innerHTML = entwurf.html;
      } else {
        textAlsZeilen(editor, entwurf.text || "");
      }
    }

    // Antwort/Weiterleiten: Vorlage vom Backend holen.
    if (zustand.antwortAuf) {
      const vorlage = await invoke("antwort_vorbereiten", {
        mailId: zustand.antwortAuf,
        weiterleiten: zustand.weiterleiten,
      });
      formular.elements.an.value = vorlage.an || "";
      formular.elements.betreff.value = vorlage.betreff || "";
      const editor = el("verfassen-editor");
      editor.appendChild(document.createElement("br")); // Schreibzeile oben
      const zitat = document.createElement("div");
      zitat.id = "zitat-block";
      textAlsZeilen(zitat, (vorlage.text || "").replace(/^\n+/, ""));
      editor.appendChild(zitat);
    }

    kontoAnwenden(); // Farbe + Signatur des vorausgewählten Kontos

    // Fokus sinnvoll setzen.
    if (formular.elements.an.value) {
      const editor = el("verfassen-editor");
      editor.focus();
      window.getSelection().collapse(editor, 0);
    } else {
      formular.elements.an.focus();
    }
  } catch (f) {
    fehler(String(f));
  }
}

// ------------------------------------------------- Adress-Vorschläge --
// Beim Tippen im An-/CC-Feld schlägt das Backend bekannte Adressen vor
// (bisherige Empfänger und Absender). Auswahl per Klick oder Pfeiltasten;
// es zählt immer nur der Teil hinter dem letzten Komma.

const vorschlaege = { feld: null, eintraege: [], index: -1 };
let vorschlagsAnfrage = 0;

function vorschlaegeVerbergen() {
  el("adress-vorschlaege").classList.add("versteckt");
  vorschlaege.eintraege = [];
  vorschlaege.index = -1;
}

function letzterAdressteil(wert) {
  const teile = wert.split(/[,;]/);
  return teile[teile.length - 1].trim();
}

function vorschlagUebernehmen(email) {
  const feld = vorschlaege.feld;
  const teile = feld.value.split(/[,;]/);
  teile[teile.length - 1] = " " + email;
  feld.value = teile.join(",").trimStart();
  vorschlaegeVerbergen();
  feld.focus();
}

function vorschlaegeZeichnen() {
  const liste = el("adress-vorschlaege");
  liste.innerHTML = "";
  vorschlaege.eintraege.forEach((eintrag, index) => {
    const zeile = document.createElement("button");
    zeile.type = "button";
    zeile.className = "vorschlag" + (index === vorschlaege.index ? " aktiv" : "");
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
    // mousedown statt click: das Eingabefeld darf den Fokus nicht verlieren.
    zeile.addEventListener("mousedown", (ereignis) => {
      ereignis.preventDefault();
      vorschlagUebernehmen(eintrag.email);
    });
    liste.appendChild(zeile);
  });
  const kasten = vorschlaege.feld.getBoundingClientRect();
  liste.style.left = `${kasten.left}px`;
  liste.style.top = `${kasten.bottom + 4}px`;
  liste.style.width = `${kasten.width}px`;
  liste.classList.toggle("versteckt", vorschlaege.eintraege.length === 0);
}

async function vorschlaegeAktualisieren(feld) {
  const eingabe = letzterAdressteil(feld.value);
  if (eingabe.length < 2) {
    vorschlaegeVerbergen();
    return;
  }
  const anfrage = ++vorschlagsAnfrage;
  try {
    const treffer = await invoke("adress_vorschlaege", { eingabe });
    // Nur die jüngste Anfrage zählt — und nur, solange das Feld Fokus hat.
    if (anfrage !== vorschlagsAnfrage || document.activeElement !== feld) return;
    vorschlaege.feld = feld;
    vorschlaege.eintraege = treffer;
    vorschlaege.index = -1;
    vorschlaegeZeichnen();
  } catch {
    vorschlaegeVerbergen(); // Vorschläge sind Komfort — Fehler still ignorieren
  }
}

function vorschlaegeAnbinden(feld) {
  feld.addEventListener("input", () => vorschlaegeAktualisieren(feld));
  feld.addEventListener("blur", vorschlaegeVerbergen);
  feld.addEventListener("keydown", (ereignis) => {
    if (vorschlaege.eintraege.length === 0) return;
    if (ereignis.key === "ArrowDown" || ereignis.key === "ArrowUp") {
      ereignis.preventDefault();
      const schritt = ereignis.key === "ArrowDown" ? 1 : -1;
      const anzahl = vorschlaege.eintraege.length;
      vorschlaege.index = (vorschlaege.index + schritt + anzahl) % anzahl;
      vorschlaegeZeichnen();
    } else if (ereignis.key === "Enter" && vorschlaege.index >= 0) {
      ereignis.preventDefault(); // Enter übernimmt, statt zu senden
      vorschlagUebernehmen(vorschlaege.eintraege[vorschlaege.index].email);
    } else if (ereignis.key === "Tab" && !ereignis.shiftKey) {
      // Tab übernimmt den markierten Vorschlag (sonst den ersten), statt
      // zum nächsten Feld zu springen.
      ereignis.preventDefault();
      const wahl = vorschlaege.index >= 0 ? vorschlaege.index : 0;
      vorschlagUebernehmen(vorschlaege.eintraege[wahl].email);
    } else if (ereignis.key === "Escape") {
      vorschlaegeVerbergen();
    }
  });
}

vorschlaegeAnbinden(el("verfassen-formular").elements.an);
vorschlaegeAnbinden(el("verfassen-formular").elements.cc);

// ------------------------------------------------------------ Anhänge --

function anhangListeZeichnen() {
  const liste = el("anhang-liste");
  liste.innerHTML = "";
  zustand.anhaenge.forEach((pfad, index) => {
    const chip = document.createElement("span");
    chip.className = "anhang-chip";
    chip.appendChild(icon("paperclip"));
    const name = document.createElement("span");
    name.textContent = pfad.split("/").pop();
    chip.appendChild(name);
    const entfernen = document.createElement("button");
    entfernen.type = "button";
    entfernen.className = "anhang-entfernen";
    entfernen.title = "Anhang entfernen";
    entfernen.appendChild(icon("x"));
    entfernen.addEventListener("click", () => {
      zustand.anhaenge.splice(index, 1);
      anhangListeZeichnen();
    });
    chip.appendChild(entfernen);
    liste.appendChild(chip);
  });
}

el("anhang-knopf").addEventListener("click", async () => {
  try {
    const auswahl = await dateiDialog({ multiple: true, title: "Dateien anhängen" });
    if (!auswahl) return;
    const pfade = Array.isArray(auswahl) ? auswahl : [auswahl];
    zustand.anhaenge.push(...pfade);
    anhangListeZeichnen();
  } catch (f) {
    fehler(String(f));
  }
});

// Dateien direkt ins Fenster ziehen: Tauri liefert die Dateipfade,
// solange etwas darüber schwebt, zeigt ein Overlay den Ablage-Hinweis.
aktuellesFenster.onDragDropEvent((ereignis) => {
  const daten = ereignis.payload;
  if (daten.type === "enter" || daten.type === "over") {
    el("drop-overlay").classList.remove("versteckt");
  } else if (daten.type === "drop") {
    el("drop-overlay").classList.add("versteckt");
    zustand.anhaenge.push(...(daten.paths || []));
    anhangListeZeichnen();
  } else {
    el("drop-overlay").classList.add("versteckt");
  }
});

// ------------------------------------------------------------- Senden --

el("abbrechen-knopf").addEventListener("click", () => aktuellesFenster.close());

// ------------------------------------------------- Entwurf speichern --

el("entwurf-knopf").addEventListener("click", async () => {
  const daten = new FormData(el("verfassen-formular"));
  const editor = el("verfassen-editor");
  const knopf = el("entwurf-knopf");
  knopf.disabled = true;
  el("verfassen-fehler").classList.add("versteckt");
  try {
    await invoke("entwurf_speichern", {
      kontoId: Number(daten.get("von")),
      formular: {
        an: daten.get("an"),
        cc: daten.get("cc"),
        betreff: daten.get("betreff"),
        text: editor.innerText,
        html: editor.innerHTML,
        anhaenge: zustand.anhaenge,
        antwort_auf: null,
        weiterleiten: false,
        entwurf_von: zustand.entwurfVon, // alter Stand wird ersetzt
      },
    });
    aktuellesFenster.close();
  } catch (f) {
    fehler(String(f));
    knopf.disabled = false;
  }
});

el("verfassen-formular").addEventListener("submit", async (ereignis) => {
  ereignis.preventDefault();
  const daten = new FormData(ereignis.target);
  const editor = el("verfassen-editor");
  const knopf = el("senden-knopf");
  knopf.disabled = true;
  knopf.textContent = "Sende …";
  el("verfassen-fehler").classList.add("versteckt");
  try {
    // Feldnamen entsprechen dem Rust-Struct SendeFormular (snake_case).
    // text = reine Textfassung, html = formatierte Fassung (das Backend
    // bereinigt sie und baut daraus eine multipart/alternative-Mail).
    await invoke("mail_senden", {
      kontoId: Number(daten.get("von")),
      formular: {
        an: daten.get("an"),
        cc: daten.get("cc"),
        betreff: daten.get("betreff"),
        text: editor.innerText,
        html: editor.innerHTML,
        anhaenge: zustand.anhaenge,
        antwort_auf: zustand.antwortAuf,
        weiterleiten: zustand.weiterleiten,
        entwurf_von: zustand.entwurfVon, // Entwurf wird nach dem Senden entfernt
      },
    });
    // Hauptfenster informieren, dann schließen.
    await emit("mail:gesendet", {});
    aktuellesFenster.close();
  } catch (f) {
    fehler(String(f));
    knopf.disabled = false;
    knopf.innerHTML = "";
    knopf.appendChild(icon("paper-plane-tilt"));
    knopf.append(" Senden");
  }
});

aufbauen();

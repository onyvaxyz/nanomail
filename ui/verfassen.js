// Nanomail — Verfassen-Fenster (M3.1).
// Eigenständiges Fenster: baut sich aus Query-Parametern auf, nutzt die
// bestehenden Backend-Commands und schließt sich nach dem Senden selbst.

const { invoke } = window.__TAURI__.core;
const { emit } = window.__TAURI__.event;
const dateiDialog = window.__TAURI__.dialog.open;
const aktuellesFenster = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();

const el = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);

const zustand = {
  anhaenge: [],
  antwortAuf: params.get("antwortAuf") ? Number(params.get("antwortAuf")) : null,
  weiterleiten: params.get("weiterleiten") === "1",
  vorgabeKontoId: params.get("kontoId") ? Number(params.get("kontoId")) : null,
};

function icon(name) {
  const i = document.createElement("i");
  i.className = `ph-thin ph-${name}`;
  return i;
}

function fehler(text) {
  const feld = el("verfassen-fehler");
  feld.textContent = text;
  feld.classList.remove("versteckt");
}

async function aufbauen() {
  const formular = el("verfassen-formular");
  try {
    const konten = await invoke("konten_liste");
    const auswahl = el("von-auswahl");
    for (const konto of konten) {
      const option = document.createElement("option");
      option.value = konto.id;
      option.textContent = `${konto.name} <${konto.email}>`;
      if (konto.id === zustand.vorgabeKontoId) option.selected = true;
      auswahl.appendChild(option);
    }
    // „Von“ nur zeigen, wenn es mehr als ein Konto gibt.
    el("von-label").classList.toggle("versteckt", konten.length <= 1);
    if (konten.length === 0) {
      fehler("Kein Konto vorhanden.");
      return;
    }

    // Antwort/Weiterleiten: Vorlage vom Backend holen.
    if (zustand.antwortAuf) {
      const vorlage = await invoke("antwort_vorbereiten", {
        mailId: zustand.antwortAuf,
        weiterleiten: zustand.weiterleiten,
      });
      formular.elements.an.value = vorlage.an || "";
      formular.elements.betreff.value = vorlage.betreff || "";
      formular.elements.text.value = vorlage.text || "";
    }
    // Fokus sinnvoll setzen.
    if (formular.elements.an.value) {
      el("verfassen-text").focus();
      el("verfassen-text").setSelectionRange(0, 0);
    } else {
      formular.elements.an.focus();
    }
  } catch (f) {
    fehler(String(f));
  }
}

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

el("abbrechen-knopf").addEventListener("click", () => aktuellesFenster.close());

el("verfassen-formular").addEventListener("submit", async (ereignis) => {
  ereignis.preventDefault();
  const daten = new FormData(ereignis.target);
  const knopf = el("senden-knopf");
  knopf.disabled = true;
  knopf.textContent = "Sende …";
  el("verfassen-fehler").classList.add("versteckt");
  try {
    // Feldnamen entsprechen dem Rust-Struct SendeFormular (snake_case).
    await invoke("mail_senden", {
      kontoId: Number(daten.get("von")),
      formular: {
        an: daten.get("an"),
        cc: daten.get("cc"),
        betreff: daten.get("betreff"),
        text: daten.get("text"),
        anhaenge: zustand.anhaenge,
        antwort_auf: zustand.antwortAuf,
        weiterleiten: zustand.weiterleiten,
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

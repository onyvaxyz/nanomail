// Nanomail — Hell/Dunkel-Design, gilt für alle Fenster.
//
// Vier Wahlmöglichkeiten, gespeichert in localStorage (bleibt über
// Neustarts erhalten; Start-Standard ist „system“):
//   "dunkel" – immer dunkel
//   "hell"   – immer hell
//   "system" – die Ubuntu-Einstellung (hell/dunkel) übernehmen
//   "omarchy" – aktive Linux-Omarchy-Palette, sonst Systemeinstellung
//
// Angewendet wird das Ergebnis als data-thema="hell"/"dunkel" am
// <html>-Element; styles.css überschreibt darüber die Farbvariablen.
(function () {
  const SPEICHER_SCHLUESSEL = "nanomail-thema";
  const systemHell = window.matchMedia("(prefers-color-scheme: light)");

  // Der Knopf zeigt immer an, was der nächste Klick bewirkt:
  // Mond = dunkel einschalten → Sonne = hell einschalten →
  // Palette = Omarchy → Bildschirm = Systemeinstellung → wieder Mond.
  const NAECHSTE_WAHL = {
    system: { wahl: "dunkel", icon: "ph-moon", titel: "Dunkles Design einschalten" },
    dunkel: { wahl: "hell", icon: "ph-sun", titel: "Helles Design einschalten" },
    hell: { wahl: "omarchy", icon: "ph-palette", titel: "Omarchy-Theme übernehmen" },
    omarchy: { wahl: "system", icon: "ph-monitor", titel: "Systemeinstellung übernehmen" },
  };

  function wahlLesen() {
    const wert = localStorage.getItem(SPEICHER_SCHLUESSEL);
    return Object.hasOwn(NAECHSTE_WAHL, wert) ? wert : "system";
  }

  let palette = null;
  let tokens = [];
  let anfrage = 0;
  let angewendet = "";
  async function anwenden() {
    const nummer = ++anfrage;
    const wahl = wahlLesen();
    if (wahl === "omarchy") {
      palette = await window.__TAURI__?.core.invoke("omarchy_thema").catch(() => null);
      if (nummer !== anfrage) return;
    }
    const root = document.documentElement;
    const farben = wahl === "omarchy" ? palette?.farben || {} : {};
    const hell = wahl === "hell" ||
      (wahl === "omarchy" && palette ? palette.hell : ["system", "omarchy"].includes(wahl) && systemHell.matches);
    const stand = JSON.stringify([wahl, hell, farben]);
    if (stand === angewendet) return;
    angewendet = stand;
    for (const token of tokens) root.style.removeProperty(token);
    tokens = Object.keys(farben);
    for (const [token, farbe] of Object.entries(farben)) root.style.setProperty(token, farbe);
    root.dataset.profil = wahl;
    root.dataset.omarchy = String(wahl === "omarchy" && !!palette);
    document.documentElement.dataset.thema = hell ? "hell" : "dunkel";

    const knopf = document.getElementById("thema-knopf");
    if (knopf) {
      const naechste = NAECHSTE_WAHL[wahl];
      knopf.title = naechste.titel;
      knopf.setAttribute("aria-label", `Design: ${wahl}. ${naechste.titel}`);
      knopf.firstElementChild.className = `ph-regular ${naechste.icon}`;
    }

    // Andere Skripte (z. B. die Mail-Anzeige) können darauf reagieren.
    window.dispatchEvent(new CustomEvent("thema:gewechselt"));
  }

  document.getElementById("thema-knopf")?.addEventListener("click", () => {
    localStorage.setItem(SPEICHER_SCHLUESSEL, NAECHSTE_WAHL[wahlLesen()].wahl);
    anwenden();
  });

  // Wechselt Ubuntu selbst (z. B. abends auf dunkel), sofort mitziehen.
  systemHell.addEventListener("change", () => {
    if (["system", "omarchy"].includes(wahlLesen())) anwenden();
  });

  // Umschaltung in einem anderen Fenster (z. B. Hauptfenster) übernehmen.
  window.addEventListener("storage", (ereignis) => {
    if (ereignis.key === SPEICHER_SCHLUESSEL) anwenden();
  });

  // Omarchy ersetzt den Theme-Ordner beim Wechsel. Wiederholtes Öffnen des
  // festen Pfads erfasst auch diesen Austausch, ohne eigene Hooks zu installieren.
  setInterval(() => { if (wahlLesen() === "omarchy") anwenden(); }, 3000);
  window.addEventListener("focus", () => { if (wahlLesen() === "omarchy") anwenden(); });

  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 1;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  function kontraste() {
    for (const knopf of document.querySelectorAll("button, button .avatar, .konto-icon-abzeichen")) {
      if (!knopf.getClientRects().length) continue;
      const eltern = [];
      for (let n = knopf; n; n = n.parentElement) eltern.unshift(n);
      ctx.fillStyle = "white";
      ctx.fillRect(0, 0, 1, 1);
      for (const n of eltern) {
        ctx.fillStyle = getComputedStyle(n).backgroundColor;
        ctx.fillRect(0, 0, 1, 1);
      }
      const rgb = [...ctx.getImageData(0, 0, 1, 1).data].slice(0, 3);
      const linear = rgb.map(v => v / 255).map(v => v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
      const luminanz = linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
      const farbe = (luminanz + 0.05) / 0.05 >= 1.05 / (luminanz + 0.05) ? "#000000" : "#ffffff";
      if (knopf.style.getPropertyValue("--knopf-text") !== farbe) knopf.style.setProperty("--knopf-text", farbe);
    }
  }
  let geplant = false;
  function kontrastePlanen() {
    if (geplant) return;
    geplant = true;
    requestAnimationFrame(() => { geplant = false; kontraste(); });
  }
  new MutationObserver(kontrastePlanen).observe(document.documentElement, {
    subtree: true, childList: true, attributes: true, attributeFilter: ["class", "style", "data-thema"],
  });
  for (const event of ["pointerover", "pointerout", "pointerdown", "pointerup", "focusin", "focusout", "transitionend"]) document.addEventListener(event, kontrastePlanen);
  anwenden();
})();

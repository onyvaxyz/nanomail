// Nanomail — Fenster-Anfasser (Nachbesserung zu M3.4).
// Ohne Systemrahmen (decorations: false) bietet GNOME keine Ränder zum
// Größenändern an. Diese unsichtbaren Leisten an allen Kanten und Ecken
// starten das native Größenändern des Fensters. Wird von beiden Fenstern
// (index.html und verfassen.html) vor dem jeweiligen Haupt-Skript geladen.

// Nur HTTP(S)-Links aktivieren; Text/Markup bleibt unvertrauenswürdiger Text.
// Externe Navigation wird weiterhin im Rust-Navigationshandler abgefangen.
function textMitLinks(ziel, text) {
  let ende = 0;
  for (const treffer of text.matchAll(/https?:\/\/[^\s<>"']+[^\s<>"'.,;:!?)\]]/g)) {
    ziel.append(document.createTextNode(text.slice(ende, treffer.index)));
    const a = document.createElement("a");
    a.href = treffer[0];
    a.textContent = treffer[0];
    ziel.append(a);
    ende = treffer.index + treffer[0].length;
  }
  ziel.append(document.createTextNode(text.slice(ende)));
}

(() => {
  const fenster = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();

  // CSS-Klasse → Richtung für startResizeDragging.
  const RICHTUNGEN = [
    ["n", "North"],
    ["s", "South"],
    ["w", "West"],
    ["e", "East"],
    ["nw", "NorthWest"],
    ["ne", "NorthEast"],
    ["sw", "SouthWest"],
    ["se", "SouthEast"],
  ];

  for (const [klasse, richtung] of RICHTUNGEN) {
    const griff = document.createElement("div");
    griff.className = `fenster-griff griff-${klasse}`;
    griff.addEventListener("mousedown", async (ereignis) => {
      if (ereignis.button !== 0) return;
      ereignis.preventDefault();
      // Maximiert gibt es nichts zu ziehen.
      if (await fenster.isMaximized()) return;
      fenster.startResizeDragging(richtung);
    });
    document.body.appendChild(griff);
  }
})();

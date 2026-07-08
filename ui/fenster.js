// Nanomail — Fenster-Anfasser (Nachbesserung zu M3.4).
// Ohne Systemrahmen (decorations: false) bietet GNOME keine Ränder zum
// Größenändern an. Diese unsichtbaren Leisten an allen Kanten und Ecken
// starten das native Größenändern des Fensters. Wird von beiden Fenstern
// (index.html und verfassen.html) vor dem jeweiligen Haupt-Skript geladen.

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

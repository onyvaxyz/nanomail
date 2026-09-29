// Nanomail — Fenster-Anfasser (Nachbesserung zu M3.4).
// Ohne Systemrahmen (decorations: false) bietet GNOME keine Ränder zum
// Größenändern an. Diese unsichtbaren Leisten an allen Kanten und Ecken
// starten das native Größenändern des Fensters. Wird von beiden Fenstern
// (index.html und verfassen.html) vor dem jeweiligen Haupt-Skript geladen.

// Muster für HTTP(S)-Links — identisch für Text- und HTML-Anzeige, damit
// ein Link in beiden Ansichten gleich erkannt wird. Beide Nutzer nutzen
// nur matchAll (verstellt lastIndex nicht).
const LINK_MUSTER = /https?:\/\/[^\s<>"']+[^\s<>"'.,;:!)\]]/g;

// Nur HTTP(S)-Links aktivieren; Text/Markup bleibt unvertrauenswürdiger Text.
// Externe Navigation wird weiterhin im Rust-Navigationshandler abgefangen.
function textMitLinks(ziel, text) {
  let ende = 0;
  for (const treffer of text.matchAll(LINK_MUSTER)) {
    ziel.append(document.createTextNode(text.slice(ende, treffer.index)));
    const a = document.createElement("a");
    a.href = treffer[0];
    a.textContent = treffer[0];
    ziel.append(a);
    ende = treffer.index + treffer[0].length;
  }
  ziel.append(document.createTextNode(text.slice(ende)));
}

// Nackte HTTP(S)-Links im bereits bereinigten Mail-HTML klickbar machen:
// Manche Absender (z. B. Anmelde-Links) verschicken URLs als reinen Text.
// Sicher, weil das HTML schon vom Backend mit ammonia bereinigt wurde, der
// Browser-Parser (DOMParser) dabei nichts ausführt und nichts nachlädt und
// neue Links ausschließlich per createElement/textContent entstehen — nie
// durch Einsetzen von Fremd-HTML.
function htmlMitLinks(html) {
  const doc = new DOMParser().parseFromString(html, "text/html");
  // Textknoten sammeln; Elemente mit eigener Link-/Code-Bedeutung außen vor.
  const ueberspringen = new Set(["a", "style", "script", "textarea", "title"]);
  const textknoten = [];
  const sammeln = (element) => {
    for (const kind of element.childNodes) {
      if (kind.nodeType === Node.TEXT_NODE) textknoten.push(kind);
      else if (kind.nodeType === Node.ELEMENT_NODE && !ueberspringen.has(kind.localName)) sammeln(kind);
    }
  };
  sammeln(doc.body);
  for (const knoten of textknoten) {
    let ende = 0;
    let ersatz = null;
    for (const treffer of knoten.data.matchAll(LINK_MUSTER)) {
      if (!ersatz) ersatz = doc.createDocumentFragment();
      ersatz.append(doc.createTextNode(knoten.data.slice(ende, treffer.index)));
      const a = doc.createElement("a");
      a.href = treffer[0];
      a.target = "_top"; // Klick wie bei allen Mail-Links vom Backend abfangen lassen
      a.textContent = treffer[0];
      ersatz.append(a);
      ende = treffer.index + treffer[0].length;
    }
    if (ersatz) {
      ersatz.append(doc.createTextNode(knoten.data.slice(ende)));
      knoten.replaceWith(ersatz);
    }
  }
  // Nur lesend: serialisiert ausschließlich die oben gebauten Knoten.
  return doc.body.innerHTML;
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

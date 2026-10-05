// Nanomail — gemeinsame UI-Helfer (wird in jedem Fenster zuerst geladen).
//
// `el`, `zeige`, `icon` und `anzahlText` standen früher wortgleich in
// mehreren Dateien (main.js, mail.js, verfassen.js; kalender.js nutzte sie
// über die Ladereihenfolge). Genau eine Quelle — keine Kopien mehr.

/// DOM-Kürzel: Element per ID holen.
const el = (id) => document.getElementById(id);

/// Bereich ein-/ausblenden (über die Klasse `versteckt`).
function zeige(id, sichtbar) {
  el(id).classList.toggle("versteckt", !sichtbar);
}

/// Phosphor-Symbol erzeugen.
function icon(name) {
  const i = document.createElement("i");
  i.className = `ph-light ph-${name}`;
  return i;
}

/// Anzahltext mit Einzahl/Mehrzahl: „1 Mail“ bzw. „5 Mails“.
function anzahlText(n, singular, plural) {
  return n === 1 ? `1 ${singular}` : `${n} ${plural}`;
}

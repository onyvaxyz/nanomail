// Nanomail — Hell/Dunkel-Design, gilt für alle Fenster.
//
// Drei Wahlmöglichkeiten, gespeichert in localStorage (bleibt über
// Neustarts erhalten; Start-Standard ist „system“):
//   "dunkel" – immer dunkel
//   "hell"   – immer hell
//   "system" – die Ubuntu-Einstellung (hell/dunkel) übernehmen
//
// Angewendet wird das Ergebnis als data-thema="hell"/"dunkel" am
// <html>-Element; styles.css überschreibt darüber die Farbvariablen.
(function () {
  const SPEICHER_SCHLUESSEL = "nanomail-thema";
  const systemHell = window.matchMedia("(prefers-color-scheme: light)");

  // Der Knopf zeigt immer an, was der nächste Klick bewirkt:
  // Mond = dunkel einschalten → Sonne = hell einschalten →
  // Bildschirm = Systemeinstellung übernehmen → wieder Mond.
  const NAECHSTE_WAHL = {
    system: { wahl: "dunkel", icon: "ph-moon", titel: "Dunkles Design einschalten" },
    dunkel: { wahl: "hell", icon: "ph-sun", titel: "Helles Design einschalten" },
    hell: { wahl: "system", icon: "ph-monitor", titel: "Systemeinstellung (Ubuntu) übernehmen" },
  };

  function wahlLesen() {
    const wert = localStorage.getItem(SPEICHER_SCHLUESSEL);
    return wert in NAECHSTE_WAHL ? wert : "system";
  }

  function anwenden() {
    const wahl = wahlLesen();
    const hell = wahl === "hell" || (wahl === "system" && systemHell.matches);
    document.documentElement.dataset.thema = hell ? "hell" : "dunkel";

    const knopf = document.getElementById("thema-knopf");
    if (knopf) {
      const naechste = NAECHSTE_WAHL[wahl];
      knopf.title = naechste.titel;
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
    if (wahlLesen() === "system") anwenden();
  });

  // Umschaltung in einem anderen Fenster (z. B. Hauptfenster) übernehmen.
  window.addEventListener("storage", (ereignis) => {
    if (ereignis.key === SPEICHER_SCHLUESSEL) anwenden();
  });

  anwenden();
})();

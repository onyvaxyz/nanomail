// Nanomail — Anhang-Auswahl (Paket D).
// Gemeinsam für Haupt- und Lesefenster: fragt pro Anhang, ob er auf die
// Festplatte gespeichert (Standard) oder mit dem Systemprogramm geöffnet
// werden soll. Das Backend holt den Inhalt in beiden Fällen frisch vom
// Server; Anhänge liegen nie im lokalen Cache.

/// Fragt Speichern vs. Öffnen. Liefert "speichern", "oeffnen" oder null
/// (abgebrochen). Speichern ist vorausgewählt.
function anhangWahlAnzeigen(dateiname) {
  return new Promise((fertig) => {
    document.getElementById("anhang-wahl-dialog")?.remove();
    const dialog = document.createElement("dialog");
    dialog.id = "anhang-wahl-dialog";
    const titel = document.createElement("h2");
    titel.textContent = "Anhang öffnen oder speichern?";
    const untertitel = document.createElement("p");
    untertitel.className = "dialog-untertitel";
    // Dateiname nur als Text — niemals als HTML ausführen.
    untertitel.textContent = `„${dateiname}“ — was soll passieren?`;
    const formular = document.createElement("form");
    formular.method = "dialog";
    for (const [wert, text, vorauswahl] of [
      ["speichern", "Auf Festplatte speichern", true],
      ["oeffnen", "Mit Systemprogramm öffnen", false],
    ]) {
      const option = document.createElement("label");
      option.className = "anhang-wahl-option";
      const radio = document.createElement("input");
      radio.type = "radio";
      radio.name = "wahl";
      radio.value = wert;
      radio.checked = vorauswahl;
      option.append(radio, document.createTextNode(` ${text}`));
      formular.appendChild(option);
    }
    const knoepfe = document.createElement("div");
    knoepfe.className = "dialog-knoepfe";
    const abbrechen = document.createElement("button");
    abbrechen.type = "submit";
    abbrechen.value = "abbrechen";
    abbrechen.className = "knopf-sekundaer";
    abbrechen.textContent = "Abbrechen";
    const fueller = document.createElement("span");
    fueller.className = "fueller";
    const weiter = document.createElement("button");
    weiter.type = "submit";
    weiter.value = "ok";
    weiter.className = "knopf-primaer";
    weiter.textContent = "Weiter";
    knoepfe.append(abbrechen, fueller, weiter);
    formular.appendChild(knoepfe);
    dialog.append(titel, untertitel, formular);
    dialog.addEventListener("close", () => {
      const wahl = dialog.returnValue === "ok" ? formular.elements.wahl.value : null;
      dialog.remove();
      fertig(wahl);
    });
    document.body.appendChild(dialog);
    dialog.showModal();
  });
}

/// Führt die gewählte Aktion aus. `melden(text, klasse)` zeigt das Ergebnis
/// (z. B. Statuszeile oder Fehlerfeld des jeweiligen Fensters).
async function anhangAktion(mailId, anhang, knopf, melden) {
  const { invoke } = window.__TAURI__.core;
  const wahl = await anhangWahlAnzeigen(anhang.dateiname);
  if (!wahl) return; // abgebrochen
  knopf.disabled = true;
  try {
    if (wahl === "oeffnen") {
      melden(await invoke("anhang_oeffnen", { mailId, index: anhang.index }), "ok");
    } else {
      const ziel = await window.__TAURI__.dialog.save({
        title: "Anhang speichern",
        defaultPath: await invoke("datei_standardpfad", { dateiname: anhang.dateiname }),
      });
      if (!ziel) return; // Speichern-Dialog abgebrochen
      await invoke("anhang_speichern", { mailId, index: anhang.index, zielPfad: ziel });
      melden(`✓ Anhang gespeichert: ${anhang.dateiname}`, "ok");
    }
  } catch (fehler) {
    melden(`✗ ${fehler}`, "fehler");
  } finally {
    knopf.disabled = false;
  }
}

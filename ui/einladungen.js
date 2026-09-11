// Strukturierte Backend-Daten, niemals HTML aus einem Kalenderteil ausführen.
function einladungenAnzeigen(ziel, einladungen, mailId) {
  ziel.replaceChildren();
  ziel.classList.toggle("versteckt", !einladungen.length);
  for (const einladung of einladungen) {
    const karte = document.createElement("section");
    const titel = document.createElement("h3");
    const typ = { REQUEST: "Kalendereinladung", CANCEL: "Terminabsage", REPLY: "Terminantwort" }[einladung.methode] || "Kalendertermin";
    titel.textContent = `${typ}: ${einladung.termin.titel}`;
    karte.append(titel);
    const format = new Intl.DateTimeFormat("de-DE", einladung.termin.ganztags
      ? { dateStyle: "full" } : { dateStyle: "medium", timeStyle: "short" });
    const zeit = document.createElement("p");
    zeit.textContent = `${format.format(new Date(einladung.termin.beginn * 1000))} – ${format.format(new Date(einladung.termin.ende * 1000))}${einladung.termin.ganztags ? " (Ende exklusiv)" : ""}`;
    karte.append(zeit);
    for (const text of [einladung.termin.ort, einladung.termin.beschreibung, einladung.termin.url]) {
      if (!text) continue;
      const p = document.createElement("p");
      textMitLinks(p, text);
      karte.append(p);
    }
    const hinweis = document.createElement("p");
    hinweis.setAttribute("role", "status");
    hinweis.textContent = einladung.antwortbar
      ? `Antwort an: ${einladung.organisator}. Keine automatische Kalenderübernahme.`
      : "Keine direkte Zu-/Absage möglich: keine Anfrage, kein gültiger Organisator oder dieses Konto ist nicht eingeladen.";
    karte.append(hinweis);
    if (einladung.antwortbar) {
      const aktionen = document.createElement("div");
      aktionen.className = "popover-aktionen";
      for (const [text, zusage] of [["Zusagen", true], ["Absagen", false]]) {
        const knopf = document.createElement("button");
        knopf.type = "button";
        knopf.className = zusage ? "knopf-primaer" : "knopf-sekundaer";
        knopf.textContent = text;
        knopf.addEventListener("click", async () => {
          if (!confirm(`${text} per Mail an ${einladung.organisator} senden?`)) return;
          const knoepfe = aktionen.querySelectorAll("button");
          knoepfe.forEach(k => { k.disabled = true; });
          try {
            hinweis.textContent = await window.__TAURI__.core.invoke("mail_einladung_antworten", {
              mailId, kalenderIndex: einladung.kalender_index,
              ereignisIndex: einladung.ereignis_index, zusage,
            });
          } catch (fehler) {
            hinweis.textContent = String(fehler);
            knoepfe.forEach(k => { k.disabled = false; });
          }
        });
        aktionen.append(knopf);
      }
      karte.append(aktionen);
    }
    ziel.append(karte);
  }
}

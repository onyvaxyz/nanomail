// Strukturierte Backend-Daten, niemals HTML aus einem Kalenderteil ausführen.
let einladungKalender = null; // einmal geladen: [{id, anzeige_name, konto_name}]

async function einladungKalenderLaden() {
  if (!einladungKalender) {
    try {
      einladungKalender = await window.__TAURI__.core.invoke("kalender_liste");
    } catch {
      einladungKalender = []; // ohne Kalender keine Übernahme, Antworten geht trotzdem
    }
  }
  return einladungKalender;
}

/// Lässt sich die Einladung in einen Kalender übernehmen? Absagen und
/// fremde Antworten sind keine Termine zum Ablegen.
function einladungImportierbar(einladung) {
  return einladung.methode === "REQUEST" || einladung.methode === "PUBLISH";
}

/// Zuletzt gewählter Kalender (wird gemerkt), sonst der erste.
function gemerkterKalenderId(kalender) {
  try {
    const id = Number(localStorage.getItem("einladung-kalender"));
    if (kalender.some((k) => k.id === id)) return id;
  } catch {
    // Merken ist Komfort — Fehler still ignorieren.
  }
  return kalender.length ? kalender[0].id : null;
}

async function einladungUebernehmen(mailId, einladung, kalenderId, hinweis) {
  hinweis.textContent = "Übernehme in Kalender …";
  try {
    const meldung = await window.__TAURI__.core.invoke("mail_einladung_uebernehmen", {
      mailId, kalenderIndex: einladung.kalender_index,
      ereignisIndex: einladung.ereignis_index, kalenderId,
    });
    try {
      localStorage.setItem("einladung-kalender", String(kalenderId));
    } catch {
      // Merken ist Komfort — Fehler still ignorieren.
    }
    hinweis.textContent = meldung;
  } catch (fehler) {
    hinweis.textContent = String(fehler);
  }
}

async function einladungenAnzeigen(ziel, einladungen, mailId) {
  ziel.replaceChildren();
  ziel.classList.toggle("versteckt", !einladungen.length);
  const kalender = await einladungKalenderLaden();
  const mitKonto = new Set(kalender.map((k) => k.konto_name)).size > 1;
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
      ? `Antwort an: ${einladung.organisator}.`
      : "Keine direkte Zu-/Absage möglich: keine Anfrage, kein gültiger Organisator oder dieses Konto ist nicht eingeladen.";
    karte.append(hinweis);
    // Kalenderwahl + Übernehmen (auch ohne Antwort, z. B. bei PUBLISH).
    let kalenderAuswahl = null;
    if (einladungImportierbar(einladung) && kalender.length > 0) {
      const zeile = document.createElement("div");
      zeile.className = "popover-aktionen";
      const label = document.createElement("label");
      label.textContent = "Kalender: ";
      kalenderAuswahl = document.createElement("select");
      for (const k of kalender) {
        const option = document.createElement("option");
        option.value = k.id;
        option.textContent = mitKonto ? `${k.anzeige_name} (${k.konto_name})` : k.anzeige_name;
        kalenderAuswahl.appendChild(option);
      }
      kalenderAuswahl.value = String(gemerkterKalenderId(kalender));
      label.appendChild(kalenderAuswahl);
      const uebernehmen = document.createElement("button");
      uebernehmen.type = "button";
      uebernehmen.className = "knopf-sekundaer";
      uebernehmen.textContent = "In Kalender übernehmen";
      uebernehmen.addEventListener("click", () => {
        if (mailId === null) return;
        void einladungUebernehmen(mailId, einladung, Number(kalenderAuswahl.value), hinweis);
      });
      zeile.append(label, uebernehmen);
      karte.append(zeile);
    }
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
            const antwort = await window.__TAURI__.core.invoke("mail_einladung_antworten", {
              mailId, kalenderIndex: einladung.kalender_index,
              ereignisIndex: einladung.ereignis_index, zusage,
            });
            // Bei Zusage wandert der Termin gleich in den gewählten Kalender.
            if (zusage && kalenderAuswahl && mailId !== null) {
              await einladungUebernehmen(mailId, einladung, Number(kalenderAuswahl.value), hinweis);
              hinweis.textContent = `${antwort} ${hinweis.textContent}`;
            } else {
              hinweis.textContent = antwort;
            }
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

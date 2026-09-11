// Eigenständiges Lesefenster für den Doppelklick in der Mail-Liste.
// Alle Maildaten kommen bereits bereinigt aus dem Backend; HTML bleibt im
// gleichen abgeschotteten iframe wie im Hauptfenster.

const { invoke } = window.__TAURI__.core;
const { WebviewWindow, getCurrentWebviewWindow } = window.__TAURI__.webviewWindow;
const params = new URLSearchParams(location.search);
const mailId = Number(params.get("mailId"));
const kontoId = Number(params.get("kontoId"));
const el = (id) => document.getElementById(id);
const fenster = getCurrentWebviewWindow();

let ansicht = null;
let fensterZaehler = 0;

el("fenster-minimieren").addEventListener("click", () => fenster.minimize());
el("fenster-maximieren").addEventListener("click", () => fenster.toggleMaximize());
el("fenster-schliessen").addEventListener("click", () => fenster.close());

function zeige(id, sichtbar) {
  el(id).classList.toggle("versteckt", !sichtbar);
}

function leseStil() {
  const stil = getComputedStyle(document.documentElement);
  const [hintergrund, text, link, linie, zitat] = ["--bg-editor", "--text", "--blau", "--border", "--text-muted"]
    .map(token => stil.getPropertyValue(token).trim());
  return (
    `body{background:${hintergrund};color:${text};font-family:system-ui,sans-serif;` +
    "font-size:15px;line-height:1.75;max-width:72ch;margin:0 auto;padding:40px 36px;" +
    "overflow-wrap:break-word}" +
    `a{color:${link}}img{max-width:100%;height:auto}` +
    `blockquote{border-left:3px solid ${linie};margin:10px 0;padding:2px 14px;color:${zitat}}` +
    `hr{border:none;border-top:1px solid ${linie}}pre{white-space:pre-wrap}`
  );
}

function htmlAnzeigen(html) {
  if (!html) return;
  const rahmen = el("mailfenster-html");
  rahmen.srcdoc = `<style>${leseStil()}</style>${html}`;
  zeige("mailfenster-text", false);
  zeige("mailfenster-html", true);
}

window.addEventListener("thema:gewechselt", () => {
  if (ansicht?.html_schlicht) htmlAnzeigen(ansicht.html_schlicht);
});

async function bilderLaden() {
  const knopf = el("mailfenster-bilder-laden");
  knopf.disabled = true;
  try {
    const bilder = await invoke("mail_bilder_laden", { mailId });
    ansicht.html_schlicht = bilder.html_schlicht;
    htmlAnzeigen(bilder.html_schlicht);
    zeige("mailfenster-bilder", false);
  } catch (fehler) {
    fehlerZeigen(fehler);
  } finally {
    knopf.disabled = false;
  }
}

function verfassenOeffnen(allenAntworten = false, weiterleiten = false) {
  fensterZaehler += 1;
  const query = new URLSearchParams({
    antwortAuf: String(mailId),
    weiterleiten: weiterleiten ? "1" : "0",
    allenAntworten: allenAntworten ? "1" : "0",
    kontoId: String(kontoId),
  });
  new WebviewWindow(`verfassen-${Date.now()}-${fensterZaehler}`, {
    url: `verfassen.html?${query}`,
    title: weiterleiten ? "Weiterleiten" : allenAntworten ? "Allen antworten" : "Antworten",
    width: 680,
    height: 780,
    minWidth: 480,
    minHeight: 420,
    decorations: false,
  });
}

function fehlerZeigen(fehler) {
  el("mailfenster-fehler").textContent = String(fehler);
  zeige("mailfenster-fehler", true);
}

function anhaengeAnzeigen(anhaenge) {
  const leiste = el("mailfenster-anhaenge");
  leiste.innerHTML = "";
  zeige("mailfenster-anhaenge", anhaenge.length > 0);
  for (const anhang of anhaenge) {
    const knopf = document.createElement("button");
    knopf.type = "button";
    knopf.className = "anhang-knopf";
    knopf.textContent = anhang.dateiname;
    knopf.addEventListener("click", async () => {
      try {
        const ziel = await window.__TAURI__.dialog.save({
          defaultPath: await invoke("datei_standardpfad", { dateiname: anhang.dateiname }),
        });
        if (ziel) await invoke("anhang_speichern", { mailId, index: anhang.index, zielPfad: ziel });
      } catch (fehler) {
        fehlerZeigen(fehler);
      }
    });
    leiste.appendChild(knopf);
  }
}

async function laden() {
  try {
    ansicht = await invoke("mail_lesen", { mailId });
    const kopf = ansicht.kopf;
    const titel = kopf.betreff || "(kein Betreff)";
    document.title = `${titel} — Nanomail`;
    el("fenster-titel").textContent = titel;
    el("mailfenster-titel").textContent = titel;
    el("mailfenster-absender").textContent = kopf.von_email
      ? `${kopf.von || kopf.von_email} <${kopf.von_email}>`
      : kopf.von || "(unbekannt)";
    el("mailfenster-empfaenger").textContent =
      `${kopf.an ? `An: ${kopf.an}` : ""}${kopf.cc ? ` · Cc: ${kopf.cc}` : ""}`;
    el("mailfenster-datum").textContent = kopf.datum
      ? new Intl.DateTimeFormat("de-DE", { dateStyle: "medium", timeStyle: "short" })
          .format(new Date(kopf.datum * 1000))
      : "";
    if (ansicht.html_schlicht) {
      htmlAnzeigen(ansicht.html_schlicht);
    } else {
      el("mailfenster-text").textContent = ansicht.text;
      zeige("mailfenster-text", true);
    }
    anhaengeAnzeigen(ansicht.anhaenge || []);
    einladungenAnzeigen(el("mail-einladungen"), ansicht.einladungen || [], mailId);
    zeige("mailfenster-bilder", ansicht.hatte_externe_bilder && !ansicht.bilder_automatisch);
    if (ansicht.hatte_externe_bilder && ansicht.bilder_automatisch) bilderLaden();
  } catch (fehler) {
    fehlerZeigen(fehler);
  }
}

el("mailfenster-antworten").addEventListener("click", () => verfassenOeffnen());
el("mailfenster-allen-antworten").addEventListener("click", () => verfassenOeffnen(true));
el("mailfenster-weiterleiten").addEventListener("click", () => verfassenOeffnen(false, true));
el("mailfenster-bilder-laden").addEventListener("click", bilderLaden);
el("mailfenster-bilder-immer").addEventListener("click", async () => {
  try {
    await invoke("mail_bild_quelle_erlauben", { mailId });
    await bilderLaden();
  } catch (fehler) {
    fehlerZeigen(fehler);
  }
});

laden();

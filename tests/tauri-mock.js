// Ausschließlich lokale Test-/Reviewdaten; kein Netzwerk, keine Zugangsdaten.
window.testAufrufe = [];
window.testOmarchy = { hell: false, farben: {
  "--bg-editor": "#1a1b26", "--bg-panel": "#1a1b26", "--bg-surface": "#1a1b26",
  "--bg-window": "#13141c", "--text": "#a9b1d6", "--text-hell": "#a9b1d6",
  "--omarchy-akzent": "#7aa2f7", "--blau": "#7aa2f7",
} };
const testKonto = { id: 1, name: "Reviewkonto", email: "anna@example.org", anzeigename: "Anna Beispiel",
  farbe: "#54b879", signatur: "Anna Beispiel\nNanomail Review", smtp_host: "smtp.example.org" };
window.testTermin = { id: 1, kalender_id: 1, kalender_name: "Team", farbe: "#54b879", href: "review.ics", etag: "test",
  titel: "Projektbesprechung", ort: "https://example.org/meeting",
  url: "https://example.org/termin",
  beschreibung: "Unterlagen gemeinsam durchgehen.\nNotizen können markiert und kopiert werden.\nhttps://example.org/agenda",
  beginn: new Date(2026, 8, 11, 14, 0).getTime() / 1000, ende: new Date(2026, 8, 11, 15, 0).getTime() / 1000,
  ganztags: false, teilnehmer: [{ email: "anna@example.org", status: "needs_action" }] };
window.testEinladung = { termin: window.testTermin, methode: "REQUEST", organisator: "team@example.org",
  antwortbar: true, kalender_index: 0, ereignis_index: 0 };
const testKopf = { id: 1, ordner_id: 1, uid: 1, von: "Team", von_email: "team@example.org", an: "anna@example.org",
  betreff: "Einladung zur Projektbesprechung", datum: window.testTermin.beginn - 86400, gelesen: false, hat_anhang: true };
const testFenster = new Proxy({}, { get: (_, key) => key === "label" ? "main" : async () => key === "isMaximized" ? false : () => {} });
window.__TAURI__ = {
  core: { invoke: async (command, args) => {
    window.testAufrufe.push({ command, args });
    switch (command) {
      case "konten_liste": return [testKonto];
      case "ordner_liste": return [{ id: 1, name: "INBOX", rolle: "inbox", ungelesen: 1, konto_id: 1 }];
      case "mails_liste": return [testKopf];
      case "mail_lesen": return { kopf: testKopf, text: "Hallo Anna,\n\nbitte nimm am Termin teil.\nVielen Dank!", html: null, html_schlicht: null, anhaenge: [], einladungen: [window.testEinladung] };
      case "antwort_vorbereiten": return { an: "team@example.org", betreff: "Re: Besprechung", text: "Am Freitag schrieb Team:\n> Bitte nimm am Termin teil." };
      case "entwurf_laden": return { an: "team@example.org", betreff: "Entwurf", text: "Erster Absatz\n\nZweiter Absatz\nmit Zeilenumbruch", html: null };
      case "kalender_liste": return [{ id: 1, name: "Team", farbe: "#54b879", sichtbar: true }];
      case "kalender_termine": return [window.testTermin];
      case "omarchy_thema": return window.testOmarchy;
      case "datei_standardpfad": return "/home/review/Desktop" + (args?.dateiname ? "/" + args.dateiname : "");
      case "mail_einladung_antworten": return "Zusage versendet (Test).";
      case "absender_avatar": return null;
      default: return [];
    }
  } },
  event: { listen: async () => () => {}, emit: async () => {} },
  webviewWindow: { getCurrentWebviewWindow: () => testFenster, WebviewWindow: function () { return testFenster; } },
  window: { getCurrentWindow: () => testFenster },
  dialog: { open: async options => { window.testDialog = options; return null; }, save: async options => { window.testDialog = options; return null; } },
};

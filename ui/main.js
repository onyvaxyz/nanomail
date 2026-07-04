// Nanomail — Frontend-Logik (M0).
// Spricht über Tauri-Commands mit dem Rust-Backend.
// `window.__TAURI__` steht bereit, weil in tauri.conf.json
// `app.withGlobalTauri` aktiviert ist — kein Build-Schritt nötig.

const { invoke } = window.__TAURI__.core;

async function backendPruefen() {
  const status = document.getElementById("backend-status");
  try {
    const antwort = await invoke("ping");
    status.innerHTML =
      `<span class="ok">✓ Backend verbunden</span> — ` +
      `${antwort.app} ${antwort.version} (${antwort.meilenstein})`;
  } catch (fehler) {
    status.innerHTML =
      `<span class="fehler">✗ Backend nicht erreichbar: ${fehler}</span>`;
  }
}

backendPruefen();

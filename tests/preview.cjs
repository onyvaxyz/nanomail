// Review der echten UI mit ausdrücklich simuliertem Tauri-Backend.
const http = require("node:http");
const fs = require("node:fs/promises");
const path = require("node:path");
const root = path.resolve(__dirname, "../ui");
const server = http.createServer(async (req, res) => {
  try {
    const pathname = new URL(req.url, "http://localhost").pathname;
    const file = pathname === "/tauri-mock.js" ? path.join(__dirname, "tauri-mock.js")
      : path.resolve(root, "." + (pathname === "/" ? "/index.html" : pathname));
    if (file !== path.join(__dirname, "tauri-mock.js") && !file.startsWith(root + path.sep)) throw Error("Pfad");
    let body = await fs.readFile(file);
    const ext = path.extname(file);
    if (ext === ".html") body = body.toString().replace("<head>", '<head><script src="/tauri-mock.js"></script>');
    res.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".woff2": "font/woff2" }[ext] || "application/octet-stream");
    res.end(body);
  } catch { res.writeHead(404); res.end("Nicht gefunden"); }
});
module.exports = server;
if (require.main === module) server.listen(4173, "0.0.0.0", () => process.stdout.write("Nanomail Review mit Testdaten auf Port 4173\n"));

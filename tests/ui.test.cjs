const { test, before, after } = require("node:test");
const assert = require("node:assert/strict");
const { chromium, webkit } = require("playwright");
const server = require("./preview.cjs");
let base;
before(async () => {
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${server.address().port}`;
});
after(() => new Promise(resolve => server.close(resolve)));

for (const [name, engine] of Object.entries({ chromium, webkit })) {
  test(`${name}: Editor, Text-Roundtrip, Kontrast, Theme und Kalender`, async t => {
    const browser = await engine.launch();
    t.after(() => browser.close());
    const page = await browser.newPage({ viewport: { width: 1100, height: 800 }, deviceScaleFactor: 2 });
    const errors = [];
    page.on("pageerror", e => errors.push(e.message));
    for (const query of ["", "?antwortAuf=1", "?antwortAuf=1&allenAntworten=1"]) {
      await page.goto(base + "/verfassen.html" + query);
      await page.waitForSelector("#signatur-block");
      assert.equal(await page.locator("#verfassen-editor > p").count(), 2);
      // Tatsächliche Tastaturbefehle, keine Simulation der entstehenden Tags.
      await page.locator("#verfassen-editor").focus();
      await page.keyboard.press("Control+Home");
      await page.keyboard.type("Erste Zeile");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Zweiter Absatz");
      await page.keyboard.press("Shift+Enter");
      await page.keyboard.type("Einfache Zeile");
      const ergebnis = await page.evaluate(() => {
        const editor = document.getElementById("verfassen-editor");
        const ps = [...editor.querySelectorAll(":scope > p")];
        return { text: editorAlsText(editor), p: ps.map(p => p.innerHTML), abstand: ps[1].offsetTop - ps[0].offsetTop,
          zeilenhoehe: parseFloat(getComputedStyle(ps[0]).lineHeight) };
      });
      assert.match(ergebnis.text, /^Erste Zeile\n\nZweiter Absatz\nEinfache Zeile\n\n-- \nAnna Beispiel/);
      assert.ok(ergebnis.abstand > ergebnis.zeilenhoehe + 5, JSON.stringify(ergebnis));
      assert.match(ergebnis.p[1], /Zweiter Absatz<br>Einfache Zeile/);
      await page.keyboard.press("Control+z");
      assert.ok(!(await page.locator("#verfassen-editor").innerText()).includes("Einfache Zeile"));
    }
    const texte = ["Alpha\n\nBeta\nGamma", "\n\nAnfang", "A\n\n\nB", "A\n\n\n\nB", "A\n", "A\n\n", "<script>x</script>\n& Text", ""];
    for (const text of texte) {
      assert.equal(await page.evaluate(text => {
        const div = document.createElement("div");
        textAlsAbsaetze(div, text);
        return editorAlsText(div);
      }, text), text);
    }
    await page.goto(base + "/verfassen.html?entwurfId=1");
    await page.waitForFunction(() => document.getElementById("verfassen-editor").textContent.includes("Erster"));
    assert.equal(await page.evaluate(() => editorAlsText(document.getElementById("verfassen-editor"))), "Erster Absatz\n\nZweiter Absatz\nmit Zeilenumbruch");
    await page.click("#anhang-knopf");
    assert.equal(await page.evaluate(() => window.testDialog.defaultPath), "/home/review/Desktop");

    // Asymmetrische helle/dunkle und nahe am Umschaltpunkt liegende Farben.
    for (const [farbe, erwartet] of [["#ffff00", "rgb(0, 0, 0)"], ["#111133", "rgb(255, 255, 255)"], ["#777777", "rgb(0, 0, 0)"], ["#666666", "rgb(255, 255, 255)"]]) {
      await page.evaluate(f => document.documentElement.style.setProperty("--akzent", f), farbe);
      await page.waitForFunction(erwartet => getComputedStyle(document.getElementById("senden-knopf")).color === erwartet, erwartet);
    }
    await page.evaluate(() => {
      document.documentElement.style.setProperty("--akzent", "#777777");
      localStorage.setItem("nanomail-thema", "hell");
      window.dispatchEvent(new StorageEvent("storage", { key: "nanomail-thema" }));
    });
    await page.hover("#senden-knopf");
    await page.waitForFunction(() => getComputedStyle(document.getElementById("senden-knopf")).color === "rgb(255, 255, 255)");
    await page.mouse.move(0, 0);
    await page.waitForFunction(() => getComputedStyle(document.getElementById("senden-knopf")).color === "rgb(0, 0, 0)");
    // Transparenz wird gegen den Elternhintergrund komponiert, nicht als Schwarz gelesen.
    await page.evaluate(() => {
      const b = document.createElement("button"); b.id = "kontrast-test";
      b.style.background = "rgba(0,0,0,0.05)"; b.textContent = "Test";
      document.getElementById("verfassen-formular").append(b);
    });
    await page.waitForFunction(() => getComputedStyle(document.getElementById("kontrast-test")).color === "rgb(0, 0, 0)");
    await page.evaluate(() => localStorage.removeItem("nanomail-thema"));
    await page.goto(base + "/index.html");
    await page.click("#thema-knopf"); // system → dunkel
    await page.click("#thema-knopf"); // dunkel → hell
    assert.equal(await page.getAttribute("html", "data-thema"), "hell");
    await page.click("#thema-knopf"); // hell → omarchy
    await page.waitForFunction(() => document.documentElement.dataset.profil === "omarchy");
    assert.equal(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--bg-editor").trim()), "#1a1b26");
    await page.evaluate(() => { window.testOmarchy.farben["--bg-editor"] = "#f0eedd"; window.testOmarchy.hell = true; window.dispatchEvent(new Event("focus")); });
    await page.waitForFunction(() => document.documentElement.dataset.thema === "hell");
    await page.evaluate(() => { window.testOmarchy = null; window.dispatchEvent(new Event("focus")); });
    await page.waitForFunction(() => !document.documentElement.style.getPropertyValue("--bg-editor"));
    assert.equal(await page.getAttribute("html", "data-omarchy"), "false");
    await page.click("#thema-knopf"); // omarchy → system
    assert.equal(await page.getAttribute("html", "data-profil"), "system");

    await page.evaluate(() => {
      const chip = terminChip(window.testTermin, new Date(window.testTermin.beginn * 1000));
      chip.id = "test-termin";
      document.getElementById("lesebereich").append(chip);
    });
    await page.click("#test-termin");
    assert.equal(await page.getAttribute("#termin-popover", "role"), "dialog");
    const box = await page.locator(".popover-beschreibung").boundingBox();
    await page.mouse.move(box.x + 12, box.y + 12);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width + 20, box.y + box.height - 12, { steps: 12 });
    await page.mouse.up();
    assert.ok(await page.locator("#termin-popover").isVisible());
    assert.ok((await page.evaluate(() => getSelection().toString())).length > 10);
    await page.evaluate(() => {
      const text = document.querySelector(".popover-beschreibung");
      const range = document.createRange(); range.selectNodeContents(text);
      getSelection().removeAllRanges(); getSelection().addRange(range);
      text.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      window.dispatchEvent(new Event("blur"));
    });
    assert.ok(await page.locator("#termin-popover").isVisible());
    assert.match(await page.evaluate(() => getSelection().toString()), /Notizen können markiert/);
    assert.equal(await page.locator("#termin-popover a").count(), 3);
    await page.keyboard.press("Escape");
    assert.ok(!(await page.locator("#termin-popover").isVisible()));
    assert.equal(await page.evaluate(() => document.activeElement.id), "test-termin");
    await page.keyboard.press("Enter");
    assert.ok(await page.locator("#termin-popover").isVisible());
    await page.click("#thema-knopf");
    assert.ok(!(await page.locator("#termin-popover").isVisible()));

    await page.goto(base + "/mail.html?mailId=1&kontoId=1");
    await page.waitForSelector("#mail-einladungen button");
    assert.equal(await page.locator("#mail-einladungen button").count(), 2);
    assert.equal(await page.locator("#mail-einladungen a").count(), 3);
    assert.match(await page.locator("#mailfenster-text").textContent(), /Anna,\n\nbitte.*\nVielen/);
    page.once("dialog", d => d.accept());
    await page.getByRole("button", { name: "Zusagen", exact: true }).click();
    await page.waitForFunction(() => document.querySelector("#mail-einladungen [role=status]").textContent.includes("versendet"));
    assert.deepEqual(await page.evaluate(() => window.testAufrufe.find(a => a.command === "mail_einladung_antworten").args),
      { mailId: 1, kalenderIndex: 0, ereignisIndex: 0, zusage: true });
    assert.equal(await page.getAttribute("#mailfenster-html", "sandbox"), "allow-top-navigation-by-user-activation");
    await page.evaluate(() => einladungenAnzeigen(document.getElementById("mail-einladungen"), [{ ...window.testEinladung, antwortbar: false, methode: "CANCEL" }], 1));
    assert.equal(await page.locator("#mail-einladungen button").count(), 0);
    assert.match(await page.locator("#mail-einladungen").textContent(), /Terminabsage/);
    await page.evaluate(() => {
      const ziel = document.getElementById("mail-einladungen");
      ziel.replaceChildren();
      textMitLinks(ziel, '<img src=x onerror=alert(1)> javascript:alert(1) https://example.org/sicher');
    });
    assert.equal(await page.locator("#mail-einladungen img").count(), 0);
    assert.equal(await page.locator("#mail-einladungen a").count(), 1);
    assert.equal(await page.locator("#mail-einladungen a").getAttribute("href"), "https://example.org/sicher");
    assert.deepEqual(errors, []);
  });
}

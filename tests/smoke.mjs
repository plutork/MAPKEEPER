// One-off manual smoke check for the flow-first pass (D-21) — not the
// permanent Playwright suite (see mapkeeper-web-tests skill / README).
import { chromium } from "playwright";

const url = process.argv[2] ?? "http://127.0.0.1:4100";

const browser = await chromium.launch();
const page = await browser.newPage();
const consoleErrors = [];
page.on("console", (msg) => {
  if (msg.type() === "error") consoleErrors.push(msg.text());
});
page.on("pageerror", (err) => consoleErrors.push(String(err)));

await page.goto(url, { waitUntil: "networkidle" });
await page.waitForTimeout(300); // let wasm start() draw the initial grid

const canvas = page.locator("#map");
await canvas.click({ position: { x: 360, y: 280 } }); // center hex q0,r0

await page.waitForSelector("#panel.open", { timeout: 2000 });
await page.fill("#title", "Old mill");
await page.fill("#notes", "Grinds grain for the village");
await page.click("#save");
await page.waitForFunction(() => document.getElementById("status")?.textContent === "Saved.", { timeout: 3000 });

await browser.close();

if (consoleErrors.length) {
  console.error("Console errors during smoke run:", consoleErrors);
  process.exit(1);
}
console.log("SMOKE OK: clicked hex, saved placeholder profile, no console errors.");

// One-off manual smoke check for the flow-first + launcher passes (D-21,
// D-12/5.7) — not the permanent Playwright suite (see mapkeeper-web-tests
// skill / README).
//
// Usage: node smoke.mjs [serverUrl] [worldId] [worldPath]
// Headless: default on (explicit). Set SMOKE_HEADED=1 for local headed debug.
// Env: SMOKE_HEADLESS=1 (default behavior), SMOKE_HEADED=1 to show browser window.
import { chromium } from "playwright";

const url = process.argv[2] ?? "http://127.0.0.1:4100";
const worldId = process.argv[3] ?? "smoke-world";
const worldPath = process.argv[4] ?? "C:\\projects\\smoke-world";

const headless = process.env.SMOKE_HEADED !== "1";

const browser = await chromium.launch({ headless });
const page = await browser.newPage();
const consoleErrors = [];
page.on("console", (msg) => {
  if (msg.type() === "error") consoleErrors.push(msg.text());
});
page.on("pageerror", (err) => consoleErrors.push(String(err)));

await page.goto(url, { waitUntil: "networkidle" });
await page.waitForTimeout(300); // let wasm start() run + fetch /api/projects

const homeActive = await page.locator("#home.active").count();
if (homeActive) {
  await page.fill("#new-id", worldId);
  await page.fill("#new-path", worldPath);
  await page.click("#create");
  await page.waitForSelector("#editor.active", { timeout: 3000 });
} else {
  console.log("Home not shown (server started with --world) — skipping wizard step.");
}

await page.waitForSelector("#map", { state: "visible", timeout: 2000 });
const canvas = page.locator("#map");
await canvas.click({ position: { x: 360, y: 280 } }); // center hex q0,r0

await page.waitForSelector("#dock-drawer:not(.collapsed)", { timeout: 2000 });
await page.waitForFunction(() => !document.getElementById("title")?.disabled, { timeout: 3000 });
await page.fill("#title", "Old mill");
await page.fill("#notes", "Grinds grain for the village");
await page.click("#save");
await page.waitForFunction(() => document.getElementById("status")?.textContent === "Saved.", { timeout: 3000 });

if (homeActive) {
  // ui-shell-redesign Track 1: ← Worlds lives in the top bar (#wiz-worlds);
  // the World drawer moved into Settings ▾ and no longer duplicates navigation.
  await page.click("#wiz-worlds");
  await page.waitForSelector("#home.active", { timeout: 3000 });
  await page.locator("#project-list .id", { hasText: worldId }).waitFor({ timeout: 3000 });
}

await browser.close();

if (consoleErrors.length) {
  console.error("Console errors during smoke run:", consoleErrors);
  process.exit(1);
}
console.log("SMOKE OK: launcher create -> hex map -> saved profile -> back to Home list, no console errors.");

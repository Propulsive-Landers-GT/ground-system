#!/usr/bin/env node
// End-to-end smoke test against a bridge that starts in Standby (e.g. mock-bridge without --auto).
//   node scripts/smoke.mjs [url] [outDir]
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";
const url = process.argv[2] ?? "http://127.0.0.1:5173/";
const out = process.argv[3] ?? "screenshots";
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(e.message));
const results = [];
const check = (name, ok, extra = "") => { results.push({ name, ok }); console.log(`${ok ? "PASS" : "FAIL"}  ${name} ${extra}`); };
const log = () => page.locator(".commands .cmd-row").allInnerTexts();
// textContent, not innerText: the stepper is styled uppercase and innerText returns the transformed text.
const phase = () => page.locator('.step[aria-current="step"] .step-name').textContent();
const btn = (name) => page.getByRole("button", { name, exact: true });

await page.goto(url);
await page.waitForSelector('.step[aria-current="step"]', { timeout: 10000 });
check("starts in Standby", (await phase()) === "Standby");
check("launch disabled in Standby", (await page.locator(".launch").getAttribute("aria-disabled")) === "true");

// Test stand: open a valve
await page.keyboard.press("2");
await page.locator(".valvetable tbody tr", { hasText: "O-ISO" }).getByRole("button", { name: "Open" }).click();
await page.waitForTimeout(600);
check("valve command accepted", (await log())[0].includes("accepted"), JSON.stringify((await log())[0]));
check("valve shows open", (await page.locator(".valvetable tbody tr", { hasText: "O-ISO" }).locator(".chip").innerText()).includes("open"));
// Only the Node mock omits RCS2 and LF-PT; against other bridges these are informational.
const missingValve = (await page.locator(".valvetable tbody tr", { hasText: "RCS2" }).locator(".chip").innerText()).includes("—");
const missingCh = ((await page.locator(".tag", { hasText: "LF-PT" }).textContent()) ?? "").includes("—");
console.log(`INFO  RCS2 ${missingValve ? "missing -> dash" : "present"}, LF-PT ${missingCh ? "missing -> dash" : "present"}`);
await page.screenshot({ path: `${out}/stand-standby.png` });

// Jog
await page.keyboard.press("1");
await btn("Jog").first().click();
await page.locator(".jog .seg-btn", { hasText: "Jog" }).click();
await page.waitForTimeout(500);
check("jog mode active", (await page.locator(".phasebar .mode").innerText()) === "JOG");
await page.locator('.jog input[type="range"]').first().fill("9");
await page.waitForTimeout(700);
check("gimbal follows jog", (await page.locator(".actuation .mini dd").first().innerText()).includes("+9.0"), await page.locator(".actuation .mini dd").first().innerText());
check("arm blocked while jogging", (await btn("Arm").getAttribute("aria-disabled")) === "true");
await page.screenshot({ path: `${out}/flight-jog-active.png` });
await page.locator(".jog .seg-btn", { hasText: "Auto" }).click();
await page.waitForTimeout(400);
await page.keyboard.press("Escape");

// Arm, then hold-to-launch: a short press must not fire, a full hold must
await btn("Arm").click();
await page.waitForTimeout(500);
check("armed", (await phase()) === "Armed");
const launch = page.locator(".launch");
const box = await launch.boundingBox();
await page.mouse.move(box.x + 20, box.y + 10); await page.mouse.down(); await page.waitForTimeout(300); await page.mouse.up();
await page.waitForTimeout(300);
check("short press does not launch", (await phase()) === "Armed");
await launch.focus();
await page.keyboard.down("Space"); await page.waitForTimeout(1200); await page.keyboard.up("Space");
await page.waitForTimeout(500);
check("keyboard hold launches", (await phase()) === "Ascent");

// A rejected command shows its reason
if (await page.evaluate(() => !!window.__gs)) {
  await page.evaluate(() => window.__gs.sendCommand("Arm"));
  await page.waitForTimeout(500);
  check("rejection shows reason", (await log())[0].includes("rejected"), JSON.stringify((await log())[0]));
} else console.log("INFO  production build: skipping forced-rejection check (dev-only hook)");

await page.waitForTimeout(4000);
await btn("Hold hover").click();
await page.waitForTimeout(600);
check("hold hover override", (await phase()) === "Hover");
await page.waitForTimeout(2500);
await page.screenshot({ path: `${out}/flight-pad-cam-pre.png` });
await btn("Pad").click();
await page.waitForTimeout(1500);
await page.screenshot({ path: `${out}/flight-pad-cam.png` });
await btn("Top-down").click();
await page.waitForTimeout(1500);
await page.screenshot({ path: `${out}/flight-top-cam.png` });

// Abort by pointer hold
const abort = page.locator(".abort");
const ab = await abort.boundingBox();
await page.mouse.move(ab.x + 30, ab.y + 20); await page.mouse.down(); await page.waitForTimeout(1250); await page.mouse.up();
await page.waitForTimeout(800);
check("abort shows termination banner", await page.locator(".termination[data-on]").isVisible());
await page.screenshot({ path: `${out}/flight-terminated.png` });
console.log((await log()).slice(0, 8).join("\n").replace(/\n+/g, " | "));

check("no console errors", errors.length === 0, errors.join("; "));
await browser.close();
process.exit(results.every((r) => r.ok) ? 0 : 1);

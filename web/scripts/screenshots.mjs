#!/usr/bin/env node
// Loads the app in headless Chromium at 1280x800, waits for data, and saves screenshots of
// both views. Fails if the browser console reports errors.
//   node scripts/screenshots.mjs [url] [outDir] [--wait seconds]
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";

const pos = process.argv.slice(2).filter((a, i, all) => !a.startsWith("--") && all[i - 1] !== "--wait");
const url = pos[0] ?? "http://127.0.0.1:5173/";
const out = pos[1] ?? "screenshots";
const wi = process.argv.indexOf("--wait");
const waitS = wi > 0 ? Number(process.argv[wi + 1]) : 14;
mkdirSync(out, { recursive: true });

const browser = await chromium.launch({ args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
const problems = [];
page.on("console", (m) => { if (m.type() === "error" || m.type() === "warning") problems.push(`[${m.type()}] ${m.text()}`); });
page.on("pageerror", (e) => problems.push(`[pageerror] ${e.message}`));

await page.goto(url);
await page.waitForTimeout(waitS * 1000);
const shot = async (name) => { await page.screenshot({ path: `${out}/${name}.png` }); console.log(`saved ${out}/${name}.png`); };

await shot("flight-dark");
const scroll = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, sh: document.documentElement.scrollHeight, w: innerWidth, h: innerHeight }));
console.log("page scroll size", scroll);

await page.keyboard.press("2");
await page.waitForTimeout(1500);
await shot("stand-dark");

await page.keyboard.press("1");
await page.getByRole("button", { name: "Tuning" }).click();
await page.waitForTimeout(500);
await shot("flight-tuning");
await page.getByRole("button", { name: "Jog", exact: true }).click();
await page.waitForTimeout(500);
await shot("flight-jog");
await page.keyboard.press("Escape");

await page.getByRole("button", { name: "Light", exact: true }).click();
await page.waitForTimeout(1200);
await shot("flight-light");
await page.keyboard.press("2");
await page.waitForTimeout(1200);
await shot("stand-light");

console.log(problems.length ? `console problems:\n${problems.join("\n")}` : "console clean");
await browser.close();
process.exit(problems.some((p) => !p.startsWith("[warning]")) ? 1 : 0);

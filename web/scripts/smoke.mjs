#!/usr/bin/env node
// End-to-end smoke test against a bridge that starts in Standby (e.g. mock-bridge without --auto).
// With a test stand on the link (mock-bridge --stand, or gs-bridge --stand) it also drives the stand:
// arm, valve interlock, MTV, igniter, a sequence, stand abort, and the recording control.
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
const hold = async (loc, ms = 1250) => {
  const b = await loc.boundingBox();
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2); await page.mouse.down(); await page.waitForTimeout(ms); await page.mouse.up();
};
const standMode = () => page.locator(".standstrip .stand-mode").getAttribute("data-mode");
const valveRow = (name) => page.locator(".valvetable tbody tr", { hasText: name });
const noScroll = async (name) => {
  const s = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, sh: document.documentElement.scrollHeight }));
  check(`no page scroll: ${name}`, s.sw <= 1280 && s.sh <= 800, JSON.stringify(s));
};

await page.goto(url);
await page.waitForSelector('.step[aria-current="step"]', { timeout: 10000 });
check("starts in Standby", (await phase()) === "Standby");
check("launch disabled in Standby", (await page.locator(".launch").getAttribute("aria-disabled")) === "true");
const devHook = await page.evaluate(() => !!window.__gs);
if (!devHook) console.log("INFO  production build: forced-rejection checks need the dev-only hook and are skipped");

// ---------------------------------------------------------------- recording control (header)
{
  const wasRecording = await page.locator(".recording[data-on]").count();
  if (wasRecording) {
    await btn("Stop").click();
    await page.waitForTimeout(700);
  }
  check("idle shows Record button", await page.locator(".rec-btn").isVisible());
  if (devHook) {
    await page.evaluate(() => window.__gs.sendControl({ control: "stop_recording" }));
    await page.waitForTimeout(500);
    check("stop while idle surfaces bridge error", (await page.locator(".rec-error").count()) > 0, await page.locator(".rec-error").allInnerTexts().then((t) => t.join()));
  }
  await page.locator(".rec-btn").click();
  await page.locator(".rec-input").fill("smoke-test");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(800);
  check("recording starts", await page.locator(".recording[data-on]").isVisible());
  check("REC indicator shows", (await page.locator(".rec-word").innerText()) === "REC");
  check("recording error cleared", (await page.locator(".rec-error").count()) === 0);
}

// ---------------------------------------------------------------- test stand
await page.keyboard.press("2");
await page.waitForTimeout(800);
const standConfigured = (await page.locator(".standstrip .stand-unconfigured").count()) === 0;
console.log(`INFO  test stand ${standConfigured ? "configured" : "not configured (vehicle valve rule)"}`);

if (!standConfigured) {
  // Vehicle checkout rule: valves in Standby.
  await valveRow("O-ISO").getByRole("button", { name: "Open" }).click();
  await page.waitForTimeout(600);
  check("valve command accepted", (await log())[0].includes("accepted"), JSON.stringify((await log())[0]));
  check("valve shows open", (await valveRow("O-ISO").locator(".chip").innerText()).includes("open"));
  // Only the Node mock omits RCS2 and LF-PT; against other bridges these are informational.
  const missingValve = (await valveRow("RCS2").locator(".chip").innerText()).includes("—");
  const missingCh = ((await page.locator(".tag", { hasText: "LF-PT" }).textContent()) ?? "").includes("—");
  console.log(`INFO  RCS2 ${missingValve ? "missing -> dash" : "present"}, LF-PT ${missingCh ? "missing -> dash" : "present"}`);
  check("stand commands report no stand", (await page.locator(".standcmd .title-note").innerText()).includes("no stand configured"));
  check("stand abort disabled without a stand", (await page.locator(".abort-stand").getAttribute("aria-disabled")) === "true");
  await noScroll("stand view");
  await page.screenshot({ path: `${out}/stand-standby.png` });
} else {
  await page.waitForFunction(() => document.querySelector(".standstrip .stand-mode")?.dataset.mode !== "none", null, { timeout: 5000 });
  if ((await standMode()) !== "Safe") {
    await hold(page.locator(".abort-stand"));
    await page.waitForTimeout(800);
  }
  check("stand starts Safe", (await standMode()) === "Safe", await standMode());
  check("STAND badge in header", await page.locator('.source[data-source="Stand"]').isVisible());
  check("valve controls locked while Safe", (await valveRow("OMV").getByRole("button", { name: "Open" }).getAttribute("aria-disabled")) === "true");
  check("valve lock has a reason", ((await valveRow("OMV").getByRole("button", { name: "Open" }).getAttribute("title")) ?? "").length > 0);
  check("start sequence locked while Safe", (await page.locator(".start-seq").getAttribute("aria-disabled")) === "true");
  check("stand abort enabled while Safe", (await page.locator(".abort-stand").getAttribute("aria-disabled")) !== "true");

  if (devHook) {
    await page.evaluate(() => window.__gs.sendCommand({ SetValve: { id: "Omv", open: true } }));
    await page.waitForTimeout(600);
    const row = (await log())[0];
    check("OMV open before Arm rejected with reason", row.includes("rejected") && /arm|safe/i.test(row), JSON.stringify(row));
    check("rejection announced (aria-live)", ((await page.locator(".commands [role=status]").textContent()) ?? "").includes("rejected"));
  }

  await btn("Arm").click();
  await page.waitForTimeout(800);
  check("stand Arm accepted", (await log())[0].includes("Stand arm") && (await log())[0].includes("accepted"), JSON.stringify((await log())[0]));
  check("mode badge ARMED", (await standMode()) === "Armed");
  check("valve controls enabled when Armed", (await valveRow("OMV").getByRole("button", { name: "Open" }).getAttribute("aria-disabled")) !== "true");

  await valveRow("OMV").getByRole("button", { name: "Open" }).click();
  await page.waitForTimeout(600);
  check("OMV opens when Armed", (await valveRow("OMV").locator(".chip").innerText()).includes("open"));
  await valveRow("OMV").getByRole("button", { name: "Close" }).click();
  await page.waitForTimeout(400);

  // MTV 20 % via quick button: telemetry round-trips into the P&ID tag, the table and the readout.
  await btn("20 %").click();
  await page.waitForTimeout(800);
  const mtvTag = await page.locator(".pid .valve", { hasText: "MTV" }).locator(".valve-state").textContent();
  check("MTV 20 % round-trips into P&ID tag", (mtvTag ?? "").includes("20 %"), mtvTag ?? "");
  check("MTV 20 % in valve table", (await valveRow("MTV").locator(".numcol").innerText()).includes("20"));
  check("MTV commanded readout 20", (await page.locator(".mtv-tele .val").innerText()).trim() === "20");
  // Numeric entry + Enter also sends.
  await page.locator(".mtv-entry .winput").fill("35");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(800);
  check("MTV entry sends on Enter", (await page.locator(".mtv-tele .val").innerText()).trim() === "35");
  await btn("0 %").click();
  await page.waitForTimeout(600);
  await noScroll("stand armed");
  await page.screenshot({ path: `${out}/stand-armed.png` });

  // Igniter: a short press must not fire, a full hold must.
  const fire = page.locator(".fire");
  await hold(fire, 300);
  await page.waitForTimeout(400);
  check("short press does not fire igniter", (await page.locator(".lamp-fire[data-on]").count()) === 0);
  await hold(fire);
  await page.waitForTimeout(700);
  check("igniter hold fires", (await page.locator(".lamp-fire[data-on]").count()) === 1);
  check("P&ID igniter lamp on", (await page.locator(".ig-lamp[data-on]").count()) === 1);
  await btn("Igniter off").click();
  await page.waitForTimeout(600);
  check("igniter off", (await page.locator(".lamp-fire[data-on]").count()) === 0);

  // DAQ sync toggle: the switch shows the stand's reported state, so it flips when the telemetry does.
  // Abort leaves the DAQ line alone, so its starting state depends on earlier runs.
  const daq = page.locator(".daq-switch input");
  const daq0 = await daq.isChecked();
  await daq.click();
  await page.waitForTimeout(700);
  check(`DAQ sync toggles ${daq0 ? "off" : "on"}`, (await daq.isChecked()) === !daq0);
  await daq.click();
  await page.waitForTimeout(700);
  check(`DAQ sync toggles back ${daq0 ? "on" : "off"}`, (await daq.isChecked()) === daq0);

  // Sequence: keyboard hold to start, T+ clock, controls locked, then abort.
  const start = page.locator(".start-seq");
  const seqName = await page.locator(".standcmd select").inputValue();
  await start.focus();
  await page.keyboard.down("Space"); await page.waitForTimeout(1250); await page.keyboard.up("Space");
  await page.waitForTimeout(900);
  check("StartSequence accepted", (await log())[0].includes("Start sequence") && (await log())[0].includes("accepted"), JSON.stringify((await log())[0]));
  check("mode badge SEQUENCE", (await standMode()) === "Sequence");
  check("T+ clock shows", /^T[+−]\d/.test((await page.locator(".standstrip .seq-clock").innerText()).trim()), await page.locator(".standstrip .seq-clock").innerText());
  check("progress bar present", (await page.locator(".standstrip .seq-bar[role=progressbar]").count()) === 1);
  check("valve controls locked in sequence", (await valveRow("OMV").getByRole("button", { name: "Open" }).getAttribute("aria-disabled")) === "true");
  check("MTV locked in sequence", (await btn("50 %").getAttribute("aria-disabled")) === "true");
  check("igniter locked in sequence", (await fire.getAttribute("aria-disabled")) === "true");
  check("stand abort enabled in sequence", (await page.locator(".abort-stand").getAttribute("aria-disabled")) !== "true");
  await page.waitForTimeout(5500);
  check("sequence-step event legible", (await page.locator(".events :is(.ev-tplus, .ev-step)").count()) > 0);
  await noScroll("stand sequence");
  await page.screenshot({ path: `${out}/stand-sequence.png` });

  await hold(page.locator(".abort-stand"));
  await page.waitForTimeout(900);
  check("Stand abort accepted", (await log())[0].includes("Stand abort") && (await log())[0].includes("accepted"), JSON.stringify((await log())[0]));
  check("Stand Abort returns to SAFE", (await standMode()) === "Safe", await standMode());
  check("sequence progress gone", (await page.locator(".standstrip .seq-clock").count()) === 0);
  await page.screenshot({ path: `${out}/stand-aborted.png` });
  console.log(`INFO  sequence used: ${seqName}`);
}

// Stop the recording started above.
await btn("Stop").click();
await page.waitForTimeout(700);
check("recording stops", await page.locator(".rec-btn").isVisible());

// ---------------------------------------------------------------- flight (unchanged)
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
check("armed without recording shows NOT RECORDING", (await page.locator(".recording[data-armed]").count()) === 1);
const launch = page.locator(".launch");
await hold(launch, 300);
await page.waitForTimeout(300);
check("short press does not launch", (await phase()) === "Armed");
await launch.focus();
await page.keyboard.down("Space"); await page.waitForTimeout(1200); await page.keyboard.up("Space");
await page.waitForTimeout(500);
check("keyboard hold launches", (await phase()) === "Ascent");

// A rejected command shows its reason
if (devHook) {
  await page.evaluate(() => window.__gs.sendCommand("Arm"));
  await page.waitForTimeout(500);
  check("rejection shows reason", (await log())[0].includes("rejected"), JSON.stringify((await log())[0]));
}

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

// Abort by pointer hold (flight abort lives only on the Flight tab)
check("flight abort only on Flight tab", (await page.locator(".abort:not(.abort-stand)").count()) === 1 && (await page.locator(".abort-stand").count()) === 0);
await hold(page.locator(".abort"));
await page.waitForTimeout(800);
check("abort shows termination banner", await page.locator(".termination[data-on]").isVisible());
await page.screenshot({ path: `${out}/flight-terminated.png` });
console.log((await log()).slice(0, 8).join("\n").replace(/\n+/g, " | "));

check("no console errors", errors.length === 0, errors.join("; "));
await browser.close();
process.exit(results.every((r) => r.ok) ? 0 : 1);

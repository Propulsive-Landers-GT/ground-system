#!/usr/bin/env node
// Stand-in for gs-bridge + a vehicle, for UI work without the Rust side.
// Implements the WebSocket API in docs/DESIGN.md on ws://127.0.0.1:8080/ws:
// flight 50 Hz, stand 20 Hz, trajectory 1 Hz, link 2 Hz, events, sent echo, acks with the
// vehicle's interlock rules, params, jog with a 0.5 s deadman.
//
//   node scripts/mock-bridge.mjs [--port 8080] [--auto] [--source Sim|Vehicle|Replay] [--drop-acks] [--stand] [--record]
//   --auto       arms and launches by itself, and relaunches after landing
//   --drop-acks  never ack (to see the "no ack" state)
//   --stand      also pretend a gs-stand is on the link: `stand` telemetry with source "Stand" (valves, MTV,
//                igniter, load cells), `stand_status` at 5 Hz, the stand interlocks, three ~20 s sequences,
//                and `link.stand`. Without it `link.stand` is null and the sim's own stand telemetry flows.
//   --record     start with a recording open (like `gs-bridge --record`)
// Recording control messages ({"control":"start_recording"|"stop_recording"}) toggle link.recording.

import { WebSocketServer } from "ws";

const args = process.argv.slice(2);
const flag = (n) => args.includes(n);
const opt = (n, d) => (args.includes(n) ? args[args.indexOf(n) + 1] : d);
const PORT = Number(opt("--port", 8080));
const AUTO = flag("--auto");
const SOURCE = opt("--source", "Sim");
const DROP_ACKS = flag("--drop-acks");
const STAND = flag("--stand");
const STAND_ADDR = "127.0.0.1:18889";

const wss = new WebSocketServer({ port: PORT, path: "/ws" });
const send = (ws, type, data) => ws.readyState === 1 && ws.send(JSON.stringify({ type, data }));
const broadcast = (type, data) => wss.clients.forEach((c) => send(c, type, data));

const VALVES = ["Omv", "Mtv", "IgV", "OFill", "OIso", "OVnt", "PuMv", "PuFill", "PuIso", "PuVnt", "PuMvnt", "LfVnt", "TVnt", "Rcs1", "Rcs2"];
const DRY = 50;

const s = {
  t: 0, seq: 0, upSeq: 0, phase: "Standby", phaseT: 0, mode: "Auto", terminated: false,
  pos: [0, 0, 0], vel: [0, 0, 0], tilt: [0, 0], mass: 74, thrust: 0, gth: 0, gph: 0, rcs: 0, yaw: 0,
  jog: null, jogAt: -1,
  params: { flight: { hover_altitude_m: 50, hover_duration_s: 10, max_tilt_deg: 30, max_trajectory_deviation_m: 10 }, manual_mpc_weights: null },
  valves: Object.fromEntries(VALVES.map((v) => [v, { state: "Closed", pos: v === "Mtv" ? 0 : null }])),
  events: [], traj: null, tank: 52, autoT: 0,
};
s.valves.PuVnt.state = "Open"; // normally-open vent
s.valves.LfVnt.state = "Unknown";

// ---------------------------------------------------------------- test stand (--stand)
// Mirrors the "Test-stand rules enforced by gs-stand" table in docs/DESIGN.md.
const SAFING_CLOSE = ["Omv", "IgV", "OFill", "PuMv", "PuIso", "PuFill"];
const SAFING_OPEN = ["OVnt", "PuVnt", "LfVnt"];
const OISO_STROKE_S = 21;
const SEQUENCES = {
  hotfire: [
    [0, "DAQ sync on", () => (st.daq = true)],
    [2, "OMV open", () => setStandValve("Omv", true)],
    [3, "igniter on", () => (st.igniter = true)],
    [5, "MTV 20 %", () => (st.mtv = 20)],
    [8, "MTV 50 %", () => (st.mtv = 50)],
    [11, "MTV 100 %", () => (st.mtv = 100)],
    [16, "MTV 0 %", () => (st.mtv = 0)],
    [17, "OMV close", () => setStandValve("Omv", false)],
    [17.5, "igniter off", () => (st.igniter = false)],
    [20, "DAQ sync off", () => (st.daq = false)],
  ],
  coldflow: [
    [0, "DAQ sync on", () => (st.daq = true)],
    [2, "OMV open", () => setStandValve("Omv", true)],
    [3, "MTV 30 %", () => (st.mtv = 30)],
    [8, "MTV 0 %", () => (st.mtv = 0)],
    [9, "OMV close", () => setStandValve("Omv", false)],
    [12, "DAQ sync off", () => (st.daq = false)],
  ],
  rcs: [
    [0, "RCS1 open", () => setStandValve("Rcs1", true)],
    [2, "RCS1 close", () => setStandValve("Rcs1", false)],
    [3, "RCS2 open", () => setStandValve("Rcs2", true)],
    [5, "RCS2 close", () => setStandValve("Rcs2", false)],
    [8, "done", () => {}],
  ],
};
const st = {
  mode: "Safe", actuationOk: true, loadcellOk: true,
  valves: Object.fromEntries(VALVES.map((v) => [v, { state: "Unknown", pos: v === "Mtv" ? 0 : null }])),
  oisoUnknownUntil: -1,
  mtv: 0, igniter: false, daq: false,
  seq: null, // { name, t0, next }
  thrust: 0, n2o: 14.2, rcsThrust: 0,
  flapT: 0,
};
function setStandValve(id, open) {
  const v = st.valves[id];
  v.state = open ? "Open" : "Closed";
  if (id === "Mtv") v.pos = open ? 90 : 0;
  // OISO is motorized: no feedback for one full stroke.
  if (id === "OIso") st.oisoUnknownUntil = s.t + OISO_STROKE_S;
}
function safingList() {
  for (const id of SAFING_CLOSE) setStandValve(id, false);
  for (const id of SAFING_OPEN) setStandValve(id, true);
  st.igniter = false;
  st.mtv = 0;
}
function standAbort(reason) {
  const wasSeq = st.seq;
  st.seq = null;
  safingList();
  st.mode = "Safe";
  event(wasSeq ? "Critical" : "Warning", `Stand abort: ${reason}${wasSeq ? ` (sequence ${wasSeq.name} ended)` : ""}`);
}
function handleStand(cmd) {
  const rej = (why) => ({ Rejected: why });
  const name = typeof cmd === "string" ? cmd : Object.keys(cmd)[0];
  const arg = typeof cmd === "string" ? null : cmd[name];
  const inSeq = st.mode === "Sequence";
  switch (name) {
    case "Arm":
      if (st.mode !== "Safe") return rej(inSeq ? "sequence running: only Abort" : "stand is already Armed");
      if (!st.actuationOk) return rej("actuation Arduino link down");
      if (!st.loadcellOk) return rej("load-cell Arduino link down");
      st.mode = "Armed"; event("Info", "Stand armed"); return "Accepted";
    case "Disarm":
      if (inSeq) return rej("sequence running: use Abort");
      safingList(); st.mode = "Safe"; event("Info", "Stand disarmed: safing list run"); return "Accepted";
    case "Abort":
      standAbort("operator"); return "Accepted";
    case "SetMtvPercent":
      if (st.mode !== "Armed") return rej(inSeq ? "sequence running: only Abort" : "stand not armed (Safe)");
      if (!(arg >= 0 && arg <= 100)) return rej("percent out of range 0-100");
      st.mtv = arg; return "Accepted";
    case "SetOutput":
      if (arg.id === "DaqSync") {
        if (inSeq) return rej("sequence running: only Abort");
        st.daq = !!arg.on; return "Accepted";
      }
      if (arg.id === "Igniter") {
        if (st.mode !== "Armed") return rej(inSeq ? "sequence running: only Abort" : "igniter needs stand Armed (Safe)");
        st.igniter = !!arg.on; event(arg.on ? "Warning" : "Info", `Igniter ${arg.on ? "ON" : "off"} (manual)`); return "Accepted";
      }
      return rej(`unknown output ${arg.id}`);
    case "StartSequence":
      if (st.mode !== "Armed") return rej(inSeq ? "sequence already running" : "stand not armed (Safe)");
      if (!SEQUENCES[arg]) return rej(`unknown sequence "${arg}"`);
      st.seq = { name: arg, t0: s.t, next: 0 };
      st.mode = "Sequence";
      event("Info", `Sequence ${arg} started (${SEQUENCES[arg].length} steps, ${seqDuration(arg)} s)`);
      return "Accepted";
    default:
      return rej(`unknown stand command ${name}`);
  }
}
const seqDuration = (name) => SEQUENCES[name][SEQUENCES[name].length - 1][0];
function stepStand(dt) {
  const seq = st.seq;
  if (seq) {
    const steps = SEQUENCES[seq.name];
    const t = s.t - seq.t0;
    while (seq.next < steps.length && t >= steps[seq.next][0]) {
      const [at, text, fn] = steps[seq.next];
      fn();
      event("Info", `T+${at.toFixed(1)} ${text}`);
      seq.next++;
    }
    if (seq.next >= steps.length) {
      st.seq = null;
      st.mode = "Armed";
      event("Info", `Sequence ${seq.name} complete; stand Armed`);
    }
  }
  // Physics-ish: thrust follows MTV while the main is open and the igniter has lit it.
  const flowing = st.valves.Omv.state === "Open" && st.mtv > 0;
  const burning = flowing && (st.igniter || st.thrust > 100);
  const target = burning ? 1100 * (st.mtv / 100) : flowing ? 40 : 0;
  st.thrust += (target - st.thrust) * Math.min(1, dt * 6);
  if (flowing) st.n2o = Math.max(0, st.n2o - 0.35 * (st.mtv / 100) * dt);
  const rcsOn = st.valves.Rcs1.state === "Open" || st.valves.Rcs2.state === "Open";
  st.rcsThrust += ((rcsOn ? 16 : 0) - st.rcsThrust) * Math.min(1, dt * 8);
  // The load-cell Arduino drops out now and then so the lamp and the Arm interlock get exercised.
  st.flapT += dt;
  st.loadcellOk = !(st.flapT % 90 > 84 && st.mode === "Safe");
}
function standStatus() {
  const seq = st.seq;
  let progress = null;
  if (seq) {
    const steps = SEQUENCES[seq.name];
    const n = steps[seq.next];
    progress = {
      name: seq.name, t_s: s.t - seq.t0, duration_s: seqDuration(seq.name),
      next_step: n ? [seq.next, `T+${n[0].toFixed(1)} ${n[1]}`] : null, steps_total: steps.length,
    };
  }
  return {
    time_s: s.t, mode: st.mode, actuation_link_ok: st.actuationOk, loadcell_link_ok: st.loadcellOk,
    sequences: Object.keys(SEQUENCES), sequence: progress,
  };
}
function standTelemetry() {
  const tankP = 48 + (st.n2o - 14.2) * 1.5 + noise(0.05);
  const pc = st.thrust / 55;
  const channels = [
    ["Opt", tankP], ["Ipt", st.thrust > 50 ? pc * 1.25 + noise(0.1) : 1.0 + noise(0.02)], ["Ept", pc + 1.0 + noise(0.08)],
    ["M1", st.valves.Omv.state === "Open" ? tankP - 1.5 + noise(0.1) : 1.0], ["M2", st.thrust > 50 ? pc * 1.4 + noise(0.1) : 1.0],
    ["Pupt", st.valves.PuIso.state === "Open" ? 60 + noise(0.2) : 1.0 + noise(0.02)],
    ["Lfpt", st.valves.IgV.state === "Open" ? 6 + noise(0.05) : 1.0 + noise(0.02)],
    ["T1", 18 + noise(0.05)], ["T2", st.thrust > 50 ? 240 + st.thrust / 6 + noise(2) : 24 + noise(0.2)],
    ["Thrust", st.thrust + noise(3)], ["NitrousMass", st.n2o + noise(0.01)], ["RcsThrust", st.rcsThrust + noise(0.3)],
  ];
  const outputs = [];
  if (st.igniter) outputs.push("Igniter");
  if (st.daq) outputs.push("DaqSync");
  return {
    time_s: s.t, source: "Stand", channels,
    valves: VALVES.map((id) => ({
      id,
      // The MTV is driven by percent, so its state follows the commanded opening.
      state: id === "OIso" && s.t < st.oisoUnknownUntil ? "Unknown" : id === "Mtv" ? (st.mtv > 0 ? "Open" : "Closed") : st.valves[id].state,
      position_deg: id === "Mtv" ? st.mtv * 0.9 : st.valves[id].pos,
    })),
    outputs_on: outputs, mtv_percent: st.mtv,
  };
}

// ---------------------------------------------------------------- recording (control messages)
let recording = flag("--record") ? recDir("") : null;
function recDir(name) {
  const stamp = new Date().toISOString().replace(/[-:]/g, "").replace(/\.\d+Z$/, "Z");
  const clean = String(name ?? "").replace(/[^A-Za-z0-9._-]/g, "");
  return `logs/${stamp}${clean ? `-${clean}` : ""}`;
}
function handleControl(ws, msg) {
  const err = (message) => send(ws, "error", { message });
  if (msg.control === "start_recording") {
    if (recording) return err(`already recording to ${recording}`);
    recording = recDir(msg.name);
    event("Info", `Recording started: ${recording}`);
  } else if (msg.control === "stop_recording") {
    if (!recording) return err("not recording");
    event("Info", `Recording stopped: ${recording}`);
    recording = null;
  } else return err(`unknown control ${msg.control}`);
  broadcast("link", linkStatus());
}

function event(severity, text) {
  const e = { time_s: s.t, severity, text };
  s.events.push(e);
  if (s.events.length > 200) s.events.shift();
  broadcast("event", e);
}
function setPhase(p) {
  if (s.phase === p) return;
  s.phase = p; s.phaseT = 0; s.mode = "Auto";
  event("Info", `Phase -> ${p}`);
  if (p === "Ascent" || p === "Descent") makeTrajectory();
}
function makeTrajectory() {
  const h = s.params.flight.hover_altitude_m;
  const up = s.phase !== "Descent";
  const from = [...s.pos];
  const to = up ? [0, 0, h] : [0, 0, 0];
  const tof = Math.max(4, Math.abs(to[2] - from[2]) / 4.5 + 3);
  const n = 48;
  const positions = [];
  for (let i = 0; i < n; i++) {
    const u = i / (n - 1);
    const e = u * u * (3 - 2 * u);
    positions.push([from[0] + (to[0] - from[0]) * e, from[1] + (to[1] - from[1]) * e, from[2] + (to[2] - from[2]) * e]);
  }
  s.traj = { generated_at_s: s.t, time_of_flight_s: tof, positions, target: to };
  broadcast("trajectory", s.traj);
}
function refAt(t) {
  const tr = s.traj;
  if (!tr) return null;
  const u = Math.min(1, Math.max(0, (t - tr.generated_at_s) / tr.time_of_flight_s));
  const x = u * (tr.positions.length - 1);
  const i = Math.min(Math.floor(x), tr.positions.length - 2);
  const f = x - i;
  return tr.positions[i].map((v, k) => v * (1 - f) + tr.positions[i + 1][k] * f);
}

function handle(kind) {
  const name = typeof kind === "string" ? kind : Object.keys(kind)[0];
  const arg = typeof kind === "string" ? null : kind[name];
  const rej = (why) => ({ Rejected: why });
  switch (name) {
    case "Arm":
      if (s.phase !== "Standby") return rej(`cannot arm in ${s.phase}`);
      if (s.mode !== "Auto") return rej("control mode is Jog");
      setPhase("Armed"); return "Accepted";
    case "Disarm":
      if (s.phase !== "Armed") return rej(`not armed (phase ${s.phase})`);
      setPhase("Standby"); return "Accepted";
    case "Launch":
      if (s.phase !== "Armed") return rej(`not armed (phase ${s.phase})`);
      s.terminated = false; setPhase("Ascent"); return "Accepted";
    case "Abort":
      s.terminated = true; s.thrust = 0;
      event("Critical", "Flight terminated: Operator abort"); return "Accepted";
    case "SetPhase":
      if (arg !== "Hover" && arg !== "Descent") return rej(`phase ${arg} cannot be commanded`);
      if (!["Ascent", "Hover", "Descent"].includes(s.phase)) return rej(`not in flight (phase ${s.phase})`);
      event("Warning", `Operator override: ${arg}`); setPhase(arg); return "Accepted";
    case "SetFlightParams":
      if (!(arg.max_tilt_deg > 0 && arg.max_tilt_deg <= 90)) return rej("max_tilt_deg out of range");
      s.params.flight = arg; broadcast("params", s.params); return "Accepted";
    case "SetMpcWeights":
      if (arg && [...arg.q, ...arg.r, ...arg.qn].some((v) => !(v >= 0))) return rej("weights must be >= 0");
      s.params.manual_mpc_weights = arg; broadcast("params", s.params);
      event("Info", arg ? "MPC using manual weights" : "MPC using built-in weights"); return "Accepted";
    case "RequestParams":
      broadcast("params", s.params); return "Accepted";
    case "SetControlMode":
      if (arg === "Jog" && s.phase !== "Standby") return rej("jog only in Standby");
      s.mode = arg; event("Info", `Control mode -> ${arg}`); return "Accepted";
    case "SetValve":
      if (STAND) {
        // The bridge routes SetValve to the stand when one is configured.
        if (st.mode !== "Armed") return rej(st.mode === "Sequence" ? "sequence running: only Abort" : "stand not armed (Safe)");
        if (!st.valves[arg.id]) return rej("unknown valve");
        setStandValve(arg.id, arg.open);
        return "Accepted";
      }
      if (s.phase !== "Standby") return rej("valves only in Standby");
      if (!s.valves[arg.id]) return rej("unknown valve");
      s.valves[arg.id].state = arg.open ? "Open" : "Closed";
      if (arg.id === "Mtv") s.valves.Mtv.pos = arg.open ? 90 : 0;
      return "Accepted";
    case "Stand":
      if (!STAND) return rej("no test stand configured");
      return handleStand(arg);
    default:
      return rej("unknown command");
  }
}

wss.on("connection", (ws) => {
  if (s.traj) send(ws, "trajectory", s.traj);
  send(ws, "params", s.params);
  if (STAND) send(ws, "stand_status", standStatus());
  send(ws, "link", linkStatus());
  for (const e of s.events) send(ws, "event", e);
  ws.on("message", (raw) => {
    let msg;
    try { msg = JSON.parse(raw.toString()); } catch { return; }
    if (msg && typeof msg.control === "string") return handleControl(ws, msg);
    const kind = msg?.kind;
    if (kind === undefined || kind === "Heartbeat") return;
    const seq = ++s.upSeq;
    broadcast("sent", { seq, kind });
    if (typeof kind === "object" && "Jog" in kind) {
      if (s.mode === "Jog") { s.jog = kind.Jog; s.jogAt = s.t; }
      return;
    }
    if (DROP_ACKS) return;
    const result = handle(kind);
    if (flag("--verbose")) console.log(`#${seq} ${JSON.stringify(kind)} -> ${JSON.stringify(result)}`);
    setTimeout(() => broadcast("ack", { seq, time_s: s.t, result }), 40 + Math.random() * 60);
  });
});

const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
const noise = (a) => (Math.random() - 0.5) * 2 * a;

function step(dt) {
  s.t += dt; s.phaseT += dt;
  const g = 9.81;
  let thrustCmd = 0, gth = 0, gph = 0, rcs = 0;
  const flying = ["Ascent", "Hover", "Descent"].includes(s.phase) && !s.terminated;

  if (flying) {
    const ref = refAt(s.t) ?? s.pos;
    // wind gust pushes the vehicle around so tilt and deviation are alive
    const gust = [Math.sin(s.t * 0.31) * 0.9 + Math.sin(s.t * 1.3) * 0.3, Math.cos(s.t * 0.23) * 0.7];
    const ax = clamp(1.2 * (ref[0] - s.pos[0]) - 1.6 * s.vel[0], -3, 3);
    const ay = clamp(1.2 * (ref[1] - s.pos[1]) - 1.6 * s.vel[1], -3, 3);
    const az = clamp(2.0 * (ref[2] - s.pos[2]) - 2.4 * s.vel[2], -4, 5);
    // tilt follows desired lateral accel with a lag
    s.tilt[0] += (clamp(ax / g, -0.3, 0.3) - s.tilt[0]) * dt * 4;
    s.tilt[1] += (clamp(ay / g, -0.3, 0.3) - s.tilt[1]) * dt * 4;
    gth = clamp((ax / g - s.tilt[0]) * 1.5 + noise(0.004), -0.26, 0.26);
    gph = clamp((ay / g - s.tilt[1]) * 1.5 + noise(0.004), -0.26, 0.26);
    thrustCmd = clamp(s.mass * (g + az), 300, 1200);
    s.vel[0] += (g * s.tilt[0] + gust[0] * 0.4) * dt;
    s.vel[1] += (g * s.tilt[1] + gust[1] * 0.4) * dt;
    s.vel[2] += (thrustCmd / s.mass - g) * dt;
    for (let k = 0; k < 3; k++) s.pos[k] += s.vel[k] * dt;
    s.mass = Math.max(DRY + 1, s.mass - (thrustCmd / (180 * g)) * dt);
    s.yaw += 0.02 * Math.sin(s.t * 0.4) * dt;
    rcs = Math.sin(s.t * 0.4) > 0.8 ? 1 : Math.sin(s.t * 0.4) < -0.8 ? -1 : 0;

    const h = s.params.flight.hover_altitude_m;
    if (s.phase === "Ascent" && s.pos[2] > h - 0.6 && Math.abs(s.vel[2]) < 0.8) setPhase("Hover");
    else if (s.phase === "Hover" && s.phaseT > s.params.flight.hover_duration_s) setPhase("Descent");
    else if (s.phase === "Descent" && s.pos[2] <= 0.05) {
      s.pos[2] = 0; s.vel = [0, 0, 0]; s.tilt = [0, 0]; setPhase("Landed");
    }
    if (s.phase === "Hover" && !s.traj?.hover) { s.traj = { generated_at_s: s.t, time_of_flight_s: 1, positions: [[0, 0, h], [0, 0, h]], target: [0, 0, h], hover: true }; broadcast("trajectory", { ...s.traj, hover: undefined }); }
  } else if (s.terminated && s.pos[2] > 0) {
    s.vel[2] -= g * dt;
    for (let k = 0; k < 3; k++) s.pos[k] += s.vel[k] * dt;
    s.tilt[0] += 0.3 * dt;
    if (s.pos[2] <= 0) { s.pos[2] = 0; s.vel = [0, 0, 0]; event("Critical", "Ground impact"); setPhase("Landed"); }
  } else if (s.mode === "Jog" && s.phase === "Standby") {
    if (s.jog && s.t - s.jogAt < 0.5) {
      gth = clamp(s.jog.gimbal_theta, -0.2618, 0.2618);
      gph = clamp(s.jog.gimbal_phi, -0.2618, 0.2618);
      thrustCmd = clamp(s.jog.thrust, 0, 1200);
      rcs = Math.sign(s.jog.rcs);
    }
  }
  s.thrust += (thrustCmd - s.thrust) * Math.min(1, dt * 12);
  if (s.terminated) s.thrust = 0;
  s.gth = gth; s.gph = gph; s.rcs = rcs;

  if (AUTO) {
    s.autoT += dt;
    if (s.phase === "Standby" && s.autoT > 3 && s.mode === "Auto") { handle("Arm"); s.autoT = 0; }
    else if (s.phase === "Armed" && s.autoT > 2) { handle("Launch"); s.autoT = 0; }
    else if (s.phase === "Landed" && s.phaseT > 6) { s.mass = 74; s.terminated = false; s.pos = [0, 0, 0]; setPhase("Standby"); s.autoT = 0; }
  }
}

function quatFromTilt() {
  // small-angle: pitch about +Y tips nose toward +X, roll about -X tips toward +Y, then yaw
  const [tx, ty] = s.tilt;
  const ang = Math.hypot(tx, ty);
  let q = [0, 0, 0, 1];
  if (ang > 1e-6) {
    const ax = [-ty / ang, tx / ang, 0];
    const sn = Math.sin(ang / 2);
    q = [ax[0] * sn, ax[1] * sn, 0, Math.cos(ang / 2)];
  }
  const yz = Math.sin(s.yaw / 2), yw = Math.cos(s.yaw / 2);
  return [q[0] * yw + q[1] * yz, q[1] * yw - q[0] * yz, q[3] * yz, q[3] * yw];
}

function flightMsg() {
  const ref = refAt(s.t);
  const flying = ["Ascent", "Hover", "Descent"].includes(s.phase);
  const est = s.pos.map((v, k) => v + (flying ? Math.sin(s.t * (0.7 + k * 0.2)) * 0.12 : 0) + noise(0.01));
  const att = quatFromTilt();
  const burning = s.thrust > 50;
  return {
    seq: s.seq++, time_s: s.t, source: SOURCE, phase: s.phase, phase_time_s: s.phaseT,
    control_mode: s.mode, terminated: s.terminated,
    position: est, velocity: s.vel.map((v) => v + noise(0.02)), attitude: att,
    angular_velocity: [noise(0.01), noise(0.01), noise(0.005)], mass: s.mass,
    gimbal_theta: s.gth, gimbal_phi: s.gph, thrust: s.thrust, rcs: s.rcs,
    tilt_deg: (Math.hypot(s.tilt[0], s.tilt[1]) * 180) / Math.PI,
    trajectory_deviation_m: flying && ref ? Math.hypot(...est.map((v, k) => v - ref[k])) : null,
    position_age_s: 0.02 + Math.random() * 0.05 + (Math.sin(s.t * 0.05) > 0.97 ? 6 : 0),
    link_age_s: wss.clients.size ? 0.1 + Math.random() * 0.4 : null,
    sensors: {
      imu_ok: true, gps_ok: Math.sin(s.t * 0.05) <= 0.97, uwb_ok: s.pos[2] < 30,
      accel: [noise(0.05), noise(0.05), (s.thrust / s.mass || 9.81) + noise(0.05)],
      gyro: [noise(0.01), noise(0.01), noise(0.005)],
      chamber_pressure: burning ? s.thrust / 55 + noise(0.1) : 1.0,
      tank_pressure: SOURCE === "Vehicle" ? null : s.tank,
    },
    truth: SOURCE === "Sim" ? { position: [...s.pos], velocity: [...s.vel], attitude: att, angular_velocity: [0, 0, 0] } : null,
  };
}

function standMsg() {
  const flyingOpen = s.thrust > 50;
  const v = structuredClone(s.valves);
  if (s.phase !== "Standby") {
    v.Omv.state = flyingOpen ? "Open" : "Closed";
    v.Mtv.state = flyingOpen ? "Open" : "Closed";
    v.Mtv.pos = (s.thrust / 1200) * 90;
    v.PuFill.state = "Open"; v.PuIso.state = "Open"; v.PuVnt.state = "Closed";
    v.Rcs1.state = s.rcs > 0 ? "Open" : "Closed";
    v.Rcs2.state = s.rcs < 0 ? "Open" : "Closed";
  }
  s.tank += ((flyingOpen ? 46 : 52) - s.tank) * 0.01;
  const pc = flyingOpen ? s.thrust / 55 : 1.0;
  const channels = [
    ["Opt", s.tank + noise(0.05)], ["Ipt", flyingOpen ? pc * 1.25 + noise(0.1) : 1.0], ["Ept", pc + noise(0.08)],
    ["M1", v.Omv.state === "Open" ? s.tank - 1.5 + noise(0.1) : 1.0], ["M2", flyingOpen ? pc * 1.4 + noise(0.1) : 1.0],
    ["Pupt", v.PuIso.state === "Open" ? 60 + noise(0.2) : 1.0 + noise(0.02)],
    ["T1", 18 + Math.sin(s.t * 0.02) * 2 + noise(0.05)], ["T2", flyingOpen ? 240 + s.thrust / 6 + noise(2) : 24 + noise(0.2)],
    ["Thrust", s.thrust + noise(4)],
  ];
  // LF-PT deliberately omitted: the UI must show a missing channel as "—", never 0.
  return {
    time_s: s.t, source: SOURCE, channels,
    // Rcs2 deliberately omitted while in Standby to exercise the missing-valve rendering.
    valves: VALVES.filter((id) => !(id === "Rcs2" && s.phase === "Standby")).map((id) => ({ id, state: v[id].state, position_deg: v[id].pos })),
  };
}

let rx = 0;
function linkStatus() {
  return {
    vehicle_addr: "127.0.0.1:8888", connected: true, last_rx_age_s: 0.01,
    packets_rx: rx, packets_lost: Math.floor(s.t / 40), rate_hz: 50 + noise(0.6),
    recording,
    stand: STAND ? { addr: STAND_ADDR, connected: true, last_rx_age_s: 0.02 + Math.random() * 0.03 } : null,
  };
}

// Fixed-step sim, wall-clock paced so timer jitter does not change the rate.
const t0 = performance.now();
let ticks = 0;
setInterval(() => {
  const due = Math.floor((performance.now() - t0) / 20);
  while (ticks < due) {
    ticks++;
    step(0.02);
    if (STAND) stepStand(0.02);
    rx++;
    broadcast("flight", flightMsg());
  }
}, 5);
// With a stand on the link the stand's own telemetry replaces the sim's propulsion picture.
setInterval(() => broadcast("stand", STAND ? standTelemetry() : standMsg()), 50);
if (STAND) setInterval(() => broadcast("stand_status", standStatus()), 200);
setInterval(() => broadcast("link", linkStatus()), 500);
setInterval(() => { if (s.traj && ["Ascent", "Descent"].includes(s.phase)) broadcast("trajectory", { ...s.traj, hover: undefined }); }, 1000);

event("Info", "Mock vehicle started");
if (STAND) event("Info", `Mock test stand on ${STAND_ADDR}: Safe, sequences ${Object.keys(SEQUENCES).join(", ")}`);
console.log(`mock bridge on ws://127.0.0.1:${PORT}/ws  source=${SOURCE}${AUTO ? "  (auto flight)" : ""}${STAND ? "  +stand" : ""}${recording ? `  recording ${recording}` : ""}`);

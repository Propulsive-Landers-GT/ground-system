#!/usr/bin/env node
// Stand-in for gs-bridge + a vehicle, for UI work without the Rust side.
// Implements the WebSocket API in docs/DESIGN.md on ws://127.0.0.1:8080/ws:
// flight 50 Hz, stand 20 Hz, trajectory 1 Hz, link 2 Hz, events, sent echo, acks with the
// vehicle's interlock rules, params, jog with a 0.5 s deadman.
//
//   node scripts/mock-bridge.mjs [--port 8080] [--auto] [--source Sim|Vehicle|Replay] [--drop-acks]
//   --auto       arms and launches by itself, and relaunches after landing
//   --drop-acks  never ack (to see the "no ack" state)

import { WebSocketServer } from "ws";

const args = process.argv.slice(2);
const flag = (n) => args.includes(n);
const opt = (n, d) => (args.includes(n) ? args[args.indexOf(n) + 1] : d);
const PORT = Number(opt("--port", 8080));
const AUTO = flag("--auto");
const SOURCE = opt("--source", "Sim");
const DROP_ACKS = flag("--drop-acks");

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
      if (s.phase !== "Standby") return rej("valves only in Standby");
      if (!s.valves[arg.id]) return rej("unknown valve");
      s.valves[arg.id].state = arg.open ? "Open" : "Closed";
      if (arg.id === "Mtv") s.valves.Mtv.pos = arg.open ? 90 : 0;
      return "Accepted";
    default:
      return rej("unknown command");
  }
}

wss.on("connection", (ws) => {
  if (s.traj) send(ws, "trajectory", s.traj);
  send(ws, "params", s.params);
  send(ws, "link", linkStatus());
  for (const e of s.events) send(ws, "event", e);
  ws.on("message", (raw) => {
    let msg;
    try { msg = JSON.parse(raw.toString()); } catch { return; }
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
    recording: "logs/session-mock.jsonl",
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
    rx++;
    broadcast("flight", flightMsg());
  }
}, 5);
setInterval(() => broadcast("stand", standMsg()), 50);
setInterval(() => broadcast("link", linkStatus()), 500);
setInterval(() => { if (s.traj && ["Ascent", "Descent"].includes(s.phase)) broadcast("trajectory", { ...s.traj, hover: undefined }); }, 1000);

event("Info", "Mock vehicle started");
console.log(`mock bridge on ws://127.0.0.1:${PORT}/ws  source=${SOURCE}${AUTO ? "  (auto flight)" : ""}`);

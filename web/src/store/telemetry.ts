// High-rate telemetry lives here, outside React. The WebSocket handler writes, the 3D scene
// and plots read on requestAnimationFrame, numeric readouts read on the ~12 Hz UI tick.

import type { FlightTelemetry, StandTelemetry, TrajectoryMsg, StandChannel, ValveId, ValveStatus } from "../protocol";
import { STAND_CHANNELS } from "../protocol";
import { quatToEulerDeg } from "../lib/math";

/** Column store with a sliding window. Backed by 2×capacity so views are contiguous, zero-copy subarrays. */
export class Window {
  readonly cols: Float64Array[];
  private start = 0;
  len = 0;
  constructor(readonly ncols: number, readonly cap: number) {
    this.cols = Array.from({ length: ncols }, () => new Float64Array(cap * 2));
  }
  push(row: ArrayLike<number>) {
    if (this.start + this.len === this.cap * 2) {
      for (const c of this.cols) c.copyWithin(0, this.start, this.start + this.len);
      this.start = 0;
    }
    const i = this.start + this.len;
    for (let k = 0; k < this.ncols; k++) this.cols[k][i] = row[k];
    if (this.len === this.cap) this.start++;
    else this.len++;
  }
  view(col: number): Float64Array {
    return this.cols[col].subarray(this.start, this.start + this.len);
  }
  last(col: number): number {
    return this.len ? this.cols[col][this.start + this.len - 1] : NaN;
  }
  clear() {
    this.start = 0;
    this.len = 0;
  }
}

export const WINDOW_S = 120;

// Flight plot columns
export const F = {
  t: 0, z: 1, zTruth: 2, zRef: 3, vx: 4, vy: 5, vz: 6, roll: 7, pitch: 8, yaw: 9,
  tilt: 10, dev: 11, gTheta: 12, gPhi: 13, thrust: 14,
} as const;
const F_COLS = 15;

// Stand plot columns: t, then STAND_CHANNELS order
export const standCol = (ch: StandChannel) => 1 + STAND_CHANNELS.indexOf(ch);

const TRAIL_CAP = 2400; // 10 Hz × 240 s, trimmed by window
const GIMBAL_TRAIL = 40;

export const tele = {
  flight: null as FlightTelemetry | null,
  flightRxMs: 0,
  stand: null as StandTelemetry | null,
  standRxMs: 0,
  /** Last time a `stand` message with `source: "Stand"` (the real adapter) arrived. */
  standSourceRxMs: -1e9,
  channels: new Map<StandChannel, number>(),
  valves: new Map<ValveId, ValveStatus>(),
  trajectory: null as TrajectoryMsg | null,
  trajectoryRev: 0,

  flightBuf: new Window(F_COLS, WINDOW_S * 50),
  standBuf: new Window(1 + STAND_CHANNELS.length, WINDOW_S * 20),

  /** Flown path, xyz triplets, world frame. */
  trail: new Float32Array(TRAIL_CAP * 3),
  trailLen: 0,
  trailRev: 0,

  /** Recent gimbal (theta, phi) pairs in degrees, newest last. */
  gimbalTrail: [] as [number, number][],
};

let trailDecim = 0;
const row = new Float64Array(F_COLS);

/** Reference altitude at mission time t, if the latest trajectory covers it. */
function refAltitude(t: number): number {
  const tr = tele.trajectory;
  if (!tr || tr.positions.length < 2 || tr.time_of_flight_s <= 0) return NaN;
  const u = (t - tr.generated_at_s) / tr.time_of_flight_s;
  if (u < 0 || u > 1) return NaN;
  const x = u * (tr.positions.length - 1);
  const i = Math.min(Math.floor(x), tr.positions.length - 2);
  const f = x - i;
  return tr.positions[i][2] * (1 - f) + tr.positions[i + 1][2] * f;
}

export function resetHistory() {
  tele.flightBuf.clear();
  tele.standBuf.clear();
  tele.trailLen = 0;
  tele.trailRev++;
  tele.gimbalTrail.length = 0;
}

export function ingestFlight(m: FlightTelemetry) {
  const prev = tele.flight;
  // Mission clock jumped backwards: vehicle/sim restarted or replay looped.
  if (prev && m.time_s < prev.time_s - 0.5) resetHistory();
  tele.flight = m;
  tele.flightRxMs = performance.now();

  const [roll, pitch, yaw] = quatToEulerDeg(m.attitude);
  row[F.t] = m.time_s;
  row[F.z] = m.position[2];
  row[F.zTruth] = m.truth ? m.truth.position[2] : NaN;
  row[F.zRef] = refAltitude(m.time_s);
  row[F.vx] = m.velocity[0];
  row[F.vy] = m.velocity[1];
  row[F.vz] = m.velocity[2];
  row[F.roll] = roll;
  row[F.pitch] = pitch;
  row[F.yaw] = yaw;
  row[F.tilt] = m.tilt_deg;
  row[F.dev] = m.trajectory_deviation_m ?? NaN;
  row[F.gTheta] = (m.gimbal_theta * 180) / Math.PI;
  row[F.gPhi] = (m.gimbal_phi * 180) / Math.PI;
  row[F.thrust] = m.thrust;
  tele.flightBuf.push(row);

  if (++trailDecim >= 5) {
    trailDecim = 0;
    if (tele.trailLen === TRAIL_CAP) {
      tele.trail.copyWithin(0, 3 * (TRAIL_CAP / 2));
      tele.trailLen = TRAIL_CAP / 2;
    }
    tele.trail.set(m.position, tele.trailLen * 3);
    tele.trailLen++;
    tele.trailRev++;
    tele.gimbalTrail.push([row[F.gTheta], row[F.gPhi]]);
    if (tele.gimbalTrail.length > GIMBAL_TRAIL) tele.gimbalTrail.shift();
  }
}

const srow = new Float64Array(1 + STAND_CHANNELS.length);
/** A session can carry `stand` messages from both a vehicle/sim and a real test stand. */
const STAND_PRIORITY_MS = 2000;

/** Returns false when the message was dropped in favour of the physical stand's telemetry. */
export function ingestStand(m: StandTelemetry): boolean {
  const now = performance.now();
  if (m.source === "Stand") {
    tele.standSourceRxMs = now;
  } else if (now - tele.standSourceRxMs < STAND_PRIORITY_MS) {
    // The real stand is the authority on the valves; a sim's propulsion model must not overwrite it.
    return false;
  }
  if (tele.stand && (m.time_s < tele.stand.time_s - 0.5 || m.source !== tele.stand.source)) tele.standBuf.clear();
  tele.stand = m;
  tele.standRxMs = now;
  // Only what this message carries is "present"; anything absent must read as missing, not zero.
  tele.channels.clear();
  srow.fill(NaN);
  srow[0] = m.time_s;
  for (const [ch, v] of m.channels) {
    tele.channels.set(ch, v);
    const c = standCol(ch);
    if (c > 0) srow[c] = v;
  }
  tele.valves.clear();
  for (const v of m.valves) tele.valves.set(v.id, v);
  // A stand that drives the MTV by percent may not list it as a valve; the commanded opening is its state.
  const pct = m.mtv_percent;
  if (!tele.valves.has("Mtv") && pct !== null && pct !== undefined && Number.isFinite(pct)) {
    tele.valves.set("Mtv", { id: "Mtv", state: pct > 0.5 ? "Open" : "Closed", position_deg: null });
  }
  tele.standBuf.push(srow);
  return true;
}

export function ingestTrajectory(m: TrajectoryMsg) {
  tele.trajectory = m;
  tele.trajectoryRev++;
}

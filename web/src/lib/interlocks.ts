// UI mirror of the "Command rules enforced on the vehicle" and "Test-stand rules enforced by gs-stand"
// tables in docs/DESIGN.md. The vehicle / stand is the authority; this only greys buttons out and says why.

import type { ControlMode, FlightPhase, Source, StandMode } from "../protocol";
import type { WsState } from "../store/ui";

export interface Ctx {
  ws: WsState;
  source: Source | null;
  phase: FlightPhase | null;
  controlMode: ControlMode | null;
}

/** Everything the stand gates need. `configured` is `link.stand !== null`. */
export interface StandCtx {
  ws: WsState;
  configured: boolean;
  linkUp: boolean;
  /** Source of the stand telemetry on screen; "Replay" disables everything. */
  standSource: Source | null;
  mode: StandMode | null;
  actuationOk: boolean;
  loadcellOk: boolean;
}

export type Gate = { ok: true } | { ok: false; why: string };
const ok: Gate = { ok: true };
const no = (why: string): Gate => ({ ok: false, why });

function base(c: Ctx): Gate | null {
  if (c.ws !== "open") return no("No connection to the bridge");
  if (c.source === "Replay") return no("Replay session: commands are disabled");
  return null;
}

function needPhase(c: Ctx, allowed: FlightPhase[], verb: string): Gate {
  const b = base(c);
  if (b) return b;
  if (c.phase === null) return no("Waiting for telemetry");
  if (!allowed.includes(c.phase)) return no(`${verb} needs ${allowed.join(" / ")}; vehicle is in ${c.phase}`);
  return ok;
}

export const gates = {
  arm(c: Ctx): Gate {
    const g = needPhase(c, ["Standby"], "Arm");
    if (!g.ok) return g;
    if (c.controlMode !== "Auto") return no("Arm needs control mode Auto; switch jog off first");
    return ok;
  },
  disarm: (c: Ctx) => needPhase(c, ["Armed"], "Disarm"),
  launch: (c: Ctx) => needPhase(c, ["Armed"], "Launch"),
  setPhase: (c: Ctx) => needPhase(c, ["Ascent", "Hover", "Descent"], "Phase override"),
  /** Abort is accepted in every phase; only a dead socket or a replay can block it. */
  abort: (c: Ctx): Gate => base(c) ?? ok,
  tuning: (c: Ctx): Gate => base(c) ?? ok,
  jogMode: (c: Ctx) => needPhase(c, ["Standby"], "Jog"),
  /** Vehicle rule for SetValve. With a test stand configured use `standGates.output` instead (see `valveGate`). */
  valve: (c: Ctx) => needPhase(c, ["Standby"], "Valve commands"),
};

function standBase(c: StandCtx): Gate | null {
  if (c.ws !== "open") return no("No connection to the bridge");
  if (!c.configured) return no("No test stand configured on the bridge (--stand)");
  if (c.standSource === "Replay") return no("Replay session: commands are disabled");
  return null;
}

function needMode(c: StandCtx, allowed: StandMode[], verb: string): Gate {
  const b = standBase(c);
  if (b) return b;
  if (!c.linkUp) return no("Test stand link is down");
  if (c.mode === null) return no("Waiting for stand status");
  if (!allowed.includes(c.mode)) {
    if (c.mode === "Sequence") return no(`Sequence running: only Abort is accepted`);
    return no(`${verb} needs stand ${allowed.join(" / ")}; stand is ${c.mode}`);
  }
  return ok;
}

export const standGates = {
  arm(c: StandCtx): Gate {
    const g = needMode(c, ["Safe"], "Arm");
    if (!g.ok) return g;
    if (!c.actuationOk) return no("Arm needs the actuation Arduino link up");
    if (!c.loadcellOk) return no("Arm needs the load-cell Arduino link up");
    return ok;
  },
  disarm: (c: StandCtx) => needMode(c, ["Safe", "Armed"], "Disarm"),
  /** Accepted in every mode. Left enabled through a link dropout: sending an abort into a dead link costs nothing. */
  abort: (c: StandCtx): Gate => standBase(c) ?? ok,
  /** SetValve, SetMtvPercent, SetOutput{Igniter}. */
  output: (c: StandCtx) => needMode(c, ["Armed"], "Valve, MTV and igniter commands"),
  daqSync: (c: StandCtx) => needMode(c, ["Safe", "Armed"], "DAQ sync"),
  startSequence: (c: StandCtx) => needMode(c, ["Armed"], "Starting a sequence"),
};

/** SetValve goes to whichever endpoint owns the valves: the stand when one is configured, else the vehicle. */
export function valveGate(c: Ctx, s: StandCtx): Gate {
  return s.configured ? standGates.output(s) : gates.valve(c);
}

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

// Reasons are shown inline under the greyed-out control, so they read as a sentence an operator can act on:
// what to do first, or why nothing can be done right now.

function base(c: Ctx): Gate | null {
  if (c.ws !== "open") return no("No connection to the bridge.");
  if (c.source === "Replay") return no("Replaying a recording: commands are off.");
  return null;
}

const list = (xs: string[]) => (xs.length === 1 ? xs[0] : `${xs.slice(0, -1).join(", ")} or ${xs[xs.length - 1]}`);

function needPhase(c: Ctx, allowed: FlightPhase[], verb: string): Gate {
  const b = base(c);
  if (b) return b;
  if (c.phase === null) return no("Waiting for vehicle telemetry.");
  if (!allowed.includes(c.phase)) return no(`${verb} is available in ${list(allowed)}. The vehicle is in ${c.phase}.`);
  return ok;
}

export const gates = {
  arm(c: Ctx): Gate {
    const g = needPhase(c, ["Standby"], "Arm");
    if (!g.ok) return g;
    if (c.controlMode !== "Auto") return no("Switch jog off first: Arm needs control mode Auto.");
    return ok;
  },
  disarm(c: Ctx): Gate {
    const g = needPhase(c, ["Armed"], "Disarm");
    if (!g.ok && c.phase === "Standby") return no("Nothing to disarm: the vehicle is in Standby.");
    return g;
  },
  launch(c: Ctx): Gate {
    const g = needPhase(c, ["Armed"], "Launch");
    if (!g.ok && c.phase === "Standby") return no("Arm the vehicle first.");
    return g;
  },
  setPhase(c: Ctx): Gate {
    const g = needPhase(c, ["Ascent", "Hover", "Descent"], "Changing phase");
    if (!g.ok && (c.phase === "Standby" || c.phase === "Armed")) return no("Available once the vehicle is in flight.");
    return g;
  },
  /** Abort is accepted in every phase; only a dead socket or a replay can block it. */
  abort: (c: Ctx): Gate => base(c) ?? ok,
  tuning: (c: Ctx): Gate => base(c) ?? ok,
  jogMode(c: Ctx): Gate {
    const g = needPhase(c, ["Standby"], "Jog");
    if (!g.ok && c.phase !== null && c.phase !== "Standby") return no(`Jog only works in Standby. The vehicle is in ${c.phase}.`);
    return g;
  },
  /** Vehicle rule for SetValve. With a test stand configured use `standGates.output` instead (see `valveGate`). */
  valve(c: Ctx): Gate {
    const g = needPhase(c, ["Standby"], "Valve commands");
    if (!g.ok && c.phase !== null && c.phase !== "Standby") return no(`Valves can only be commanded in Standby. The vehicle is in ${c.phase}.`);
    return g;
  },
};

function standBase(c: StandCtx): Gate | null {
  if (c.ws !== "open") return no("No connection to the bridge.");
  if (!c.configured) return no("No test stand on this bridge (start gs-bridge with --stand).");
  if (c.standSource === "Replay") return no("Replaying a recording: commands are off.");
  return null;
}

function needMode(c: StandCtx, allowed: StandMode[], verb: string): Gate {
  const b = standBase(c);
  if (b) return b;
  if (!c.linkUp) return no("The link to the test stand is down.");
  if (c.mode === null) return no("Waiting for the stand to report its mode.");
  if (!allowed.includes(c.mode)) {
    if (c.mode === "Sequence") return no("A sequence is running: only Stand abort is accepted.");
    if (c.mode === "Safe" && allowed.includes("Armed")) return no("Arm the stand first.");
    return no(`${verb} needs the stand to be ${list(allowed)}. It is ${c.mode}.`);
  }
  return ok;
}

export const standGates = {
  arm(c: StandCtx): Gate {
    const g = needMode(c, ["Safe"], "Arm");
    if (!g.ok) {
      if (c.mode === "Armed") return no("The stand is already armed.");
      return g;
    }
    if (!c.actuationOk) return no("Cannot arm: the actuation Arduino link is down.");
    if (!c.loadcellOk) return no("Cannot arm: the load-cell Arduino link is down.");
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

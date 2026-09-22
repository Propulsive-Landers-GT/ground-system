// UI mirror of the "Command rules enforced on the vehicle" table in docs/DESIGN.md.
// The vehicle is the authority; this only greys buttons out and says why.

import type { ControlMode, FlightPhase, Source } from "../protocol";
import type { WsState } from "../store/ui";

export interface Ctx {
  ws: WsState;
  source: Source | null;
  phase: FlightPhase | null;
  controlMode: ControlMode | null;
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
  valve: (c: Ctx) => needPhase(c, ["Standby"], "Valve commands"),
};

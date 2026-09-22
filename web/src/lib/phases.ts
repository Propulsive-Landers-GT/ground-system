// One plain sentence per state, shown beside the stepper and in the command rail so a new operator
// knows what the vehicle / stand is doing and what the next sensible action is.

import type { FlightPhase, StandMode } from "../protocol";

export function phaseMeaning(phase: FlightPhase | null, terminated: boolean, hoverAlt?: number, hoverDur?: number): string {
  if (terminated) return "Flight terminated: thrust is cut and the control loop has stopped.";
  switch (phase) {
    case "Standby": return "On the pad, not armed. Arm to enable Launch.";
    case "Armed": return "Ready to launch. Hold Launch to go, or Disarm.";
    case "Ascent": return hoverAlt ? `Climbing to the ${hoverAlt.toFixed(0)} m hover altitude.` : "Climbing to the hover altitude.";
    case "Hover": return hoverDur ? `Holding altitude for ${hoverDur.toFixed(0)} s, then descending.` : "Holding altitude, then descending.";
    case "Descent": return "Controlled descent to the pad.";
    case "Landed": return "On the pad. Restart the flight software for another run.";
    default: return "Waiting for vehicle telemetry.";
  }
}

export function standMeaning(mode: StandMode | null, configured: boolean): string {
  if (!configured) return "No test stand on this bridge.";
  switch (mode) {
    case "Safe": return "Safed: mains closed, vents open, igniter off. Arm to enable outputs.";
    case "Armed": return "Armed: valves, MTV and igniter accept commands. Start a sequence when ready.";
    case "Sequence": return "Sequence running. Only Stand abort is accepted until it ends.";
    default: return "Waiting for the stand to report its mode.";
  }
}

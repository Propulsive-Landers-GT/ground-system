import type { Gate } from "../../lib/interlocks";

/**
 * Inline interlock reason. Rendered under a greyed-out control so the operator sees why without hovering;
 * the control keeps the same text in its tooltip and aria-description.
 */
export function Reason({ gate, className = "" }: { gate: Gate; className?: string }) {
  if (gate.ok) return null;
  return <p className={`why ${className}`}>{gate.why}</p>;
}

export const why = (g: Gate) => (g.ok ? undefined : g.why);

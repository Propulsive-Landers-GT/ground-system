export type Level = "nominal" | "caution" | "warning" | "none";

export function levelFor(value: number | null | undefined, cautionAt: number, warnAt: number): Level {
  if (value === null || value === undefined || !Number.isFinite(value)) return "none";
  if (value >= warnAt) return "warning";
  if (value >= cautionAt) return "caution";
  return "nominal";
}

interface Props {
  value: number | null | undefined;
  /** Full-scale of the bar (the limit). */
  max: number;
  cautionAt: number;
  warnAt: number;
  label: string;
}

/** Margin bar: fill is value/max, ticks at the caution and warning thresholds. */
export function LimitBar({ value, max, cautionAt, warnAt, label }: Props) {
  const level = levelFor(value, cautionAt, warnAt);
  const has = level !== "none";
  const frac = has ? Math.max(0, Math.min(1, (value as number) / max)) : 0;
  return (
    <div
      className="limitbar"
      data-level={level}
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={has ? (value as number) : undefined}
      aria-valuetext={has ? `${(frac * 100).toFixed(0)} percent of limit, ${level}` : "no data"}
    >
      <div className="limitbar-fill" style={{ transform: `scaleX(${frac})` }} />
      <span className="limitbar-tick" style={{ left: `${(cautionAt / max) * 100}%` }} />
      {warnAt < max && <span className="limitbar-tick" style={{ left: `${(warnAt / max) * 100}%` }} />}
    </div>
  );
}

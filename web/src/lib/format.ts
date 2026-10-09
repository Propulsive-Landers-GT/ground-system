export const DASH = "—";

/** Fixed-decimals number, or an em dash for null/NaN. Uses a true minus sign so widths stay stable. */
export function num(v: number | null | undefined, decimals = 1): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return DASH;
  const s = Math.abs(v).toFixed(decimals);
  const neg = v < 0 && Number(s) !== 0;
  return (neg ? "−" : "") + s;
}

/** Signed variant: always shows + or −. */
export function signed(v: number | null | undefined, decimals = 1): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return DASH;
  const s = Math.abs(v).toFixed(decimals);
  if (Number(s) === 0) return "\u2007" + s;
  return (v < 0 ? "−" : "+") + s;
}

/** Mission clock: T+MM:SS.s */
export function clock(t: number | null | undefined): string {
  if (t === null || t === undefined || !Number.isFinite(t)) return "T+" + DASH + DASH + ":" + DASH + DASH + "." + DASH;
  const neg = t < 0;
  const a = Math.abs(t);
  const m = Math.floor(a / 60);
  const s = a - m * 60;
  return `T${neg ? "−" : "+"}${String(m).padStart(2, "0")}:${s.toFixed(1).padStart(4, "0")}`;
}

export function shortClock(t: number | null | undefined): string {
  if (t === null || t === undefined || !Number.isFinite(t)) return DASH;
  const m = Math.floor(Math.abs(t) / 60);
  const s = Math.abs(t) - m * 60;
  return `${String(m).padStart(2, "0")}:${s.toFixed(1).padStart(4, "0")}`;
}

/** Sequence clock: T+48.7 for short sequences, T+MM:SS.s past ten minutes. */
export function tplus(t: number | null | undefined): string {
  if (t === null || t === undefined || !Number.isFinite(t)) return "T+" + DASH;
  if (Math.abs(t) >= 600) return clock(t);
  return `T${t < 0 ? "−" : "+"}${Math.abs(t).toFixed(1)}`;
}

/** Elapsed wall time as M:SS or H:MM:SS. */
export function elapsed(s: number | null | undefined): string {
  if (s === null || s === undefined || !Number.isFinite(s)) return DASH;
  const total = Math.max(0, Math.floor(s));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const sec = total % 60;
  return (h ? `${h}:${String(m).padStart(2, "0")}` : String(m)) + ":" + String(sec).padStart(2, "0");
}

export function duration(t: number | null | undefined): string {
  if (t === null || t === undefined || !Number.isFinite(t)) return DASH;
  if (t < 60) return `${t.toFixed(1)} s`;
  const m = Math.floor(t / 60);
  return `${m} m ${String(Math.floor(t - m * 60)).padStart(2, "0")} s`;
}

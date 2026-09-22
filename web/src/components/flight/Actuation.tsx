import { useEffect, useRef } from "react";
import { useTick, useUi } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { num, signed } from "../../lib/format";
import { rad2deg } from "../../lib/math";
import { THRUST_MAX_N, THRUST_MIN_THROTTLE_N } from "../../protocol";

const R = 56; // px radius that maps to the ±15° bound
const LIM = 15;

/** θ on x, φ on y, ±15° box, short fading trail. Drawn on rAF straight from the telemetry store. */
function GimbalBullseye() {
  const dot = useRef<SVGCircleElement>(null);
  const trail = useRef<SVGPolylineElement>(null);
  useEffect(() => {
    let raf = 0;
    const draw = () => {
      raf = requestAnimationFrame(draw);
      const f = tele.flight;
      if (!dot.current || !trail.current) return;
      if (!f) {
        dot.current.setAttribute("visibility", "hidden");
        return;
      }
      const sx = (d: number) => (Math.max(-LIM * 1.15, Math.min(LIM * 1.15, d)) / LIM) * R;
      dot.current.setAttribute("visibility", "visible");
      dot.current.setAttribute("cx", String(sx(rad2deg(f.gimbal_theta))));
      dot.current.setAttribute("cy", String(-sx(rad2deg(f.gimbal_phi))));
      trail.current.setAttribute("points", tele.gimbalTrail.map(([t, p]) => `${sx(t).toFixed(1)},${(-sx(p)).toFixed(1)}`).join(" "));
    };
    raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  }, []);

  return (
    <svg className="bullseye" viewBox="-76 -76 152 152" role="img" aria-label="Gimbal angles, theta horizontal, phi vertical, bound plus or minus 15 degrees">
      <rect x={-R} y={-R} width={2 * R} height={2 * R} className="be-bound" />
      <circle r={R / 3} className="be-ring" />
      <circle r={(2 * R) / 3} className="be-ring" />
      <line x1={-R} x2={R} className="be-axis" />
      <line y1={-R} y2={R} className="be-axis" />
      <text x={R + 4} y={4} className="be-text">θ</text>
      <text x={-4} y={-R - 5} className="be-text">φ</text>
      <text x={-R} y={R + 13} className="be-text">−15°</text>
      <text x={R} y={R + 13} className="be-text" textAnchor="end">+15°</text>
      <polyline ref={trail} className="be-trail" />
      <circle ref={dot} r={4.5} className="be-dot" visibility="hidden" />
    </svg>
  );
}

/** Collapsed-summary line: thrust, gimbal and RCS at a glance. */
export function ActuationSummary() {
  useTick();
  const f = tele.flight;
  const pct = f ? (f.thrust / THRUST_MAX_N) * 100 : null;
  return (
    <span className="summary-line" data-live>
      <span><b className="val">{num(f?.thrust, 0)}</b> N · {num(pct, 0)} %</span>
      <span>θ <b className="val">{signed(f ? rad2deg(f.gimbal_theta) : null, 1)}</b>° φ <b className="val">{signed(f ? rad2deg(f.gimbal_phi) : null, 1)}</b>°</span>
      <span>RCS {f ? (f.rcs === 1 ? "CW" : f.rcs === -1 ? "CCW" : "off") : "—"}</span>
    </span>
  );
}

export function Actuation() {
  useTick();
  const mode = useUi((s) => s.controlMode);
  const f = tele.flight;
  const thrust = f?.thrust ?? null;
  const pct = thrust === null ? null : (thrust / THRUST_MAX_N) * 100;

  return (
    <div className="actuation act-body" aria-label="Actuation">
      <div className="gimbal">
        <GimbalBullseye />
        <dl className="mini" data-live>
          <dt>Gimbal θ</dt>
          <dd>{signed(f ? rad2deg(f.gimbal_theta) : null, 2)}<span className="unit">°</span></dd>
          <dt>Gimbal φ</dt>
          <dd>{signed(f ? rad2deg(f.gimbal_phi) : null, 2)}<span className="unit">°</span></dd>
          <dt>Control</dt>
          <dd><span className="badge" data-mode={mode ?? "none"}>{mode ? mode.toUpperCase() : "—"}</span></dd>
        </dl>
      </div>

      <div className="thrust">
        <div className="thrust-head">
          <span className="readout-label">Thrust command</span>
          <span className="thrust-val" data-live>
            <span className="val">{num(thrust, 0)}</span>
            <span className="unit">N</span>
            <span className="val pct">{num(pct, 0)}</span>
            <span className="unit">%</span>
          </span>
        </div>
        <div className="thrustbar" role="meter" aria-label="Thrust" aria-valuemin={0} aria-valuemax={THRUST_MAX_N} aria-valuenow={thrust ?? undefined}>
          <div className="thrustbar-band" style={{ left: `${(THRUST_MIN_THROTTLE_N / THRUST_MAX_N) * 100}%` }} />
          <div className="thrustbar-fill" style={{ transform: `scaleX(${thrust === null ? 0 : Math.max(0, Math.min(1, thrust / THRUST_MAX_N))})` }} />
        </div>
        <div className="thrustbar-scale" aria-hidden="true">
          <span>0</span>
          <span style={{ left: "25%" }}>300 min throttle</span>
          <span style={{ left: "100%" }}>1200 N</span>
        </div>
        <div className="rcs" role="group" aria-label="Roll thrusters">
          <span className="readout-label">RCS roll</span>
          <span className="lamp" data-on={f?.rcs === -1 || undefined}>CCW</span>
          <span className="lamp" data-on={f?.rcs === 1 || undefined}>CW</span>
        </div>
      </div>
    </div>
  );
}

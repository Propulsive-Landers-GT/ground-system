import { useRef, type ReactNode } from "react";
import { useTick, useUi } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { num, signed } from "../../lib/format";
import { norm3, quatToEulerDeg } from "../../lib/math";
import { LimitBar, levelFor, type Level } from "./LimitBar";

// Not carried by the protocol. state.rs uses 50 kg; shown only as a rough propellant gauge.
export const ASSUMED_DRY_MASS_KG = 50;
const LEVEL_WORD: Record<Level, string> = { nominal: "", caution: "Caution", warning: "Warning", none: "" };

function Readout(p: {
  label: string; value: string; unit: string; size?: "hero" | "big" | "md"; level?: Level; note?: ReactNode; children?: ReactNode;
}) {
  return (
    <div className="readout" data-size={p.size ?? "md"} data-level={p.level}>
      <div className="readout-head">
        <span className="readout-label">{p.label}</span>
        {p.level && LEVEL_WORD[p.level] && <span className="readout-flag">{LEVEL_WORD[p.level]}</span>}
        {p.note && <span className="readout-note">{p.note}</span>}
      </div>
      <div className="readout-value" data-live>
        <span className="val">{p.value}</span>
        <span className="unit">{p.unit}</span>
      </div>
      {p.children}
    </div>
  );
}

/** The four numbers an operator reads first. Altitude and vertical speed are the biggest thing on the page. */
export function FlightHero() {
  useTick();
  const hasFlight = useUi((s) => s.hasFlight);
  const stale = useUi((s) => s.stale);
  const f = tele.flight;
  return (
    <section className="panel hero" aria-label="Flight readouts">
      <Readout size="hero" label="Altitude" value={num(f?.position[2], 2)} unit="m" />
      <Readout size="hero" label="Vertical speed" value={signed(f?.velocity[2], 2)} unit="m/s" />
      <Readout size="big" label="Offset from pad" value={num(f ? Math.hypot(f.position[0], f.position[1]) : null, 2)} unit="m" />
      <Readout size="big" label="Speed" value={num(f ? norm3(f.velocity) : null, 2)} unit="m/s" />
      {!hasFlight && <span className="hero-note">Waiting for telemetry</span>}
      {stale && <span className="hero-note flag-warn">Stale · last packet {num((performance.now() - tele.flightRxMs) / 1000, 0)} s ago</span>}
    </section>
  );
}

/** Termination margins and the propellant estimate: how close the flight is to the limits that end it. */
export function Margins() {
  useTick();
  const params = useUi((s) => s.params);
  const f = tele.flight;
  const wetMass = useRef(0);

  const maxTilt = params?.flight.max_tilt_deg ?? 30;
  const maxDev = params?.flight.max_trajectory_deviation_m ?? 10;
  const limitNote = params ? undefined : "default limit";

  if (f && f.mass > wetMass.current) wetMass.current = f.mass;
  const prop = f ? Math.max(0, f.mass - ASSUMED_DRY_MASS_KG) : null;
  const propFull = Math.max(1, wetMass.current - ASSUMED_DRY_MASS_KG);

  const tiltLevel = levelFor(f?.tilt_deg, 0.7 * maxTilt, 0.9 * maxTilt);
  const devLevel = levelFor(f?.trajectory_deviation_m, 0.7 * maxDev, 0.9 * maxDev);
  const ageLevel = levelFor(f?.position_age_s, 5, 15);

  return (
    <section className="panel margins" aria-label="Termination margins">
      <h2 className="panel-title">Margins <span className="title-note">flight ends past a limit</span></h2>
      <div className="panel-body">
        <Readout label="Tilt" value={num(f?.tilt_deg, 1)} unit={`/ ${num(maxTilt, 0)} °`} level={tiltLevel} note={limitNote}>
          <LimitBar label="Tilt against limit" value={f?.tilt_deg} max={maxTilt} cautionAt={0.7 * maxTilt} warnAt={0.9 * maxTilt} />
        </Readout>
        <Readout label="Off trajectory" value={num(f?.trajectory_deviation_m, 2)} unit={`/ ${num(maxDev, 0)} m`} level={devLevel} note={limitNote}>
          <LimitBar label="Trajectory deviation against limit" value={f?.trajectory_deviation_m} max={maxDev} cautionAt={0.7 * maxDev} warnAt={0.9 * maxDev} />
        </Readout>
        <Readout label="Position fix age" value={num(f?.position_age_s, 2)} unit="s" level={ageLevel} note="caution 5 s · limit 15 s">
          <LimitBar label="Position fix age" value={f?.position_age_s} max={15} cautionAt={5} warnAt={15} />
        </Readout>
        <Readout label="Uplink age" value={num(f?.link_age_s, 2)} unit="s" level={levelFor(f?.link_age_s, 1, 3)} note="since the vehicle heard us" />
        <Readout label="Mass" value={num(f?.mass, 1)} unit="kg" note={`propellant ≈ ${num(prop, 1)} kg`}>
          <div
            className="massbar"
            role="meter"
            aria-label="Propellant remaining (estimate)"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={prop === null ? undefined : Math.round((prop / propFull) * 100)}
            title={`Estimate: assumes ${ASSUMED_DRY_MASS_KG} kg dry mass and a full tank at the highest mass seen this session`}
          >
            <div className="massbar-fill" style={{ transform: `scaleX(${prop === null ? 0 : Math.min(1, prop / propFull)})` }} />
          </div>
        </Readout>
      </div>
    </section>
  );
}

/** Raw state vector, for the people debugging the estimator. */
export function StateEstimate() {
  useTick();
  const f = tele.flight;
  return (
    <div className="state-body">
    <table className="vec" data-live>
      <thead>
        <tr><th /><th>x</th><th>y</th><th>z</th><th /></tr>
      </thead>
      <tbody>
        <tr>
          <th>Position</th>
          {[0, 1, 2].map((i) => <td key={i}>{signed(f?.position[i], 2)}</td>)}
          <td className="unit">m</td>
        </tr>
        <tr>
          <th>Velocity</th>
          {[0, 1, 2].map((i) => <td key={i}>{signed(f?.velocity[i], 2)}</td>)}
          <td className="unit">m/s</td>
        </tr>
        <tr>
          <th>Angular rate</th>
          {[0, 1, 2].map((i) => <td key={i}>{signed(f?.angular_velocity[i], 2)}</td>)}
          <td className="unit">rad/s</td>
        </tr>
        <tr>
          <th>Roll · pitch · yaw</th>
          {(f ? quatToEulerDeg(f.attitude) : [null, null, null]).map((v, i) => <td key={i}>{signed(v, 1)}</td>)}
          <td className="unit">°</td>
        </tr>
      </tbody>
    </table>
    </div>
  );
}

export function stateSummary(): string {
  const f = tele.flight;
  if (!f) return "no telemetry";
  const [roll, pitch, yaw] = quatToEulerDeg(f.attitude);
  return `xyz ${signed(f.position[0], 1)} ${signed(f.position[1], 1)} ${signed(f.position[2], 1)} m · r/p/y ${signed(roll, 0)} ${signed(pitch, 0)} ${signed(yaw, 0)} °`;
}

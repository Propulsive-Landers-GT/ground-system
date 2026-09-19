import { useRef, type ReactNode } from "react";
import { useTick, useUi } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { num, signed } from "../../lib/format";
import { norm3, quatToEulerDeg } from "../../lib/math";
import { LimitBar, levelFor, type Level } from "./LimitBar";

// Not carried by the protocol. state.rs uses 50 kg; shown only as a rough propellant gauge.
export const ASSUMED_DRY_MASS_KG = 50;
const LEVEL_WORD: Record<Level, string> = { nominal: "", caution: "CAUTION", warning: "WARNING", none: "" };

function Readout(p: {
  label: string; value: string; unit: string; big?: boolean; level?: Level; note?: ReactNode; children?: ReactNode;
}) {
  return (
    <div className="readout" data-big={p.big || undefined} data-level={p.level}>
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

export function Instruments() {
  useTick();
  const params = useUi((s) => s.params);
  const hasFlight = useUi((s) => s.hasFlight);
  const stale = useUi((s) => s.stale);
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
    <section className="panel instruments" aria-label="Flight instruments">
      <h2 className="panel-title">
        Navigation
        {!hasFlight && <span className="title-note">waiting for telemetry</span>}
        {stale && <span className="title-note flag-warn">STALE {num((performance.now() - tele.flightRxMs) / 1000, 0)} s</span>}
      </h2>
      <div className="panel-scroll">
        <Readout big label="Altitude" value={num(f?.position[2], 2)} unit="m" />
        <Readout big label="Vertical speed" value={signed(f?.velocity[2], 2)} unit="m/s" />
        <div className="readout-pair">
          <Readout label="Offset from pad" value={num(f ? Math.hypot(f.position[0], f.position[1]) : null, 2)} unit="m" />
          <Readout label="Speed" value={num(f ? norm3(f.velocity) : null, 2)} unit="m/s" />
        </div>

        <h3 className="sub-title">Termination margins</h3>
        <Readout label="Tilt" value={num(f?.tilt_deg, 1)} unit={`/ ${num(maxTilt, 0)} °`} level={tiltLevel} note={limitNote}>
          <LimitBar label="Tilt against limit" value={f?.tilt_deg} max={maxTilt} cautionAt={0.7 * maxTilt} warnAt={0.9 * maxTilt} />
        </Readout>
        <Readout label="Trajectory deviation" value={num(f?.trajectory_deviation_m, 2)} unit={`/ ${num(maxDev, 0)} m`} level={devLevel} note={limitNote}>
          <LimitBar label="Trajectory deviation against limit" value={f?.trajectory_deviation_m} max={maxDev} cautionAt={0.7 * maxDev} warnAt={0.9 * maxDev} />
        </Readout>
        <Readout label="Position fix age" value={num(f?.position_age_s, 2)} unit="s" level={ageLevel} note="5 s caution, 15 s limit">
          <LimitBar label="Position fix age" value={f?.position_age_s} max={15} cautionAt={5} warnAt={15} />
        </Readout>
        <Readout label="Vehicle uplink age" value={num(f?.link_age_s, 2)} unit="s" level={levelFor(f?.link_age_s, 1, 3)} />

        <h3 className="sub-title">Mass</h3>
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

        <h3 className="sub-title">State estimate</h3>
        <table className="vec" data-live>
          <tbody>
            <tr>
              <th>Pos x y z</th>
              {[0, 1, 2].map((i) => <td key={i}>{signed(f?.position[i], 1)}</td>)}
              <td className="unit">m</td>
            </tr>
            <tr>
              <th>Vel x y z</th>
              {[0, 1, 2].map((i) => <td key={i}>{signed(f?.velocity[i], 1)}</td>)}
              <td className="unit">m/s</td>
            </tr>
            <tr>
              <th>Roll pitch yaw</th>
              {(f ? quatToEulerDeg(f.attitude) : [null, null, null]).map((v, i) => <td key={i}>{signed(v, 1)}</td>)}
              <td className="unit">°</td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>
  );
}

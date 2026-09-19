import { useTick } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { num, signed } from "../../lib/format";

function Ok({ label, ok }: { label: string; ok: boolean | undefined }) {
  const state = ok === undefined ? "none" : ok ? "ok" : "fail";
  return (
    <span className="oklight" data-state={state}>
      <span className="ok-mark" aria-hidden="true">{ok === undefined ? "—" : ok ? "✓" : "×"}</span>
      {label}
      <span className="sr-only">{ok === undefined ? "no data" : ok ? "ok" : "fault"}</span>
    </span>
  );
}

export function SensorHealth() {
  useTick();
  const s = tele.flight?.sensors;
  return (
    <section className="panel sensors" aria-label="Sensor health">
      <h2 className="panel-title">Sensors</h2>
      <div className="sens-body">
        <div className="ok-row">
          <Ok label="IMU" ok={s?.imu_ok} />
          <Ok label="GPS" ok={s?.gps_ok} />
          <Ok label="UWB" ok={s?.uwb_ok} />
        </div>
        <dl className="kvgrid" data-live>
          <dt>Chamber</dt>
          <dd>{num(s?.chamber_pressure, 1)}<span className="unit">bar</span></dd>
          <dt>Tank</dt>
          <dd>{num(s?.tank_pressure, 1)}<span className="unit">bar</span></dd>
        </dl>
        <table className="vec" data-live>
          <thead>
            <tr><th /><th>x</th><th>y</th><th>z</th><th /></tr>
          </thead>
          <tbody>
            <tr>
              <th>Accel</th>
              {[0, 1, 2].map((i) => <td key={i}>{signed(s?.accel[i], 2)}</td>)}
              <td className="unit">m/s²</td>
            </tr>
            <tr>
              <th>Gyro</th>
              {[0, 1, 2].map((i) => <td key={i}>{signed(s?.gyro[i], 2)}</td>)}
              <td className="unit">rad/s</td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>
  );
}

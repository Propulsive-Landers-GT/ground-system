import { Plot } from "../Plot";
import { F, tele } from "../../store/telemetry";
import { THRUST_MAX_N, THRUST_MIN_THROTTLE_N } from "../../protocol";

const b = tele.flightBuf;
const cols = (...c: number[]) => () => [b.view(F.t), ...c.map((i) => b.view(i))];

export function FlightPlots() {
  return (
    <section className="plots plots-flight" aria-label="Flight plots, last 60 seconds">
      <Plot syncKey="flight" title="Altitude" unit="m" getData={cols(F.zRef, F.zTruth, F.z)}
        series={[
          { label: "ref", color: "--s-ref", dash: [4, 3] },
          { label: "truth", color: "--s-truth" },
          { label: "est", color: "--s1" },
        ]} />
      <Plot syncKey="flight" title="Velocity" unit="m/s" getData={cols(F.vx, F.vy, F.vz)}
        series={[{ label: "x", color: "--s2" }, { label: "y", color: "--s3" }, { label: "z", color: "--s1" }]} />
      <Plot syncKey="flight" title="Attitude" unit="°" getData={cols(F.roll, F.pitch, F.yaw, F.tilt)}
        series={[
          { label: "roll", color: "--s2" }, { label: "pitch", color: "--s3" },
          { label: "yaw", color: "--s4" }, { label: "tilt", color: "--s1" },
        ]} />
      <Plot syncKey="flight" title="Gimbal" unit="°" getData={cols(F.gTheta, F.gPhi)} yRange={[-16, 16]} marks={[-15, 15]}
        series={[{ label: "θ", color: "--s1" }, { label: "φ", color: "--s2" }]} />
      <Plot syncKey="flight" title="Thrust" unit="N" getData={cols(F.thrust)} yRange={[0, 1300]} marks={[THRUST_MIN_THROTTLE_N, THRUST_MAX_N]}
        series={[{ label: "cmd", color: "--s1" }]} />
      <Plot syncKey="flight" title="Trajectory deviation" unit="m" getData={cols(F.dev)}
        series={[{ label: "dev", color: "--s1" }]} />
    </section>
  );
}

import { Plot } from "../Plot";
import { PlotStrip, type PlotDef } from "../PlotStrip";
import { F, tele } from "../../store/telemetry";
import { THRUST_MAX_N, THRUST_MIN_THROTTLE_N } from "../../protocol";

const b = tele.flightBuf;
const cols = (...c: number[]) => () => [b.view(F.t), ...c.map((i) => b.view(i))];

// Series colours follow the entity, never the rank: z / est / thrust are always --s1, x --s2, y --s4, and so on.
const PLOTS: PlotDef[] = [
  {
    id: "altitude", label: "Altitude",
    render: () => (
      <Plot syncKey="flight" title="Altitude" unit="m" getData={cols(F.zRef, F.zTruth, F.z)}
        series={[
          { label: "reference", color: "--s-ref", dash: [5, 4] },
          { label: "sim truth", color: "--s-truth" },
          { label: "estimate", color: "--s1" },
        ]} />
    ),
  },
  {
    id: "thrust", label: "Thrust",
    render: () => (
      <Plot syncKey="flight" title="Thrust" unit="N" getData={cols(F.thrust)} yRange={[0, 1300]} marks={[THRUST_MIN_THROTTLE_N, THRUST_MAX_N]}
        series={[{ label: "command", color: "--s1" }]} />
    ),
  },
  {
    id: "velocity", label: "Velocity",
    render: () => (
      <Plot syncKey="flight" title="Velocity" unit="m/s" getData={cols(F.vx, F.vy, F.vz)}
        series={[{ label: "x", color: "--s2" }, { label: "y", color: "--s4" }, { label: "z", color: "--s1" }]} />
    ),
  },
  {
    id: "attitude", label: "Attitude",
    render: () => (
      <Plot syncKey="flight" title="Attitude" unit="°" getData={cols(F.roll, F.pitch, F.yaw, F.tilt)}
        series={[
          { label: "roll", color: "--s2" }, { label: "pitch", color: "--s4" },
          { label: "yaw", color: "--s3" }, { label: "tilt", color: "--s1", width: 2.2 },
        ]} />
    ),
  },
  {
    id: "gimbal", label: "Gimbal",
    render: () => (
      <Plot syncKey="flight" title="Gimbal" unit="°" getData={cols(F.gTheta, F.gPhi)} yRange={[-16, 16]} marks={[-15, 15]}
        series={[{ label: "θ", color: "--s1" }, { label: "φ", color: "--s3" }]} />
    ),
  },
  {
    id: "deviation", label: "Off trajectory",
    render: () => (
      <Plot syncKey="flight" title="Off trajectory" unit="m" getData={cols(F.dev)}
        series={[{ label: "deviation", color: "--s1" }]} />
    ),
  },
];

export function FlightPlots() {
  return <PlotStrip view="flight" plots={PLOTS} label="Flight plots, last 60 seconds" />;
}

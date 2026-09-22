import { Plot } from "../Plot";
import { standCol, tele } from "../../store/telemetry";
import { CHANNEL_LABEL, type StandChannel } from "../../protocol";

const b = tele.standBuf;
const cols = (...ch: StandChannel[]) => () => [b.view(0), ...ch.map((c) => b.view(standCol(c)))];
const PT: StandChannel[] = ["Opt", "Ipt", "Ept", "M1", "M2", "Pupt", "Lfpt"];

export function StandPlots() {
  return (
    <section className="plots plots-stand" aria-label="Test stand plots, last 60 seconds">
      <Plot syncKey="stand" title="Pressures" unit="bar" getData={cols(...PT)}
        series={PT.map((c, i) => ({ label: CHANNEL_LABEL[c], color: `--s${i + 1}` }))} />
      <Plot syncKey="stand" title="Temperatures" unit="°C" getData={cols("T1", "T2")}
        series={[{ label: "T1", color: "--s1" }, { label: "T2", color: "--s2" }]} />
      {/* Load cells: the two force channels share an axis; mass gets its own plot so kg never sits on an N axis. */}
      <Plot syncKey="stand" title="Load cells" unit="N" getData={cols("Thrust", "RcsThrust")}
        series={[{ label: "thrust", color: "--s1" }, { label: "RCS", color: "--s4" }]} />
      <Plot syncKey="stand" title="N2O mass" unit="kg" getData={cols("NitrousMass")}
        series={[{ label: "load cell", color: "--s3" }]} />
    </section>
  );
}

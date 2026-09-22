import { Plot } from "../Plot";
import { PlotStrip, type PlotDef } from "../PlotStrip";
import { standCol, tele } from "../../store/telemetry";
import { CHANNEL_LABEL, type StandChannel } from "../../protocol";

const b = tele.standBuf;
const cols = (...ch: StandChannel[]) => () => [b.view(0), ...ch.map((c) => b.view(standCol(c)))];

// Pressures are split along the plant so no plot carries more than four series (the categorical palette
// has four validated slots). Feed side: run tank, GN2 pressurant, igniter line. Engine side: injector,
// chamber exit and the two manifold taps. Colour follows the tap, never its rank in a plot.
const FEED: StandChannel[] = ["Opt", "Pupt", "Lfpt"];
const ENGINE: StandChannel[] = ["Ipt", "Ept", "M1", "M2"];
const SLOT: Partial<Record<StandChannel, string>> = {
  Opt: "--s1", Pupt: "--s2", Lfpt: "--s3",
  Ipt: "--s1", Ept: "--s4", M1: "--s2", M2: "--s3",
  T1: "--s1", T2: "--s3",
  Thrust: "--s1", RcsThrust: "--s3", NitrousMass: "--s4",
};
const series = (chs: StandChannel[]) => chs.map((c) => ({ label: CHANNEL_LABEL[c], color: SLOT[c] ?? "--s1" }));

const PLOTS: PlotDef[] = [
  { id: "feed", label: "Feed pressures", render: () => <Plot syncKey="stand" title="Feed pressures" unit="bar" getData={cols(...FEED)} series={series(FEED)} /> },
  { id: "engine", label: "Engine pressures", render: () => <Plot syncKey="stand" title="Engine pressures" unit="bar" getData={cols(...ENGINE)} series={series(ENGINE)} /> },
  // Load cells: the two force channels share an axis; mass gets its own plot so kg never sits on an N axis.
  { id: "loadcells", label: "Load cells", render: () => <Plot syncKey="stand" title="Load cells" unit="N" getData={cols("Thrust", "RcsThrust")} series={series(["Thrust", "RcsThrust"])} /> },
  { id: "mass", label: "N2O mass", render: () => <Plot syncKey="stand" title="N2O mass" unit="kg" getData={cols("NitrousMass")} series={series(["NitrousMass"])} /> },
  { id: "temps", label: "Temperatures", render: () => <Plot syncKey="stand" title="Temperatures" unit="°C" getData={cols("T1", "T2")} series={series(["T1", "T2"])} /> },
];

export function StandPlots() {
  return <PlotStrip view="stand" plots={PLOTS} label="Test stand plots, last 60 seconds" />;
}

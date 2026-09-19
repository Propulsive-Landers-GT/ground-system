import { useEffect, useMemo, useState } from "react";
import { useUi } from "../../store/ui";
import { sendCommand } from "../../ws";
import { gates } from "../../lib/interlocks";
import { useCtx } from "../shell/CommandPanel";
import { MPC_INPUT_NAMES, MPC_STATE_NAMES, type FlightParams, type MpcWeights } from "../../protocol";

type FP = Record<keyof FlightParams, string>;
type W = { q: string[]; r: string[]; qn: string[] };

const FP_FIELDS: { key: keyof FlightParams; label: string; unit: string; min: number; max: number }[] = [
  { key: "hover_altitude_m", label: "Hover altitude", unit: "m", min: 0, max: 500 },
  { key: "hover_duration_s", label: "Hover duration", unit: "s", min: 0, max: 600 },
  { key: "max_tilt_deg", label: "Max tilt", unit: "°", min: 0.1, max: 90 },
  { key: "max_trajectory_deviation_m", label: "Max trajectory deviation", unit: "m", min: 0.1, max: 1000 },
];
const GROUPS: { name: string; from: number; to: number }[] = [
  { name: "position", from: 0, to: 3 },
  { name: "quaternion", from: 3, to: 7 },
  { name: "velocity", from: 7, to: 10 },
  { name: "angular rate", from: 10, to: 13 },
];

const fmt = (v: number) => String(Number(v.toPrecision(7)));
const fpToText = (p: FlightParams): FP => ({
  hover_altitude_m: fmt(p.hover_altitude_m), hover_duration_s: fmt(p.hover_duration_s),
  max_tilt_deg: fmt(p.max_tilt_deg), max_trajectory_deviation_m: fmt(p.max_trajectory_deviation_m),
});
const wToText = (w: MpcWeights | null): W => ({
  q: MPC_STATE_NAMES.map((_, i) => (w ? fmt(w.q[i]) : "")),
  r: MPC_INPUT_NAMES.map((_, i) => (w ? fmt(w.r[i]) : "")),
  qn: MPC_STATE_NAMES.map((_, i) => (w ? fmt(w.qn[i]) : "")),
});

function parse(text: string, min = -Infinity, max = Infinity): number | null {
  if (text.trim() === "") return null;
  const v = Number(text);
  return Number.isFinite(v) && v >= min && v <= max ? v : null;
}

function NumField(p: { id: string; label: string; value: string; onChange: (v: string) => void; valid: boolean; dirty: boolean; unit?: string }) {
  return (
    <label className="numfield" data-invalid={!p.valid || undefined} data-dirty={p.dirty || undefined} htmlFor={p.id}>
      <span className="numfield-label">{p.label}</span>
      <input id={p.id} inputMode="decimal" spellCheck={false} autoComplete="off" value={p.value}
        aria-invalid={!p.valid || undefined} onChange={(e) => p.onChange(e.target.value)} />
      {p.unit && <span className="unit">{p.unit}</span>}
    </label>
  );
}

export function TuningDrawer() {
  const params = useUi((s) => s.params);
  const rev = useUi((s) => s.paramsRev);
  const gate = gates.tuning(useCtx());
  const [fp, setFp] = useState<FP | null>(null);
  const [w, setW] = useState<W>(wToText(null));
  const [touchedFp, setTouchedFp] = useState(false);
  const [touchedW, setTouchedW] = useState(false);

  // Load from the vehicle whenever it reports params, unless the operator has unsent edits.
  useEffect(() => {
    if (!params) return;
    if (!touchedFp) setFp(fpToText(params.flight));
    if (!touchedW) setW(wToText(params.manual_mpc_weights));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rev]);

  const base = useMemo(() => (params ? { fp: fpToText(params.flight), w: wToText(params.manual_mpc_weights) } : null), [params]);

  const fpValid = FP_FIELDS.map((f) => fp !== null && parse(fp[f.key], f.min, f.max) !== null);
  const fpAllValid = fp !== null && fpValid.every(Boolean);
  const fpDirty = !!(fp && base && FP_FIELDS.some((f) => fp[f.key] !== base.fp[f.key]));
  const wValid = (k: keyof W, i: number) => parse(w[k][i], 0) !== null;
  const wAllValid = (["q", "r", "qn"] as const).every((k) => w[k].every((_, i) => wValid(k, i)));
  const wDirty = !!(base && (["q", "r", "qn"] as const).some((k) => w[k].some((v, i) => v !== base.w[k][i])));

  const applyFp = () => {
    if (!fp || !fpAllValid || !gate.ok) return;
    const out = Object.fromEntries(FP_FIELDS.map((f) => [f.key, Number(fp[f.key])])) as unknown as FlightParams;
    if (sendCommand({ SetFlightParams: out })) setTouchedFp(false);
  };
  const applyW = () => {
    if (!wAllValid || !gate.ok) return;
    const out: MpcWeights = { q: w.q.map(Number), r: w.r.map(Number), qn: w.qn.map(Number) };
    if (sendCommand({ SetMpcWeights: out })) setTouchedW(false);
  };
  const restore = () => {
    if (gate.ok && sendCommand({ SetMpcWeights: null })) setTouchedW(false);
  };
  const revert = () => {
    if (!base) return;
    setFp(base.fp); setW(base.w); setTouchedFp(false); setTouchedW(false);
  };
  const setWi = (k: keyof W, i: number, v: string) => {
    setTouchedW(true);
    setW((old) => ({ ...old, [k]: old[k].map((x, j) => (j === i ? v : x)) }));
  };

  const dis = (on: boolean) => ({ "aria-disabled": !on || undefined, "data-disabled": !on || undefined });

  return (
    <div className="drawer-body tuning">
      <div className="drawer-actions">
        <button className="btn btn-quiet" {...dis(gate.ok)} title={gate.ok ? "Ask the vehicle for its current parameters" : gate.why}
          onClick={() => gate.ok && sendCommand("RequestParams")}>Refresh</button>
        <button className="btn btn-quiet" {...dis(fpDirty || wDirty)} onClick={revert}>Discard edits</button>
        {!params && <span className="hint">No parameters received yet. Press Refresh.</span>}
      </div>

      <fieldset className="fieldset">
        <legend>Flight parameters {fpDirty && <span className="dirty">edited, not applied</span>}</legend>
        <div className="fp-grid">
          {FP_FIELDS.map((f, i) => (
            <NumField key={f.key} id={`fp-${f.key}`} label={f.label} unit={f.unit} value={fp?.[f.key] ?? ""}
              valid={fp === null || fpValid[i]} dirty={!!(fp && base && fp[f.key] !== base.fp[f.key])}
              onChange={(v) => { setTouchedFp(true); setFp((o) => ({ ...(o ?? { hover_altitude_m: "", hover_duration_s: "", max_tilt_deg: "", max_trajectory_deviation_m: "" }), [f.key]: v })); }} />
          ))}
        </div>
        {fp && !fpAllValid && <p className="field-error" role="alert">Enter a number within range in every field.</p>}
        <button className="btn btn-primary" {...dis(gate.ok && fpAllValid && fpDirty)} title={gate.ok ? undefined : gate.why} onClick={applyFp}>
          Apply flight parameters
        </button>
      </fieldset>

      <fieldset className="fieldset">
        <legend>
          MPC weights
          <span className="weights-state">{params ? (params.manual_mpc_weights ? "vehicle is using manual weights" : "vehicle is using built-in weights") : ""}</span>
          {wDirty && <span className="dirty">edited, not applied</span>}
        </legend>
        <table className="weights">
          <thead>
            <tr><th scope="col">state</th><th scope="col">Q</th><th scope="col">Qn (terminal)</th></tr>
          </thead>
          {GROUPS.map((g) => (
            <tbody key={g.name}>
              <tr className="group-row"><th colSpan={3} scope="rowgroup">{g.name}</th></tr>
              {MPC_STATE_NAMES.slice(g.from, g.to).map((name, j) => {
                const i = g.from + j;
                return (
                  <tr key={name}>
                    <th scope="row">{name}</th>
                    {(["q", "qn"] as const).map((k) => (
                      <td key={k}>
                        <input className="winput" inputMode="decimal" aria-label={`${k === "q" ? "Q" : "Qn"} ${name}`} value={w[k][i]}
                          data-invalid={(w[k][i] !== "" || touchedW) && !wValid(k, i) || undefined}
                          data-dirty={base && w[k][i] !== base.w[k][i] || undefined}
                          onChange={(e) => setWi(k, i, e.target.value)} />
                      </td>
                    ))}
                  </tr>
                );
              })}
            </tbody>
          ))}
          <tbody>
            <tr className="group-row"><th colSpan={3} scope="rowgroup">input cost R</th></tr>
            {MPC_INPUT_NAMES.map((name, i) => (
              <tr key={name}>
                <th scope="row">{name}</th>
                <td>
                  <input className="winput" inputMode="decimal" aria-label={`R ${name}`} value={w.r[i]}
                    data-invalid={(w.r[i] !== "" || touchedW) && !wValid("r", i) || undefined}
                    data-dirty={base && w.r[i] !== base.w.r[i] || undefined}
                    onChange={(e) => setWi("r", i, e.target.value)} />
                </td>
                <td />
              </tr>
            ))}
          </tbody>
        </table>
        {touchedW && !wAllValid && <p className="field-error" role="alert">All 29 weights must be numbers ≥ 0 before they can be applied.</p>}
        <div className="row-actions">
          <button className="btn btn-primary" {...dis(gate.ok && wAllValid && (wDirty || !params?.manual_mpc_weights))} title={gate.ok ? undefined : gate.why} onClick={applyW}>
            Apply weights
          </button>
          <button className="btn" {...dis(gate.ok)} title={gate.ok ? "Vehicle goes back to its per-phase built-in weights" : gate.why} onClick={restore}>
            Restore built-in
          </button>
        </div>
      </fieldset>
    </div>
  );
}

import { FLIGHT_PHASES } from "../../protocol";
import { useTick, useUi, type Drawer } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { duration } from "../../lib/format";

export function PhaseBar() {
  const phase = useUi((s) => s.phase);
  const terminated = useUi((s) => s.terminated);
  const mode = useUi((s) => s.controlMode);
  const view = useUi((s) => s.view);
  const drawer = useUi((s) => s.drawer);
  const idx = phase ? FLIGHT_PHASES.indexOf(phase) : -1;

  const toggle = (d: Exclude<Drawer, null>) => useUi.setState({ drawer: drawer === d ? null : d });

  return (
    <div className="phasebar">
      <ol className="stepper" aria-label="Flight phase" data-terminated={terminated || undefined}>
        {FLIGHT_PHASES.map((p, i) => (
          <li
            key={p}
            className="step"
            data-state={i === idx ? "current" : i < idx ? "done" : "todo"}
            aria-current={i === idx ? "step" : undefined}
          >
            <span className="step-name">{p}</span>
            {i === idx && <PhaseTime />}
          </li>
        ))}
      </ol>
      <span className="mode" data-mode={mode ?? "none"} title="Control mode reported by the vehicle">
        {mode === "Jog" ? "JOG" : mode === "Auto" ? "AUTO" : "—"}
      </span>
      {view === "flight" && (
        <div className="drawer-toggles">
          <button className="btn btn-quiet" aria-expanded={drawer === "tuning"} onClick={() => toggle("tuning")}>
            Tuning
          </button>
          <button className="btn btn-quiet" aria-expanded={drawer === "jog"} onClick={() => toggle("jog")}>
            Jog
          </button>
        </div>
      )}
    </div>
  );
}

function PhaseTime() {
  useTick();
  return <span className="step-time" data-live>{duration(tele.flight?.phase_time_s)}</span>;
}

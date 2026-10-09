import { FLIGHT_PHASES } from "../../protocol";
import { useTick, useUi } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { duration } from "../../lib/format";
import { phaseMeaning } from "../../lib/phases";

const MODE_WORD = { Safe: "Safe", Armed: "Armed", Sequence: "Sequence" } as const;

/** Flight state strip: the stepper, the control mode and one sentence about what the vehicle is doing. */
export function PhaseBar() {
  const phase = useUi((s) => s.phase);
  const terminated = useUi((s) => s.terminated);
  const mode = useUi((s) => s.controlMode);
  const params = useUi((s) => s.params);
  const standMode = useUi((s) => s.standStatus?.mode ?? null);
  const standConfigured = useUi((s) => (s.link?.stand ?? null) !== null);
  const idx = phase ? FLIGHT_PHASES.indexOf(phase) : -1;

  return (
    <div className="phasebar strip" role="group" aria-label="Flight state">
      <div className="strip-row">
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
      </div>
      <div className="strip-row strip-sub">
        <p className="phase-meaning" aria-live="polite">
          {phaseMeaning(phase, terminated, params?.flight.hover_altitude_m, params?.flight.hover_duration_s)}
        </p>
        {standConfigured && (
          <span className="xref" data-mode={standMode ?? "none"} title="Test stand mode (see the Test stand tab)">
            Test stand <b>{standMode ? MODE_WORD[standMode] : "—"}</b>
          </span>
        )}
      </div>
    </div>
  );
}

function PhaseTime() {
  useTick();
  return <span className="step-time" data-live>{duration(tele.flight?.phase_time_s)}</span>;
}

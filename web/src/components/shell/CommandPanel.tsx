import { useTick, useUi } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { gates } from "../../lib/interlocks";
import { duration } from "../../lib/format";
import { sendCommand } from "../../ws";
import { HoldButton } from "./HoldButton";
import { Reason, why } from "./Reason";

export function useCtx() {
  const ws = useUi((s) => s.ws);
  const source = useUi((s) => s.source);
  const phase = useUi((s) => s.phase);
  const controlMode = useUi((s) => s.controlMode);
  return { ws, source, phase, controlMode };
}

/**
 * Flight commands, top to bottom in the order an operator uses them: where the vehicle is, what can be
 * done now (pad, then in flight, then ground checkout), and the abort set apart at the bottom.
 * Greyed-out controls say why inline; the vehicle remains the authority and acks say the rest.
 */
export function CommandPanel() {
  const ctx = useCtx();
  const arm = gates.arm(ctx);
  const disarm = gates.disarm(ctx);
  const launch = gates.launch(ctx);
  const setPhase = gates.setPhase(ctx);
  const abort = gates.abort(ctx);
  const tuning = gates.tuning(ctx);
  const drawer = useUi((s) => s.drawer);
  const terminated = useUi((s) => s.terminated);
  const replay = ctx.source === "Replay";

  // One reason per group. Pad: nothing to explain while Arm or Disarm is available; otherwise the
  // most useful sentence (Launch's "Arm first" in Standby, Arm's phase rule in flight).
  const padReason = arm.ok || disarm.ok ? (launch.ok ? null : launch) : arm;
  const toggle = (d: "tuning" | "jog") => useUi.setState({ drawer: drawer === d ? null : d });

  return (
    <section className="panel commandpanel" aria-label="Vehicle commands">
      <h2 className="panel-title">
        Vehicle
        {replay && <span className="title-note">commands off in replay</span>}
      </h2>

      <div className="cmd-state" data-terminated={terminated || undefined}>
        <span className="state-word">{terminated ? "Terminated" : ctx.phase ?? "No telemetry"}</span>
        <span className="state-sub">
          <PhaseElapsed />
          {ctx.controlMode && <> · control {ctx.controlMode.toLowerCase()}</>}
        </span>
      </div>

      <div className="cmd-group">
        <h3 className="group-title">On the pad</h3>
        <div className="cmd-grid">
          <button
            className="btn"
            aria-disabled={!arm.ok || undefined}
            data-disabled={!arm.ok || undefined}
            title={why(arm)}
            aria-description={why(arm)}
            onClick={() => arm.ok && sendCommand("Arm")}
          >
            Arm
          </button>
          <button
            className="btn"
            aria-disabled={!disarm.ok || undefined}
            data-disabled={!disarm.ok || undefined}
            title={why(disarm)}
            aria-description={why(disarm)}
            onClick={() => disarm.ok && sendCommand("Disarm")}
          >
            Disarm
          </button>
        </div>
        <HoldButton className="launch btn-primary" disabled={!launch.ok} why={why(launch)} onConfirm={() => sendCommand("Launch")} sub="Hold for 1 s">
          Launch
        </HoldButton>
        {padReason && <Reason gate={padReason} />}
      </div>

      <div className="cmd-group">
        <h3 className="group-title">In flight</h3>
        <div className="cmd-grid">
          <button
            className="btn"
            aria-disabled={!setPhase.ok || undefined}
            data-disabled={!setPhase.ok || undefined}
            title={why(setPhase) ?? "Switch to Hover and hold position"}
            aria-description={why(setPhase) ?? "Switch to Hover and hold position"}
            onClick={() => setPhase.ok && sendCommand({ SetPhase: "Hover" })}
          >
            Hold hover
          </button>
          <HoldButton
            disabled={!setPhase.ok}
            why={why(setPhase)}
            onConfirm={() => sendCommand({ SetPhase: "Descent" })}
            sub="Controlled descent · hold 1 s"
          >
            Land now
          </HoldButton>
        </div>
        <Reason gate={setPhase} />
      </div>

      <div className="cmd-group">
        <h3 className="group-title">Ground checkout</h3>
        <div className="cmd-grid">
          <button className="btn btn-quiet" aria-expanded={drawer === "tuning"} aria-disabled={!tuning.ok || undefined} data-disabled={!tuning.ok || undefined} title={why(tuning) ?? "Flight parameters and MPC weights"} onClick={() => toggle("tuning")}>
            Tuning
          </button>
          <button className="btn btn-quiet" aria-expanded={drawer === "jog"} title="Move the actuators by hand (Standby only)" onClick={() => toggle("jog")}>
            Jog
          </button>
        </div>
      </div>

      <div className="abort-zone">
        <HoldButton className="abort" disabled={!abort.ok} why={why(abort)} onConfirm={() => sendCommand("Abort")} sub="Cuts thrust. In flight the vehicle falls. Hold 1 s.">
          ABORT
        </HoldButton>
        <Reason gate={abort} />
        <p className="abort-note">To bring the vehicle down under control, use Land now instead.</p>
      </div>
    </section>
  );
}

function PhaseElapsed() {
  useTick();
  const t = tele.flight?.phase_time_s;
  return <span data-live>{t === undefined ? "—" : `${duration(t)} in phase`}</span>;
}

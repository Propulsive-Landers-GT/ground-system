import { useUi } from "../../store/ui";
import { gates, type Gate } from "../../lib/interlocks";
import { sendCommand } from "../../ws";
import { HoldButton } from "./HoldButton";

export function useCtx() {
  const ws = useUi((s) => s.ws);
  const source = useUi((s) => s.source);
  const phase = useUi((s) => s.phase);
  const controlMode = useUi((s) => s.controlMode);
  return { ws, source, phase, controlMode };
}

const why = (g: Gate) => (g.ok ? undefined : g.why);

export function CommandPanel() {
  const ctx = useCtx();
  const arm = gates.arm(ctx);
  const disarm = gates.disarm(ctx);
  const launch = gates.launch(ctx);
  const setPhase = gates.setPhase(ctx);
  const abort = gates.abort(ctx);
  const replay = ctx.source === "Replay";

  return (
    <section className="panel commandpanel" aria-label="Vehicle commands">
      <h2 className="panel-title">
        Commands
        {replay && <span className="title-note">disabled in replay</span>}
      </h2>

      <div className="cmd-grid">
        <button
          className="btn"
          aria-disabled={!arm.ok || undefined}
          data-disabled={!arm.ok || undefined}
          title={why(arm)}
          onClick={() => arm.ok && sendCommand("Arm")}
        >
          Arm
        </button>
        <button
          className="btn"
          aria-disabled={!disarm.ok || undefined}
          data-disabled={!disarm.ok || undefined}
          title={why(disarm)}
          onClick={() => disarm.ok && sendCommand("Disarm")}
        >
          Disarm
        </button>
        <HoldButton className="span2 launch" disabled={!launch.ok} why={why(launch)} onConfirm={() => sendCommand("Launch")} sub="hold 1 s">
          Launch
        </HoldButton>
      </div>

      <h3 className="sub-title">Phase override</h3>
      <div className="cmd-grid">
        <button
          className="btn"
          aria-disabled={!setPhase.ok || undefined}
          data-disabled={!setPhase.ok || undefined}
          title={why(setPhase) ?? "Switch to Hover and hold position"}
          onClick={() => setPhase.ok && sendCommand({ SetPhase: "Hover" })}
        >
          Hold hover
        </button>
        <HoldButton
          disabled={!setPhase.ok}
          why={why(setPhase)}
          onConfirm={() => sendCommand({ SetPhase: "Descent" })}
          sub="controlled descent, hold 1 s"
        >
          Land now
        </HoldButton>
      </div>

      <div className="abort-zone">
        <HoldButton className="abort" disabled={!abort.ok} why={why(abort)} onConfirm={() => sendCommand("Abort")} sub="Cuts thrust. In flight the vehicle falls. Hold 1 s.">
          ABORT
        </HoldButton>
        <p className="abort-note">To bring it down under control use Land now.</p>
      </div>
    </section>
  );
}

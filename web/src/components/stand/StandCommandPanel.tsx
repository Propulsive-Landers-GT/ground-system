// Right-rail command panel for the Test Stand tab: state, arming, sequences and the stand abort.
// Individual outputs (valves, MTV, igniter, DAQ sync) live in the view itself, beside the P&ID.

import { useEffect, useState } from "react";
import { useUi } from "../../store/ui";
import { standGates } from "../../lib/interlocks";
import { sendCommand } from "../../ws";
import { HoldButton } from "../shell/HoldButton";
import { Reason, why } from "../shell/Reason";
import { useStandCtx } from "./useStandCtx";
import { SequenceBar, firedText } from "./StandStrip";
import { shortClock, tplus } from "../../lib/format";

const MODE_WORD = { Safe: "Safe", Armed: "Armed", Sequence: "Sequence running" } as const;

export function StandCommandPanel() {
  const ctx = useStandCtx();
  const status = useUi((s) => s.standStatus);
  const arm = standGates.arm(ctx);
  const disarm = standGates.disarm(ctx);
  const start = standGates.startSequence(ctx);
  const abort = standGates.abort(ctx);

  const sequences = status?.sequences ?? [];
  const running = status?.sequence ?? null;
  const [selected, setSelected] = useState<string>("");
  useEffect(() => {
    if (sequences.length && !sequences.includes(selected)) setSelected(sequences[0]);
  }, [sequences, selected]);
  const canStart = start.ok && selected !== "";
  const startGate = start.ok && selected === "" ? { ok: false as const, why: "The stand reported no sequences." } : start;

  // Arming group: nothing to explain while Arm or Disarm is available, and the running-sequence card
  // already says why everything is locked during a sequence.
  const armReason = arm.ok || disarm.ok || ctx.mode === "Sequence" ? null : arm;

  return (
    <section className="panel commandpanel standcmd" aria-label="Test stand commands" data-mode={ctx.mode ?? undefined}>
      <h2 className="panel-title">
        Test stand
        {(!ctx.configured || ctx.standSource === "Replay" || !ctx.mode) && (
          <span className="title-note" title={!ctx.configured ? "The bridge was started without --stand; stand commands have nowhere to go" : undefined}>
            {!ctx.configured ? "no stand configured" : ctx.standSource === "Replay" ? "commands off in replay" : "waiting for status"}
          </span>
        )}
      </h2>

      <div className="cmd-state" data-mode={ctx.mode ?? "none"}>
        <span className="state-word">{ctx.configured ? (ctx.mode ? MODE_WORD[ctx.mode] : "No status") : "No stand"}</span>
        <span className="state-sub">
          {ctx.configured
            ? ctx.mode === "Safe" ? "outputs locked" : ctx.mode === "Armed" ? "outputs unlocked" : ctx.mode === "Sequence" ? "outputs locked to the sequence" : "waiting for gs-stand"
            : "start gs-bridge with --stand"}
        </span>
      </div>

      <div className="cmd-group">
        <h3 className="group-title">Arming</h3>
        <div className="cmd-grid">
          <button
            className="btn"
            aria-disabled={!arm.ok || undefined}
            data-disabled={!arm.ok || undefined}
            title={why(arm) ?? "Safe → Armed. Unlocks valve, MTV and igniter commands."}
            aria-description={why(arm) ?? "Safe to Armed. Unlocks valve, MTV and igniter commands."}
            onClick={() => arm.ok && sendCommand({ Stand: "Arm" })}
          >
            Arm
          </button>
          <button
            className="btn"
            aria-disabled={!disarm.ok || undefined}
            data-disabled={!disarm.ok || undefined}
            title={why(disarm) ?? "Back to Safe. Runs the safing list."}
            aria-description={why(disarm) ?? "Back to Safe. Runs the safing list."}
            onClick={() => disarm.ok && sendCommand({ Stand: "Disarm" })}
          >
            Disarm
          </button>
        </div>
        {armReason && <Reason gate={armReason} />}
        {ctx.configured && ctx.mode !== "Sequence" && (
          <p className="hint">Disarm runs the safing list: mains closed, igniter off, MTV 0 %, vents open.</p>
        )}
      </div>

      <div className="cmd-group">
        <h3 className="group-title">Sequence</h3>
        <div className="seq-block">
          {running ? (
            <div className="seq-running" role="status">
              <span className="seq-running-name">{running.name}</span>
              <span className="seq-running-clock val" data-live>
                {tplus(running.t_s)}
                <span className="seq-dur"> / {shortClock(running.duration_s)}</span>
              </span>
              <SequenceBar seq={running} />
              <span className="seq-next">
                {running.next_step ? <><span className="k">next</span> <b>{running.next_step[1]}</b></> : <span className="k">last step fired</span>}
              </span>
              <span className="seq-steps val">{firedText(running)} steps fired</span>
              <span className="hint">Running: only Stand abort is accepted.</span>
            </div>
          ) : (
            <>
              <label className="seq-pick">
                <span className="field-label">Sequence to run</span>
                <select
                  className="select"
                  value={selected}
                  onChange={(e) => setSelected(e.target.value)}
                  disabled={sequences.length === 0}
                  aria-label="Sequence to start"
                >
                  {sequences.length === 0 && <option value="">no sequences reported</option>}
                  {sequences.map((n) => <option key={n} value={n}>{n}</option>)}
                </select>
              </label>
              <HoldButton
                className="start-seq btn-primary"
                disabled={!canStart}
                why={why(startGate)}
                onConfirm={() => sendCommand({ Stand: { StartSequence: selected } })}
                sub="Hold for 1 s"
              >
                Start {selected || "sequence"}
              </HoldButton>
              <Reason gate={startGate} />
            </>
          )}
        </div>
      </div>

      <div className="abort-zone">
        <span className="abort-owner" aria-hidden="true">Test stand</span>
        <HoldButton
          className="abort abort-stand"
          disabled={!abort.ok}
          why={why(abort)}
          onConfirm={() => sendCommand({ Stand: "Abort" })}
          sub="Closes mains, igniter off, MTV closed, vents open. Hold 1 s."
        >
          STAND ABORT
        </HoldButton>
        <Reason gate={abort} />
        <p className="abort-note">Ends a running sequence and safes the stand. The vehicle is not affected.</p>
      </div>
    </section>
  );
}

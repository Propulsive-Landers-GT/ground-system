// State strip of the Test Stand tab: stand mode, Arduino link lamps, UDP link to gs-stand, the running
// sequence's T+ clock and progress, and one sentence about what the mode means.

import { useTick, useUi } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { num, shortClock, tplus } from "../../lib/format";
import { standMeaning } from "../../lib/phases";
import type { SequenceProgress as SequenceProgressMsg } from "../../protocol";

const MODE_WORD = { Safe: "SAFE", Armed: "ARMED", Sequence: "SEQUENCE" } as const;

export function StandStrip() {
  const standLink = useUi((s) => s.link?.stand ?? null);
  const ws = useUi((s) => s.ws);
  const status = useUi((s) => s.standStatus);
  const phase = useUi((s) => s.phase);
  const hasFlight = useUi((s) => s.hasFlight);
  const configured = standLink !== null;
  const mode = status?.mode ?? null;

  return (
    <div className="standstrip strip" data-mode={mode ?? "none"} role="group" aria-label="Test stand state">
      <div className="strip-row">
        <span className="stand-mode" data-mode={mode ?? "none"} title="Stand arming mode reported by gs-stand">
          {mode ? MODE_WORD[mode] : "—"}
        </span>
        {!configured ? (
          <span className="stand-unconfigured">
            {ws === "open" ? "No test stand on this bridge" : "No bridge"}
          </span>
        ) : (
          <>
            <span className="stand-lamps" role="group" aria-label="Arduino links">
              <Lamp label="actuation" ok={status?.actuation_link_ok ?? null} title="Serial link to the actuation Arduino (valves, igniter, DAQ sync)" />
              <Lamp label="load cells" ok={status?.loadcell_link_ok ?? null} title="Serial link to the load-cell Arduino" />
            </span>
            <StandLink />
          </>
        )}
        <SequenceProgress />
      </div>
      <div className="strip-row strip-sub">
        <p className="phase-meaning" aria-live="polite">{standMeaning(mode, configured)}</p>
        {hasFlight && (
          <span className="xref" title="Vehicle phase (see the Flight tab)">
            Vehicle <b>{phase ?? "—"}</b>
          </span>
        )}
      </div>
    </div>
  );
}

function Lamp({ label, ok, title }: { label: string; ok: boolean | null; title: string }) {
  const state = ok === null ? "unknown" : ok ? "ok" : "fail";
  return (
    <span className="oklight stand-lamp" data-state={state} title={title}>
      <span className="ok-mark" aria-hidden="true">{ok === null ? "·" : ok ? "✓" : "×"}</span>
      {label}
      <span className="sr-only">{ok === null ? " unknown" : ok ? " link up" : " link down"}</span>
    </span>
  );
}

function StandLink() {
  useTick();
  const standLink = useUi((s) => s.link?.stand ?? null);
  const hasStand = useUi((s) => s.hasStand);
  const standSource = useUi((s) => s.standSource);
  if (!standLink) return null;
  // Prefer our own measurement when the adapter's telemetry reaches this browser.
  const own = hasStand && standSource === "Stand" ? (performance.now() - tele.standRxMs) / 1000 : null;
  const age = own ?? standLink.last_rx_age_s;
  const up = standLink.connected;
  return (
    <span
      className="kv stand-link"
      data-state={up ? "up" : "down"}
      title={`UDP link from the bridge to gs-stand at ${standLink.addr}; seconds since the last packet`}
      role="status"
      aria-label={`Stand link ${up ? "up" : "down"}`}
    >
      <span className="dot" aria-hidden="true" />
      <span className="k">stand link</span>
      <span className="v w5" data-flag={age !== null && age > 1 ? "warn" : undefined}>
        {age === null ? "—" : age > 99 ? ">99" : num(age, age < 10 ? 2 : 0)}
      </span>
      <span className="u">s ago</span>
      <span className="v addr">{standLink.addr}</span>
    </span>
  );
}

/** Progress for the strip: name, T+ clock, bar, next step, fired count. */
function SequenceProgress() {
  const status = useUi((s) => s.standStatus);
  const seq = status?.sequence ?? null;
  if (!seq) {
    return (
      <span className="seqprog seqprog-idle">
        {status?.sequences?.length ? `${status.sequences.length} sequence${status.sequences.length === 1 ? "" : "s"} available` : ""}
      </span>
    );
  }
  const next = seq.next_step;
  return (
    <div className="seqprog" role="group" aria-label={`Sequence ${seq.name}`}>
      <span className="seq-name">{seq.name}</span>
      <span className="seq-clock val" data-live>{tplus(seq.t_s)}</span>
      <SequenceBar seq={seq} />
      <span className="seq-next">
        {next ? (
          <>
            <span className="k">next</span> <b>{next[1]}</b>
          </>
        ) : (
          <span className="k">last step fired</span>
        )}
      </span>
      <span className="seq-steps val" title="Steps fired so far / total">{firedText(seq)}</span>
    </div>
  );
}

/** next_step carries the 0-based index of the step still to fire, so it is also the count fired. */
export function firedText(seq: SequenceProgressMsg): string {
  return `${seq.next_step ? seq.next_step[0] : seq.steps_total}/${seq.steps_total}`;
}

export function SequenceBar({ seq }: { seq: SequenceProgressMsg }) {
  const dur = seq.duration_s > 0 ? seq.duration_s : NaN;
  const p = Number.isFinite(dur) ? Math.max(0, Math.min(1, seq.t_s / dur)) : 0;
  return (
    <div
      className="seq-bar"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={Number.isFinite(dur) ? dur : undefined}
      aria-valuenow={seq.t_s}
      aria-valuetext={`${tplus(seq.t_s)} of ${shortClock(dur)}`}
    >
      <span className="seq-bar-fill" style={{ transform: `scaleX(${p})` }} />
    </div>
  );
}

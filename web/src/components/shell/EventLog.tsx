import { useUi } from "../../store/ui";
import { shortClock } from "../../lib/format";

const SEV_MARK = { Info: "i", Warning: "!", Critical: "×" } as const;
// Sequence-step events read "T+48.7 igniter on" or "hotfire step 5/9: MTV 100 %"; the clock or the step
// counter gets its own tabular slot so the action reads at a glance.
const TPLUS = /^(T[+\-−]\s?\d+(?:\.\d+)?)\s+(.*)$/s;
const STEP = /^(.*?\bstep\s+\d+\s*\/\s*\d+):\s*(.*)$/s;

function EventText({ text }: { text: string }) {
  const t = TPLUS.exec(text);
  if (t) return <><span className="ev-tplus">{t[1].replace(/\s/, "")}</span> {t[2]}</>;
  const s = STEP.exec(text);
  if (s) return <><span className="ev-step">{s[1]}</span> <b>{s[2]}</b></>;
  return <>{text}</>;
}

export function EventLog() {
  const events = useUi((s) => s.events);
  return (
    <section className="panel log events" aria-label="Event log">
      <h2 className="panel-title">
        Events <span className="count">{events.length || ""}</span>
      </h2>
      <ol className="log-list">
        {events.length === 0 && <li className="empty">No events yet</li>}
        {events.map((e) => (
          <li key={e.key} className="log-row" data-sev={e.severity}>
            <span className="sev" aria-label={e.severity}>{SEV_MARK[e.severity] ?? "i"}</span>
            <span className="log-time">{shortClock(e.time_s)}</span>
            <span className="log-text"><EventText text={e.text} /></span>
          </li>
        ))}
      </ol>
    </section>
  );
}

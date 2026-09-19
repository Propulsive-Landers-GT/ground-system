import { useUi } from "../../store/ui";
import { shortClock } from "../../lib/format";

const SEV_MARK = { Info: "i", Warning: "!", Critical: "×" } as const;

export function EventLog() {
  const events = useUi((s) => s.events);
  return (
    <section className="panel log events" aria-label="Event log">
      <h2 className="panel-title">
        Events <span className="count">{events.length || ""}</span>
      </h2>
      <ol className="log-list">
        {events.length === 0 && <li className="empty">No events from the vehicle yet</li>}
        {events.map((e) => (
          <li key={e.key} className="log-row" data-sev={e.severity}>
            <span className="sev" aria-label={e.severity}>{SEV_MARK[e.severity] ?? "i"}</span>
            <span className="log-time">{shortClock(e.time_s)}</span>
            <span className="log-text">{e.text}</span>
          </li>
        ))}
      </ol>
    </section>
  );
}

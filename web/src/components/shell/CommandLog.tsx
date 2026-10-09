import { useUi, type CommandStatus } from "../../store/ui";

const STATUS_TEXT: Record<CommandStatus, string> = {
  sending: "sending",
  sent: "sent",
  accepted: "accepted",
  rejected: "rejected",
  noack: "no ack",
  unsent: "not sent",
};

export function CommandLog() {
  const commands = useUi((s) => s.commands);
  const lastRejection = useUi((s) => s.lastRejection);
  return (
    <section className="panel log commands" aria-label="Command log">
      <h2 className="panel-title">Command log</h2>
      <div className="sr-only" role="status" aria-live="assertive">{lastRejection}</div>
      <ol className="log-list">
        {commands.length === 0 && <li className="empty">Nothing sent this session</li>}
        {commands.map((c) => (
          <li key={c.key} className="log-row cmd-row" data-status={c.status}>
            <span className="seq">{c.seq === null ? "#—" : `#${c.seq}`}</span>
            <span className="log-text">
              {c.label}
              {c.reason && <span className="reason">{c.reason}</span>}
            </span>
            <span className="cmd-status">{STATUS_TEXT[c.status]}</span>
          </li>
        ))}
      </ol>
    </section>
  );
}

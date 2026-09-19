import { useUi } from "../../store/ui";
import { shortClock } from "../../lib/format";

export function TerminationBanner() {
  const terminated = useUi((s) => s.terminated);
  const reason = useUi((s) => s.events.find((e) => e.severity === "Critical"));
  return (
    <div className="termination" role="alert" aria-live="assertive" data-on={terminated || undefined}>
      {terminated && (
        <>
          <strong>FLIGHT TERMINATED</strong>
          <span>
            Thrust is cut and the control loop has stopped.
            {reason ? ` ${reason.text} (T+${shortClock(reason.time_s)})` : ""}
          </span>
        </>
      )}
    </div>
  );
}

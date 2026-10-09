import { useEffect } from "react";
import { useUi } from "../../store/ui";
import { TuningDrawer } from "./TuningDrawer";
import { JogPanel } from "./JogPanel";

/** Tuning / Jog drawer. Slides over the main pane from the leading edge; Esc or Close dismisses it. */
export function FlightDrawer() {
  const drawer = useUi((s) => s.drawer);

  useEffect(() => {
    if (!drawer) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") useUi.setState({ drawer: null }); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [drawer]);

  if (!drawer) return null;
  return (
    <aside className="drawer" aria-label={drawer === "tuning" ? "Tuning" : "Jog"}>
      <header className="drawer-head">
        <div>
          <h2>{drawer === "tuning" ? "Tuning" : "Actuator jog"}</h2>
          <p className="hint">{drawer === "tuning" ? "Flight parameters and MPC weights on the vehicle." : "Move the gimbal, thrust and RCS by hand. Standby only."}</p>
        </div>
        <button className="btn btn-quiet" onClick={() => useUi.setState({ drawer: null })}>Close</button>
      </header>
      {drawer === "tuning" ? <TuningDrawer /> : <JogPanel />}
    </aside>
  );
}

import { useEffect } from "react";
import { useUi } from "../../store/ui";
import { Instruments } from "./Instruments";
import { Scene3D } from "./Scene3D";
import { Actuation } from "./Actuation";
import { SensorHealth } from "./SensorHealth";
import { FlightPlots } from "./FlightPlots";
import { TuningDrawer } from "./TuningDrawer";
import { JogPanel } from "./JogPanel";

export function FlightView() {
  const drawer = useUi((s) => s.drawer);

  useEffect(() => {
    if (!drawer) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") useUi.setState({ drawer: null }); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [drawer]);

  return (
    <div className="flightview">
      <Instruments />
      <Scene3D />
      <div className="sidecol">
        <Actuation />
        <SensorHealth />
      </div>
      <FlightPlots />
      {drawer && (
        <aside className="drawer" aria-label={drawer === "tuning" ? "Tuning" : "Jog"}>
          <header className="drawer-head">
            <h2>{drawer === "tuning" ? "Tuning" : "Actuator jog"}</h2>
            <button className="btn btn-quiet" onClick={() => useUi.setState({ drawer: null })}>Close</button>
          </header>
          {drawer === "tuning" ? <TuningDrawer /> : <JogPanel />}
        </aside>
      )}
    </div>
  );
}

/*
  Layout sketch (1280×800 laptop is the target; 1440 and 1920 widen the main pane and the rail's logs).

  ┌ status bar ─────────────────────────────────────────────────────────────────────────────────────┐
  │ GTPL · Flight | Test stand · ● Link up ▾ (popover: vehicle addr, rate, lost, last rx, stand link) │
  │ SIM  STAND  · T+00:24.0 · [● REC name 0:14 Stop | Record] · Light                                 │
  ├ state strip (per view) ─────────────────────────────────────────────────────────────────────────┤
  │ Flight:  STANDBY › ARMED › ASCENT 9.4 s › HOVER › DESCENT › LANDED   AUTO   "Climbing to 30 m."   │
  │ Stand :  ARMED · actuation ✓ · load cells ✓ · stand 0.05 s   HOTFIRE T+12.2 ▬▬▬▬ next MTV 0 % 6/10│
  ├ termination banner (only when terminated) ──────────────────────────────────────────────────────┤
  ├ main (scrolls vertically) ───────────────────────────────────────────┬ rail (sticky, own scroll) ┤
  │ FLIGHT                                                               │ Commands                  │
  │  hero row: Altitude · Vertical speed · Offset from pad · Speed       │  state + one-line meaning │
  │  3D scene (primary) ──────────────────────┬ Margins: tilt, deviation,│  Arm  Disarm  (+ reason)  │
  │                                           │ fix age, uplink, propellant│  Launch (hold)            │
  │  ▸ Actuation  ▸ Sensors  ▸ State estimate (disclosures, summaries)   │  In flight: Hold hover /  │
  │  Plots: chooser chips; default Altitude + Thrust; others one click   │    Land now (hold)        │
  │                                                                      │  Checkout: Tuning… Jog…   │
  │ STAND                                                                │  ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  │
  │  P&ID with live values (primary) ──────────┬ Outputs: MTV, igniter,  │  ABORT (hold), separated  │
  │                                            │ DAQ · Load cells        │ Command log / Events      │
  │  ▸ Valves (table, all 15, Open/Close)                                │   (stacked, fill the rail)│
  │  Plots: default Feed pressures + Engine pressures + Load cells       │                           │
  └──────────────────────────────────────────────────────────────────────┴───────────────────────────┘

  Primary: Flight = scene + phase/margins; Stand = P&ID + mode/sequence. Everything else is one click
  away (disclosure, plot chip, link popover) and nothing was removed: see web/PARITY.md.
  The document itself never scrolls; the main pane does, so the rail and the strip stay put.
  Tuning and Jog drawers open over the main pane from the rail (Esc closes).
*/

import { useEffect } from "react";
import { useUi } from "./store/ui";
import { StatusBar } from "./components/shell/StatusBar";
import { PhaseBar } from "./components/shell/PhaseBar";
import { TerminationBanner } from "./components/shell/TerminationBanner";
import { CommandPanel } from "./components/shell/CommandPanel";
import { CommandLog } from "./components/shell/CommandLog";
import { EventLog } from "./components/shell/EventLog";
import { FlightView } from "./components/flight/FlightView";
import { FlightDrawer } from "./components/flight/FlightDrawer";
import { StandView } from "./components/stand/StandView";
import { StandStrip } from "./components/stand/StandStrip";
import { StandCommandPanel } from "./components/stand/StandCommandPanel";

export function App() {
  const view = useUi((s) => s.view);
  const stale = useUi((s) => s.stale);
  const hasFlight = useUi((s) => s.hasFlight);
  const hasStand = useUi((s) => s.hasStand);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT")) return;
      if (e.key === "1") useUi.setState({ view: "flight" });
      if (e.key === "2") useUi.setState({ view: "stand" });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="app" data-view={view} data-stale={stale || undefined} data-waiting={(!hasFlight && !hasStand) || undefined}>
      <StatusBar />
      {view === "flight" ? <PhaseBar /> : <StandStrip />}
      <TerminationBanner />
      <div className="stage">
        <main className="main" id={`panel-${view}`} role="tabpanel" aria-labelledby={`tab-${view}`}>
          {view === "flight" ? <FlightView /> : <StandView />}
        </main>
        {view === "flight" && <FlightDrawer />}
      </div>
      <aside className="rail" aria-label="Commands and logs">
        {/* Each tab carries its own ABORT so the two systems are never one click apart. */}
        {view === "flight" ? <CommandPanel /> : <StandCommandPanel />}
        <CommandLog />
        <EventLog />
      </aside>
    </div>
  );
}

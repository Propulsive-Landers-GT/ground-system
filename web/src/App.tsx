import { useEffect } from "react";
import { useUi } from "./store/ui";
import { StatusBar } from "./components/shell/StatusBar";
import { PhaseBar } from "./components/shell/PhaseBar";
import { TerminationBanner } from "./components/shell/TerminationBanner";
import { CommandPanel } from "./components/shell/CommandPanel";
import { CommandLog } from "./components/shell/CommandLog";
import { EventLog } from "./components/shell/EventLog";
import { FlightView } from "./components/flight/FlightView";
import { StandView } from "./components/stand/StandView";

export function App() {
  const view = useUi((s) => s.view);
  const stale = useUi((s) => s.stale);
  const hasFlight = useUi((s) => s.hasFlight);

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
    <div className="app" data-stale={stale || undefined} data-waiting={!hasFlight || undefined}>
      <StatusBar />
      <PhaseBar />
      <TerminationBanner />
      <main className="main" id={`panel-${view}`} role="tabpanel" aria-labelledby={`tab-${view}`}>
        {view === "flight" ? <FlightView /> : <StandView />}
      </main>
      <aside className="rail" aria-label="Commands and logs">
        <CommandPanel />
        <CommandLog />
        <EventLog />
      </aside>
    </div>
  );
}

import { useUi } from "../../store/ui";
import { Pid } from "./Pid";
import { ValveTable } from "./ValveTable";
import { StandPlots } from "./StandPlots";
import { StandStrip } from "./StandStrip";
import { StandOutputs } from "./StandOutputs";

export function StandView() {
  const has = useUi((s) => s.hasStand);
  const stale = useUi((s) => s.standStale);
  const source = useUi((s) => s.standSource);
  const configured = useUi((s) => (s.link?.stand ?? null) !== null);
  return (
    <div className="standview" data-stand-stale={stale || undefined} data-stand-waiting={!has || undefined}>
      <StandStrip />
      <section className="panel pidpanel" aria-label="Propulsion schematic">
        <h2 className="panel-title">
          Propulsion schematic
          {!has && <span className="title-note">waiting for stand telemetry</span>}
          {has && !stale && source && source !== "Stand" && (
            <span className="title-note" title="Valve states and sensors come from the vehicle or simulator, not from gs-stand">
              from {source === "Sim" ? "simulator" : source === "Replay" ? "replay" : "vehicle"}
              {configured ? " (stand telemetry not flowing)" : ""}
            </span>
          )}
          {stale && <span className="title-note flag-warn">STALE</span>}
        </h2>
        <Pid />
      </section>
      <div className="standcol">
        <StandOutputs />
        <ValveTable />
      </div>
      <StandPlots />
    </div>
  );
}

import { useTick } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { Pid } from "./Pid";
import { ValveTable } from "./ValveTable";
import { StandPlots } from "./StandPlots";

export function StandView() {
  useTick();
  const has = tele.stand !== null;
  const stale = has && performance.now() - tele.standRxMs > 1000;
  return (
    <div className="standview" data-stand-stale={stale || undefined}>
      <section className="panel pidpanel" aria-label="Propulsion schematic">
        <h2 className="panel-title">
          Propulsion schematic
          {!has && <span className="title-note">waiting for stand telemetry</span>}
          {stale && <span className="title-note flag-warn">STALE</span>}
        </h2>
        <Pid />
      </section>
      <ValveTable />
      <StandPlots />
    </div>
  );
}

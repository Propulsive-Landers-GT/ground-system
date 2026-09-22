import { useTick, useUi } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { Pid } from "./Pid";
import { ValveTable } from "./ValveTable";
import { StandPlots } from "./StandPlots";
import { StandOutputs, LoadCells } from "./StandOutputs";
import { Disclosure } from "../shell/Disclosure";
import { VALVE_IDS } from "../../protocol";

/**
 * Test stand tab. Primary: the P&ID with live valve states and sensor tags, with the outputs and
 * load cells beside it. The full valve table is a disclosure (its summary counts the states) and the
 * plots end the page.
 */
export function StandView() {
  const has = useUi((s) => s.hasStand);
  const stale = useUi((s) => s.standStale);
  const source = useUi((s) => s.standSource);
  const configured = useUi((s) => (s.link?.stand ?? null) !== null);
  return (
    <div className="standview" data-stand-stale={stale || undefined} data-stand-waiting={!has || undefined}>
      <div className="primary-row">
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
            {stale && <span className="title-note flag-warn">Stale</span>}
          </h2>
          <Pid />
        </section>
        <div className="standcol">
          <LoadCells />
          <StandOutputs />
        </div>
      </div>
      <Disclosure id="stand.valves" title="Valves" summary={<ValveSummary />} className="valves">
        <ValveTable />
      </Disclosure>
      <StandPlots />
    </div>
  );
}

function ValveSummary() {
  useTick();
  let open = 0, closed = 0, unknown = 0, missing = 0;
  for (const id of VALVE_IDS) {
    const st = tele.valves.get(id)?.state;
    if (st === "Open") open++; else if (st === "Closed") closed++; else if (st === "Unknown") unknown++; else missing++;
  }
  return (
    <span className="summary-line" data-live>
      <span><b className="val">{open}</b> open</span>
      <span><b className="val">{closed}</b> closed</span>
      {unknown > 0 && <span><b className="val">{unknown}</b> unknown</span>}
      {missing > 0 && <span><b className="val">{missing}</b> not reported</span>}
      <span className="summary-hint">Open to command a single valve</span>
    </span>
  );
}

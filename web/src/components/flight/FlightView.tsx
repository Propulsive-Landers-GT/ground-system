import { useTick } from "../../store/ui";
import { FlightHero, Margins, StateEstimate, stateSummary } from "./Instruments";
import { Scene3D } from "./Scene3D";
import { Actuation, ActuationSummary } from "./Actuation";
import { SensorHealth, SensorSummary } from "./SensorHealth";
import { FlightPlots } from "./FlightPlots";
import { Disclosure } from "../shell/Disclosure";

/**
 * Flight tab. Primary: the hero readouts, the 3D scene and the margins beside it. Secondary detail
 * (actuation, sensors, raw state) sits in disclosures that show their one-line summary while closed,
 * and the plot strip ends the page. The pane scrolls; the rail and the state strip do not.
 */
export function FlightView() {
  return (
    <div className="flightview">
      <FlightHero />
      <div className="primary-row">
        <Scene3D />
        <Margins />
      </div>
      <div className="details-row">
        <Disclosure id="flight.actuation" title="Actuation" summary={<ActuationSummary />}>
          <Actuation />
        </Disclosure>
        <Disclosure id="flight.sensors" title="Sensors" summary={<SensorSummary />}>
          <SensorHealth />
        </Disclosure>
        <Disclosure id="flight.state" title="State estimate" summary={<StateSummary />}>
          <StateEstimate />
        </Disclosure>
      </div>
      <FlightPlots />
    </div>
  );
}

function StateSummary() {
  useTick();
  return <span className="summary-line" data-live>{stateSummary()}</span>;
}

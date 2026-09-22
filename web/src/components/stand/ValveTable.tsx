import { useTick } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { sendCommand } from "../../ws";
import { gates } from "../../lib/interlocks";
import { useCtx } from "../shell/CommandPanel";
import { VALVE_IDS, VALVE_LABEL, VALVE_ROLE } from "../../protocol";
import { num } from "../../lib/format";

const CHIP = { Open: "O", Closed: "C", Unknown: "?", Missing: "—" } as const;
const WORD = { Open: "open", Closed: "closed", Unknown: "unknown", Missing: "no data" } as const;

export function ValveTable() {
  useTick();
  const gate = gates.valve(useCtx());
  const dis = { "aria-disabled": !gate.ok || undefined, "data-disabled": !gate.ok || undefined, title: gate.ok ? undefined : gate.why };
  return (
    <section className="panel valves" aria-label="Valve commands">
      <h2 className="panel-title">
        Valves
        <span className="title-note" title={gate.ok ? undefined : gate.why}>{gate.ok ? "commands enabled" : "locked: Standby only"}</span>
      </h2>
      <div className="panel-scroll">
        <table className="valvetable">
          <thead>
            <tr><th scope="col">Valve</th><th scope="col">State</th><th scope="col" className="numcol">Pos</th><th scope="col"><span className="sr-only">Command</span></th></tr>
          </thead>
          <tbody>
            {VALVE_IDS.map((id) => {
              const v = tele.valves.get(id);
              const state = v?.state ?? "Missing";
              return (
                <tr key={id}>
                  <th scope="row" title={VALVE_ROLE[id]}>
                    <span className="vt-name">{VALVE_LABEL[id]}</span>
                    <span className="vt-role">{VALVE_ROLE[id]}</span>
                  </th>
                  <td>
                    <span className="chip" data-state={state}><b>{CHIP[state]}</b>{WORD[state]}</span>
                  </td>
                  <td className="numcol">{v?.position_deg === null || v?.position_deg === undefined ? "—" : `${num(v.position_deg, 0)}°`}</td>
                  <td className="vt-cmd">
                    <button className="btn btn-sm" {...dis} onClick={() => gate.ok && sendCommand({ SetValve: { id, open: true } })}>Open</button>
                    <button className="btn btn-sm" {...dis} onClick={() => gate.ok && sendCommand({ SetValve: { id, open: false } })}>Close</button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

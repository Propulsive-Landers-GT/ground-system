import type { ReactNode } from "react";
import { togglePlot, useUi, type View } from "../store/ui";

export interface PlotDef {
  id: string;
  label: string;
  render: () => ReactNode;
}

/**
 * A row of plots with a chooser. The defaults are the two or three that matter during a run; every other
 * plot stays one click away. The selection is per view and persisted (store/ui.ts).
 */
export function PlotStrip({ view, plots, label }: { view: View; plots: PlotDef[]; label: string }) {
  const shown = useUi((s) => s.plots[view]);
  const visible = plots.filter((p) => shown.includes(p.id));
  return (
    <section className="plotstrip" aria-label={label}>
      <div className="plotstrip-head">
        <h2 className="panel-title plotstrip-title">Plots <span className="title-note">last 60 s</span></h2>
        <div className="chips" role="group" aria-label="Plots to show">
          {plots.map((p) => {
            const on = shown.includes(p.id);
            const last = on && shown.length === 1;
            return (
              <button
                key={p.id}
                type="button"
                className="chip-btn"
                aria-pressed={on}
                aria-disabled={last || undefined}
                title={last ? "At least one plot stays visible" : on ? `Hide ${p.label}` : `Show ${p.label}`}
                onClick={() => !last && togglePlot(view, p.id)}
              >
                {p.label}
              </button>
            );
          })}
        </div>
      </div>
      <div className="plots" data-count={Math.min(visible.length, 4)}>
        {visible.map((p) => <div key={p.id} className="plot-slot">{p.render()}</div>)}
      </div>
    </section>
  );
}

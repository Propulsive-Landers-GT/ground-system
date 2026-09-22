import { useEffect, useRef } from "react";
import uPlot from "uplot";
import { useUi } from "../store/ui";

export interface PlotSeries {
  label: string;
  /** CSS custom property holding the colour, e.g. "--s1". */
  color: string;
  dash?: number[];
  width?: number;
}

interface Props {
  title: string;
  unit: string;
  series: PlotSeries[];
  /** Returns [t, ...ys]; called on animation frames, must be cheap (subarray views). */
  getData: () => Float64Array[];
  /** Seconds of history shown. */
  span?: number;
  syncKey: string;
  /** Fixed y range, e.g. [0, 1300]. Omit to autoscale. */
  yRange?: [number, number];
  /** Horizontal reference lines in y units. */
  marks?: number[];
}

// One rAF loop feeds every mounted plot, throttled to ~30 fps; packets never trigger a draw.
const live = new Set<() => void>();
let loop = 0;
let lastDraw = 0;
function frame(now: number) {
  loop = requestAnimationFrame(frame);
  if (now - lastDraw < 33) return;
  lastDraw = now;
  for (const f of live) f();
}
function register(f: () => void) {
  live.add(f);
  if (live.size === 1) loop = requestAnimationFrame(frame);
  return () => {
    live.delete(f);
    if (live.size === 0) cancelAnimationFrame(loop);
  };
}

const cssVar = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

export function Plot({ title, unit, series, getData, span = 60, syncKey, yRange, marks }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const theme = useUi((s) => s.theme);
  const getRef = useRef(getData);
  getRef.current = getData;

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const grid = cssVar("--plot-grid");
    const axis = cssVar("--text-3");
    const font = `10px ${cssVar("--font-num")}`;
    const markColor = cssVar("--text-3");

    const opts: uPlot.Options = {
      width: Math.max(50, el.clientWidth),
      height: Math.max(40, el.clientHeight),
      pxAlign: 0,
      legend: { show: false },
      cursor: { sync: { key: syncKey }, points: { show: false }, drag: { x: false, y: false } },
      scales: {
        x: { time: false, auto: false },
        y: {
          range: yRange
            ? () => yRange
            : (u) => {
                // Own min/max over the visible window: series can be all-NaN (missing channels),
                // which must not poison the shared scale.
                const xs = u.data[0] as ArrayLike<number>;
                const x0 = u.scales.x.min ?? -Infinity;
                let min = Infinity;
                let max = -Infinity;
                for (let k = 1; k < u.data.length; k++) {
                  const ys = u.data[k] as ArrayLike<number>;
                  for (let i = xs.length - 1; i >= 0 && xs[i] >= x0; i--) {
                    const v = ys[i];
                    if (v < min) min = v;
                    if (v > max) max = v;
                  }
                }
                if (!Number.isFinite(min) || !Number.isFinite(max)) return [0, 1];
                if (max - min < 1e-3) return [min - 0.5, max + 0.5];
                const [lo, hi] = uPlot.rangeNum(min, max, 0.12, true);
                return [lo ?? 0, hi ?? 1];
              },
        },
      },
      axes: [
        {
          stroke: axis, font, size: 18, gap: 2,
          grid: { stroke: grid, width: 1 }, ticks: { show: false },
          values: (_u, vals) => vals.map((v) => `${Math.round(v)}`),
          space: 50,
        },
        {
          stroke: axis, font, size: 38, gap: 3,
          grid: { stroke: grid, width: 1 }, ticks: { show: false }, space: 22,
          values: (_u, vals) => vals.map((v) => (Math.abs(v) >= 1000 ? String(Math.round(v)) : String(+v.toFixed(2)))),
        },
      ],
      series: [
        {},
        ...series.map((s) => ({
          label: s.label,
          stroke: cssVar(s.color),
          width: s.width ?? 1.4,
          dash: s.dash,
          spanGaps: false,
          points: { show: false },
        })),
      ],
      hooks: marks
        ? {
            draw: [
              (u) => {
                const ctx = u.ctx;
                ctx.save();
                ctx.strokeStyle = markColor;
                ctx.setLineDash([2, 4]);
                ctx.lineWidth = 1;
                for (const m of marks) {
                  const y = Math.round(u.valToPos(m, "y", true)) + 0.5;
                  if (y < u.bbox.top || y > u.bbox.top + u.bbox.height) continue;
                  ctx.beginPath();
                  ctx.moveTo(u.bbox.left, y);
                  ctx.lineTo(u.bbox.left + u.bbox.width, y);
                  ctx.stroke();
                }
                ctx.restore();
              },
            ],
          }
        : undefined,
    };

    const empty = [new Float64Array(0), ...series.map(() => new Float64Array(0))];
    const u = new uPlot(opts, empty as unknown as uPlot.AlignedData, el);

    const draw = () => {
      if (document.hidden) return;
      const d = getRef.current();
      const t = d[0];
      if (!t || t.length === 0) {
        if (u.data[0].length) u.setData(empty as unknown as uPlot.AlignedData, false);
        return;
      }
      const tEnd = t[t.length - 1];
      u.batch(() => {
        u.setData(d as unknown as uPlot.AlignedData, false);
        u.setScale("x", { min: tEnd - span, max: tEnd });
      });
    };
    const unregister = register(draw);

    const ro = new ResizeObserver(() => {
      const w = el.clientWidth;
      const h = el.clientHeight;
      if (w > 0 && h > 0) u.setSize({ width: w, height: h });
    });
    ro.observe(el);

    return () => {
      unregister();
      ro.disconnect();
      u.destroy();
    };
    // Series definitions are static per plot; theme change rebuilds to pick up new colours.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [theme, syncKey, span]);

  return (
    <figure className="plot">
      <figcaption className="plot-head">
        <span className="plot-title">{title}</span>
        <span className="plot-unit">{unit}</span>
        <span className="plot-legend">
          {series.map((s) => (
            <span key={s.label} className="legend-item">
              <span className="swatch" data-dashed={s.dash ? true : undefined} style={{ color: `var(${s.color})` }} aria-hidden="true" />
              {s.label}
            </span>
          ))}
        </span>
      </figcaption>
      <div className="plot-host" ref={host} />
    </figure>
  );
}

// Schematic P&ID, drawn from the tag list in gs-protocol and the topology of the team's Visio
// P&ID (GN2 purge/pressurant row, N2O supply row, run tank, main line to the engine, GOX igniter
// row). It is a readable schematic, not a copy of the drawing.
//
// Valve convention (team standard): GREEN = OPEN, RED = CLOSED. Colour is always paired with a
// word and a shape cue (closed valves carry a blocking bar; unknown is hatched with "?").

import { useTick } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { CHANNEL_LABEL, CHANNEL_UNIT, VALVE_LABEL, outputOn, type StandChannel, type ValveId } from "../../protocol";
import { num } from "../../lib/format";

/** Commanded MTV opening from telemetry, if the sender reports one. */
const mtvPercent = (): number | null => {
  const p = tele.stand?.mtv_percent;
  return p === null || p === undefined || !Number.isFinite(p) ? null : p;
};

type Node =
  | "gn2a" | "gn2b" | "puVntOut" | "puMvntOut" | "rcs1Out" | "rcs2Out"
  | "n2oA" | "n2oB" | "oVntOut" | "tank" | "tVntOut" | "main1" | "main2"
  | "goxA" | "ig" | "lfVntOut";

const PIPES: Record<Node, string> = {
  gn2a: "M58,120 H263 M125,120 V22 H648 M610,22 V62 H648 M215,120 V87",
  puVntOut: "M215,63 V44",
  gn2b: "M287,120 H403 M345,120 V87 M345,120 V148",
  puMvntOut: "M345,63 V44",
  rcs1Out: "M672,22 H695",
  rcs2Out: "M672,62 H695",
  n2oA: "M58,300 H158 M110,300 V267",
  oVntOut: "M110,243 V224",
  n2oB: "M182,300 H258",
  tank: "M345,172 V195 M282,300 H310 M380,215 H418 M345,345 V400 H393",
  tVntOut: "M442,215 H475",
  main1: "M417,400 H558 M427,120 H520 V400",
  main2: "M582,400 H670",
  goxA: "M58,500 H188",
  ig: "M212,500 H700 V418 M380,500 V518",
  lfVntOut: "M380,542 V560",
};

const EDGES: [Node, Node, ValveId][] = [
  ["gn2a", "puVntOut", "PuVnt"], ["gn2a", "gn2b", "PuIso"], ["gn2a", "rcs1Out", "Rcs1"], ["gn2a", "rcs2Out", "Rcs2"],
  ["gn2b", "puMvntOut", "PuMvnt"], ["gn2b", "main1", "PuMv"], ["gn2b", "tank", "PuFill"],
  ["n2oA", "oVntOut", "OVnt"], ["n2oA", "n2oB", "OIso"], ["n2oB", "tank", "OFill"],
  ["tank", "tVntOut", "TVnt"], ["tank", "main1", "Omv"], ["main1", "main2", "Mtv"],
  ["goxA", "ig", "IgV"], ["ig", "lfVntOut", "LfVnt"],
];
const DEAD_ENDS: Node[] = ["puVntOut", "puMvntOut", "rcs1Out", "rcs2Out", "oVntOut", "tVntOut", "lfVntOut"];

function isOpen(id: ValveId): boolean {
  const v = tele.valves.get(id);
  if (id === "Mtv") {
    const p = mtvPercent();
    if (p !== null) return p > 0.5;
  }
  if (!v) return false;
  return v.state === "Open" || (v.state !== "Closed" && (v.position_deg ?? 0) > 1);
}

/** Segments connected to a supply (or a pressurised run tank) through open valves. */
function liveNodes(): Set<Node> {
  const live = new Set<Node>(["gn2a", "n2oA", "goxA"]);
  if ((tele.channels.get("Opt") ?? 0) > 1.5) live.add("tank");
  let grew = true;
  while (grew) {
    grew = false;
    for (const [a, b, v] of EDGES) {
      if (!isOpen(v)) continue;
      for (const [from, to] of [[a, b], [b, a]] as const) {
        if (live.has(from) && !live.has(to) && !DEAD_ENDS.includes(from)) {
          live.add(to);
          grew = true;
        }
      }
    }
  }
  return live;
}

interface ValvePos { id: ValveId; x: number; y: number; vertical?: boolean; labelSide?: "above" | "below" | "right"; dx?: number; note?: string }
const VALVES: ValvePos[] = [
  { id: "PuVnt", x: 215, y: 75, vertical: true, labelSide: "right" },
  { id: "PuIso", x: 275, y: 120, labelSide: "below" },
  { id: "PuMvnt", x: 345, y: 75, vertical: true, labelSide: "right" },
  { id: "PuMv", x: 415, y: 120, labelSide: "above" },
  { id: "PuFill", x: 345, y: 160, vertical: true, labelSide: "right" },
  { id: "Rcs1", x: 660, y: 22, labelSide: "right", dx: 40, note: "CW" },
  { id: "Rcs2", x: 660, y: 62, labelSide: "right", dx: 40, note: "CCW" },
  { id: "OVnt", x: 110, y: 255, vertical: true, labelSide: "right" },
  { id: "OIso", x: 170, y: 300, labelSide: "below" },
  { id: "OFill", x: 270, y: 300, labelSide: "below" },
  { id: "TVnt", x: 430, y: 215, labelSide: "below" },
  { id: "Omv", x: 405, y: 400, labelSide: "below" },
  { id: "Mtv", x: 570, y: 400, labelSide: "below" },
  { id: "IgV", x: 200, y: 500, labelSide: "below" },
  { id: "LfVnt", x: 380, y: 530, vertical: true, labelSide: "right" },
];

function Valve({ v }: { v: ValvePos }) {
  const st = tele.valves.get(v.id);
  const state = st?.state ?? "Missing";
  const word = state === "Open" ? "OPEN" : state === "Closed" ? "CLOSED" : state === "Unknown" ? "UNKNOWN ?" : "—";
  const pos = st?.position_deg;
  const pct = v.id === "Mtv" ? mtvPercent() : null;
  // The MTV is a throttle: the commanded percent says more than open/closed.
  const text = pct !== null ? `${word} ${num(pct, 0)} %` : pos !== null && pos !== undefined ? `${word} ${num(pos, 0)}°` : word;
  const side = v.labelSide ?? "below";
  const lx = side === "right" ? 14 + (v.dx ?? 0) : 0;
  const anchor = side === "right" ? "start" : "middle";
  const ly = side === "below" ? 24 : side === "above" ? -26 : -2;
  return (
    <g transform={`translate(${v.x},${v.y})`} className="valve" data-state={state}>
      <title>{`${VALVE_LABEL[v.id]}: ${text}`}</title>
      <g transform={v.vertical ? "rotate(90)" : undefined}>
        <path d="M-12,-8 L0,0 L-12,8 Z M12,-8 L0,0 L12,8 Z" className="valve-body" />
        {state === "Closed" && <line x1={0} y1={-10} x2={0} y2={10} className="valve-bar" />}
        {v.id === "Mtv" && <circle r={4.5} className="valve-throttle" />}
      </g>
      <text x={lx} y={ly} textAnchor={anchor} className="valve-name">
        {VALVE_LABEL[v.id]}
        {v.note && <tspan className="valve-note"> {v.note}</tspan>}
      </text>
      <text x={lx} y={ly + 12} textAnchor={anchor} className="valve-state">{text}</text>
    </g>
  );
}

interface TagPos { ch: StandChannel; x: number; y: number; tx: number; ty: number }
// (tx,ty) is the tap point on the line; (x,y) is the tag's top-left corner.
const TAGS: TagPos[] = [
  { ch: "Pupt", x: 136, y: 140, tx: 165, ty: 120 },
  { ch: "Opt", x: 394, y: 264, tx: 380, ty: 279 },
  { ch: "T1", x: 191, y: 246, tx: 222, ty: 300 },
  { ch: "M1", x: 433, y: 340, tx: 462, ty: 400 },
  { ch: "M2", x: 593, y: 340, tx: 622, ty: 400 },
  { ch: "Ipt", x: 655, y: 296, tx: 676, ty: 384 },
  { ch: "Ept", x: 722, y: 340, tx: 722, ty: 384 },
  { ch: "T2", x: 606, y: 442, tx: 690, ty: 418 },
  { ch: "Lfpt", x: 261, y: 452, tx: 290, ty: 500 },
  { ch: "Thrust", x: 728, y: 442, tx: 778, ty: 424 },
];
const TAG_W = 62;
const TAG_H = 32;

function Tag({ t }: { t: TagPos }) {
  const v = tele.channels.get(t.ch);
  const unit = CHANNEL_UNIT[t.ch];
  const above = t.y + TAG_H <= t.ty;
  return (
    <g className="tag" data-missing={v === undefined || undefined}>
      <line x1={t.tx} y1={t.ty} x2={t.x + TAG_W / 2} y2={above ? t.y + TAG_H : t.y} className="tag-leader" />
      <circle cx={t.tx} cy={t.ty} r={2.2} className="tag-tap" />
      <rect x={t.x} y={t.y} width={TAG_W} height={TAG_H} rx={2} className="tag-box" />
      <text x={t.x + 5} y={t.y + 12} className="tag-name">{CHANNEL_LABEL[t.ch]}</text>
      <text x={t.x + TAG_W - 5} y={t.y + 27} textAnchor="end" className="tag-val">
        {num(v, unit === "N" ? 0 : 1)}
        <tspan className="tag-unit"> {unit}</tspan>
      </text>
    </g>
  );
}

function Bottle({ x, y, label, sub }: { x: number; y: number; label: string; sub: string }) {
  return (
    <g>
      <rect x={x} y={y} width={46} height={80} rx={14} className="vessel" />
      <text x={x + 23} y={y + 38} textAnchor="middle" className="vessel-name">{label}</text>
      <text x={x + 23} y={y + 51} textAnchor="middle" className="vessel-sub">{sub}</text>
    </g>
  );
}

const Vent = ({ x, y, dir }: { x: number; y: number; dir: "up" | "down" | "right" }) => {
  const r = dir === "up" ? 0 : dir === "right" ? 90 : 180;
  return (
    <g transform={`translate(${x},${y}) rotate(${r})`} className="vent">
      <path d="M-5,0 L0,-9 L5,0 Z" />
    </g>
  );
};

function IgniterLamp() {
  const on = outputOn(tele.stand, "Igniter");
  return (
    <g className="ig-lamp" data-on={on || undefined} transform="translate(700,487)">
      <title>{`Igniter ${on ? "ON" : "off"}`}</title>
      {on && <circle r={11} className="ig-glow" />}
      <circle r={5.5} className="ig-dot" />
      <path d="M-3,2 Q0,-6 3,2 Q0,0 -3,2 Z" className="ig-flame" />
      <text x={10} y={4} className="passive-name ig-word">{on ? "IGNITER ON" : "igniter"}</text>
    </g>
  );
}

export function Pid() {
  useTick();
  const live = liveNodes();
  const has = tele.stand !== null;
  return (
    <svg className="pid" viewBox="0 0 800 580" role="img" aria-label="Propulsion schematic with live valve states and sensor values" preserveAspectRatio="xMidYMid meet">
      <defs>
        <pattern id="hatch" width="4" height="4" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
          <rect width="4" height="4" className="hatch-bg" />
          <line x1="0" y1="0" x2="0" y2="4" className="hatch-line" />
        </pattern>
      </defs>

      <text x={12} y={192} className="row-cap">GN2 purge and pressurant</text>
      <text x={12} y={372} className="row-cap">N2O supply</text>
      <text x={12} y={572} className="row-cap">GOX igniter</text>

      {(Object.keys(PIPES) as Node[]).map((n) => (
        <path key={n} d={PIPES[n]} className="pipe" data-live={(has && live.has(n)) || undefined} />
      ))}
      {[[125, 120], [215, 120], [345, 120], [610, 22], [110, 300], [520, 400], [380, 500]].map(([x, y]) => (
        <circle key={`${x}-${y}`} cx={x} cy={y} r={2.8} className="junction" />
      ))}

      <Bottle x={12} y={80} label="GN2" sub="source" />
      <Bottle x={12} y={260} label="N2O" sub="source" />
      <Bottle x={12} y={460} label="GOX" sub="ignition" />

      {/* Passive regulators */}
      {([[92, 120, "PU-REG"], [110, 500, "GOX-REG"]] as const).map(([x, y, name]) => (
        <g key={name} transform={`translate(${x},${y})`} className="regulator">
          <path d="M-10,-7 L0,0 L-10,7 Z M10,-7 L0,0 L10,7 Z" />
          <path d="M0,0 V-10 M-7,-10 A7,5 0 0 1 7,-10 Z" />
          <text y={22} textAnchor="middle" className="passive-name">{name}</text>
        </g>
      ))}

      {/* Run tank */}
      <rect x={310} y={195} width={70} height={150} rx={26} className="vessel tank" />
      <text x={345} y={250} textAnchor="middle" className="vessel-name">N2O</text>
      <text x={345} y={263} textAnchor="middle" className="vessel-sub">run tank</text>

      <Vent x={215} y={44} dir="up" />
      <Vent x={345} y={44} dir="up" />
      <Vent x={110} y={224} dir="up" />
      <Vent x={475} y={215} dir="right" />
      <Vent x={380} y={560} dir="down" />
      <text x={526} y={112} className="flow-note">purge to main line</text>
      <text x={131} y={16} className="flow-note">RCS feed</text>

      {([[695, 22], [695, 62]] as const).map(([x, y]) => (
        <path key={y} transform={`translate(${x},${y})`} d="M0,-3 L12,-8 V8 L0,3 Z" className="nozzle" />
      ))}

      {/* Engine: injector, chamber, nozzle */}
      <g className="engine">
        <rect x={670} y={382} width={12} height={36} className="injector" />
        <path d="M682,382 H735 L748,393 L785,374 V426 L748,407 L735,418 H682 Z" className="chamber" />
        <text x={709} y={404} textAnchor="middle" className="vessel-sub">chamber</text>
      </g>
      <IgniterLamp />

      {VALVES.map((v) => <Valve key={v.id} v={v} />)}
      {TAGS.map((t) => <Tag key={t.ch} t={t} />)}

      {/* Legend */}
      <g transform="translate(500,532)" className="legend">
        <g transform="translate(12,10)" className="valve" data-state="Open"><path d="M-12,-8 L0,0 L-12,8 Z M12,-8 L0,0 L12,8 Z" className="valve-body" /></g>
        <text x={30} y={14} className="valve-state">OPEN</text>
        <g transform="translate(88,10)" className="valve" data-state="Closed"><path d="M-12,-8 L0,0 L-12,8 Z M12,-8 L0,0 L12,8 Z" className="valve-body" /><line y1={-10} y2={10} className="valve-bar" /></g>
        <text x={106} y={14} className="valve-state">CLOSED</text>
        <g transform="translate(180,10)" className="valve" data-state="Unknown"><path d="M-12,-8 L0,0 L-12,8 Z M12,-8 L0,0 L12,8 Z" className="valve-body" /></g>
        <text x={198} y={14} className="valve-state">UNKNOWN</text>
        <path d="M0,36 H40" className="pipe" data-live />
        <text x={48} y={40} className="valve-state">line open to a supply</text>
      </g>
    </svg>
  );
}

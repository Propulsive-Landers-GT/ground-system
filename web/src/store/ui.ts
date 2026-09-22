// Low-rate UI state (zustand). Anything that changes at telemetry rate stays in store/telemetry.ts;
// components that show live numbers subscribe to `tick` (~12 Hz) and read `tele` directly.

import { create } from "zustand";
import type {
  CommandKind, ControlMode, EventMsg, FlightPhase, LinkStatus, ParamsMsg, Source, StandStatus,
} from "../protocol";
import { tele } from "./telemetry";

export type WsState = "connecting" | "open" | "closed";
export type CommandStatus = "sending" | "sent" | "accepted" | "rejected" | "noack" | "unsent";

export interface CommandEntry {
  key: number;
  seq: number | null;
  kind: CommandKind;
  label: string;
  status: CommandStatus;
  reason?: string;
  atMs: number;
  missionTime: number | null;
}

export interface EventEntry extends EventMsg {
  key: number;
}

export type View = "flight" | "stand";
export type Theme = "dark" | "light";
export type Drawer = null | "tuning" | "jog";

interface UiState {
  ws: WsState;
  wsRetryInS: number | null;
  link: LinkStatus | null;
  hasFlight: boolean;
  stale: boolean;
  phase: FlightPhase | null;
  source: Source | null;
  controlMode: ControlMode | null;
  terminated: boolean;
  /** Stand telemetry has arrived this session (from a vehicle, a sim or the stand adapter). */
  hasStand: boolean;
  standStale: boolean;
  /** Source of the stand telemetry currently shown; "Stand" means the real adapter. */
  standSource: Source | null;
  standStatus: StandStatus | null;
  /** performance.now() when `link.recording` last became non-null; null while idle. */
  recordingSinceMs: number | null;
  /** Last `error` from the bridge (recording control), cleared on the next attempt. */
  recordingError: string;
  params: ParamsMsg | null;
  paramsRev: number;
  events: EventEntry[];
  commands: CommandEntry[];
  lastRejection: string;
  view: View;
  theme: Theme;
  drawer: Drawer;
  /** Which disclosure sections are open, by id. Persisted so the layout an operator set up survives a reload. */
  open: Record<string, boolean>;
  /** Which plots are shown per view, by plot id. Persisted. */
  plots: Record<View, string[]>;
  tick: number;
}

const initialTheme = (): Theme =>
  typeof document !== "undefined" && document.documentElement.dataset.theme === "light" ? "light" : "dark";

const PREFS_KEY = "gs-prefs";
export const DEFAULT_PLOTS: Record<View, string[]> = {
  flight: ["altitude", "thrust"],
  stand: ["feed", "engine", "loadcells"],
};

function loadPrefs(): { open: Record<string, boolean>; plots: Record<View, string[]> } {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (raw) {
      const p = JSON.parse(raw) as Partial<{ open: Record<string, boolean>; plots: Partial<Record<View, string[]>> }>;
      return {
        open: p.open && typeof p.open === "object" ? p.open : {},
        plots: {
          flight: Array.isArray(p.plots?.flight) ? p.plots.flight : DEFAULT_PLOTS.flight,
          stand: Array.isArray(p.plots?.stand) ? p.plots.stand : DEFAULT_PLOTS.stand,
        },
      };
    }
  } catch {
    /* private mode or bad JSON */
  }
  return { open: {}, plots: DEFAULT_PLOTS };
}

function savePrefs() {
  const s = useUi.getState();
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify({ open: s.open, plots: s.plots }));
  } catch {
    /* private mode */
  }
}

/** Open or close a disclosure section. `fallback` is what an id with no saved preference reads as. */
export function isOpen(id: string, fallback: boolean): boolean {
  const v = useUi.getState().open[id];
  return v === undefined ? fallback : v;
}
export function setOpen(id: string, open: boolean) {
  useUi.setState((s) => ({ open: { ...s.open, [id]: open } }));
  savePrefs();
}
export function togglePlot(view: View, id: string) {
  useUi.setState((s) => {
    const cur = s.plots[view];
    const next = cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id];
    // Never hide the last plot: an empty strip reads as broken.
    return next.length ? { plots: { ...s.plots, [view]: next } } : {};
  });
  savePrefs();
}

const prefs = loadPrefs();

export const useUi = create<UiState>(() => ({
  ws: "connecting",
  wsRetryInS: null,
  link: null,
  hasFlight: false,
  stale: false,
  phase: null,
  source: null,
  controlMode: null,
  terminated: false,
  hasStand: false,
  standStale: false,
  standSource: null,
  standStatus: null,
  recordingSinceMs: null,
  recordingError: "",
  params: null,
  paramsRev: 0,
  events: [],
  commands: [],
  lastRejection: "",
  view: "flight",
  theme: initialTheme(),
  drawer: null,
  open: prefs.open,
  plots: prefs.plots,
  tick: 0,
}));

export const MAX_EVENTS = 300;
export const MAX_COMMANDS = 60;
export const STALE_AFTER_MS = 1000;

export function setTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem("gs-theme", theme);
  } catch {
    /* private mode */
  }
  useUi.setState({ theme });
}

/** Subscribe a component to the ~12 Hz readout tick. */
export function useTick(): number {
  return useUi((s) => s.tick);
}

let tickTimer: number | undefined;
export function startTick() {
  if (tickTimer !== undefined) return;
  tickTimer = window.setInterval(() => {
    const s = useUi.getState();
    const now = performance.now();
    const stale = s.hasFlight && now - tele.flightRxMs > STALE_AFTER_MS;
    const standStale = s.hasStand && now - tele.standRxMs > STALE_AFTER_MS;
    useUi.setState(
      stale !== s.stale || standStale !== s.standStale
        ? { tick: s.tick + 1, stale, standStale }
        : { tick: s.tick + 1 },
    );
  }, 80);
}

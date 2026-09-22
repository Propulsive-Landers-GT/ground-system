// Low-rate UI state (zustand). Anything that changes at telemetry rate stays in store/telemetry.ts;
// components that show live numbers subscribe to `tick` (~12 Hz) and read `tele` directly.

import { create } from "zustand";
import type {
  CommandKind, ControlMode, EventMsg, FlightPhase, LinkStatus, ParamsMsg, Source,
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
  params: ParamsMsg | null;
  paramsRev: number;
  events: EventEntry[];
  commands: CommandEntry[];
  lastRejection: string;
  view: View;
  theme: Theme;
  drawer: Drawer;
  tick: number;
}

const initialTheme = (): Theme =>
  typeof document !== "undefined" && document.documentElement.dataset.theme === "light" ? "light" : "dark";

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
  params: null,
  paramsRev: 0,
  events: [],
  commands: [],
  lastRejection: "",
  view: "flight",
  theme: initialTheme(),
  drawer: null,
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
    const stale = s.hasFlight && performance.now() - tele.flightRxMs > STALE_AFTER_MS;
    useUi.setState(stale !== s.stale ? { tick: s.tick + 1, stale } : { tick: s.tick + 1 });
  }, 80);
}

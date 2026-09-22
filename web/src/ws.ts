// WebSocket client for the gs-bridge API (docs/DESIGN.md). Same host, /ws; the Vite dev
// server proxies /ws to the bridge. Reconnects with exponential backoff.

import type { ClientMsg, CommandKind, ServerMsg } from "./protocol";
import { describeCommand, isJog } from "./protocol";
import { ingestFlight, ingestStand, ingestTrajectory, tele } from "./store/telemetry";
import { MAX_COMMANDS, MAX_EVENTS, useUi, type CommandEntry, type EventEntry } from "./store/ui";

const ACK_TIMEOUT_MS = 1500;
const BACKOFF_MIN_MS = 500;
const BACKOFF_MAX_MS = 8000;

let sock: WebSocket | null = null;
let backoff = BACKOFF_MIN_MS;
let retryTimer: number | undefined;
let countdownTimer: number | undefined;
let nextKey = 1;
let pendingEvents: EventEntry[] = [];
let eventFlush: number | undefined;

export function wsUrl(): string {
  const override = new URLSearchParams(location.search).get("ws");
  if (override) return override;
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}/ws`;
}

export function connect() {
  if (sock && (sock.readyState === WebSocket.OPEN || sock.readyState === WebSocket.CONNECTING)) return;
  window.clearTimeout(retryTimer);
  window.clearInterval(countdownTimer);
  useUi.setState({ ws: "connecting", wsRetryInS: null });
  let s: WebSocket;
  try {
    s = new WebSocket(wsUrl());
  } catch {
    scheduleRetry();
    return;
  }
  sock = s;
  s.onopen = () => {
    backoff = BACKOFF_MIN_MS;
    // The bridge replays its recent events on connect; start from a clean list to avoid duplicates.
    useUi.setState({ ws: "open", wsRetryInS: null, events: [] });
  };
  s.onmessage = (ev) => {
    if (typeof ev.data !== "string") return;
    let msg: ServerMsg;
    try {
      msg = JSON.parse(ev.data) as ServerMsg;
    } catch {
      return;
    }
    try {
      handle(msg);
    } catch (e) {
      console.warn("bad message", msg, e);
    }
  };
  s.onclose = () => {
    if (sock === s) sock = null;
    useUi.setState({ ws: "closed", link: null });
    scheduleRetry();
  };
  s.onerror = () => s.close();
}

function scheduleRetry() {
  const delay = backoff;
  backoff = Math.min(BACKOFF_MAX_MS, backoff * 2);
  const at = performance.now() + delay;
  useUi.setState({ ws: "closed", wsRetryInS: Math.ceil(delay / 1000) });
  window.clearInterval(countdownTimer);
  countdownTimer = window.setInterval(() => {
    useUi.setState({ wsRetryInS: Math.max(0, Math.ceil((at - performance.now()) / 1000)) });
  }, 500);
  retryTimer = window.setTimeout(() => {
    window.clearInterval(countdownTimer);
    connect();
  }, delay);
}

function handle(msg: ServerMsg) {
  switch (msg.type) {
    case "flight": {
      const m = msg.data;
      ingestFlight(m);
      const s = useUi.getState();
      if (
        !s.hasFlight || s.stale || s.phase !== m.phase || s.source !== m.source ||
        s.controlMode !== m.control_mode || s.terminated !== m.terminated
      ) {
        useUi.setState({
          hasFlight: true, stale: false, phase: m.phase, source: m.source,
          controlMode: m.control_mode, terminated: m.terminated,
        });
      }
      break;
    }
    case "stand": {
      ingestStand(msg.data);
      const s = useUi.getState();
      if (s.source === null) useUi.setState({ source: msg.data.source });
      break;
    }
    case "trajectory":
      ingestTrajectory(msg.data);
      break;
    case "link":
      useUi.setState({ link: msg.data });
      break;
    case "params":
      useUi.setState((s) => ({ params: msg.data, paramsRev: s.paramsRev + 1 }));
      break;
    case "event":
      // The connect-time replay can deliver 200 of these at once; batch them into one render.
      pendingEvents.push({ ...msg.data, key: nextKey++ });
      if (eventFlush === undefined) {
        eventFlush = window.setTimeout(() => {
          eventFlush = undefined;
          const add = pendingEvents.reverse();
          pendingEvents = [];
          useUi.setState((s) => ({ events: [...add, ...s.events].slice(0, MAX_EVENTS) }));
        }, 50);
      }
      break;
    case "sent":
      onSent(msg.data.seq, msg.data.kind);
      break;
    case "ack": {
      const { seq, result } = msg.data;
      const rejected = typeof result === "object" && result !== null && "Rejected" in result;
      updateCommand(
        (c) => c.seq === seq,
        (c) => ({ ...c, status: rejected ? "rejected" : "accepted", reason: rejected ? result.Rejected : undefined }),
        (c) => {
          if (rejected) useUi.setState({ lastRejection: `${c.label} rejected: ${result.Rejected}` });
        },
      );
      break;
    }
  }
}

function updateCommand(
  match: (c: CommandEntry) => boolean,
  change: (c: CommandEntry) => CommandEntry,
  after?: (c: CommandEntry) => void,
): boolean {
  const cmds = useUi.getState().commands;
  const i = cmds.findIndex(match);
  if (i < 0) return false;
  const next = cmds.slice();
  next[i] = change(cmds[i]);
  useUi.setState({ commands: next });
  after?.(next[i]);
  return true;
}

function pushCommand(entry: CommandEntry) {
  useUi.setState((s) => ({ commands: [entry, ...s.commands].slice(0, MAX_COMMANDS) }));
}

function armAckTimeout(key: number) {
  window.setTimeout(() => {
    updateCommand(
      (c) => c.key === key && (c.status === "sent" || c.status === "sending"),
      (c) => ({
        ...c,
        status: "noack",
        reason: c.status === "sending" ? "bridge did not confirm it was sent" : undefined,
      }),
      (c) => useUi.setState({ lastRejection: `${c.label}: no ack from vehicle` }),
    );
  }, ACK_TIMEOUT_MS);
}

function onSent(seq: number, kind: CommandKind) {
  if (kind === "Heartbeat" || isJog(kind)) return;
  const sig = JSON.stringify(kind);
  // Oldest local entry still waiting for its echo (list is newest first).
  const cmds = useUi.getState().commands;
  let target = -1;
  for (let i = cmds.length - 1; i >= 0; i--) {
    if (cmds[i].status === "sending" && JSON.stringify(cmds[i].kind) === sig) {
      target = i;
      break;
    }
  }
  if (target >= 0) {
    const key = cmds[target].key;
    updateCommand((c) => c.key === key, (c) => ({ ...c, seq, status: "sent" }));
    return;
  }
  // Sent by another operator's browser.
  const key = nextKey++;
  pushCommand({
    key, seq, kind, label: describeCommand(kind), status: "sent",
    atMs: performance.now(), missionTime: tele.flight?.time_s ?? null,
  });
  armAckTimeout(key);
}

/** Send a command. Returns false if the bridge socket is not open. */
export function sendCommand(kind: CommandKind): boolean {
  const open = sock !== null && sock.readyState === WebSocket.OPEN;
  if (isJog(kind)) {
    if (open) sock!.send(JSON.stringify({ kind } satisfies ClientMsg));
    return open;
  }
  const key = nextKey++;
  pushCommand({
    key, seq: null, kind, label: describeCommand(kind),
    status: open ? "sending" : "unsent",
    reason: open ? undefined : "no connection to bridge",
    atMs: performance.now(), missionTime: tele.flight?.time_s ?? null,
  });
  if (!open) {
    useUi.setState({ lastRejection: `${describeCommand(kind)} not sent: no connection to bridge` });
    return false;
  }
  sock!.send(JSON.stringify({ kind } satisfies ClientMsg));
  armAckTimeout(key);
  return true;
}

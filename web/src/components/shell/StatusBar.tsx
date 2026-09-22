import { useEffect, useRef, useState } from "react";
import { setTheme, useTick, useUi, type View } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { clock, elapsed, num } from "../../lib/format";
import { sendControl } from "../../ws";
import mark from "../../assets/gtpl-mark.png";

const TABS: { id: View; label: string; key: string }[] = [
  { id: "flight", label: "Flight", key: "1" },
  { id: "stand", label: "Test stand", key: "2" },
];

export function StatusBar() {
  const view = useUi((s) => s.view);
  const theme = useUi((s) => s.theme);
  return (
    <header className="statusbar">
      <div className="brand" aria-label="GT Propulsive Landers ground station">
        <img className="brand-mark" src={mark} alt="" width={30} height={30} />
        <span className="brand-name">GTPL</span>
        <span className="brand-sub">Ground Station</span>
      </div>
      <nav className="tabs" role="tablist" aria-label="Views">
        {TABS.map((t) => (
          <button
            key={t.id}
            id={`tab-${t.id}`}
            role="tab"
            aria-selected={view === t.id}
            aria-controls={`panel-${t.id}`}
            className="tab"
            title={`Shortcut: ${t.key}`}
            onClick={() => useUi.setState({ view: t.id })}
          >
            {t.label}
            <kbd>{t.key}</kbd>
          </button>
        ))}
      </nav>
      <LinkBlock />
      <SourceBadge />
      <MissionClock />
      <Recording />
      <button
        className="btn btn-quiet theme-toggle"
        onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
        title="Switch between dark and light theme"
      >
        {theme === "dark" ? "Light" : "Dark"}
      </button>
    </header>
  );
}

/**
 * One indicator for the whole link. The word is always visible; address, rate, lost packets and packet
 * age live in a popover that opens on hover or focus and pins on click.
 */
function LinkBlock() {
  const ws = useUi((s) => s.ws);
  const retry = useUi((s) => s.wsRetryInS);
  const link = useUi((s) => s.link);
  const stale = useUi((s) => s.stale);
  const hasFlight = useUi((s) => s.hasFlight);
  const [pinned, setPinned] = useState(false);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!pinned) return;
    const onDown = (e: PointerEvent) => { if (!root.current?.contains(e.target as Node)) setPinned(false); };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") setPinned(false); };
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("keydown", onKey);
    return () => { window.removeEventListener("pointerdown", onDown); window.removeEventListener("keydown", onKey); };
  }, [pinned]);

  let state: "up" | "stale" | "down" = "down";
  let word = "Link down";
  if (ws !== "open") {
    word = ws === "connecting" ? "Connecting" : "No bridge";
  } else if (link?.connected && hasFlight && !stale) {
    state = "up";
    word = "Link up";
  } else if (link?.connected || (hasFlight && stale && link?.connected !== false)) {
    state = "stale";
    word = "Stale";
  }

  const lost = link ? link.packets_lost : 0;

  return (
    <div className="linkblock" data-state={state} data-open={pinned || undefined} ref={root}>
      <button
        type="button"
        className="linkbtn"
        aria-expanded={pinned}
        aria-controls="link-details"
        onClick={() => setPinned((p) => !p)}
        title="Link details"
      >
        <span className="dot" aria-hidden="true" />
        <span className="link-state" role="status" aria-label={`Vehicle link: ${word}`}>{word}</span>
        {lost > 0 && <span className="link-lost" title={`${lost} packets lost`}>{lost} lost</span>}
        <span className="chev" aria-hidden="true" />
      </button>
      <div className="link-pop" id="link-details" role="group" aria-label="Link details">
        <dl className="kvlist">
          <dt>Bridge</dt>
          <dd>{ws === "open" ? "connected" : ws === "connecting" ? "connecting…" : `retry in ${retry ?? 0} s`}</dd>
          <dt>Vehicle</dt>
          <dd>{link?.vehicle_addr ?? "—"}</dd>
          <dt>Telemetry rate</dt>
          <dd className="val">{num(link?.rate_hz, 0)}<span className="unit">Hz</span></dd>
          <dt>Packets lost</dt>
          <dd className="val" data-flag={lost > 0 ? "caution" : undefined}>{link ? lost : "—"}</dd>
          <dt>Last packet</dt>
          <dd><RxAge /></dd>
          {link?.stand && (
            <>
              <dt>Test stand</dt>
              <dd>{link.stand.addr} · {link.stand.connected ? "connected" : "down"}</dd>
            </>
          )}
          <dt>Recording</dt>
          <dd className="link-pop-rec">{link?.recording ?? "off"}</dd>
        </dl>
      </div>
    </div>
  );
}

function RxAge() {
  useTick();
  const hasFlight = useUi((s) => s.hasFlight);
  const link = useUi((s) => s.link);
  // Prefer our own measurement (time since the last flight packet reached this browser).
  const age = hasFlight ? (performance.now() - tele.flightRxMs) / 1000 : link?.last_rx_age_s ?? null;
  return (
    <span className="val" data-flag={age !== null && age > 1 ? "warn" : undefined}>
      {age === null ? "—" : age > 99 ? ">99" : num(age, age < 10 ? 2 : 0)}<span className="unit">s ago</span>
    </span>
  );
}

/** One badge per data source on the link: the vehicle (or sim / replay) and, separately, the test stand. */
function SourceBadge() {
  const source = useUi((s) => s.source);
  const standSource = useUi((s) => s.standSource);
  const hasStand = useUi((s) => s.hasStand);
  const standStale = useUi((s) => s.standStale);
  const stand = hasStand && standSource === "Stand";
  if (!source && !stand) return <span className="source" data-source="none">No data</span>;
  return (
    <span className="sources">
      {source && (
        <span
          className="source"
          data-source={source}
          title={
            source === "Vehicle"
              ? "Telemetry is from the real vehicle. Commands act on hardware."
              : source === "Sim"
                ? "Telemetry is from the simulator"
                : "Replaying a recorded session. Commands are disabled."
          }
        >
          {source === "Vehicle" ? "Vehicle" : source === "Sim" ? "Sim" : "Replay"}
          {source === "Vehicle" && <small>live hardware</small>}
        </span>
      )}
      {stand && (
        <span
          className="source"
          data-source="Stand"
          data-stale={standStale || undefined}
          title={standStale ? "Test-stand telemetry has stopped" : "Test-stand telemetry is flowing from gs-stand. Stand commands act on hardware."}
        >
          Stand
          <small>{standStale ? "stale" : "live hardware"}</small>
        </span>
      )}
    </span>
  );
}

function MissionClock() {
  useTick();
  return (
    <span className="clock" aria-label="Mission clock" data-live>
      {clock(tele.flight?.time_s ?? tele.stand?.time_s)}
    </span>
  );
}

/**
 * Recording is operator-controlled (docs/DESIGN.md, Recording). Idle: a Record button that opens a tiny
 * name field. Recording: pulsing REC, directory name, elapsed time and Stop. When either system is armed
 * and nothing is being recorded, the idle state shouts a little.
 */
function Recording() {
  const rec = useUi((s) => s.link?.recording ?? null);
  const ws = useUi((s) => s.ws);
  const error = useUi((s) => s.recordingError);
  const phase = useUi((s) => s.phase);
  const standMode = useUi((s) => s.standStatus?.mode ?? null);
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const input = useRef<HTMLInputElement>(null);

  const armed = phase === "Armed" || phase === "Ascent" || phase === "Hover" || phase === "Descent" ||
    standMode === "Armed" || standMode === "Sequence";
  const dirName = rec ? rec.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? rec : null;

  useEffect(() => {
    if (naming) input.current?.focus();
  }, [naming]);
  useEffect(() => {
    if (rec) setNaming(false);
  }, [rec]);

  const start = () => {
    const clean = name.trim().replace(/[^A-Za-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "");
    if (sendControl(clean ? { control: "start_recording", name: clean } : { control: "start_recording" })) {
      setNaming(false);
      setName("");
    }
  };

  if (rec) {
    return (
      <span className="recording" data-on title={rec}>
        <span className="rec-dot" aria-hidden="true" />
        <span className="rec-word">REC</span>
        <span className="rec-name">{dirName}</span>
        <RecElapsed />
        <button className="btn btn-quiet rec-stop" onClick={() => sendControl({ control: "stop_recording" })} title="Stop recording; the files are flushed and closed">
          Stop
        </button>
        {error && <span className="rec-error" role="alert">{error}</span>}
      </span>
    );
  }
  return (
    <span className="recording" data-armed={(armed && !naming) || undefined} title="The bridge is not recording. Data on the link is not being saved.">
      {naming ? (
        <form
          className="rec-form"
          onSubmit={(e) => { e.preventDefault(); start(); }}
        >
          <input
            ref={input}
            className="rec-input"
            value={name}
            placeholder="name, e.g. hotfire-3"
            aria-label="Recording name (optional)"
            maxLength={40}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Escape") { setNaming(false); setName(""); } }}
          />
          <button type="submit" className="btn btn-quiet rec-start" title="Start recording (Enter)">Start</button>
          <button type="button" className="btn btn-quiet" onClick={() => { setNaming(false); setName(""); }} title="Cancel (Esc)">Cancel</button>
        </form>
      ) : (
        <>
          <span className="rec-hint">Not recording</span>
          <button
            className="btn btn-quiet rec-btn"
            aria-disabled={ws !== "open" || undefined}
            data-disabled={ws !== "open" || undefined}
            title={ws === "open" ? "Start recording this session to logs/ on the bridge" : "No connection to the bridge"}
            onClick={() => ws === "open" && setNaming(true)}
          >
            <span className="rec-dot" aria-hidden="true" />
            Record
          </button>
        </>
      )}
      {error && <span className="rec-error" role="alert">{error}</span>}
    </span>
  );
}

function RecElapsed() {
  useTick();
  const since = useUi((s) => s.recordingSinceMs);
  const s = since === null ? null : (performance.now() - since) / 1000;
  // Counted from when this browser first saw the recording, so it is a lower bound after a reconnect.
  return (
    <span className="rec-elapsed val" title="Time since this browser saw the recording start">
      {elapsed(s)}
    </span>
  );
}

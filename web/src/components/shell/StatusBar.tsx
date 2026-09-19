import { setTheme, useTick, useUi, type View } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { clock, num } from "../../lib/format";

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
        <span className="brand-mark" aria-hidden="true" />
        GTPL
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
            onClick={() => useUi.setState({ view: t.id })}
          >
            <kbd>{t.key}</kbd>
            {t.label}
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

function LinkBlock() {
  const ws = useUi((s) => s.ws);
  const retry = useUi((s) => s.wsRetryInS);
  const link = useUi((s) => s.link);
  const stale = useUi((s) => s.stale);
  const hasFlight = useUi((s) => s.hasFlight);

  let state: "up" | "stale" | "down" = "down";
  let word = "DOWN";
  if (ws !== "open") {
    word = ws === "connecting" ? "CONNECTING" : "NO BRIDGE";
  } else if (link?.connected && hasFlight && !stale) {
    state = "up";
    word = "LINK UP";
  } else if (link?.connected || (hasFlight && stale && link?.connected !== false)) {
    state = "stale";
    word = "STALE";
  } else {
    word = "LINK DOWN";
  }

  return (
    <div className="linkblock" data-state={state} role="status" aria-label={`Vehicle link ${word}`}>
      <span className="link-state">
        <span className="dot" aria-hidden="true" />
        {word}
      </span>
      {ws !== "open" ? (
        <span className="kv">
          <span className="k">bridge</span>
          <span className="v">{ws === "connecting" ? "connecting" : `retry in ${retry ?? 0} s`}</span>
        </span>
      ) : (
        <>
          <span className="kv">
            <span className="k">vehicle</span>
            <span className="v">{link?.vehicle_addr ?? "—"}</span>
          </span>
          <span className="kv">
            <span className="k">rate</span>
            <span className="v w4">{num(link?.rate_hz, 0)}</span>
            <span className="u">Hz</span>
          </span>
          <span className="kv" data-flag={link && link.packets_lost > 0 ? "caution" : undefined}>
            <span className="k">lost</span>
            <span className="v w4">{link ? link.packets_lost : "—"}</span>
          </span>
          <RxAge />
        </>
      )}
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
    <span className="kv" data-flag={age !== null && age > 1 ? "warn" : undefined}>
      <span className="k">last rx</span>
      <span className="v w5">{age === null ? "—" : age > 99 ? ">99" : num(age, age < 10 ? 2 : 0)}</span>
      <span className="u">s</span>
    </span>
  );
}

function SourceBadge() {
  const source = useUi((s) => s.source);
  if (!source) return <span className="source" data-source="none">NO DATA</span>;
  return (
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
      {source === "Vehicle" ? "VEHICLE" : source === "Sim" ? "SIM" : "REPLAY"}
      {source === "Vehicle" && <small>live hardware</small>}
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

function Recording() {
  const rec = useUi((s) => s.link?.recording ?? null);
  const name = rec ? rec.split(/[\\/]/).pop() : null;
  return (
    <span className="recording" data-on={rec ? true : undefined} title={rec ?? "The bridge is not recording this session"}>
      <span className="rec-dot" aria-hidden="true" />
      <span className="rec-name">{name ?? "not recording"}</span>
    </span>
  );
}

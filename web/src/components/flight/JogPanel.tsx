import { useCallback, useEffect, useRef, useState } from "react";
import { sendCommand } from "../../ws";
import { gates } from "../../lib/interlocks";
import { useCtx } from "../shell/CommandPanel";
import { deg2rad } from "../../lib/math";
import { THRUST_MAX_N, type JogSetpoint } from "../../protocol";
import { num, signed } from "../../lib/format";

const STREAM_MS = 100; // 10 Hz; the vehicle deadman is 0.5 s

export function JogPanel() {
  const ctx = useCtx();
  const gate = gates.jogMode(ctx);
  const jogActive = ctx.controlMode === "Jog";
  const [theta, setTheta] = useState(0);
  const [phi, setPhi] = useState(0);
  const [thrust, setThrust] = useState(0);
  const [thrustEnabled, setThrustEnabled] = useState(false);
  const [rcs, setRcs] = useState(0);
  const [paused, setPaused] = useState(false);

  const sp = useRef<JogSetpoint>({ gimbal_theta: 0, gimbal_phi: 0, thrust: 0, rcs: 0 });
  sp.current = {
    gimbal_theta: deg2rad(theta), gimbal_phi: deg2rad(phi),
    thrust: thrustEnabled ? thrust : 0, rcs,
  };

  const zero = useCallback(() => {
    setTheta(0); setPhi(0); setThrust(0); setRcs(0);
  }, []);

  const streaming = jogActive && gate.ok && !paused;

  // Stream while Jog is active. Any loss of attention stops the stream and zeroes the setpoints;
  // the vehicle's 0.5 s deadman then returns the actuators to zero on its own.
  useEffect(() => {
    if (!streaming) return;
    const id = window.setInterval(() => sendCommand({ Jog: sp.current }), STREAM_MS);
    const stop = () => {
      zero();
      setThrustEnabled(false);
      setPaused(true);
      sendCommand({ Jog: { gimbal_theta: 0, gimbal_phi: 0, thrust: 0, rcs: 0 } });
    };
    const onVis = () => { if (document.hidden) stop(); };
    window.addEventListener("blur", stop);
    document.addEventListener("visibilitychange", onVis);
    return () => {
      window.clearInterval(id);
      window.removeEventListener("blur", stop);
      document.removeEventListener("visibilitychange", onVis);
      sendCommand({ Jog: { gimbal_theta: 0, gimbal_phi: 0, thrust: 0, rcs: 0 } });
    };
  }, [streaming, zero]);

  // Leaving Jog (phase change, another operator) resets the local setpoints.
  useEffect(() => {
    if (!jogActive) { zero(); setThrustEnabled(false); setPaused(false); }
  }, [jogActive, zero]);

  const controlsOn = streaming;
  const dis = (on: boolean) => ({ "aria-disabled": !on || undefined, "data-disabled": !on || undefined });

  return (
    <div className="drawer-body jog" data-active={jogActive || undefined}>
      <p className="hint">
        Moves the actuators directly for ground checkout. Only in Standby. Setpoints are streamed at 10 Hz and
        the vehicle zeroes the actuators if they stop for 0.5 s.
      </p>
      <div className="row-actions">
        <div className="seg" role="group" aria-label="Control mode">
          <button className="seg-btn" aria-pressed={!jogActive} {...dis(ctx.ws === "open" && ctx.source !== "Replay")}
            onClick={() => jogActive && sendCommand({ SetControlMode: "Auto" })}>Auto</button>
          <button className="seg-btn" aria-pressed={jogActive} {...dis(gate.ok)} title={gate.ok ? undefined : gate.why}
            onClick={() => gate.ok && !jogActive && sendCommand({ SetControlMode: "Jog" })}>Jog</button>
        </div>
        <span className="stream-state" data-on={streaming || undefined} role="status">
          {streaming ? "streaming 10 Hz" : paused && jogActive ? "paused: window lost focus" : "not streaming"}
        </span>
        {paused && jogActive && <button className="btn" onClick={() => setPaused(false)}>Resume</button>}
      </div>
      {!gate.ok && <p className="gate-why">{gate.why}</p>}

      <fieldset className="fieldset" disabled={!controlsOn}>
        <legend>Gimbal</legend>
        <Slider label="θ" unit="°" min={-15} max={15} step={0.1} value={theta} onChange={setTheta} text={signed(theta, 1)} />
        <Slider label="φ" unit="°" min={-15} max={15} step={0.1} value={phi} onChange={setPhi} text={signed(phi, 1)} />
      </fieldset>

      <fieldset className="fieldset" disabled={!controlsOn}>
        <legend>Thrust</legend>
        <label className="switch">
          <input type="checkbox" role="switch" checked={thrustEnabled} onChange={(e) => { setThrustEnabled(e.target.checked); if (!e.target.checked) setThrust(0); }} />
          <span>Enable thrust jog</span>
          <span className="hint">{thrustEnabled ? "thrust setpoint is live" : "off: thrust is sent as 0 N"}</span>
        </label>
        <Slider label="F" unit="N" min={0} max={THRUST_MAX_N} step={10} value={thrust} onChange={setThrust} text={num(thrustEnabled ? thrust : 0, 0)} disabled={!thrustEnabled} />
      </fieldset>

      <fieldset className="fieldset" disabled={!controlsOn}>
        <legend>RCS roll</legend>
        <div className="seg" role="group" aria-label="RCS">
          {([[-1, "−1 CCW"], [0, "0 off"], [1, "+1 CW"]] as const).map(([v, label]) => (
            <button key={v} type="button" className="seg-btn" aria-pressed={rcs === v} onClick={() => setRcs(v)}>{label}</button>
          ))}
        </div>
      </fieldset>

      <button className="btn zero-all" {...dis(controlsOn)} onClick={() => { zero(); }}>Zero all</button>
    </div>
  );
}

function Slider(p: { label: string; unit: string; min: number; max: number; step: number; value: number; onChange: (v: number) => void; text: string; disabled?: boolean }) {
  return (
    <label className="slider">
      <span className="slider-label">{p.label}</span>
      <input type="range" min={p.min} max={p.max} step={p.step} value={p.value} disabled={p.disabled}
        onChange={(e) => p.onChange(Number(e.target.value))} onDoubleClick={() => p.onChange(0)} aria-valuetext={`${p.text} ${p.unit}`} />
      <span className="slider-val"><span className="val">{p.text}</span><span className="unit">{p.unit}</span></span>
    </label>
  );
}

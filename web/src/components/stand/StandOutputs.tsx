// Individual stand outputs: MTV throttle, igniter, DAQ sync, plus the three load-cell readouts.
// Armed-only rules come from lib/interlocks; the stand is the authority and acks say why.

import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTick } from "../../store/ui";
import { tele } from "../../store/telemetry";
import { sendCommand } from "../../ws";
import { standGates, type Gate } from "../../lib/interlocks";
import { HoldButton } from "../shell/HoldButton";
import { Reason } from "../shell/Reason";
import { useStandCtx } from "./useStandCtx";
import { CHANNEL_LABEL, CHANNEL_UNIT, outputOn, type StandChannel } from "../../protocol";
import { num } from "../../lib/format";

const QUICK = [0, 20, 50, 100];
const LOAD_CELLS: StandChannel[] = ["Thrust", "NitrousMass", "RcsThrust"];

const dis = (g: Gate) => ({ "aria-disabled": !g.ok || undefined, "data-disabled": !g.ok || undefined, title: g.ok ? undefined : g.why });
const clampPct = (v: number) => Math.max(0, Math.min(100, Math.round(v)));

/** Short lock reason for the panel title; the controls carry the full sentence inline and in their tooltip. */
function lockNote(g: Gate, mode: string | null): string {
  if (g.ok) return "unlocked";
  if (mode === "Sequence") return "locked: sequence running";
  if (mode === "Safe") return "locked while Safe";
  return "locked";
}

/** The three load cells, large: thrust is what a hotfire is for. */
export function LoadCells() {
  useTick();
  return (
    <section className="panel loadcells" aria-label="Load cells">
      <h2 className="panel-title">Load cells</h2>
      <div className="lc-grid" role="group" aria-label="Load cells">
        {LOAD_CELLS.map((ch) => {
          const v = tele.channels.get(ch);
          const unit = CHANNEL_UNIT[ch];
          return (
            <div key={ch} className="lc" data-missing={v === undefined || undefined}>
              <span className="readout-label">{CHANNEL_LABEL[ch]}</span>
              <span className="lc-value">
                <span className="val" data-live>{num(v, unit === "kg" ? 2 : 0)}</span>
                <span className="unit">{unit}</span>
              </span>
            </div>
          );
        })}
      </div>
    </section>
  );
}

export function StandOutputs() {
  useTick();
  const ctx = useStandCtx();
  const out = standGates.output(ctx);
  const daq = standGates.daqSync(ctx);
  const m = tele.stand;
  const igniterOn = outputOn(m, "Igniter");
  const daqOn = outputOn(m, "DaqSync");

  return (
    <section className="panel outputs" aria-label="Stand outputs">
      <h2 className="panel-title">
        Outputs
        <span className="title-note" title={out.ok ? undefined : out.why}>{lockNote(out, ctx.mode)}</span>
      </h2>
      <div className="outputs-body">
        <Reason gate={out} />
        <MtvControl gate={out} telemetry={m?.mtv_percent ?? null} />

        <div className="out-row" data-on={igniterOn || undefined}>
          <span className="readout-label">Igniter</span>
          <span className="lamp lamp-fire" data-on={igniterOn || undefined} role="status" aria-label={`Igniter ${igniterOn ? "on" : "off"}`}>
            {igniterOn ? "ON" : "off"}
          </span>
          <HoldButton
            className="fire btn-sm"
            disabled={!out.ok}
            why={out.ok ? undefined : out.why}
            onConfirm={() => sendCommand({ Stand: { SetOutput: { id: "Igniter", on: true } } })}
            sub="hold 1 s"
          >
            Fire igniter
          </HoldButton>
          <button className="btn btn-sm" {...dis(out)} onClick={() => out.ok && sendCommand({ Stand: { SetOutput: { id: "Igniter", on: false } } })}>
            Igniter off
          </button>
        </div>

        <div className="out-row">
          <span className="readout-label">DAQ sync</span>
          <span className="lamp" data-on={daqOn || undefined} role="status" aria-label={`DAQ sync ${daqOn ? "on" : "off"}`}>
            {daqOn ? "ON" : "off"}
          </span>
          <label className="switch daq-switch" title={daq.ok ? "Trigger line to the external DAQ" : daq.why}>
            <input
              type="checkbox"
              role="switch"
              checked={daqOn}
              disabled={!daq.ok}
              aria-label="DAQ sync"
              onChange={(e) => sendCommand({ Stand: { SetOutput: { id: "DaqSync", on: e.target.checked } } })}
            />
            <span>{daqOn ? "sync line high" : "sync line low"}</span>
          </label>
        </div>
      </div>
    </section>
  );
}

/**
 * Slider + numeric entry + quick buttons. Sends SetMtvPercent on release / Enter only, never while dragging.
 * The slider follows the commanded value from telemetry while nobody is touching it.
 */
function MtvControl({ gate, telemetry }: { gate: Gate; telemetry: number | null }) {
  const [value, setValue] = useState(0);
  const [text, setText] = useState("0");
  const interacting = useRef(false);
  const lastTele = useRef<number | null>(null);
  const valueRef = useRef(0);
  valueRef.current = value;
  const gateRef = useRef(gate);
  gateRef.current = gate;

  // A drag can end outside the slider; the release is what sends.
  const beginDrag = () => {
    interacting.current = true;
    const end = () => {
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      if (!interacting.current) return;
      interacting.current = false;
      if (gateRef.current.ok) sendCommand({ Stand: { SetMtvPercent: clampPct(valueRef.current) } });
    };
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  };

  useEffect(() => {
    if (telemetry === lastTele.current) return;
    lastTele.current = telemetry;
    if (telemetry !== null && !interacting.current) {
      const p = clampPct(telemetry);
      setValue(p);
      setText(String(p));
    }
  }, [telemetry]);

  const send = (p: number) => {
    if (!gate.ok) return;
    const v = clampPct(p);
    setValue(v);
    setText(String(v));
    sendCommand({ Stand: { SetMtvPercent: v } });
  };
  const commitText = () => {
    const n = Number(text);
    if (text.trim() === "" || !Number.isFinite(n)) {
      setText(String(value));
      return;
    }
    send(n);
  };
  const onSliderKeyUp = (e: KeyboardEvent<HTMLInputElement>) => {
    if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown", "Enter"].includes(e.key)) {
      interacting.current = false;
      send(Number(e.currentTarget.value));
    }
  };

  const dirty = telemetry !== null && clampPct(telemetry) !== value;
  const has = telemetry !== null;

  return (
    <div className="mtv" data-disabled={!gate.ok || undefined}>
      <div className="mtv-head">
        <span className="readout-label">MTV throttle</span>
        <span className="mtv-tele" title="Commanded opening reported by the stand">
          <span className="val" data-live data-missing={!has || undefined}>{has ? num(telemetry, 0) : "—"}</span>
          <span className="unit">% commanded</span>
        </span>
      </div>
      <div className="mtv-row">
        <input
          type="range"
          min={0}
          max={100}
          step={1}
          value={value}
          disabled={!gate.ok}
          aria-label="MTV throttle percent"
          aria-valuetext={`${value} %`}
          title={gate.ok ? "Release to send" : gate.why}
          onPointerDown={beginDrag}
          onChange={(e) => { valueRef.current = Number(e.target.value); setValue(valueRef.current); setText(e.target.value); }}
          onKeyDown={() => { interacting.current = true; }}
          onKeyUp={onSliderKeyUp}
          onBlur={() => { interacting.current = false; }}
        />
        <span className="mtv-entry" data-dirty={dirty || undefined}>
          <input
            className="winput"
            inputMode="numeric"
            value={text}
            disabled={!gate.ok}
            aria-label="MTV throttle percent, type and press Enter"
            title={gate.ok ? "Enter sends" : gate.why}
            onFocus={() => { interacting.current = true; }}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") { e.preventDefault(); commitText(); }
              if (e.key === "Escape") { setText(String(value)); e.currentTarget.blur(); }
            }}
            onBlur={() => { interacting.current = false; setText(String(value)); }}
          />
          <span className="unit">%</span>
        </span>
      </div>
      <div className="quick" role="group" aria-label="MTV presets">
        {QUICK.map((q) => (
          <button
            key={q}
            className="btn btn-sm"
            aria-pressed={has && clampPct(telemetry) === q ? true : undefined}
            {...dis(gate)}
            onClick={() => send(q)}
          >
            {q} %
          </button>
        ))}
      </div>
    </div>
  );
}

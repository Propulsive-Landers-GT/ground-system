# Ground station web UI

Vite + React + TypeScript. Talks to `gs-bridge` over the WebSocket API in `../docs/DESIGN.md`.

```sh
npm install
npm run dev          # http://localhost:5173, proxies /ws to ws://127.0.0.1:8080 (GS_BRIDGE=host:port to change)
npm run build        # type-checks, then writes web/dist, which gs-bridge serves
```

Against the real bridge (from the repo root): `cargo run -p gs-bridge --bin gs-mock` (fake vehicle),
optionally `cargo run -p gs-bridge --bin gs-mock -- --stand --port 8889` (fake test stand), then
`cargo run -p gs-bridge --bin gs-bridge -- [--stand 127.0.0.1:8889]` and open http://localhost:8080/
(built UI) or run `npm run dev`.

Without Rust: `npm run mock` starts a Node stand-in for bridge + vehicle on port 8080
(`-- --auto` flies by itself, `--source Vehicle|Sim|Replay`, `--drop-acks`, `--port N`,
`--stand` adds a fake gs-stand with the stand interlocks and three ~20 s sequences, `--record` starts
with a recording open, `--verbose` logs commands).

Checks (need `npx playwright install chromium` once):
`node scripts/screenshots.mjs [url] [outDir]` saves 1280x800 screenshots of both views;
`node scripts/smoke.mjs [url] [outDir]` drives the recording control, valves, jog, arm, hold-to-launch,
overrides and abort; with a stand on the link it also arms the stand, checks the Armed-only interlocks,
MTV, igniter, a sequence and the stand abort. It opens the disclosures before using what is inside them,
checks the inline interlock reasons, the link popover and the plot chooser, and is the functionality-parity
proof for `PARITY.md`. Needs a bridge that starts in Standby / Safe (restart the mock).

Keys: `1` Flight, `2` Test stand, `Esc` closes a drawer or the link popover. Launch, Land now, Abort,
Stand abort, Start sequence and Fire igniter are press-and-hold for 1 s (mouse, or hold Space/Enter).
`?ws=ws://host:port/ws` overrides the WebSocket URL.

Layout (see the sketch at the top of `src/App.tsx` and `PARITY.md` for where every control went): the
header carries one link indicator (address, rate, lost packets and packet age are in its popover), the
source badges, the clock and the recording control. Under it a state strip shows the flight phase stepper
(or, on the Test stand tab, the stand mode and sequence progress) with a one-line explanation. The main
pane scrolls; the strip and the command rail stay put. Primary content sits above the fold: on Flight the
four hero readouts, the 3D scene and the termination margins; on Test stand the P&ID, load cells and
outputs. Actuation, sensors, the raw state estimate and the valve table are collapsible sections whose
summary line shows the key values while closed. Plots have a chooser: two or three are shown by default
and every other plot is one click away. Open sections and chosen plots are remembered in `localStorage`.

The rail reads top to bottom as the operator's flow: state, what can be done now (with the interlock reason
written under a greyed-out control, not only in its tooltip), and the abort set apart at the bottom. The
flight ABORT is only on the Flight tab and STAND ABORT only on the Test stand tab. Valves, MTV, igniter and
DAQ sync follow the rules in `docs/DESIGN.md`. The MTV slider sends on release or Enter. Recording is
started and stopped from the header (`Record` / `Stop`); the header shows NOT RECORDING in orange when
either system is armed and nothing is being saved.

Coordinates: the scene uses the vehicle's Z-up world frame directly (`src/three-setup.ts` sets
three.js `DEFAULT_UP` to +Z); no axis remapping anywhere.

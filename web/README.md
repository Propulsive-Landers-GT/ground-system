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
MTV, igniter, a sequence and the stand abort. Needs a bridge that starts in Standby / Safe (restart the mock).

Keys: `1` Flight, `2` Test stand, `Esc` closes a drawer. Launch, Land now, Abort, Stand abort, Start
sequence and Fire igniter are press-and-hold for 1 s (mouse, or hold Space/Enter). `?ws=ws://host:port/ws`
overrides the WebSocket URL.

Test stand tab: the strip at the top shows the stand mode (SAFE / ARMED / SEQUENCE), the two Arduino
links and the UDP link to gs-stand; the rail carries Arm / Disarm, the sequence picker and STAND ABORT
(the flight ABORT is only on the Flight tab). Valves, MTV, igniter and DAQ sync follow the rules in
`docs/DESIGN.md`; locked controls say why in their tooltip. The MTV slider sends on release or Enter.
Recording is started and stopped from the header (`Record` / `Stop`); the header shows NOT RECORDING in
orange when either system is armed and nothing is being saved.

Coordinates: the scene uses the vehicle's Z-up world frame directly (`src/three-setup.ts` sets
three.js `DEFAULT_UP` to +Z); no axis remapping anywhere.

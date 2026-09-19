# Ground station web UI

Vite + React + TypeScript. Talks to `gs-bridge` over the WebSocket API in `../docs/DESIGN.md`.

```sh
npm install
npm run dev          # http://localhost:5173, proxies /ws to ws://127.0.0.1:8080 (GS_BRIDGE=host:port to change)
npm run build        # type-checks, then writes web/dist, which gs-bridge serves
```

Against the real bridge (from the repo root): `cargo run -p gs-bridge --bin gs-mock` and
`cargo run -p gs-bridge --bin gs-bridge`, then open http://localhost:8080/ (built UI) or run `npm run dev`.

Without Rust: `npm run mock` starts a Node stand-in for bridge + vehicle on port 8080
(`-- --auto` flies by itself, `--source Vehicle|Sim|Replay`, `--drop-acks`, `--port N`).

Checks (need `npx playwright install chromium` once):
`node scripts/screenshots.mjs [url] [outDir]` saves 1280x800 screenshots of both views;
`node scripts/smoke.mjs [url] [outDir]` drives valves, jog, arm, hold-to-launch, overrides and abort.

Keys: `1` Flight, `2` Test stand, `Esc` closes a drawer. Launch, Land now and Abort are press-and-hold
for 1 s (mouse, or hold Space/Enter). `?ws=ws://host:port/ws` overrides the WebSocket URL.

Coordinates: the scene uses the vehicle's Z-up world frame directly (`src/three-setup.ts` sets
three.js `DEFAULT_UP` to +Z); no axis remapping anywhere.

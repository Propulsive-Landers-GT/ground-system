# ground-station

Ground station for GT Propulsive Landers: see what the vehicle is doing, live, and command it.
Works the same against the simulator, a bench vehicle, or a flight.

- **Flight view**: 3D position and attitude against the guidance trajectory, flight phase,
  TVC gimbal / thrust / RCS actuation, navigation and sensor health, and live margins against the
  flight-termination limits.
- **Test stand view**: P&ID schematic with live pressures, temperatures and valve states.
- **Commands**: arm, disarm, launch, abort, hold hover / land now, live MPC and flight-parameter
  tuning, and a Standby-only jog mode for actuator checkout. Every command is acked by the vehicle.
- Every session is recorded and can be replayed through the same UI.

```
 vehicle (Lander)  ─┐
                    ├─ UDP ─►  gs-bridge  ─ WebSocket ─►  browser(s)
 sim               ─┘
```

How the pieces fit, the link rules and the WebSocket API are in [docs/DESIGN.md](docs/DESIGN.md).

## Layout

| Path | What |
|---|---|
| `crates/gs-protocol` | Wire types and encoding, shared with the flight software and the sim |
| `crates/gs-bridge` | `gs-bridge` (UDP ⇄ WebSocket server, recorder, replay) and `gs-mock` (fake vehicle) |
| `web/` | Browser UI (Vite, React, TypeScript) |

## Run it

Needs Rust (stable) and Node 20+.

```sh
# once
cd web && npm install && npm run build && cd ..

# a fake vehicle, for trying the UI with nothing else set up
cargo run --release --bin gs-mock

# the bridge (serves the UI on http://localhost:8080)
cargo run --release --bin gs-bridge
```

Open <http://localhost:8080>, then Arm and Launch.

### Against the simulator

The sim runs the real `Lander` flight state machine, so this is the closest thing to flying.
Until the link is merged it lives on the `ground-station-link` branch of both
`simulations` and `monoprop-flight-software`, checked out side by side with this repo.

```sh
cd ../simulations/RocketSimulation/rust_rocket_sim
cargo run --release -- --ground-station
```

### Against the vehicle

Run `Lander` on the flight computer, then point the bridge at it:

```sh
cargo run --release --bin gs-bridge -- --vehicle <flight-computer-ip>:8888
```

The vehicle sends telemetry back to whichever bridge it last heard from; nothing to configure on board.

### UI development

```sh
cd web && npm run dev     # proxies /ws to a bridge on :8080
```

### Replay

```sh
cargo run --release --bin gs-bridge -- --replay logs/session-<utc>.jsonl
```

## Safety notes

- **Abort cuts thrust.** In flight the vehicle falls. "Land now" is the controlled option. The UI
  keeps them apart and both need a press-and-hold.
- The vehicle is the authority on every interlock; the UI only mirrors them.
- Loss of link triggers nothing on the vehicle yet. It reports link age; the policy is a team decision.
- The link has no authentication. Run it on an isolated network.

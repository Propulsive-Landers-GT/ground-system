# Ground station design

## Pieces

```
 vehicle (Jetson: Lander)  ─┐
 sim (rust_rocket_sim)     ─┼─ UDP, gs-protocol ─►  gs-bridge  ─ WebSocket, JSON ─►  web UI (any number of browsers)
 test stand (gs-stand)     ─┘   ◄─ commands ─────              ◄─ commands ──────
      │ USB serial ×2                
      ├─ actuation Arduino: 10 valves, igniter, DAQ sync
      ├─ load-cell Arduino: engine / nitrous / RCS HX711
      └─ Jetson PWM: MTV throttle servos
```

The bridge talks to up to two endpoints at once: `--vehicle` (Lander or sim) and `--stand` (gs-stand).
`CommandKind::Stand(_)` goes to the stand; everything else goes to the vehicle. Heartbeats go to both.
`link` status reports each separately.

| Piece | Where | Role |
|---|---|---|
| `gs-protocol` | `crates/gs-protocol` | Wire types + postcard encoding + `VehicleLink` UDP helper. The single source of truth for the link. |
| `gs-bridge` | `crates/gs-bridge` | Ground-side server. Owns the UDP socket, heartbeats the vehicle, fans telemetry out to browsers, forwards commands, records the session, serves the UI. Also contains `gs-mock`, a fake vehicle for UI work. |
| `gs-stand` | `crates/gs-stand` | Test-stand adapter, runs on the Jetson. Talks to the two Arduinos over USB serial and drives the MTV servos; exposes it all over the same UDP protocol as the vehicle. Runs the timed sequences (hotfire, cold flow, RCS) from `stand/sequences/*.toml`. |
| web UI | `web/` | Vite + React + TypeScript. Flight view, test-stand view, command panel. |
| legacy | `jetson stuff/` | The original Arduino sketch (still the firmware `gs-stand` talks to) and the Python procedures `gs-stand` replaces. Kept as the reference for the serial protocol and pin map. |
| vehicle side | `monoprop-flight-software`, `Lander/src/telemetry.rs` | `GroundLink`: telemetry out, commands in. The FSM has an abort path, phase overrides, live tuning and jog. |
| sim side | `simulations`, `rust_rocket_sim --ground-station` | Real-time mode that uses the same Lander telemetry code, plus truth state and stand telemetry from the propulsion model. |

The sim steps the *real* `Lander::fsm::FlightStateMachine`, so the telemetry builder and command
handler live in `Lander` and the sim reuses them. Anything the GUI does against the sim exercises
the same code that flies.

## UDP link

- Vehicle binds `0.0.0.0:8888`, bridge binds `0.0.0.0:9999`. One packet per datagram, `GT | version | postcard`.
- The bridge sends `Heartbeat` at 2 Hz to the configured vehicle address. The vehicle sends telemetry
  to whoever it last heard a valid packet from, so the vehicle needs no ground-side configuration.
- Rates: `Flight` 50 Hz, `Stand` 20 Hz, `Trajectory` on regeneration (~1 Hz), `Event`/`Ack`/`Params` on demand.
- Every command except `Heartbeat` and `Jog` is acked with `Accepted` or `Rejected(reason)`.
  The vehicle is the authority on interlocks; the UI mirrors them only to grey out buttons.

### Command rules enforced on the vehicle

| Command | Accepted when |
|---|---|
| `Arm` | Standby, control mode Auto |
| `Disarm` | Armed |
| `Launch` | Armed |
| `Abort` | always. Sets `flight_terminated` with reason "Operator abort": controls zeroed, loop stops. |
| `SetPhase(Hover \| Descent)` | Ascent, Hover or Descent. Any other target is rejected. |
| `SetFlightParams`, `SetMpcWeights` | always (values are range-checked) |
| `SetControlMode(Jog)` | Standby only. Any phase change forces Auto. |
| `Jog` | control mode Jog. Clamped to gimbal ±15°, thrust 0–1200 N. Setpoint expires after 0.5 s without refresh. |
| `SetValve` | Standby only |

`Abort` cuts thrust. In flight that means the vehicle falls; "land now" is `SetPhase(Descent)`.
The UI keeps the two visibly distinct. Loss of link triggers no automatic action yet: the vehicle
reports `link_age_s` and the policy is left to the team.

### Test-stand rules enforced by `gs-stand`

The stand has its own arming, independent of the vehicle: `StandMode` is `Safe`, `Armed` or `Sequence`.

| Command | Accepted when |
|---|---|
| `Stand(Arm)` | Safe, and both Arduino links are up |
| `Stand(Disarm)` | Safe or Armed. Runs the safing list. |
| `Stand(Abort)` | always. Ends any sequence, igniter off, safing list, MTV closed. |
| `SetValve`, `Stand(SetMtvPercent)`, `Stand(SetOutput{Igniter})` | Armed |
| `Stand(SetOutput{DaqSync})` | Safe or Armed |
| `Stand(StartSequence)` | Armed. Mode becomes Sequence until the last step fires or Abort. |

Safing list (`stand/config.toml`, default): OMV, IGV, OFILL, PUMV, PUISO, PUFILL closed; igniter off;
MTV to 0 %; vents (OVENT, PUVENT, LFVENT) **opened**. This matches the Arduino's `reset all` and the
team's fail-safe convention (vents normally open). The legacy `exit` path only closed OFILL and IGV.

The Arduino serial protocol has no acks, so a valve's reported state is the commanded state, marked
`Unknown` until the first command after connect. OISO (motorized, ~21 s stroke) is reported as
`Unknown` for 21 s after each command.

### Known limits

- One bridge per vehicle. The vehicle follows the last valid uplink source, so two bridges would
  steal telemetry from each other. Several people watching means several browsers on one bridge.
- An empty `TrajectoryMsg` (`positions: []`) means "no active trajectory" (Hover, Landed).
- `time_s` restarts from zero when the vehicle reboots; there is no boot id yet. The bridge treats
  a large backwards jump in `seq` as a restart.
- Nothing clears `terminated` or leaves `Landed`: restart the flight software between runs.
- A guidance solve that fails can block the flight loop for seconds. From the ground that looks
  like a link dropout (`link_age_s` and last-rx age both spike).
- No authentication. Keep the link on an isolated network.

## WebSocket API (`ws://<bridge>:8080/ws`)

JSON is the serde default for the `gs-protocol` types: unit enum variants are strings (`"Hover"`),
data-carrying variants are single-key objects (`{"Rejected": "not armed"}`), tuples are arrays.

Bridge → browser, always `{ "type": ..., "data": ... }`:

| `type` | `data` |
|---|---|
| `flight` | `FlightTelemetry` |
| `trajectory` | `TrajectoryMsg` |
| `stand` | `StandTelemetry` |
| `event` | `EventMsg` |
| `ack` | `CommandAck` |
| `params` | `ParamsMsg` |
| `sent` | `{ "seq": number, "kind": CommandKind }` echo of a command the bridge put on the wire, from any client |
| `stand_status` | `StandStatus` |
| `link` | `{ "vehicle_addr": string, "connected": bool, "last_rx_age_s": number \| null, "packets_rx": number, "packets_lost": number, "rate_hz": number, "recording": string \| null, "stand": { "addr": string, "connected": bool, "last_rx_age_s": number \| null } \| null }` at 2 Hz |

On connect the bridge replays the latest `trajectory`, `params`, `stand_status`, `link` and the last 200 `event`s.

Browser → bridge: `{ "kind": CommandKind }`, e.g. `{"kind":"Arm"}`, `{"kind":{"SetPhase":"Descent"}}`,
`{"kind":{"Jog":{"gimbal_theta":0.05,"gimbal_phi":0,"thrust":0,"rcs":0}}}`. The bridge assigns `seq`.
The bridge generates heartbeats itself; browsers never send them.

`packets_lost` comes from gaps in `FlightTelemetry.seq`. `connected` means a packet arrived in the last second.

## Recording

Each bridge run writes `logs/session-<utc>.jsonl`, one line per downlink message and per command sent,
`{ "t": <unix seconds>, "dir": "down" | "up", ... }`. `gs-bridge --replay <file>` plays a session back
through the same WebSocket API with `source: "Replay"`.

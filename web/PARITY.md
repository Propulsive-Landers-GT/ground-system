# Functionality parity: pre-redesign UI → redesigned UI

Every control and readout that existed before the layout redesign, and where it lives now. Nothing was
removed; secondary detail moved one click away (a disclosure, a plot chip or the link popover).
`scripts/smoke.mjs` exercises every control below against a real bridge and is the executable proof.

Legend: **visible** = on screen without interaction · **disclosure** = inside a collapsible section (its
summary line shows the key values while closed) · **popover** = behind the link indicator · **chip** =
in the plot chooser · **drawer** = Tuning / Jog drawer (unchanged).

## Header (both tabs)

| Before | Now |
|---|---|
| GTPL brand + "Ground Station" | Header, unchanged |
| Flight / Test stand tabs with `1` / `2` key hints | Header tabs; key hints shown ≥ 1400 px, keys always work |
| LINK UP / STALE / LINK DOWN / NO BRIDGE / CONNECTING word + dot | Header, single link indicator (`Link up`, `Stale`, `Link down`, `No bridge`, `Connecting`) |
| Bridge retry countdown | Link popover, "Bridge: retry in N s" |
| Vehicle address | Link popover |
| Telemetry rate (Hz) | Link popover |
| Packets lost | Link popover; also a `N lost` pill beside the link word whenever it is non-zero |
| Last rx age (s) | Link popover ("Last packet … s ago") |
| Stand link addr / connected | Link popover (and the stand strip on the Test stand tab, as before) |
| VEHICLE / SIM / REPLAY badge (+ "live hardware") | Header, unchanged |
| STAND badge (+ live / stale) | Header, unchanged |
| Mission clock T+MM:SS.s | Header, unchanged |
| Record button → name field → Start / Cancel | Header, unchanged |
| REC · name · elapsed · Stop | Header, unchanged (REC pulses; name hidden < 1400 px, kept in the tooltip) |
| "not recording" / NOT RECORDING (armed) warning | Header, unchanged (uppercase orange when either system is armed) |
| Recording error (bridge `error`) | Header, unchanged (`role=alert`) |
| Light / Dark theme toggle | Header, unchanged |

## State strip

| Before | Now |
|---|---|
| Flight phase stepper (6 steps, current phase time) | Flight tab strip, larger (34 px), current step wider |
| AUTO / JOG control-mode chip | Flight tab strip, unchanged |
| Tuning / Jog drawer toggles (in the phase bar) | Flight rail, "Ground checkout" group (Tuning, Jog); drawer opens over the main pane, Esc closes |
| Flight phase visible on the Test stand tab | Test stand strip, "Vehicle <phase>" cross-reference |
| — (new) | One-sentence meaning of the current phase / stand mode under the stepper |
| — (new) | "Test stand <mode>" cross-reference on the Flight strip when a stand is configured |
| FLIGHT TERMINATED banner + reason | Unchanged, full width under the strip |

## Flight tab

| Before | Now |
|---|---|
| Altitude (big) | Hero row, 40 px |
| Vertical speed (big) | Hero row, 40 px |
| Offset from pad | Hero row, 28 px |
| Speed | Hero row, 28 px |
| "waiting for telemetry" / STALE N s note | Hero row note (top right) + scene overlay as before |
| Tilt / limit + bar + CAUTION/WARNING flag | Margins panel, visible |
| Trajectory deviation / limit + bar + flag | Margins panel ("Off trajectory"), visible |
| Position fix age + bar + flag | Margins panel, visible |
| Vehicle uplink age + flag | Margins panel ("Uplink age"), visible |
| Mass + propellant estimate bar | Margins panel, visible |
| State estimate: pos xyz, vel xyz, roll/pitch/yaw | Disclosure "State estimate" (summary: xyz + r/p/y); angular rate added |
| 3D scene: canvas, orbit | Scene panel, visible, larger |
| Follow / Pad / Top-down camera | Scene panel, unchanged |
| Scene legend (flown path, reference, sim truth, axes note) | Scene panel, unchanged |
| "Waiting for telemetry" / STALE overlay | Scene panel, unchanged |
| Actuation: gimbal bullseye, θ / φ values | Disclosure "Actuation" (summary: thrust N · %, θ, φ, RCS) |
| Actuation: thrust N, %, bar with min-throttle band and scale | Disclosure "Actuation" |
| Actuation: RCS CCW / CW lamps | Disclosure "Actuation" (and in its summary) |
| Actuation: AUTO / JOG mode badge | Disclosure "Actuation" ("Control" row); also strip chip and rail state line |
| Sensors: IMU / GPS / UWB lights | Disclosure "Sensors" (also in its summary) |
| Sensors: chamber, tank pressure | Disclosure "Sensors" (also in its summary) |
| Sensors: accel xyz, gyro xyz | Disclosure "Sensors" |
| Plot: Altitude (ref, truth, est) | Plot chip "Altitude", shown by default |
| Plot: Velocity x y z | Plot chip "Velocity" |
| Plot: Attitude roll pitch yaw tilt | Plot chip "Attitude" |
| Plot: Gimbal θ φ (±15° marks) | Plot chip "Gimbal" |
| Plot: Thrust (marks at 300 / 1200 N) | Plot chip "Thrust", shown by default |
| Plot: Trajectory deviation | Plot chip "Off trajectory" |
| Tuning drawer: Refresh, Discard edits, 4 flight params, Apply, MPC weights Q/Qn/R table, Apply weights, Restore built-in | Drawer, unchanged (opened from the rail) |
| Jog drawer: Auto/Jog switch, streaming state, Resume, θ φ sliders, thrust enable + slider, RCS −1/0/+1, Zero all | Drawer, unchanged (opened from the rail) |

## Flight rail (commands)

| Before | Now |
|---|---|
| — (new) | State line: phase word, time in phase, control mode |
| Arm | "On the pad" group |
| Disarm | "On the pad" group |
| Launch (hold 1 s) | "On the pad" group, hold unchanged |
| Hold hover | "In flight" group |
| Land now (hold 1 s, "controlled descent") | "In flight" group, hold unchanged |
| ABORT (hold 1 s) + note | Abort zone at the bottom, separated by space and a dashed rule; unchanged behaviour |
| Interlock reason in tooltip only | Tooltip **and** inline sentence under the group, plus `aria-description` |
| Command log (seq, label, status, rejection reason; aria-live for rejections) | Rail, unchanged |
| Event log (severity, time, text with T+ / step formatting) | Rail, unchanged |

## Test stand tab

| Before | Now |
|---|---|
| Strip: SAFE / ARMED / SEQUENCE mode badge | Strip, larger |
| Strip: actuation / load-cell Arduino lamps | Strip, unchanged |
| Strip: stand UDP link dot, age, address | Strip, unchanged |
| Strip: "no test stand configured" | Strip ("No test stand on this bridge") |
| Strip: sequence name, T+ clock, progress bar, next step, fired/total | Strip, clock now 28 px |
| Strip: "N sequences available" | Strip, unchanged |
| P&ID: pipes, live segments, vessels, regulators, vents, valves with state word / position / MTV %, sensor tags, igniter lamp, legend, source note | P&ID panel, visible, unchanged content |
| Outputs: MTV slider (send on release), numeric entry (Enter), 0/20/50/100 % presets, commanded readout | Outputs panel, visible |
| Outputs: igniter lamp, Fire igniter (hold 1 s), Igniter off | Outputs panel, visible |
| Outputs: DAQ sync lamp + switch | Outputs panel, visible |
| Outputs: lock note in the title | Outputs panel title **and** inline reason sentence |
| Load cells: Thrust, N2O mass, RCS thrust | Own "Load cells" panel above Outputs, 28 px |
| Valve table: 15 valves, state chip, position, Open / Close, lock note | Disclosure "Valves" (summary: open / closed / unknown / not reported counts); role column added; lock reason inline |
| Plot: Pressures (7 series) | Split by plant side: chip "Feed pressures" (O-PT, PU-PT, LF-PT) + chip "Engine pressures" (I-PT, E-PT, M1-PT, M2-PT), both shown by default |
| Plot: Temperatures T1 T2 | Plot chip "Temperatures" |
| Plot: Load cells (thrust, RCS) | Plot chip "Load cells", shown by default |
| Plot: N2O mass | Plot chip "N2O mass" |

## Test stand rail (commands)

| Before | Now |
|---|---|
| Title note: mode / no stand / replay | Title note only for "no stand configured", "commands off in replay", "waiting for status"; the mode is the state line |
| — (new) | State line: Safe / Armed / Sequence running + what is unlocked |
| Arm | "Arming" group |
| Disarm | "Arming" group; a hint describes the safing list |
| Sequence picker | "Sequence" group, labelled select |
| Start <sequence> (hold 1 s) | "Sequence" group, hold unchanged |
| Running sequence card (name, clock / duration, bar, next, fired, "only Abort") | "Sequence" group, unchanged |
| STAND ABORT (hold 1 s) + owner label + note | Abort zone at the bottom, unchanged behaviour |
| Interlock reasons in tooltips | Tooltip **and** inline sentence ("Arm the stand first.") |
| Command log, Event log | Rail, unchanged |

## Behaviour that did not change

Hold-to-confirm (pointer and Space/Enter, release cancels), interlock gating with the vehicle / stand as
authority, `1` / `2` / `Esc` keys, the Jog deadman (blur or hidden tab zeroes and pauses), MTV send-on-release,
recording control messages, stale dimming, reduced-motion handling (now covers the new disclosure and
phase transitions too), dark / light themes.

## Persistence (new)

Which disclosures are open and which plots are shown are saved in `localStorage` (`gs-prefs`) so an
operator's arrangement survives a reload. Defaults: all disclosures closed; Flight shows Altitude +
Thrust, Test stand shows Feed pressures + Engine pressures + Load cells.

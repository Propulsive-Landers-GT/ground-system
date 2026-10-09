# gs-stand

Test-stand adapter. Runs on the Jetson next to the two Arduinos and the MTV servos and exposes
them over the same UDP protocol as the vehicle, so the bridge and the web UI drive the stand
with `Stand(...)` commands and get `StandTelemetry` / `StandStatus` / `Event`s back. Timed
sequences (hotfire, cold flow, RCS, igniter check) are TOML files in `stand/sequences/`.

The Arduino sketch (`jetson stuff/Arduino/test_stand/test_stand.ino`) is unchanged; gs-stand
speaks its protocol exactly. The legacy Python procedures are replaced:

| Legacy | Now |
|---|---|
| `procedure.py` / `rcs_procedure.py` (serial handshake, timed loop, CSV) | `gs-stand` itself; CSV in `logs/stand-<utc>.csv` |
| `hotfire.py` `pfs_commands` + `"hold-20-8"` at 53.6 s | `stand/sequences/hotfire.toml` |
| `rcs_procedure.py` list | `stand/sequences/rcs.toml` |
| `coldflow.py` list (script itself is broken) | `stand/sequences/coldflow.toml` |
| `kaboom_test.py` list (script itself is broken) | `stand/sequences/igniter_check.toml` |
| `click_checks.py` manual typing (`omv open`, `mtv 20`, ...) | UI buttons while Armed: `SetValve`, `SetMtvPercent`, `SetOutput` |
| `mtv_module.py` / `mtv_servo.py` | still used by the `python` MTV backend via `stand/mtv_helper.py`; re-implemented for the `sysfs` backend |
| typing `exit` (only closed OFILL + IGV) | `Abort`: igniter off, full safing list, MTV closed, sync low |
| `encoder.py` | not ported (nothing read it) |

## Run

```sh
# on a laptop, no hardware
cargo run -p gs-stand -- --fake-arduino                  # listens on UDP 8889
cargo run -p gs-stand -- --fake-arduino --time-scale 8   # sequences 8x faster
cargo run -p gs-stand -- --check                         # parse config + sequences and exit

# then point a bridge at it
cargo run -p gs-bridge -- --stand 127.0.0.1:8889
```

On the Jetson (from the repo root, so `stand/config.toml`, `stand/sequences` and
`jetson stuff/jetson/mtv` resolve):

```sh
cargo build --release -p gs-stand
./target/release/gs-stand                                # uses stand/config.toml
RUST_LOG=debug ./target/release/gs-stand                 # every serial line in the log
```

Startup order does not matter: gs-stand waits for the Arduinos and for the bridge and
reconnects to both. Startup and every Arduino (re)connect put the stand in `Safe`.

Flags: `--config <file>`, `--root <dir>`, `--port <udp>`, `--fake-arduino`, `--time-scale <x>`
(dev only), `--no-csv`, `--check`.

## Jetson setup

Rust target `aarch64-unknown-linux-gnu`; `serialport` is built without libudev so there are no
native deps. Python side (only for the default `python` MTV backend):

```sh
sudo pip3 install Jetson.GPIO numpy     # what mtv_module.py / mtv_servo.py import
sudo groupadd -f gpio && sudo usermod -aG gpio,dialout $USER   # PWM pins and /dev/ttyACM*
```

Bench-check the servo path without gs-stand:

```sh
echo -e "0\n20\n0\nquit" | python3 stand/mtv_helper.py --legacy-dir "jetson stuff/jetson/mtv"
```

### Stable serial names

`/dev/ttyACM0` and `ttyACM1` swap on reboot. gs-stand identifies the boards by behaviour anyway
(only the actuation sketch answers `sync status`), but stable names make the config and the
logs readable. Find the serials, then add a udev rule:

```sh
udevadm info -a -n /dev/ttyACM0 | grep -E 'ATTRS\{(serial|idVendor|idProduct)\}' | head -3
```

`/etc/udev/rules.d/99-stand.rules`:

```
SUBSYSTEM=="tty", ATTRS{idVendor}=="2341", ATTRS{serial}=="<actuation board serial>", SYMLINK+="stand-actuation", MODE="0666"
SUBSYSTEM=="tty", ATTRS{idVendor}=="2341", ATTRS{serial}=="<load-cell board serial>", SYMLINK+="stand-loadcell",  MODE="0666"
```

`sudo udevadm control --reload && sudo udevadm trigger`, then set `actuation_port =
"/dev/stand-actuation"` and `loadcell_port = "/dev/stand-loadcell"` in `stand/config.toml`
(or leave `"auto"`, which also scans `/dev/stand-*`). If the boards are ever swapped, gs-stand
uses them correctly and logs a warning.

### MTV PWM backend

`[mtv] backend`:

- `auto` (default): `python` on Linux, `none` elsewhere.
- `python`: spawns `stand/mtv_helper.py`, which imports the team's `MTV`/`Servo` classes from
  `jetson stuff/jetson/mtv/` (Jetson.GPIO software PWM at 333 Hz). The proven path.
- `sysfs`: writes `/sys/class/pwm/pwmchipN/pwmM` directly (hardware PWM, no Python). Needs the
  pin muxed to PWM (`sudo /opt/nvidia/jetson-io/jetson-io.py`) and the chip/channel per servo
  in `[mtv.sysfs]`. To find them: `for c in /sys/class/pwm/pwmchip*; do echo "$c ->
  $(readlink -f $c/device)"; done` and compare with the `pwm chip dir` column for
  `JETSON_ORIN_NANO` in Jetson.GPIO's `gpio_pin_data.py` (BOARD pin 15 and 33). Write access:
  `sudo chmod -R a+rw /sys/class/pwm/pwmchip*/` or a udev rule.
- `none`: logs the servo pulse widths.

Geometry (all in `[mtv]`, from the legacy classes): valve° = % / 100 × 90; servo° = (44 +
valve°) × 2; pulse = 500 + 2000 × servo°/355 µs at 333 Hz. Startup re-homes to −40° for 3 s then
0 %, like `procedure.py`.

## Behaviour

Modes and interlocks are the table in `docs/DESIGN.md` ("Test-stand rules enforced by
gs-stand"); every row has a unit test in `src/stand.rs`. Summary: `Arm` needs Safe and both
Arduino links; `Disarm` (Safe/Armed) runs the safing list; `Abort` always works and does igniter
off, safing list, MTV 0 %, sync low; manual valve/MTV/igniter commands need Armed; `DaqSync` needs
Safe or Armed; `StartSequence` needs Armed and holds `Sequence` until the last step (then back to
Armed) or Abort. Rejections carry a reason in the ack.

Safing list and every other constant: `stand/config.toml` (commented, with legacy sources).

Loss of ground link (no uplink for `ground_link.timeout_s`, default 3 s): while Armed run the
safing list and go Safe (`armed_on_loss = "safe"`); while in Sequence log a Critical event and
keep going (`sequence_on_loss = "log"`), because a hotfire must not be killed by WiFi. Both are
configurable.

Serial: commands are written as one `write_all` + flush with no terminator, at least
`min_command_gap_ms` (30) apart per board, because the sketch frames by a 10 ms silence and
drains its buffer after each command. After (re)connect gs-stand waits for `connected`, sends a
throwaway `sync status` (which also identifies the board), then `<name> setup` for each load cell
with `setup_on_connect`. The actuation board is probed with `sync status` once a second when idle
(never during a sequence); three misses close and reopen the port.

Telemetry: `StandTelemetry` 20 Hz (load cells from the stream, valves as commanded with `Unknown`
until the first command and for OISO during its 21 s stroke, `outputs_on`, `mtv_percent`),
`StandStatus` 5 Hz, `Event` for every sequence step (`T+48.7 igniter on`), mode change, link
change, abort and parse problem. All with `source: Stand`.

## Writing a sequence

`stand/sequences/<name>.toml`; the file name is the sequence name unless `name =` says otherwise.
Unknown valves, actions or keys fail at startup with the offending file and step listed.

```toml
description = "what this does"

[mtv_profile]            # optional
start_t = 53.6           # seconds after T-0
profile = "hold-20-8"    # hold-<pct>-<dur> ramp-<pct1>-<pct2>-<dur> ..., played back to back
preposition = true       # command the first segment's percent at T-0 (legacy behaviour)

[[step]]
t = 48.7                 # seconds after T-0; steps may be in any order in the file
action = "output igniter on"
```

Actions: `valve <id> open|close` (ids `omv igv ofill oiso ovnt pumv pufill puiso puvnt lfvnt`,
Arduino spellings `ovent puvent lfvent` accepted), `output igniter|daq_sync on|off`,
`loadcell engine|nitrous|rcs begin|end`, `mtv <percent>`.

T-0 raises the DAQ sync line; the sequence ends (`duration_s` = last step or profile end,
whichever is later) with sync low and mode back to Armed. After the profile ends the MTV holds
the profile's final percent until a step or the operator moves it.

## Tests

```sh
cargo test -p gs-stand            # unit tests + fake-Arduino integration tests over UDP (~12 s)
cargo clippy -p gs-stand --all-targets
```

`--fake-arduino` runs both boards in-process with the sketch's quirks (banner on reset, 10 ms
merge rule, `<name> loadcell ready`, ~10 Hz stream that reacts to OMV / MTV / PUMV).

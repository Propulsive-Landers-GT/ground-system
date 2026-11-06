# Single-servo RCS load test

`rcs_load_test.py` is a terminal-only, operator-driven checkout for one servo-actuated valve, selected Arduino-driven solenoids, and one or more HX711 load cells. It does **not** use the two-servo MTV or the timed `rcs_procedure.py` sequence. The `blips` command performs a fixed 10-second servo motion test; it does not command the solenoids.

## Before connecting the test article

1. Open `jetson stuff/Arduino/test_stand/test_stand.ino` in Arduino IDE. Make the bundled `jetson stuff/Arduino/libraries/HX711_Arduino_Library` available in the IDE's sketchbook libraries so `HX711.h` resolves. Select the relay Arduino's board type and port, leave `USE_LOADCELL = false`, and upload. Then select the HX711 Arduino's board type and port, change `USE_LOADCELL = true`, and upload again. Restore the source line to the role you intend to edit next time. The two boards use different roles despite sharing the same source sketch. Keep the installed firmware version and pin map with the test record. **Flash the updated relay sketch before running this version of `rcs_load_test.py`; the new `state` query is required.**
2. Verify the RCS load cell on HX711 DOUT 6 / SCK 7 of the load-cell Arduino. The separate `loadcell_callibration/loadcell_callibration.ino` sketch uses the same pins. Upload that sketch temporarily with Arduino IDE if calibration is needed. With a known applied load, enter that load in the 115200-baud Serial Monitor in the unit you want the readings to use, then transfer the resulting scale factor to the `rcs` `setup_loadcell(..., 7550)` call in `test_stand.ino` and reflash the load-cell board with `USE_LOADCELL = true`. The load-cell board tares when it receives `rcs setup`; ensure the cell is unloaded at that point. The present `7550` is a prior value, not a universal calibration.
3. Establish the actual servo PWM pin, pulse limits, and closed/open endpoints from the installed servo and mechanism. Use the Jetson `BOARD` pin numbering. The supplied servo is a ZOSKAY 25KG/0.13S 6.8V **270° version** powered at 5 V. Its likely DS3225 family [datasheet](https://www.dsservo.com/d_file/DS3225%20datasheet.pdf) rates operation at 5 V and specifies 50–330 Hz control. The configured 50 Hz is within that family specification. `servo_range_deg` is now 270°, but the 1000–2000 µs pulse mapping, 0° closed, and 135° open are **filler values marked TODO**; those degree labels do not yet represent measured physical angles. As written, startup still commands 1000 µs and `servo 100` still commands 1500 µs. Replace the mapping and valve endpoints with measured values before using installed hardware. Check direction and mechanical clearance with the system unpressurized.
4. Identify stable USB paths for the two Arduinos. `/dev/ttyACM0` and `/dev/ttyACM1` can swap. The script asks the relay board for `SYNC is N` and rejects a port that does not answer. It also requires the new `VALVE <name> open|closed` replies after solenoid commands. The load-cell board must answer `<name> loadcell ready` and then produce `<name>:<number>` samples for each selected cell. Verify the state of other stand outputs and isolate unused fluid paths before the test; this script only closes the solenoids listed in `TEST_CONFIG["valves"]`.
5. Run from an SSH terminal with Jetson.GPIO, pyserial, GPIO access, and serial-port access installed/configured. Only one process may own the serial ports and PWM pin; stop `gs-stand`, `click_checks.py`, or other procedures first.

## Run

Edit the `TEST_CONFIG` block near the top of `rcs_load_test.py` before running. Fill in `actuation_port`, `loadcell_port`, and `servo_pin`; the script refuses to start while those fields are blank. Set `valves` to a tuple of the solenoids used, such as `("puiso", "pumv")`, or `()` for a servo-only run. Set `loadcells` to a tuple such as `("rcs",)` or `("rcs", "nitrous")`. Edit the servo values there once measured. `log_dir` already points to `data_logs` beside the script.

From the `ground-system` repository root on the Jetson, run:

```sh
cd "jetson stuff/jetson/mtv"
python3 rcs_load_test.py
```

The command-line flags remain available as optional overrides (`python3 rcs_load_test.py --help`), but none are needed once `TEST_CONFIG` is filled in. Choose only solenoid names physically installed for this test. The script accepts `omv`, `igv`, `ofill`, `pumv`, `pufill`, and `puiso` as valves to command closed on startup and exit. This is a test-scope and cleanup list, **not** a list of valve electrical types. The Arduino sketch owns the pin assignments and open/closed polarity. Vent valves are excluded from this test because closing them on exit is a different procedure decision.
Every selected load cell must be wired and calibrated; the Arduino's `setup` operation can block if its HX711 is absent.

The script positions the servo at the configured closed endpoint, opens both serial links at 115200 baud, verifies the relay board, commands each selected solenoid closed and checks its output status, tares the selected load cells, and waits for finite samples. It then prints a readiness message. Type one command per line:

| Command | Effect |
| --- | --- |
| `start` | Enables manual actuation; samples are already being logged. |
| `valve NAME open` or `valve NAME close` | Sends a command to a selected solenoid and requires the Arduino's logical output-state reply to match. |
| `servo PERCENT` | Sets the one servo between configured closed (0) and open (100) endpoints. |
| `set-zero DEG` | Moves to and stores a new closed servo endpoint for this run. |
| `set-open DEG` | Moves to and stores a new full-open servo endpoint for this run. |
| `blips` | From a commanded closed position, commands open, close, open, close, etc. every 0.5 seconds for 10 seconds: 20 transitions, ending closed. Sampling continues throughout. `stop` interrupts it. |
| `stop` | Closes selected solenoids, returns the servo to closed, ends streaming, and exits. |

Ctrl-C or terminal EOF also triggers the same best-effort cleanup. A missing sample from **any selected cell** for two seconds, a failed valve status check, or a missing idle relay-link probe stops the run. If the host misses a full blip interval, the test stops instead of issuing overdue transitions in a burst. Endpoint changes made with `set-zero` or `set-open` last only for the current process; edit `TEST_CONFIG["closed_deg"]` and `TEST_CONFIG["open_deg"]` to retain them for later runs.
The Jetson/Python loop is not a real-time controller; use the action CSV timestamps to measure the actual spacing achieved on hardware.

At 5 V, the DS3225 family [datasheet](https://www.dsservo.com/d_file/DS3225%20datasheet.pdf) lists 0.15 s per 60° **without load**, 21 kg·cm **stall** torque, and 1.9 A **stall** current. These figures apply only if the installed ZOSKAY unit is the matching DS3225 variant. Size its separate 5 V supply and wiring for the actual motor current; connect the servo signal reference to the Jetson ground, and verify the servo input accepts the Jetson GPIO signal level. Do not power the servo from a Jetson GPIO pin. Valve load can slow or stall it. A `blips` row means a PWM position was commanded; without position feedback it does not establish that the valve reached that state within 0.5 seconds.

Each run creates two files with the same `<UTC>-<id>` suffix and one monotonic time origin. `rcs-load-<UTC>-<id>.csv` has `time_s` and one column per selected load cell; an Arduino line containing multiple readings becomes one row, and a missing channel on that line is blank. `rcs-actions-<UTC>-<id>.csv` has `time_s,actuator,state,target_deg,result`. It includes startup, manual, blip, and cleanup commands. `result` distinguishes Arduino output-pin verification from an unverified cleanup send or a PWM command without position feedback. The first load-cell samples precede `start`, providing a baseline. Time is the Jetson's monotonic time since log creation; sample time marks receipt at the Jetson, not the HX711 conversion instant. The unit of each reading comes from that cell's calibration.

## Code path and protocol

| Layer | Responsibility |
| --- | --- |
| `rcs_load_test.py` | Owns this test's CLI, selection of valves to close on exit, start gate, 0.5-second blip scheduler, sample timeout, paired CSV logs, and cleanup. It sends valve names and requested states over USB serial. |
| `mtv_servo.py` `Servo` | Drives **one** Jetson.GPIO PWM pin. It receives the calibrated servo angle directly; `mtv_module.MTV` is not used. |
| `procedure_io.py` | Shared small I/O helpers: `send_command` writes the Arduino text command and leaves a 20 ms idle gap for the sketch's 10 ms `readString()` framing; `read_line` decodes available serial lines; `wait_for_message` handles startup handshakes; `terminal_input` polls SSH stdin without blocking sampling; `cleanup` tries every shutdown action even if one raises. |
| `test_stand.ino` on relay Arduino | Maps valve names to pins, drives relay outputs for `NAME open|close`, and handles `NAME state` using the sketch's polarity. The new `VALVE <name> open|closed` reply reports the logical output state. The older numeric `NAME status` reply remains for existing software. |
| `test_stand.ino` on HX711 Arduino | Handles `<name> setup` (initialize and tare), `<name> begin` (stream), and `<name> end` for `rcs`, `engine`, and `nitrous`. Streams `<name>:<value>` fields. |

The current Arduino protocol has no command acknowledgements or valve-position feedback. A matching `VALVE <name> open|closed` reply proves only that the Arduino reports the expected **output pin level interpreted through its polarity setting** after the query. It does not prove the relay energized, the solenoid moved, or the fluid path changed. Servo commands likewise have no position feedback. If USB disconnects, the sketch has no watchdog and outputs may retain their last state; software cleanup cannot be relied on as an independent isolation mechanism. Use hardware interlocks and an approved test plan for the physical test. A future firmware protocol should add framed commands, sequence IDs, explicit acknowledgements, and a board-side timeout if command completion must be guaranteed.

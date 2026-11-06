# MTV calibration commands

`calibrate_mtv.py` supports independent servo positioning, direction reversal, and coordinated MTV movement. Enter interactive commands at the `calibration>` prompt and press **Enter** to apply them. Decimal values are accepted. Every accepted adjustment is saved to a text file.

## Start the tool

From the `PropulsiveLanders` directory:

```bash
python3 "ground-system/jetson stuff/jetson/mtv/calibrate_mtv.py"
```

The tool loads `mtv_calibration.txt` beside the script and restores its settings and last displayed valve position. Each successful save replaces that file with **one current calibration**. If the file is absent, the tool uses the defaults in `mtv_module.py`; the current defaults start both servos at **88 servo degrees** on BOARD pins **15 and 32**.

Explicit command-line options override loaded values. Use `--valve 0` to start at the calibrated valve-zero position instead of resuming the saved position. Use `--defaults` to ignore an existing file and start from code defaults; subsequent saves replace the selected output file.

To import a different calibration and save future changes to the standard file:

```bash
python3 "ground-system/jetson stuff/jetson/mtv/calibrate_mtv.py" --input "saved_mtv.txt"
```

Use `--output "another_mtv.txt"` to load and update that file instead. With both `--input` and `--output`, the input supplies the starting calibration and the output receives saves. The input is retained when it is a different file from the output.

To choose initial positions, specify the lead angle and the second servo's offset. This example starts servo 1 at 80° and servo 2 at 84°:

```bash
python3 "ground-system/jetson stuff/jetson/mtv/calibrate_mtv.py" --defaults --lead 80 --offset 4
```

| Startup option | Meaning | Default |
| --- | --- | --- |
| `--pin1 N` | Override servo 1's physical BOARD pin. | Saved pin, otherwise `15` |
| `--pin2 N` | Override servo 2's physical BOARD pin; must differ from pin 1. | Saved pin, otherwise `32` |
| `--lead N` | Override servo 1's logical calibration reference, before valve and servo direction mappings, in servo degrees. | Saved close angle multiplied by saved gear ratio, otherwise code defaults |
| `--offset N` | Override the logical offset between the two servos, in servo degrees. | Saved offset, otherwise `DEFAULT_SERVO2_OFFSET` |
| `--direction1 N` | Servo 1 mapping: `1` sends the logical angle; `-1` sends `servo_range - angle`. | Saved direction, otherwise `DEFAULT_SERVO1_DIRECTION` |
| `--direction2 N` | Servo 2 mapping: `1` sends the logical angle; `-1` sends `servo_range - angle`. | Saved direction, otherwise `DEFAULT_SERVO2_DIRECTION` |
| `--valve-direction N` | Valve mapping: `1` uses the original travel; `-1` swaps valve 0 and 90. | Saved direction, otherwise `DEFAULT_VALVE_DIRECTION` |
| `--valve N` | Override the initial valve angle, from 0 to 90. | Saved position, otherwise `0` |
| `--input PATH` | Import settings from another calibration file. A missing input is an error. | Load the output file if it exists |
| `--defaults` | Ignore saved settings and use code defaults plus explicit overrides. Cannot combine with `--input`. | Off |
| `--output PATH` | File to load and replace with one current calibration on each save. | `mtv_calibration.txt` beside `calibrate_mtv.py` |
| `-h` or `--help` | Print startup options and interactive commands, then exit without starting PWM. | — |

## All interactive commands

`N` is a number. Servo positions must stay within the configured servo range, currently **0–355°**. Valve commands must stay within **0–90°**. Negative increments and offsets are allowed when the resulting servo positions remain in range.

Servo inputs are **logical angles**, before direction mapping. A reversed servo receives `355 - logical angle` with the default range. The tool prints both logical angles and the mapped PWM angles sent to the servos.

| Command | Example | Effect |
| --- | --- | --- |
| `servo1 N` | `servo1 90.5` | Set servo 1's absolute logical angle. Servo 2's commanded position stays fixed; the recorded lead zero and offset are recalculated. |
| `servo2 N` | `servo2 92` | Set servo 2's absolute logical angle. Servo 1's commanded position stays fixed; the recorded offset is recalculated. |
| `lead N` | `lead 88` | Set the lead servo's calibration reference in servo degrees. Move both servos using their relative directions, preserving the current valve-angle setting. |
| `offset N` | `offset -2` | Set servo 2's signed valve-zero offset from servo 1, in servo degrees. Only servo 2's command changes. |
| `jog N` | `jog 0.5` | Add N servo degrees to both logical zero positions, keeping the offset fixed. Each servo follows its direction mapping. |
| `trim N` | `trim -0.1` | Add N servo degrees to servo 2's offset. Only servo 2's command changes. |
| `valve N` | `valve 45` | Command the MTV to an absolute valve angle between 0° and 90°. Both servos move as needed, preserving the calibrated zero and offset. |
| `flipvalve` or `flip valve` | `flipvalve` | Swap valve 0 and 90. Keep both servos at their current positions and relabel the displayed valve angle as `90 - angle`. Save the new valve direction. |
| `direction1 N` | `direction1 -1` | Set servo 1's mapping to normal (`1`) or mirrored (`-1`). Requires `valve 0`; immediately applies the mapped position. |
| `direction2 N` | `direction2 -1` | Set servo 2's mapping to normal (`1`) or mirrored (`-1`). Requires `valve 0`; immediately applies the mapped position. |
| `flip1` | `flip1` | Toggle servo 1's mapping between normal and mirrored. Requires `valve 0`; immediately applies the mapped position to servo 1 only. |
| `flip2` | `flip2` | Toggle servo 2's mapping between normal and mirrored. Requires `valve 0`; immediately applies the mapped position to servo 2 only. |
| `show` or an empty line | `show` | Print the current commanded positions and calibration values. No movement. |
| `save` | `save` | Replace the output file with the current calibration. No movement. Adjustments also save automatically. |
| `help` | `help` | Print the interactive command list. |
| `quit`, `exit`, or `q` | `quit` | Stop both PWM outputs, clean up GPIO, and exit without commanding another position. |
| **Ctrl-C** or **EOF** | Ctrl-D on an empty terminal input line | Exit with the same PWM/GPIO cleanup. |

The tool prints and saves updated positions and calibration values after each accepted adjustment. Invalid or out-of-range input is rejected before either servo is commanded and is not saved.

## Reverse a servo's direction

At `valve 0`, use `flip1` or `flip2` to toggle a direction, or set it explicitly with `direction1 -1`, `direction2 -1`, `direction1 1`, or `direction2 1`.

- `1`: PWM angle equals the logical servo angle.
- `-1`: PWM angle equals `servo_range - logical servo angle`.

This reverses the **entire servo range**, including independent commands, startup positions, zero adjustments, and valve movement. With the default 355° range:

| Logical command | Normal PWM angle | Reversed PWM angle |
| --- | --- | --- |
| 0° | 0° | 355° |
| 88° | 88° | 267° |
| 177.5° | 177.5° | 177.5° |
| 355° | 355° | 0° |

Changing direction preserves the logical calibration but immediately mirrors that servo's PWM angle. **Disengage the gears before flipping; this can command a large movement.** Direction changes at a nonzero valve angle remain rejected. Flipping twice restores the original mapping. At the midpoint, a flip changes the mapping without moving the servo.

For example, start servo 1 at 90° and servo 2 at 265°, with servo 2 reversed:

```bash
python3 "ground-system/jetson stuff/jetson/mtv/calibrate_mtv.py" --defaults --lead 90 --offset 0 --direction2 -1
```

Then `valve 10` commands servo 1 to 110° and servo 2 to 245°. `valve 0` returns them to 90° and 265°.

## Reverse the valve's open/closed labels

If the assembled valve is open at `valve 0` and closed at `valve 90`, enter:

```text
flipvalve
```

`flip valve` is an alias. This toggles `DEFAULT_VALVE_DIRECTION` between `1` and `-1`, preserving the lead reference, offset, and both servo directions. It swaps the two endpoints without changing the available stroke. The command itself does **not** move either servo: a displayed valve angle of 0 becomes 90, 30 becomes 60, and 90 becomes 0. Subsequent `valve N` commands use the new mapping. Flipping twice restores the original mapping.

For the recorded calibration with lead reference **40°**, offset **5°**, and both servo directions **-1**, the reversed mapping gives:

| Valve command | Servo 1 PWM angle | Servo 2 PWM angle | Valve position reported for this assembly |
| --- | --- | --- | --- |
| `valve 0` | 135° | 130° | Closed |
| `valve 45` | 225° | 220° | Mid-travel |
| `valve 90` | 315° | 310° | Open |

To start with that calibration already reversed, run from the `mtv` directory:

```bash
python3 calibrate_mtv.py --lead 40 --offset 5 --direction1 -1 --direction2 -1 --valve-direction -1 --valve 0
```

This starts at the newly defined closed position with the saved 2:1 gearing and 355° servo range. If that calibration is already active with the original valve mapping, use `flipvalve`, then `valve 0` to command closure. The change autosaves; the next calibration session loads it automatically.

`MTV.from_calibration()` reads the saved `DEFAULT_VALVE_DIRECTION` along with the other settings. Its startup and shutdown `valve 0` commands use the new closed endpoint. The legacy name `DEFAULT_CLOSE_ANGLE` remains the original calibration reference divided by the gear ratio; flipping the valve does not change that number.

## Move the MTV: both servos together

Use **`valve N`** to test the assembled MTV while keeping its calibration fixed. Once the closed position is calibrated:

| Command | Valve target |
| --- | --- |
| `valve 0` | Calibrated closed position. |
| `valve 10` | 10° from the calibrated closed position. |
| `valve 45` | Halfway through the 90° valve travel. |
| `valve 90` | Full-open position, 90° from calibrated closed. |

The gearing requires **2 servo degrees per valve degree**. Moving from `valve 0` to `valve 90` changes each servo command by 180°. The servo directions and valve direction determine the sign, preserving the calibrated endpoints.

At the current 2:1 gear ratio:

```text
travel = valve angle                         when valve direction = +1
travel = 90 - valve angle                    when valve direction = -1
servo 1 logical angle = lead reference + 2 * travel
servo 2 logical angle = lead reference + offset + 2 * travel
PWM angle = logical angle                    when direction = +1
PWM angle = servo range - logical angle      when direction = -1
```

`offset` is the difference between the two **logical servo angles**, before applying their direction mappings. When directions oppose, the difference between their PWM angles changes as the valve moves.

For paired adjustments during assembly, `jog 1` increases both logical zero positions by 1 servo degree. Each normal servo's PWM angle increases by 1°; each reversed servo's PWM angle decreases by 1°. `lead N` follows the same rule, preserving the logical offset. **`jog` and `lead` change the recorded valve-zero calibration.** Use `valve N` for travel checks with that calibration fixed.

## Align the servos and find valve zero

1. Start with `valve 0` selected (`--valve 0` at startup, or `valve 0` in the tool). Set each servo's direction with `direction1` / `direction2` or `flip1` / `flip2`.
2. Use `servo1 N` and `servo2 N` to position each servo independently during assembly. Use `offset N` or `trim N` to adjust servo 2 relative to servo 1.
3. Once their relative positions agree mechanically, use `lead N` or small `jog N` adjustments to locate the valve's closed position. The paired movement follows the selected directions.
4. Use `valve N` to check travel. If 0 is open and 90 is closed, use `flipvalve` to swap those labels, then `valve 0` to command closure.
5. Enter `save` to record the current configuration explicitly. Use `MTV.from_calibration()` to consume that file in the main class.

## Saved settings

The tool writes one timestamped calibration after successful startup, after every accepted adjustment, and whenever you enter `save`. This includes the pins, directions, gear ratio, servo range, and last displayed valve angle. The default file is:

```text
ground-system/jetson stuff/jetson/mtv/mtv_calibration.txt
```

Use `--output PATH` to choose another file. Saves write a temporary file beside the destination, flush it, then replace the destination atomically. A failed save leaves the prior calibration intact and stops PWM through the existing cleanup path.

Older files containing several `# Calibration ...` blocks load only their last block and become a single record on the next successful save. Legacy pin and valve-position comments are accepted. Files missing `DEFAULT_VALVE_DIRECTION` retain the original mapping (`1`). Missing explicit inputs, incomplete records, unknown settings, and invalid numeric values stop loading before servo initialization. The reader accepts numeric assignments without executing the text as Python.

For example, the reversed-servo setup above produces:

```python
DEFAULT_SERVO1_PIN = 15
DEFAULT_SERVO2_PIN = 32
CALIBRATION_VALVE_ANGLE = 0
DEFAULT_CLOSE_ANGLE = 45
DEFAULT_SERVO2_OFFSET = 0
DEFAULT_SERVO1_DIRECTION = 1
DEFAULT_SERVO2_DIRECTION = -1
DEFAULT_VALVE_DIRECTION = 1
DEFAULT_GEAR_RATIO = 2
DEFAULT_SERVO_RANGE = 355
```

`DEFAULT_CLOSE_ANGLE` uses the existing valve-side convention: **logical lead calibration reference divided by the gear ratio**. With valve direction `1`, that reference is the lead's logical angle at valve 0; with direction `-1`, it is the angle at valve 90. `DEFAULT_SERVO2_OFFSET` is the difference between logical servo angles, in servo degrees. The tool performs this conversion when printing the values.

Older saved calibrations used unmirrored zero positions even with direction `-1`. Those reversed settings need conversion or recalibration for this mapping. To preserve an old calibration, find each old zero (`old_lead = old_close_angle * gear_ratio`, `old_follower = old_lead + old_offset`). Convert each zero to `servo_range - old_zero` for direction `-1`, or leave it unchanged for direction `1`. Set the new close angle to the converted lead divided by the gear ratio, and the new offset to the converted follower minus the converted lead. Direction values stay the same.

`CALIBRATION_VALVE_ANGLE` restores the calibration tool's last displayed position. The main MTV class deliberately starts at valve 0 instead of restoring that preview position.

If either servo lacks room for the full 0–90° valve stroke in its selected direction, the tool reports it in both the console and the saved block. Individual assembly positions can still be explored within servo limits; normal MTV initialization requires calibration that allows the full stroke. A `Full valve stroke: valid` entry checks numerical travel limits, not mechanical alignment. Printed positions are commanded values, not encoder feedback.

## Use the saved calibration in the main MTV class

```python
from mtv_module import MTV

mtv = MTV.from_calibration()
mtv.command(0)
mtv.command(90)
```

`MTV.from_calibration()` loads `mtv_calibration.txt` beside the modules, including the saved pins. It validates the full valve stroke before starting PWM, then starts at valve 0. Pass a path for another file: `MTV.from_calibration("assembled_mtv.txt")`. Explicit constructor overrides are also supported, for example `MTV.from_calibration(pin2=33)`.

This replaces a construction such as `MTV(15, 33)` when you want the file to supply the setup. Existing explicit `MTV(...)` calls retain their arguments and code defaults; they do not silently load a file. No copying of calibration constants is needed with `from_calibration()`. Changes are read at construction, not during a running procedure. Keep the usual shutdown/stop and GPIO cleanup handling around the instance.

## Assembly workflow: bare servos to a calibrated MTV

This workflow starts with two bare servos, installs their pinions around the central valve gear, aligns servo 2, and then adjusts the valve's closed reference. The central gear has twice as many teeth as either servo gear, so the valve's 90° operating stroke requires 180° of travel from each servo.

### 1. Check directions with the gears disengaged

Identify servo 1 as the lead and servo 2 as the adjustable servo. With the gears off or disengaged, compare small increases and decreases in each servo's commanded angle from the same viewing direction.

For ordinary external pinions driving the same central gear, both pinions must turn in the **same physical rotational sense**, while the central gear turns the other way. Mounting the servos on opposite sides of the central gear does not, by itself, require a software reversal. This follows from the two external meshes in the [gear-train direction rules](https://www.cs.cmu.edu/~rapidproto/mechanisms/chpt7.html). Use opposite software directions only if opposite numerical command changes produce the required physical motion on your installed servos. If identical servos face the same way and respond alike, their software directions should match.

Choose each direction so increasing `valve N` will open the valve. Direction settings can be changed at `valve 0` with `direction1`, `direction2`, `flip1`, or `flip2`. Changing the setting immediately mirrors that servo's PWM angle, so do this with the gears disengaged.

### 2. Establish starting references with adjustment margin

Choose starting angles near the appropriate ends of each servo's usable travel, leaving room for fine adjustment. This is commanded positioning, not an automatic homing operation. The configured 0–355° range does not establish the actual mechanical stops of a particular servo.

For the **verified opposite-command-direction case**, use logical zeros of **5°** for servo 1 and **10°** for servo 2, reversing servo 2. Their PWM angles start at **5°** and **345°** respectively. This leaves 5° of lower-end margin on servo 1 and 10° of upper-end margin on servo 2, allowing small adjustments in either direction before assembly.

Start the tool with the gears still disengaged:

```bash
python3 "ground-system/jetson stuff/jetson/mtv/calibrate_mtv.py" --defaults --lead 5 --offset 5 --direction1 1 --direction2 -1
```

The logical offset is `10 - 5 = 5` servo degrees. With these example settings, `valve 90` would send PWM angles of 185° to servo 1 and 165° to servo 2. Both have the required 180° travel available. Leave the valve-angle setting at `0` while fitting the gears.

The same logical zeros (`--lead 5 --offset 5`) produce these PWM starting angles for each direction combination. These are numerical examples using the configured range; choose margins appropriate to the actual servos:

| Opening directions `(servo 1, servo 2)` | Servo 1 PWM zero | Servo 2 PWM zero | `--offset` |
| --- | --- | --- | --- |
| `(+1, -1)` | 5° | 345° | `5` |
| `(+1, +1)` | 5° | 10° | `5` |
| `(-1, +1)` | 350° | 10° | `5` |
| `(-1, -1)` | 350° | 345° | `5` |

Starting near an end reserves travel in the opening direction; the adjustment margins reserve movement in the other direction. The software's valve operating range remains 0–90°.

### 3. Fit the central gear and lead-servo gear

Place the valve at its known closed reference and fit its central gear. Keep servo 1 at its chosen starting command. Fit servo 1's gear to the nearest suitable spline position and mesh it with the central gear. Leave small residual valve-zero error for the paired adjustment in step 5; do not force the spline or gear teeth into engagement.

At this stage, servo 1 establishes the reference. Keep servo 2's gear disengaged until its spline and tooth alignment are ready.

### 4. Adjust servo 2 to fit its spline and mesh

Keep servo 1's command fixed. Use `servo2 N` for an absolute logical position or small `trim N` adjustments to align servo 2's shaft with the gear's spline and its teeth with the central gear. For the reversed servo 2 starting at a PWM angle of 345° (logical angle 10°):

| Command | Adjustment |
| --- | --- |
| `servo2 10.5` | Send servo 2 a PWM angle of 344.5°; servo 1 stays fixed. |
| `trim 0.1` | Increase servo 2's logical angle by 0.1°; its PWM angle decreases by 0.1°. |
| `trim -0.1` | Decrease servo 2's logical angle by 0.1°; its PWM angle increases by 0.1°. |

Make these independent adjustments while servo 2's gear is disengaged enough to move freely, then seat and mesh it. If the pair is preloaded against itself, disengage servo 2's gear, correct its alignment, and reseat it. Once both gears are fully meshed, use paired movements for valve adjustment; an independent servo command can make the two actuators oppose each other through the gears.

### 5. Fine-adjust the valve's closed reference

With both gears meshed and `valve 0` selected, use small paired adjustments to locate the actual closed reference. For example, try `jog 0.2` or `jog -0.2` as needed. A 0.2° lead-servo adjustment corresponds to 0.1° of valve movement in magnitude with the 2:1 gearing. Servo 2 follows the selected relative direction.

The displayed valve angle stays at `0` because `jog` adjusts the calibration of that reference. The logical offset stays fixed even with opposing directions, preserving the relative calibration while moving both servos.

### 6. Check both valve limits progressively

Starting from the calibrated zero, check small opening commands such as `valve 1`, `valve 5`, and `valve 10`. Continue through the range only as the motion permits. Approach the open reference with smaller changes, for example `valve 88`, `valve 89`, `valve 89.5`, and finally `valve 90` if the mechanism has the remaining travel. Return toward `valve 0` and check closure again.

If the closed reference needs correction, return to `valve 0`, make a small paired `jog` adjustment, and recheck both ends. **Moving the zero shifts both endpoints.** This tool assumes a 90° valve stroke; it does not calibrate the open and closed stops independently. If both endpoints cannot agree over that stroke, recheck the gear indexing, alignment, and actual valve travel before adopting the settings. Keep the gear ratio at the actual tooth-count ratio of 2.

### 7. Save and use the final settings

Return to the chosen closed reference, inspect the settings, and save the current calibration:

```text
valve 0
show
save
```

Check the numerical full-stroke result and the physical endpoints you just tested. `quit` stops PWM without another position command. `MTV.from_calibration()` uses the saved settings, including both servo directions, valve direction, and pin choices.

The next calibration startup resumes the saved position automatically. Use `--valve 0` when you want it to start at the calibrated closed position instead.

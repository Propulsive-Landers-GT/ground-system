# MTV calibration commands

`calibrate_mtv.py` supports independent servo positioning, direction reversal, and coordinated MTV movement. Enter interactive commands at the `calibration>` prompt and press **Enter** to apply them. Decimal values are accepted. Every accepted adjustment is saved to a text file.

## Start the tool

From the `PropulsiveLanders` directory:

```bash
python3 "ground-system/jetson stuff/jetson/mtv/calibrate_mtv.py"
```

The tool starts both servos at the saved calibration in `mtv_module.py`. With the current defaults, both start at **88 servo degrees** on BOARD pins **15 and 33**.

To choose initial positions, specify the lead angle and the second servo's offset. This example starts servo 1 at 80° and servo 2 at 84°:

```bash
python3 "ground-system/jetson stuff/jetson/mtv/calibrate_mtv.py" --lead 80 --offset 4
```

| Startup option | Meaning | Default |
| --- | --- | --- |
| `--pin1 N` | Physical BOARD pin for servo 1, the lead servo. | `15` |
| `--pin2 N` | Physical BOARD pin for servo 2, the adjusted servo. Must differ from pin 1. | `33` |
| `--lead N` | Initial servo 1 position, in servo degrees. | `DEFAULT_CLOSE_ANGLE * DEFAULT_GEAR_RATIO` |
| `--offset N` | Initial signed offset added to servo 1's position to obtain servo 2's position. | `DEFAULT_SERVO2_OFFSET` |
| `--direction1 N` | Servo 1 direction: `1` increases its angle as the valve opens; `-1` decreases it. | `DEFAULT_SERVO1_DIRECTION` |
| `--direction2 N` | Servo 2 direction: `1` increases its angle as the valve opens; `-1` decreases it. | `DEFAULT_SERVO2_DIRECTION` |
| `--output PATH` | Text file to append calibration settings to. An existing file's contents are retained. | `mtv_calibration.txt` beside `calibrate_mtv.py` |
| `-h` or `--help` | Print startup options and interactive commands, then exit without starting PWM. | — |

## All interactive commands

`N` is a number. Servo positions must stay within the configured servo range, currently **0–355°**. Valve commands must stay within **0–90°**. Negative increments and offsets are allowed when the resulting servo positions remain in range.

| Command | Example | Effect |
| --- | --- | --- |
| `servo1 N` | `servo1 90.5` | Set servo 1's absolute angle in servo degrees. Servo 2's commanded position stays fixed; the recorded lead zero and offset are recalculated. |
| `servo2 N` | `servo2 92` | Set servo 2's absolute angle in servo degrees. Servo 1's commanded position stays fixed; the recorded offset is recalculated. |
| `lead N` | `lead 88` | Set the lead servo's valve-zero calibration in servo degrees. Move both servos using their relative directions, preserving the current valve-angle setting. |
| `offset N` | `offset -2` | Set servo 2's signed valve-zero offset from servo 1, in servo degrees. Only servo 2's command changes. |
| `jog N` | `jog 0.5` | Add N servo degrees to the lead-zero calibration. Servo 2 moves by N when directions match, or by -N when directions oppose. |
| `trim N` | `trim -0.1` | Add N servo degrees to servo 2's offset. Only servo 2's command changes. |
| `valve N` | `valve 45` | Command the MTV to an absolute valve angle between 0° and 90°. Both servos move as needed, preserving the calibrated zero and offset. |
| `direction1 N` | `direction1 -1` | Set servo 1's direction to `1` or `-1`. Requires `valve 0`; does not move either servo. |
| `direction2 N` | `direction2 -1` | Set servo 2's direction to `1` or `-1`. Requires `valve 0`; does not move either servo. |
| `flip1` | `flip1` | Toggle servo 1's direction between `1` and `-1`. Requires `valve 0`; does not move either servo. |
| `flip2` | `flip2` | Toggle servo 2's direction between `1` and `-1`. Requires `valve 0`; does not move either servo. |
| `show` or an empty line | `show` | Print the current commanded positions, calibration values, and defaults to copy into `mtv_module.py`. No movement. |
| `save` | `save` | Append the current settings to the output file immediately. No movement. Adjustments also save automatically. |
| `help` | `help` | Print the interactive command list. |
| `quit`, `exit`, or `q` | `quit` | Stop both PWM outputs, clean up GPIO, and exit without commanding another position. |
| **Ctrl-C** or **EOF** | Ctrl-D on an empty terminal input line | Exit with the same PWM/GPIO cleanup. |

The tool prints and saves updated positions and calibration values after each accepted adjustment. Invalid or out-of-range input is rejected before either servo is commanded and is not saved.

## Reverse a servo's direction

At `valve 0`, use `flip1` or `flip2` to toggle a direction, or set it explicitly with `direction1 -1`, `direction2 -1`, `direction1 1`, or `direction2 1`.

- `1`: increasing the valve angle increases this servo's commanded angle.
- `-1`: increasing the valve angle decreases this servo's commanded angle.

Changing direction keeps both calibrated zero positions fixed and causes no immediate movement. Direction changes at a nonzero valve angle are rejected. Independent `servo1 N` and `servo2 N` commands always specify absolute servo angles.

For example, start servo 1 at 90° and servo 2 at 265°, with servo 2 reversed:

```bash
python3 "ground-system/jetson stuff/jetson/mtv/calibrate_mtv.py" --lead 90 --offset 175 --direction2 -1
```

Then `valve 10` commands servo 1 to 110° and servo 2 to 245°. `valve 0` returns them to 90° and 265°.

## Move the MTV: both servos together

Use **`valve N`** to test the assembled MTV while keeping its calibration fixed. Once the closed position is calibrated:

| Command | Valve target |
| --- | --- |
| `valve 0` | Calibrated closed position. |
| `valve 10` | 10° from the calibrated closed position. |
| `valve 45` | Halfway through the 90° valve travel. |
| `valve 90` | Full-open position, 90° from calibrated closed. |

The gearing requires **2 servo degrees per valve degree**. Moving from `valve 0` to `valve 90` changes each servo command by 180° in its selected direction, preserving both calibrated zero positions.

At the current 2:1 gear ratio:

```text
servo 1 position = lead zero + direction1 * 2 * valve angle
servo 2 position = lead zero + offset + direction2 * 2 * valve angle
```

`offset` is the difference between the two servo commands **at valve zero**. When directions oppose, the difference between their commands changes as the valve moves.

For paired adjustments during assembly, `jog 1` increases the lead servo's position by 1 servo degree. The other servo moves by +1° if directions match, or -1° if they oppose. `lead N` follows the same paired-movement rule. With opposing directions, the recorded valve-zero offset is recalculated accordingly. **`jog` and `lead` change the recorded valve-zero calibration.** Use `valve N` for travel checks with that calibration fixed.

## Align the servos and find valve zero

1. Start with `valve 0` selected; this is the initial setting. Set each servo's direction with `direction1` / `direction2` or `flip1` / `flip2`.
2. Use `servo1 N` and `servo2 N` to position each servo independently during assembly. Use `offset N` or `trim N` to adjust servo 2 relative to servo 1.
3. Once their relative positions agree mechanically, use `lead N` or small `jog N` adjustments to locate the valve's closed position. The paired movement follows the selected directions.
4. Use `valve N` to check travel from that zero. Return with `valve 0`.
5. Enter `save` to record the current configuration explicitly, then copy the latest suitable settings block from the text file into `mtv_module.py`.

## Saved settings

The tool appends a timestamped block after successful startup, after every accepted adjustment, and whenever you enter `save`. This also records directions supplied through startup options. The default file is:

```text
ground-system/jetson stuff/jetson/mtv/mtv_calibration.txt
```

Use `--output PATH` to choose another file. Existing entries are retained. Each block includes the BOARD pin numbers, current valve angle and servo commands, a check of available servo travel, and copyable defaults.

For example, the reversed-servo setup above produces:

```python
DEFAULT_CLOSE_ANGLE = 45
DEFAULT_SERVO2_OFFSET = 175
DEFAULT_SERVO1_DIRECTION = 1
DEFAULT_SERVO2_DIRECTION = -1
DEFAULT_GEAR_RATIO = 2
DEFAULT_SERVO_RANGE = 355
```

`DEFAULT_CLOSE_ANGLE` uses the existing valve-side convention: **lead servo zero divided by the gear ratio**. `DEFAULT_SERVO2_OFFSET` is already in servo degrees. The tool performs this conversion when printing the values.

Copy the desired block's `DEFAULT_...` assignments into the corresponding definitions near the top of `mtv_module.py`. The procedure runners use those edited defaults when their programs are next started. The text file is a calibration record; it is not loaded automatically, and the tool does not edit `mtv_module.py` itself.

If either servo lacks room for the full 0–90° valve stroke in its selected direction, the tool reports it in both the console and the saved block. Individual assembly positions can still be explored within servo limits; normal MTV initialization requires calibration that allows the full stroke. A `Full valve stroke: valid` entry checks numerical travel limits, not mechanical alignment. Printed positions are commanded values, not encoder feedback.

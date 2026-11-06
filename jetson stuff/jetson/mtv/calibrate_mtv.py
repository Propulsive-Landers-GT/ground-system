import argparse
import math
from datetime import datetime
from pathlib import Path
from mtv_calibration import DEFAULT_CALIBRATION, calibration_writer, load_calibration

DEFAULT_OUTPUT = DEFAULT_CALIBRATION

HELP = """Commands (press Enter to apply):
  servo1 N   Set servo 1's logical angle, leaving servo 2 unchanged
  servo2 N   Set servo 2's logical angle, leaving servo 1 unchanged
  lead N     Set the lead servo's calibration reference, in servo degrees
  offset N   Set servo 2's logical offset from the lead, in servo degrees
  jog N      Move the lead by N servo degrees; servo 2 follows its direction
  trim N     Add N servo degrees to servo 2's offset only
  valve N    Test a valve angle from 0 to 90 degrees using this calibration
  flipvalve  Swap valve 0 and 90 without moving; also accepts 'flip valve'
  direction1 N / direction2 N   Set normal (+1) or mirrored (-1) mapping at valve 0
  flip1 / flip2                 Toggle full-range mapping at valve 0; moves that servo
  show       Print positions and current calibration settings
  save       Replace the saved calibration with current settings (also autosaved)
  help       Show these commands
  quit       Stop PWM without commanding another position (also Ctrl-C/EOF)
"""


def positions(lead, offset, valve, gear_ratio, servo_range, direction1=1, direction2=1, valve_direction=1):
    if not all(math.isfinite(v) for v in (lead, offset, valve, gear_ratio, servo_range)):
        raise ValueError("Positions and configuration must be finite")
    if not 0 <= valve <= 90 or gear_ratio <= 0 or servo_range <= 0:
        raise ValueError("Valve angle must be 0–90 degrees; ratio and servo range must be positive")
    if direction1 not in (-1, 1) or direction2 not in (-1, 1):
        raise ValueError("Servo directions must be +1 or -1")
    if valve_direction not in (-1, 1):
        raise ValueError("Valve direction must be +1 or -1")
    travel = 90 - valve if valve_direction == -1 else valve
    angles = lead + travel * gear_ratio, lead + offset + travel * gear_ratio
    if not all(0 <= angle <= servo_range for angle in angles):
        raise ValueError(f"Both servo positions must be within 0–{servo_range:g} degrees")
    return tuple(angle if direction == 1 else servo_range - angle
                 for angle, direction in zip(angles, (direction1, direction2)))


def adjustment(line, lead, offset, valve, gear_ratio, direction1=1, direction2=1, valve_direction=1):
    parts = line.lower().split()
    if parts in (["flipvalve"], ["flip", "valve"]):
        return lead, offset, 90 - valve, direction1, direction2, -valve_direction
    if parts in (["flip1"], ["flip2"]):
        parts = ["direction1", str(-direction1)] if parts[0] == "flip1" else ["direction2", str(-direction2)]
    if len(parts) != 2:
        raise ValueError("Expected a command and a number; type help for commands")
    command, value = parts[0], float(parts[1])
    travel = 90 - valve if valve_direction == -1 else valve
    if command == "servo1":
        offset += lead + travel * gear_ratio - value
        lead = value - travel * gear_ratio
    elif command == "servo2":
        offset = value - lead - travel * gear_ratio
    elif command == "lead":
        lead = value
    elif command == "offset":
        offset = value
    elif command == "jog":
        lead += value
    elif command == "trim":
        offset += value
    elif command == "valve":
        valve = value
    elif command in ("direction1", "direction2"):
        if valve != 0:
            raise ValueError("Set valve 0 before changing servo direction")
        if value not in (-1, 1):
            raise ValueError("Servo directions must be +1 or -1")
        if command == "direction1":
            direction1 = int(value)
        else:
            direction2 = int(value)
    else:
        raise ValueError(f"Unknown command: {command}")
    return lead, offset, valve, direction1, direction2, valve_direction


def settings_text(lead, offset, valve, gear_ratio, servo_range, direction1=1, direction2=1, valve_direction=1):
    servo1, servo2 = positions(lead, offset, valve, gear_ratio, servo_range, direction1, direction2, valve_direction)
    travel = 90 - valve if valve_direction == -1 else valve
    text = (f"# Logical angles: lead reference={lead:.12g}, offset={offset:.12g}, valve={valve:.12g}; "
            f"servo 1={lead + travel * gear_ratio:.12g}, servo 2={lead + offset + travel * gear_ratio:.12g} degrees\n"
            f"# PWM angles: servo 1={servo1:.12g}, servo 2={servo2:.12g} degrees\n"
            "# Direction -1 maps each logical angle to DEFAULT_SERVO_RANGE - angle.\n"
            "# Valve direction -1 uses 90 - valve angle before the servo mappings.\n"
            f"DEFAULT_CLOSE_ANGLE = {lead / gear_ratio:.12g}\n"
            f"DEFAULT_SERVO2_OFFSET = {offset:.12g}\n"
            f"DEFAULT_SERVO1_DIRECTION = {direction1:g}\n"
            f"DEFAULT_SERVO2_DIRECTION = {direction2:g}\n"
            f"DEFAULT_VALVE_DIRECTION = {valve_direction:g}\n"
            f"DEFAULT_GEAR_RATIO = {gear_ratio:.12g}\n"
            f"DEFAULT_SERVO_RANGE = {servo_range:.12g}\n")
    try:
        for angle in (0, 90):
            positions(lead, offset, angle, gear_ratio, servo_range, direction1, direction2, valve_direction)
    except ValueError:
        text += "# Full valve stroke exceeds servo travel; adjust calibration before using these defaults.\n"
    else:
        text += "# Full valve stroke: valid\n"
    return text


def show(lead, offset, valve, gear_ratio, servo_range, direction1=1, direction2=1, valve_direction=1):
    print(settings_text(lead, offset, valve, gear_ratio, servo_range, direction1, direction2, valve_direction), end="", flush=True)


def save_settings(write, text, pin1, pin2, valve):
    write(f"# Calibration {datetime.now().astimezone().isoformat(timespec='seconds')}\n"
          f"# BOARD pins: servo1={pin1}, servo2={pin2}\n"
          f"DEFAULT_SERVO1_PIN = {pin1}\nDEFAULT_SERVO2_PIN = {pin2}\n"
          f"CALIBRATION_VALVE_ANGLE = {valve:.12g}\n{text}")


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Position MTV servos independently or adjust their alignment and valve zero.",
        epilog=HELP, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--pin1", type=int, help="Lead servo BOARD pin (saved value, otherwise 15)")
    parser.add_argument("--pin2", type=int, help="Adjusted servo BOARD pin (saved value, otherwise 32)")
    parser.add_argument("--lead", type=float, help="Override the logical lead calibration reference in servo degrees")
    parser.add_argument("--offset", type=float, help="Override servo 2's logical offset in servo degrees")
    parser.add_argument("--direction1", type=int, choices=(-1, 1), help="Override servo 1's direction")
    parser.add_argument("--direction2", type=int, choices=(-1, 1), help="Override servo 2's direction")
    parser.add_argument("--valve-direction", type=int, choices=(-1, 1), help="Valve mapping: 1 normal, -1 swaps 0 and 90")
    parser.add_argument("--valve", type=float, help="Initial valve angle (saved position, otherwise 0)")
    source = parser.add_mutually_exclusive_group()
    source.add_argument("--input", type=Path, help="Import a calibration from another text file")
    source.add_argument("--defaults", action="store_true", help="Ignore saved calibration and start from code defaults")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT, help="Calibration file to load and replace (default: mtv_calibration.txt beside this script)")
    args = parser.parse_args(argv)
    path = args.input if args.input is not None else args.output
    try:
        saved = load_calibration(path) if not args.defaults and (args.input is not None or path.exists()) else {}
    except (OSError, ValueError) as error:
        parser.error(str(error))

    from mtv_module import DEFAULT_CLOSE_ANGLE, DEFAULT_SERVO2_OFFSET, DEFAULT_GEAR_RATIO, DEFAULT_SERVO_RANGE
    from mtv_module import DEFAULT_SERVO1_DIRECTION, DEFAULT_SERVO2_DIRECTION, DEFAULT_VALVE_DIRECTION
    from mtv_servo import Servo
    from procedure_io import cleanup
    import Jetson.GPIO as GPIO

    ratio = saved.get("gear_ratio", DEFAULT_GEAR_RATIO)
    servo_range = saved.get("servo_range", DEFAULT_SERVO_RANGE)
    lead = saved.get("close_angle", DEFAULT_CLOSE_ANGLE) * ratio if args.lead is None else args.lead
    offset = saved.get("servo2_offset", DEFAULT_SERVO2_OFFSET) if args.offset is None else args.offset
    direction1 = saved.get("servo1_direction", DEFAULT_SERVO1_DIRECTION) if args.direction1 is None else args.direction1
    direction2 = saved.get("servo2_direction", DEFAULT_SERVO2_DIRECTION) if args.direction2 is None else args.direction2
    valve_direction = saved.get("valve_direction", DEFAULT_VALVE_DIRECTION) if args.valve_direction is None else args.valve_direction
    valve = saved.get("valve", 0) if args.valve is None else args.valve
    pin1 = saved.get("pin1", 15) if args.pin1 is None else args.pin1
    pin2 = saved.get("pin2", 32) if args.pin2 is None else args.pin2
    if not 1 <= pin1 <= 40 or not 1 <= pin2 <= 40 or pin1 == pin2:
        parser.error("Servos require distinct BOARD pins from 1 to 40")
    try:
        angles = positions(lead, offset, valve, ratio, servo_range, direction1, direction2, valve_direction)
    except ValueError as error:
        parser.error(str(error))

    servo1 = servo2 = None
    try:
        with calibration_writer(args.output) as write:
            servo1 = Servo(pin1, range=servo_range, initial_angle=angles[0])
            servo2 = Servo(pin2, range=servo_range, initial_angle=angles[1])
            print(HELP)
            print(f"{ratio:g} servo degrees per valve degree; close_angle = lead reference / {ratio:g}.")
            print(f"Autosaving changes to {args.output.resolve()}", flush=True)
            text = settings_text(lead, offset, valve, ratio, servo_range, direction1, direction2, valve_direction)
            save_settings(write, text, pin1, pin2, valve)
            print(text, end="", flush=True)
            while True:
                line = input("calibration> ").strip().lower()
                if line in ("quit", "exit", "q"):
                    break
                if line == "help":
                    print(HELP)
                    continue
                if line in ("", "show"):
                    print(text, end="", flush=True)
                    continue
                if line == "save":
                    save_settings(write, text, pin1, pin2, valve)
                    print(f"Saved to {args.output.resolve()}", flush=True)
                    continue
                try:
                    candidate = adjustment(line, lead, offset, valve, ratio, direction1, direction2, valve_direction)
                    targets = positions(*candidate[:3], ratio, servo_range, *candidate[3:])
                except ValueError as error:
                    print(f"Rejected: {error}", flush=True)
                    continue
                if not math.isclose(targets[0], angles[0], rel_tol=0, abs_tol=1e-9):
                    servo1.command(targets[0])
                if not math.isclose(targets[1], angles[1], rel_tol=0, abs_tol=1e-9):
                    servo2.command(targets[1])
                lead, offset, valve, direction1, direction2, valve_direction = candidate
                angles = targets
                text = settings_text(lead, offset, valve, ratio, servo_range, direction1, direction2, valve_direction)
                save_settings(write, text, pin1, pin2, valve)
                print(text, end="", flush=True)
    except (KeyboardInterrupt, EOFError):
        pass
    finally:
        for servo in (servo1, servo2):
            if servo is not None:
                cleanup(servo.stop)
        cleanup(GPIO.cleanup)


if __name__ == "__main__":
    main()

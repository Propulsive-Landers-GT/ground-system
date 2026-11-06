import argparse
import math
from datetime import datetime
from pathlib import Path

DEFAULT_OUTPUT = Path(__file__).with_name("mtv_calibration.txt")

HELP = """Commands (press Enter to apply):
  servo1 N   Set servo 1's absolute angle, leaving servo 2 unchanged
  servo2 N   Set servo 2's absolute angle, leaving servo 1 unchanged
  lead N     Set the lead servo's valve-zero position, in servo degrees
  offset N   Set servo 2's valve-zero offset from the lead, in servo degrees
  jog N      Move the lead by N servo degrees; servo 2 follows its direction
  trim N     Add N servo degrees to servo 2's offset only
  valve N    Test a valve angle from 0 to 90 degrees using this calibration
  direction1 N / direction2 N   Set a servo's direction to +1 or -1 at valve 0
  flip1 / flip2                 Reverse a servo's direction at valve 0
  show       Print positions and defaults to copy into mtv_module.py
  save       Append current settings to the output file (changes also autosave)
  help       Show these commands
  quit       Stop PWM without commanding another position (also Ctrl-C/EOF)
"""


def positions(lead, offset, valve, gear_ratio, servo_range, direction1=1, direction2=1):
    if not all(math.isfinite(v) for v in (lead, offset, valve, gear_ratio, servo_range)):
        raise ValueError("Positions and configuration must be finite")
    if not 0 <= valve <= 90 or gear_ratio <= 0 or servo_range <= 0:
        raise ValueError("Valve angle must be 0–90 degrees; ratio and servo range must be positive")
    if direction1 not in (-1, 1) or direction2 not in (-1, 1):
        raise ValueError("Servo directions must be +1 or -1")
    angles = lead + direction1 * valve * gear_ratio, lead + offset + direction2 * valve * gear_ratio
    if not all(0 <= angle <= servo_range for angle in angles):
        raise ValueError(f"Both servo positions must be within 0–{servo_range:g} degrees")
    return angles


def adjustment(line, lead, offset, valve, gear_ratio, direction1=1, direction2=1):
    parts = line.lower().split()
    if parts in (["flip1"], ["flip2"]):
        parts = ["direction1", str(-direction1)] if parts[0] == "flip1" else ["direction2", str(-direction2)]
    if len(parts) != 2:
        raise ValueError("Expected a command and a number; type help for commands")
    command, value = parts[0], float(parts[1])
    if command == "servo1":
        offset += lead + direction1 * valve * gear_ratio - value
        lead = value - direction1 * valve * gear_ratio
    elif command == "servo2":
        offset = value - lead - direction2 * valve * gear_ratio
    elif command == "lead":
        offset += (value - lead) * (direction1 * direction2 - 1)
        lead = value
    elif command == "offset":
        offset = value
    elif command == "jog":
        offset += value * (direction1 * direction2 - 1)
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
    return lead, offset, valve, direction1, direction2


def settings_text(lead, offset, valve, gear_ratio, servo_range, direction1=1, direction2=1):
    servo1, servo2 = positions(lead, offset, valve, gear_ratio, servo_range, direction1, direction2)
    text = (f"# lead zero={lead:.12g}, offset={offset:.12g}, valve={valve:.12g}; "
            f"servo 1={servo1:.12g}, servo 2={servo2:.12g} degrees\n"
            f"DEFAULT_CLOSE_ANGLE = {lead / gear_ratio:.12g}\n"
            f"DEFAULT_SERVO2_OFFSET = {offset:.12g}\n"
            f"DEFAULT_SERVO1_DIRECTION = {direction1:g}\n"
            f"DEFAULT_SERVO2_DIRECTION = {direction2:g}\n"
            f"DEFAULT_GEAR_RATIO = {gear_ratio:.12g}\n"
            f"DEFAULT_SERVO_RANGE = {servo_range:.12g}\n")
    try:
        for angle in (0, 90):
            positions(lead, offset, angle, gear_ratio, servo_range, direction1, direction2)
    except ValueError:
        text += "# Full valve stroke exceeds servo travel; adjust calibration before using these defaults.\n"
    else:
        text += "# Full valve stroke: valid\n"
    return text


def show(lead, offset, valve, gear_ratio, servo_range, direction1=1, direction2=1):
    print(settings_text(lead, offset, valve, gear_ratio, servo_range, direction1, direction2), end="", flush=True)


def save_settings(file, text, pin1, pin2):
    file.write(f"\n# Calibration {datetime.now().astimezone().isoformat(timespec='seconds')}\n"
               f"# BOARD pins: servo1={pin1}, servo2={pin2}\n{text}")
    file.flush()


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Position MTV servos independently or adjust their alignment and valve zero.",
        epilog=HELP, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--pin1", type=int, default=15, help="Lead servo BOARD pin (default: 15)")
    parser.add_argument("--pin2", type=int, default=32, help="Adjusted servo BOARD pin (default: 32)")
    parser.add_argument("--lead", type=float, help="Initial lead position in servo degrees (default: MTV calibration)")
    parser.add_argument("--offset", type=float, help="Initial servo 2 offset in servo degrees (default: MTV calibration)")
    parser.add_argument("--direction1", type=int, choices=(-1, 1), help="Servo 1 direction (default: MTV calibration)")
    parser.add_argument("--direction2", type=int, choices=(-1, 1), help="Servo 2 direction (default: MTV calibration)")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT, help="Append calibration settings to this file (default: mtv_calibration.txt beside this script)")
    args = parser.parse_args(argv)
    if args.pin1 == args.pin2:
        parser.error("Servos must use different pins")

    from mtv_module import DEFAULT_CLOSE_ANGLE, DEFAULT_SERVO2_OFFSET, DEFAULT_GEAR_RATIO, DEFAULT_SERVO_RANGE
    from mtv_module import DEFAULT_SERVO1_DIRECTION, DEFAULT_SERVO2_DIRECTION
    from mtv_servo import Servo
    from procedure_io import cleanup
    import Jetson.GPIO as GPIO

    ratio, servo_range = DEFAULT_GEAR_RATIO, DEFAULT_SERVO_RANGE
    lead = DEFAULT_CLOSE_ANGLE * ratio if args.lead is None else args.lead
    offset = DEFAULT_SERVO2_OFFSET if args.offset is None else args.offset
    direction1 = DEFAULT_SERVO1_DIRECTION if args.direction1 is None else args.direction1
    direction2 = DEFAULT_SERVO2_DIRECTION if args.direction2 is None else args.direction2
    valve = 0
    try:
        angles = positions(lead, offset, valve, ratio, servo_range, direction1, direction2)
    except ValueError as error:
        parser.error(str(error))

    servo1 = servo2 = None
    try:
        with args.output.open("a", encoding="utf-8") as file:
            servo1 = Servo(args.pin1, range=servo_range, initial_angle=angles[0])
            servo2 = Servo(args.pin2, range=servo_range, initial_angle=angles[1])
            print(HELP)
            print(f"{ratio:g} servo degrees per valve degree; close_angle = lead zero / {ratio:g}.")
            print(f"Autosaving changes to {args.output.resolve()}", flush=True)
            text = settings_text(lead, offset, valve, ratio, servo_range, direction1, direction2)
            save_settings(file, text, args.pin1, args.pin2)
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
                    save_settings(file, text, args.pin1, args.pin2)
                    print(f"Saved to {args.output.resolve()}", flush=True)
                    continue
                try:
                    candidate = adjustment(line, lead, offset, valve, ratio, direction1, direction2)
                    targets = positions(*candidate[:3], ratio, servo_range, *candidate[3:])
                except ValueError as error:
                    print(f"Rejected: {error}", flush=True)
                    continue
                if not math.isclose(targets[0], angles[0], rel_tol=0, abs_tol=1e-9):
                    servo1.command(targets[0])
                if not math.isclose(targets[1], angles[1], rel_tol=0, abs_tol=1e-9):
                    servo2.command(targets[1])
                lead, offset, valve, direction1, direction2 = candidate
                angles = targets
                text = settings_text(lead, offset, valve, ratio, servo_range, direction1, direction2)
                save_settings(file, text, args.pin1, args.pin2)
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

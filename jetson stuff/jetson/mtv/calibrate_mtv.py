import argparse
import math


HELP = """Commands (press Enter to apply):
  lead N     Set the lead servo's valve-zero position, in servo degrees
  offset N   Set servo 2's signed offset from the lead, in servo degrees
  jog N      Add N servo degrees to the lead position and both servos
  trim N     Add N servo degrees to servo 2's offset only
  valve N    Test a valve angle from 0 to 90 degrees using this calibration
  show       Print positions and defaults to copy into mtv_module.py
  help       Show these commands
  quit       Stop PWM without commanding another position (also Ctrl-C/EOF)
"""


def positions(lead, offset, valve, gear_ratio, servo_range):
    if not all(math.isfinite(v) for v in (lead, offset, valve, gear_ratio, servo_range)):
        raise ValueError("Positions and configuration must be finite")
    if not 0 <= valve <= 90 or gear_ratio <= 0 or servo_range <= 0:
        raise ValueError("Valve angle must be 0–90 degrees; ratio and servo range must be positive")
    angles = lead + valve * gear_ratio, lead + valve * gear_ratio + offset
    if not all(0 <= angle <= servo_range for angle in angles):
        raise ValueError(f"Both servo positions must be within 0–{servo_range:g} degrees")
    return angles


def adjustment(line, lead, offset, valve):
    parts = line.lower().split()
    if len(parts) != 2:
        raise ValueError("Expected a command and a number; type help for commands")
    command, value = parts[0], float(parts[1])
    if command == "lead":
        lead = value
    elif command == "offset":
        offset = value
    elif command == "jog":
        lead += value
    elif command == "trim":
        offset += value
    elif command == "valve":
        valve = value
    else:
        raise ValueError(f"Unknown command: {command}")
    return lead, offset, valve


def show(lead, offset, valve, gear_ratio, servo_range):
    servo1, servo2 = positions(lead, offset, valve, gear_ratio, servo_range)
    print(f"lead zero={lead:.12g}, offset={offset:.12g}, valve={valve:.12g}; "
          f"servo 1={servo1:.12g}, servo 2={servo2:.12g} degrees", flush=True)
    print(f"DEFAULT_CLOSE_ANGLE = {lead / gear_ratio:.12g}\n"
          f"DEFAULT_SERVO2_OFFSET = {offset:.12g}", flush=True)
    try:
        for angle in (0, 90):
            positions(lead, offset, angle, gear_ratio, servo_range)
    except ValueError:
        print("Full valve stroke exceeds servo travel; adjust calibration before using these defaults.", flush=True)


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Adjust MTV servo alignment and valve zero interactively.",
        epilog=HELP, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--pin1", type=int, default=15, help="Lead servo BOARD pin (default: 15)")
    parser.add_argument("--pin2", type=int, default=33, help="Adjusted servo BOARD pin (default: 33)")
    parser.add_argument("--lead", type=float, help="Initial lead position in servo degrees (default: MTV calibration)")
    parser.add_argument("--offset", type=float, help="Initial servo 2 offset in servo degrees (default: MTV calibration)")
    args = parser.parse_args(argv)
    if args.pin1 == args.pin2:
        parser.error("Servos must use different pins")

    from mtv_module import DEFAULT_CLOSE_ANGLE, DEFAULT_SERVO2_OFFSET, DEFAULT_GEAR_RATIO, DEFAULT_SERVO_RANGE
    from mtv_servo import Servo
    from procedure_io import cleanup
    import Jetson.GPIO as GPIO

    ratio, servo_range = DEFAULT_GEAR_RATIO, DEFAULT_SERVO_RANGE
    lead = DEFAULT_CLOSE_ANGLE * ratio if args.lead is None else args.lead
    offset = DEFAULT_SERVO2_OFFSET if args.offset is None else args.offset
    valve = 0
    try:
        angles = positions(lead, offset, valve, ratio, servo_range)
    except ValueError as error:
        parser.error(str(error))

    servo1 = servo2 = None
    try:
        servo1 = Servo(args.pin1, range=servo_range, initial_angle=angles[0])
        servo2 = Servo(args.pin2, range=servo_range, initial_angle=angles[1])
        print(HELP)
        print(f"{ratio:g} servo degrees per valve degree; close_angle = lead zero / {ratio:g}.")
        show(lead, offset, valve, ratio, servo_range)
        while True:
            line = input("calibration> ").strip().lower()
            if line in ("quit", "exit", "q"):
                break
            if line == "help":
                print(HELP)
                continue
            if line in ("", "show"):
                show(lead, offset, valve, ratio, servo_range)
                continue
            try:
                candidate = adjustment(line, lead, offset, valve)
                targets = positions(*candidate, ratio, servo_range)
            except ValueError as error:
                print(f"Rejected: {error}", flush=True)
                continue
            if targets[0] != angles[0]:
                servo1.command(targets[0])
            if targets[1] != angles[1]:
                servo2.command(targets[1])
            lead, offset, valve = candidate
            angles = targets
            show(lead, offset, valve, ratio, servo_range)
    except (KeyboardInterrupt, EOFError):
        pass
    finally:
        for servo in (servo1, servo2):
            if servo is not None:
                cleanup(servo.stop)
        cleanup(GPIO.cleanup)


if __name__ == "__main__":
    main()

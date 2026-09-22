#!/usr/bin/env python3
"""MTV servo helper for gs-stand's `python` backend.

Reuses the team's proven `MTV` / `Servo` classes from `jetson stuff/jetson/mtv/` (Jetson.GPIO
PWM) instead of re-implementing the Jetson PWM in Rust. gs-stand spawns this once and writes
one line per command on stdin:

    <percent>\n     valve opening in percent; negative allowed (re-homing, like MTV.command(-40))
    quit\n          stop PWM, GPIO.cleanup(), exit

percent -> valve angle uses the same `percent_to_angle` as procedure.py (percent / 100 * 90),
then `MTV.command(angle)` applies close_angle, gear ratio and servo2 offset exactly as before.

Note on startup: `Servo.__init__` (legacy) starts PWM at mid-range (range/2 = 177.5°, about
50 % valve opening) for 0.5 s before anything else is commanded. That is legacy behaviour and
happens here too; gs-stand follows it immediately with the -40° re-home and then 0 %.

Run by hand for a bench check:
    echo -e "0\n20\n0\nquit" | python3 stand/mtv_helper.py --legacy-dir "jetson stuff/jetson/mtv"
"""
import argparse
import os
import sys


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--legacy-dir", required=True)
    ap.add_argument("--pin1", type=int, default=15)
    ap.add_argument("--pin2", type=int, default=33)
    ap.add_argument("--close-angle", type=float, default=44.0)
    ap.add_argument("--gear-ratio", type=float, default=2.0)
    ap.add_argument("--servo2-offset", type=float, default=0.0)
    ap.add_argument("--servo-range", type=float, default=355.0)
    ap.add_argument("--full-open", type=float, default=90.0)
    ap.add_argument("--dry-run", action="store_true", help="print instead of driving PWM")
    args = ap.parse_args()

    sys.path.insert(0, os.path.abspath(args.legacy_dir))

    mtv = None
    if not args.dry_run:
        try:
            # mtv_module sets JETSON_MODEL_NAME and imports Jetson.GPIO itself.
            from mtv_module import MTV  # type: ignore
        except Exception as e:  # noqa: BLE001
            print(f"mtv_helper: cannot import legacy MTV from {args.legacy_dir}: {e}", file=sys.stderr, flush=True)
            return 2
        mtv = MTV(
            args.pin1,
            args.pin2,
            close_angle=args.close_angle,
            gear_ratio=args.gear_ratio,
            servo2_offset=args.servo2_offset,
            servo_range=args.servo_range,
        )
    print("mtv_helper: ready", file=sys.stderr, flush=True)

    def command(percent: float) -> None:
        angle = percent / 100.0 * args.full_open
        if mtv is None:
            print(f"mtv_helper: dry-run {percent:.2f} % -> {angle:.2f} deg", file=sys.stderr, flush=True)
        else:
            mtv.command(angle)

    try:
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue
            if line == "quit":
                break
            try:
                command(float(line))
            except ValueError:
                print(f"mtv_helper: ignoring '{line}'", file=sys.stderr, flush=True)
    finally:
        if mtv is not None:
            try:
                mtv.command(0)
                mtv.stop()
                import Jetson.GPIO as GPIO  # type: ignore

                GPIO.cleanup()
            except Exception as e:  # noqa: BLE001
                print(f"mtv_helper: cleanup: {e}", file=sys.stderr, flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())

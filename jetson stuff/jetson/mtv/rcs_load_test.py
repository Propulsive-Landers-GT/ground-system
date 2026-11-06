"""Interactive, single-servo RCS load-cell checkout on the Jetson CLI.

This is separate from rcs_procedure.py: that script drives the two-servo MTV
and contains a fixed timed sequence. This script takes terminal commands for
manual actuation or a timed servo blips sequence and checks Arduino outputs.
"""

import argparse
import csv
import math
import os
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from uuid import uuid4

os.environ.setdefault("JETSON_MODEL_NAME", "JETSON_ORIN_NANO")

import Jetson.GPIO as GPIO
import serial

from mtv_servo import Servo
from procedure_io import cleanup, read_line, send_command, terminal_input, wait_for_message


# Only valves that this procedure is allowed to command closed on exit.
# Pin assignments and open/closed polarity are defined in test_stand.ino.
CLOSE_ON_EXIT_VALVES = frozenset({"omv", "igv", "ofill", "pumv", "pufill", "puiso"})
BAUD = 115200
SAMPLE_TIMEOUT_S = 2.0
STATUS_TIMEOUT_S = 1.0
BLIP_INTERVAL_S = 0.5
BLIP_COUNT = 20  # 10 seconds: open at 0.0, close at 9.5, finish at 10.0.
LOADCELL_NAMES = ("rcs", "engine", "nitrous")

# Edit this block for the installed test hardware, then run:
#     python3 rcs_load_test.py
TEST_CONFIG = {
    "actuation_port": "",  # TODO: relay Arduino path, e.g. /dev/serial/by-id/...
    "loadcell_port": "",   # TODO: HX711 Arduino path; must differ from actuation_port
    "servo_pin": None,     # TODO: Jetson BOARD PWM pin (15, 32, or 33)
    "valves": (),          # Solenoids used, e.g. ("puiso", "pumv"); () for none
    "loadcells": ("rcs",), # Any wired cells: "rcs", "engine", "nitrous"
    # ZOSKAY 25KG/0.13S 6.8V servo, powered at 5 V for this valve.
    # The installed unit is the 270-degree variant; the likely DS3225 datasheet
    # permits 50 Hz. Confirm the exact model and pulse mapping on hardware.
    "servo_hz": 50.0,
    # TODO: Filler pulse/angle mapping and valve endpoints. Measure on the
    # unpressurized mechanism; do not infer the closed/open stops from the
    # servo's advertised travel. The angle labels below are not calibrated
    # physical angles. These defaults command 1000 us closed and 1500 us open;
    # the closed command is sent as soon as the procedure initializes PWM.
    "pulse_min_us": 1000.0,
    "pulse_max_us": 2000.0,
    "servo_range_deg": 270.0,
    "closed_deg": 0.0,
    "open_deg": 135.0,
    "log_dir": Path(__file__).resolve().parent / "data_logs",
}


def finite_number(value):
    number = float(value)
    if not math.isfinite(number):
        raise argparse.ArgumentTypeError("value must be finite")
    return number


def positive_number(value):
    number = finite_number(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("value must be positive")
    return number


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--actuation-port", default=TEST_CONFIG["actuation_port"], help="Relay Arduino path")
    parser.add_argument("--loadcell-port", default=TEST_CONFIG["loadcell_port"], help="HX711 Arduino path")
    parser.add_argument("--servo-pin", type=int, default=TEST_CONFIG["servo_pin"],
                        help="Jetson BOARD pin for the single valve servo")
    parser.add_argument("--servo-hz", type=positive_number, default=TEST_CONFIG["servo_hz"])
    parser.add_argument("--pulse-min-us", type=positive_number, default=TEST_CONFIG["pulse_min_us"])
    parser.add_argument("--pulse-max-us", type=positive_number, default=TEST_CONFIG["pulse_max_us"])
    parser.add_argument("--servo-range-deg", type=positive_number, default=TEST_CONFIG["servo_range_deg"])
    parser.add_argument("--closed-deg", type=finite_number, default=TEST_CONFIG["closed_deg"])
    parser.add_argument("--open-deg", type=finite_number, default=TEST_CONFIG["open_deg"])
    parser.add_argument("--valve", action="append", choices=sorted(CLOSE_ON_EXIT_VALVES),
                        help="Solenoid this test may command and close on exit; repeat as needed")
    parser.add_argument("--loadcell", action="append", choices=LOADCELL_NAMES,
                        help="Load cell to stream; repeat for multiple cells (default: rcs)")
    parser.add_argument("--log-dir", type=Path, default=TEST_CONFIG["log_dir"])
    args = parser.parse_args(argv)
    if not args.actuation_port or not args.loadcell_port:
        parser.error("set actuation_port and loadcell_port in TEST_CONFIG before running")
    if args.actuation_port == args.loadcell_port:
        parser.error("actuation and load-cell ports must differ")
    if args.servo_pin is None:
        parser.error("set servo_pin in TEST_CONFIG before running")
    try:
        args.servo_hz = positive_number(args.servo_hz)
        args.pulse_min_us = positive_number(args.pulse_min_us)
        args.pulse_max_us = positive_number(args.pulse_max_us)
        args.servo_range_deg = positive_number(args.servo_range_deg)
        args.closed_deg = finite_number(args.closed_deg)
        args.open_deg = finite_number(args.open_deg)
    except (ValueError, TypeError, argparse.ArgumentTypeError) as error:
        parser.error(f"invalid TEST_CONFIG servo value: {error}")
    if not 0 <= args.closed_deg <= args.servo_range_deg or not 0 <= args.open_deg <= args.servo_range_deg:
        parser.error("servo endpoints must be inside servo range")
    if args.closed_deg == args.open_deg:
        parser.error("servo endpoints must differ")
    if args.pulse_min_us >= args.pulse_max_us or args.pulse_max_us >= 1e6 / args.servo_hz:
        parser.error("pulse range must fit within one PWM period")
    if args.servo_pin not in (15, 32, 33):
        parser.error("servo pin must be a PWM-capable BOARD pin: 15, 32, or 33")
    args.valve = tuple(dict.fromkeys(TEST_CONFIG["valves"] if args.valve is None else args.valve))
    args.loadcell = tuple(dict.fromkeys(TEST_CONFIG["loadcells"] if args.loadcell is None else args.loadcell))
    if any(name not in CLOSE_ON_EXIT_VALVES for name in args.valve):
        parser.error(f"TEST_CONFIG valves must be among: {', '.join(sorted(CLOSE_ON_EXIT_VALVES))}")
    if not args.loadcell or any(name not in LOADCELL_NAMES for name in args.loadcell):
        parser.error(f"TEST_CONFIG loadcells must be among: {', '.join(LOADCELL_NAMES)}")
    try:
        args.log_dir = Path(args.log_dir)
    except TypeError:
        parser.error("TEST_CONFIG log_dir must be a filesystem path")
    return args


class RcsLoadTest:
    def __init__(self, config):
        self.cfg = config
        self.actuation = self.loadcell = self.servo = None
        self.load_log = self.action_log = None
        self.load_writer = self.action_writer = None
        self.started_at = None
        self.last_sample = {}
        self.streaming = False
        self.active = False
        self.last_relay_check = None
        self.blips_started_at = None
        self.blips_index = 0
        self.servo_position_deg = None

    def elapsed(self, when=None):
        return f"{(time.monotonic() if when is None else when) - self.started_at:.6f}"

    def record_action(self, actuator, state, target_deg="", result="", when=None):
        self.action_writer.writerow((self.elapsed(when), actuator, state, target_deg, result))
        self.action_log.flush()

    def sample(self):
        line = read_line(self.loadcell)
        if not line:
            return
        values = {}
        received_at = time.monotonic()
        for token in line.split():
            name, separator, raw = token.partition(":")
            if name in self.cfg.loadcell and separator:
                try:
                    value = float(raw)
                except ValueError:
                    continue
                if math.isfinite(value):
                    values[name] = value
                    self.last_sample[name] = received_at
        if values:
            self.load_writer.writerow((self.elapsed(received_at), *(values.get(name, "") for name in self.cfg.loadcell)))
            self.load_log.flush()

    def checked_valve(self, valve, action):
        # The sketch frames each command by 10 ms of silence; send_command
        # includes a gap. The Arduino owns pin polarity and reports a logical
        # state after the command. Discard old replies before asking for it.
        while read_line(self.actuation):
            pass
        command = f"{valve} {action}"
        sent_at = time.monotonic()
        try:
            send_command(self.actuation, command)
            send_command(self.actuation, f"{valve} state")
            expected = f"VALVE {valve} {'open' if action == 'open' else 'closed'}"
            deadline = time.monotonic() + STATUS_TIMEOUT_S
            while time.monotonic() < deadline:
                line = read_line(self.actuation)
                if line == expected:
                    self.last_relay_check = time.monotonic()
                    self.record_action(valve, action, result="Arduino output state verified", when=sent_at)
                    return
                if line.startswith(f"VALVE {valve} "):
                    break
                self.sample()
                time.sleep(0.005)
            raise RuntimeError(f"No matching Arduino valve state for {command}")
        except Exception as exc:
            self.record_action(valve, action, result=f"unverified: {exc}", when=sent_at)
            raise

    def check_relay_link(self):
        while read_line(self.actuation):
            pass
        send_command(self.actuation, "sync status")
        deadline = time.monotonic() + STATUS_TIMEOUT_S
        while time.monotonic() < deadline:
            response = read_line(self.actuation)
            if response in ("SYNC is 0", "SYNC is 1"):
                self.last_relay_check = time.monotonic()
                return response
            self.sample()
            time.sleep(0.005)
        raise TimeoutError("No SYNC status from actuation board")

    def set_servo(self, percent):
        if not math.isfinite(percent) or not 0 <= percent <= 100:
            raise ValueError("servo percent must be between 0 and 100")
        angle = self.cfg.closed_deg + (self.cfg.open_deg - self.cfg.closed_deg) * percent / 100
        sent_at = time.monotonic()
        self.servo.command(angle)
        self.servo_position_deg = angle
        state = "closed" if percent == 0 else "open" if percent == 100 else f"{percent:g}%"
        self.record_action("servo", state, angle, "PWM command sent; no position feedback", sent_at)

    def set_endpoint(self, which, angle):
        if not math.isfinite(angle) or not 0 <= angle <= self.cfg.servo_range_deg:
            raise ValueError("servo endpoint must be inside the configured angle range")
        other = self.cfg.open_deg if which == "zero" else self.cfg.closed_deg
        if angle == other:
            raise ValueError("servo endpoints must differ")
        sent_at = time.monotonic()
        self.servo.command(angle)
        self.servo_position_deg = angle
        if which == "zero":
            self.cfg.closed_deg = angle
        else:
            self.cfg.open_deg = angle
        self.record_action("servo", "closed" if which == "zero" else "open", angle,
                           "endpoint updated; PWM command sent", sent_at)
        print(f"Servo {which} endpoint: {angle:g} degrees", flush=True)

    def service_blips(self):
        if self.blips_started_at is None:
            return
        now = time.monotonic()
        if self.blips_index == BLIP_COUNT:
            if now >= self.blips_started_at + BLIP_COUNT * BLIP_INTERVAL_S:
                self.blips_started_at = None
                print("Blips complete; servo closed.", flush=True)
            return
        due = self.blips_started_at + self.blips_index * BLIP_INTERVAL_S
        if now >= due:
            if now - due >= BLIP_INTERVAL_S:
                raise RuntimeError("Blips timing missed an actuation; stopping test")
            self.set_servo(100 if self.blips_index % 2 == 0 else 0)
            self.blips_index += 1

    def execute(self, text):
        parts = text.split()
        if text == "start" and not self.active:
            self.active = True
            print("Actuation enabled. Type 'stop' to close and exit.", flush=True)
        elif text in ("stop", "exit", "end", "e"):
            return False
        elif not self.active:
            print("Type 'start' before actuation, or 'stop' to exit.", flush=True)
        elif self.blips_started_at is not None:
            print("Blips running. Type 'stop' to interrupt.", flush=True)
        elif len(parts) == 3 and parts[0] == "valve" and parts[1] in self.cfg.valve and parts[2] in ("open", "close"):
            self.checked_valve(parts[1], parts[2])
        elif len(parts) == 2 and parts[0] == "servo":
            self.set_servo(float(parts[1]))
        elif len(parts) == 2 and parts[0] in ("set-zero", "set-open"):
            self.set_endpoint("zero" if parts[0] == "set-zero" else "open", float(parts[1]))
        elif text == "blips":
            if self.servo_position_deg != self.cfg.closed_deg:
                print("Command 'servo 0' before starting blips.", flush=True)
                return True
            self.blips_started_at = time.monotonic()
            self.blips_index = 0
            print("Blips started: 20 open/close transitions over 10 seconds.", flush=True)
        else:
            print("Commands: start | valve <name> open|close | servo <0..100> | "
                  "set-zero <deg> | set-open <deg> | blips | stop", flush=True)
        return True

    def run(self):
        try:
            self._run()
        finally:
            self.close()

    def _run(self):
        self.cfg.log_dir.mkdir(parents=True, exist_ok=True)
        run_id = f"{datetime.now(timezone.utc):%Y%m%dT%H%M%SZ}-{uuid4().hex[:8]}"
        load_path = self.cfg.log_dir / f"rcs-load-{run_id}.csv"
        action_path = self.cfg.log_dir / f"rcs-actions-{run_id}.csv"
        self.load_log = load_path.open("x", newline="")
        self.action_log = action_path.open("x", newline="")
        self.load_writer = csv.writer(self.load_log)
        self.action_writer = csv.writer(self.action_log)
        self.load_writer.writerow(("time_s", *self.cfg.loadcell))
        self.action_writer.writerow(("time_s", "actuator", "state", "target_deg", "result"))
        self.load_log.flush()
        self.action_log.flush()
        self.started_at = time.monotonic()
        print(f"Load-cell log: {load_path}\nAction log: {action_path}", flush=True)

        initial_at = time.monotonic()
        self.servo = Servo(self.cfg.servo_pin, hz=self.cfg.servo_hz,
                           low=self.cfg.pulse_min_us, high=self.cfg.pulse_max_us,
                           range=self.cfg.servo_range_deg, initial_angle=self.cfg.closed_deg)
        self.servo_position_deg = self.cfg.closed_deg
        self.record_action("servo", "closed", self.cfg.closed_deg, "initial PWM command", initial_at)
        self.actuation = serial.Serial(self.cfg.actuation_port, BAUD, timeout=0.05, write_timeout=1)
        self.actuation.setDTR(False)
        time.sleep(0.1)
        self.actuation.setDTR(True)
        self.loadcell = serial.Serial(self.cfg.loadcell_port, BAUD, timeout=0.05, write_timeout=1)
        self.loadcell.setDTR(False)
        time.sleep(0.1)
        self.loadcell.setDTR(True)
        for port in (self.actuation, self.loadcell):
            wait_for_message(port, "connected", 5)

        # A banner alone cannot identify either board. Only the relay sketch
        # replies to this query; reject swapped ports before any valve command.
        self.check_relay_link()
        for valve in self.cfg.valve:
            self.checked_valve(valve, "close")
        for name in self.cfg.loadcell:
            send_command(self.loadcell, f"{name} setup")
            wait_for_message(self.loadcell, f"{name} loadcell ready", 10)
        for name in self.cfg.loadcell:
            send_command(self.loadcell, f"{name} begin")
            self.streaming = True
        deadline = time.monotonic() + SAMPLE_TIMEOUT_S
        while len(self.last_sample) < len(self.cfg.loadcell) and time.monotonic() < deadline:
            self.sample()
            time.sleep(0.005)
        missing = set(self.cfg.loadcell) - self.last_sample.keys()
        if missing:
            raise TimeoutError(f"No load-cell sample after begin: {', '.join(sorted(missing))}")
        print("Load-cell samples received. Type 'start' to enable commands, or 'stop' to exit.", flush=True)
        while True:
            self.sample()
            for name in self.cfg.loadcell:
                if time.monotonic() - self.last_sample[name] > SAMPLE_TIMEOUT_S:
                    raise TimeoutError(f"{name} load-cell samples stopped")
            if self.blips_started_at is None and time.monotonic() - self.last_relay_check >= 1.0:
                self.check_relay_link()
            user_input = terminal_input(eof_value="exit")
            if user_input and not self.execute(user_input):
                break
            self.service_blips()
            time.sleep(0.005)

    def close(self):
        # Best effort only: a disconnected Arduino can retain its last output
        # state. Hardware interlocks and independent isolation remain necessary.
        if self.actuation is not None:
            for valve in self.cfg.valve:
                when = time.monotonic()
                try:
                    send_command(self.actuation, f"{valve} close")
                    result = "sent during cleanup; unverified"
                except BaseException as exc:
                    result = f"cleanup failed: {exc}"
                if self.action_writer is not None:
                    cleanup(lambda valve=valve, when=when, result=result:
                            self.record_action(valve, "close", result=result, when=when))
        if self.servo is not None:
            when = time.monotonic()
            try:
                self.servo.command(self.cfg.closed_deg)
                result = "PWM sent during cleanup; no position feedback"
            except BaseException as exc:
                result = f"cleanup failed: {exc}"
            if self.action_writer is not None:
                cleanup(lambda: self.record_action("servo", "closed", self.cfg.closed_deg, result, when))
            cleanup(self.servo.stop)
        if self.loadcell is not None and self.streaming:
            for name in self.cfg.loadcell:
                cleanup(lambda name=name: send_command(self.loadcell, f"{name} end"))
        for port in (self.actuation, self.loadcell):
            if port is not None:
                cleanup(port.close)
        cleanup(GPIO.cleanup)
        for log in (self.load_log, self.action_log):
            if log is not None:
                cleanup(log.close)


def main(argv=None):
    config = parse_args(argv)
    try:
        RcsLoadTest(config).run()
    except (Exception, KeyboardInterrupt) as exc:
        print(f"RCS load test stopped: {exc}", file=sys.stderr, flush=True)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

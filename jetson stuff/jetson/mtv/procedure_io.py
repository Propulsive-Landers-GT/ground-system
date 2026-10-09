import sys
import time
import select
from threading import Event

import Jetson.GPIO as GPIO


def cleanup(*actions):
    for action in actions:
        try:
            action()
        except (Exception, KeyboardInterrupt) as error:
            try:
                print(f"Cleanup failed: {error!r}", file=sys.stderr, flush=True)
            except (Exception, KeyboardInterrupt):
                pass


def send_command(port, command):
    data = command.encode("utf-8")
    try:
        if port.write(data) != len(data):
            raise OSError("Incomplete serial write")
        port.flush()
    finally:
        time.sleep(0.02)  # Arduino readString() uses a 10 ms idle timeout.


def read_line(port):
    return port.readline().decode("utf-8", errors="replace").strip() if port.in_waiting else ""


def terminal_input(eof_value=None):
    if sys.stdin is None:
        return None
    try:
        if select.select([sys.stdin], [], [], 0)[0]:
            line = sys.stdin.readline()
            if line == "":
                return eof_value
            return line.strip().lower() or None
    except (OSError, ValueError):
        pass
    return None


def wait_for_message(port, message, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = read_line(port)
        if value:
            print(value, flush=True)
        if value == message:
            return
        time.sleep(0.005)
    raise TimeoutError(f"Timed out waiting for {message!r}")


def wait_for_start(control, start_pin=None):
    low_seen, started = Event(), Event()

    def changed(channel):
        if GPIO.input(channel) == GPIO.LOW:
            low_seen.set()
        elif low_seen.is_set():
            started.set()

    try:
        if start_pin is not None:
            GPIO.setup(start_pin, GPIO.IN)
            changed(start_pin)
            GPIO.add_event_detect(start_pin, GPIO.BOTH, callback=changed)
            changed(start_pin)
        print("Ready for procedure start", flush=True)
        while True:
            command = control()[0]
            if command in ("exit", "e", "end"):
                return False
            if command == "start" or started.is_set():
                return True
            time.sleep(0.005)
    finally:
        if start_pin is not None:
            cleanup(lambda: GPIO.remove_event_detect(start_pin))


def shutdown(mtv=None, actuation_ser=None, loadcell_ser=None, log_file=None):
    if mtv is not None:
        cleanup(lambda: mtv.command(0))
    if actuation_ser is not None:
        cleanup(lambda: send_command(actuation_ser, "shutdown begin"),
                lambda: send_command(actuation_ser, "reset default"),
                lambda: send_command(actuation_ser, "sync low"))
    if mtv is not None:
        cleanup(lambda: time.sleep(2), mtv.stop)
    if log_file is not None:
        cleanup(log_file.close)
    for port in (actuation_ser, loadcell_ser):
        if port is not None:
            cleanup(port.close)
    cleanup(GPIO.cleanup)

import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import Jetson.GPIO as GPIO
import serial
import csv
import time
from mtv_module import MTV, PFS_Profile
from click_checks import manual_control
from procedure_io import read_line, send_command, shutdown, terminal_input, wait_for_message, wait_for_start

class RCS_Procedure:
    # 0 degrees is closed; 90 degrees is open.
    def __init__(self, pfs_profile, pin1=15, pin2=33, servo2_offset=None, debug=False,
                 start_pin=None, setup_timeout=30, actuation_port="/dev/ttyACM0", loadcell_port="/dev/ttyACM1"):
        if start_pin in (pin1, pin2):
            raise ValueError("Start pin conflicts with a servo pin")
        self.ACTUATION_SERIAL_PORT = actuation_port
        self.LOADCELL_SERIAL_PORT = loadcell_port
        self.BAUD_RATE = 115200
        self.mtv = self.actuation_ser = self.loadcell_ser = self.log_file = None
        self._closed = False
        self.start_pin = start_pin
        self.setup_timeout = setup_timeout
        self.pfs_profile = pfs_profile
        self.debug = debug
        try:
            self.mtv = MTV(pin1, pin2, servo2_offset=servo2_offset)
            self.actuation_ser = serial.Serial(self.ACTUATION_SERIAL_PORT, self.BAUD_RATE, timeout=0.05, write_timeout=1)
            self.actuation_ser.setDTR(False)
            time.sleep(0.1)
            self.actuation_ser.setDTR(True)
            self.loadcell_ser = serial.Serial(self.LOADCELL_SERIAL_PORT, self.BAUD_RATE, timeout=0.05, write_timeout=1)
            self.loadcell_ser.setDTR(False)
            time.sleep(0.1)
            self.loadcell_ser.setDTR(True)
        except BaseException:
            self.shutdown()
            raise

    def shutdown(self):
        if not self._closed:
            self._closed = True
            shutdown(self.mtv, self.actuation_ser, self.loadcell_ser, self.log_file)

    def run(self, on_restart=False):
        try:
            self._run(on_restart)
        finally:
            self.shutdown()

    def _run(self, on_restart):
        if self._closed:
            raise RuntimeError("Procedure is already shut down")
        if not on_restart:
            print("Starting...", flush=True)
            for port in (self.actuation_ser, self.loadcell_ser):
                wait_for_message(port, "connected", self.setup_timeout)
            for name in ('rcs',):
                send_command(self.loadcell_ser, f"{name} setup")
                wait_for_message(self.loadcell_ser, f"{name} loadcell ready", self.setup_timeout)
        else:
            for name in ('rcs',):
                send_command(self.loadcell_ser, f"{name} end")

        control = lambda: manual_control(self.actuation_ser, self.loadcell_ser, self.mtv, debug=self.debug)
        if not wait_for_start(control, self.start_pin):
            return

        self.log_file = file = open('rcs_log.csv', mode="w", newline="")
        writer = csv.writer(file)
        writer.writerow(['timestamp', 'rcs_value'])
        file.flush()
        print("Procedure start", flush=True)
        sync_time = time.monotonic()
        send_command(self.actuation_ser, "sync high")
        self.pfs_profile.start(sync_time)
        self.mtv.command(90)
        while not self.pfs_profile.done():
            if terminal_input() in ("exit", "e", "end"):
                return
            serial_command = self.pfs_profile.update()
            if serial_command is not None:
                if self.debug:
                    print(serial_command, flush=True)
                send_command(self.actuation_ser, serial_command)
                send_command(self.loadcell_ser, serial_command)

            actuation_read = read_line(self.actuation_ser)
            if self.debug and actuation_read:
                print("actuation: " + actuation_read, flush=True)
            loadcell_read = read_line(self.loadcell_ser)
            if loadcell_read:
                if self.debug:
                    print("loadcell: " + loadcell_read, flush=True)
                values = {}
                for token in loadcell_read.split():
                    name, separator, value = token.partition(":")
                    if separator:
                        try:
                            values[name] = float(value)
                        except ValueError:
                            pass
                if any(name in values for name in ('rcs',)):
                    writer.writerow([time.monotonic() - sync_time] + [values.get(name, "") for name in ('rcs',)])
                    file.flush()
            time.sleep(0.001)

        self.mtv.command(0)
        file.close()
        send_command(self.actuation_ser, "sync low")
        print("Manual Control Enabled", flush=True)
        while control()[0] not in ("end", "exit", "e"):
            time.sleep(0.005)

def main():
    pfs_commands = [("rcs begin", 0.0), ("rcs begin", 0.1), ("puiso open", 1.0), ("pumv open", 1.5),  ("puiso close", 4.5), ("pumv close", 5.5),  ("rcs end", 5.6), ("rcs end", 5.7)]

    pfs_profile = PFS_Profile(pfs_commands)

    procedure = RCS_Procedure(pfs_profile, 15, 33, debug=True)
    procedure.run()

if __name__ == "__main__":
    main()

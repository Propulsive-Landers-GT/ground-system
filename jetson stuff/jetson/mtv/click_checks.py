import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import math
import serial
import time
from mtv_module import MTV
from procedure_io import read_line, send_command, shutdown, terminal_input


def manual_control(actuation_ser: serial.Serial, loadcell_ser: serial.Serial, mtv: MTV, debug):
    user_input = terminal_input()
    if user_input:
        parts = user_input.split()
        if len(parts) == 2 and parts[0] == "mtv":
            if parts[1] == "blip":
                try:
                    mtv.command(90)
                    time.sleep(2)
                finally:
                    mtv.command(0)
            else:
                angles = {"open": 90, "close": 0}
                mtv.command(angles[parts[1]] if parts[1] in angles else float(parts[1]))
        elif len(parts) == 3 and parts[:2] == ["ofill", "blip"]:
            duration = float(parts[2])
            if not math.isfinite(duration) or duration < 0:
                raise ValueError("Invalid blip duration")
            try:
                send_command(actuation_ser, "ofill open")
                time.sleep(duration)
            finally:
                send_command(actuation_ser, "ofill close")
        elif user_input not in ("start", "end", "exit", "e"):
            send_command(actuation_ser, user_input)
            send_command(loadcell_ser, user_input)

    actuation_read = read_line(actuation_ser)
    if actuation_read and debug:
        print("actuation: " + actuation_read, flush=True)
    loadcell_read = read_line(loadcell_ser)
    if loadcell_read and debug:
        print("loadcell: " + loadcell_read, flush=True)
    return user_input, actuation_read, loadcell_read


def main():
    ACTUATION_SERIAL_PORT = "/dev/ttyACM0"
    LOADCELL_SERIAL_PORT = "/dev/ttyACM1"
    BAUD_RATE = 115200
    actuation_ser = loadcell_ser = mtv = None
    try:
        mtv = MTV(15, 33)
        actuation_ser = serial.Serial(ACTUATION_SERIAL_PORT, BAUD_RATE, timeout=0.05, write_timeout=1)
        actuation_ser.setDTR(False)
        time.sleep(0.1)
        actuation_ser.setDTR(True)
        loadcell_ser = serial.Serial(LOADCELL_SERIAL_PORT, BAUD_RATE, timeout=0.05, write_timeout=1)
        loadcell_ser.setDTR(False)
        time.sleep(0.1)
        loadcell_ser.setDTR(True)
        while manual_control(actuation_ser, loadcell_ser, mtv, debug=True)[0] not in ("end", "exit", "e"):
            time.sleep(0.005)
    finally:
        shutdown(mtv, actuation_ser, loadcell_ser)

if __name__ == "__main__":
    main()

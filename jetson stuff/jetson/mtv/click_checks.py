import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import Jetson.GPIO as GPIO
import gpiod
import serial
import csv
import sys
import time
from datetime import datetime
import numpy as np
import select
from mtv_module import MTV

def manual_control(actuation_ser: serial.Serial, loadcell_ser: serial.Serial, mtv: MTV, debug):
    user_input = None
    if select.select([sys.stdin], [], [], 0)[0]:
        user_input = sys.stdin.readline().strip().lower()
        input = user_input.split(" ")
        if len(input) > 1 and input[0] == "mtv":
            if input[1].isnumeric() or input[1].startswith("-") and input[1][1:].isnumeric():
                mtv.command(float(input[1]))
            else:
                match input[1]:
                    case "open":
                        mtv.command(90)
                    case "close":
                        mtv.command(0)
                    case "blip":
                        mtv.command(90)
                        time.sleep(2)
                        mtv.command(0)
                    case _:
                        pass
        if len(input) > 2 and input[0] == "ofill" and input[1] == "blip":
            actuation_ser.write("ofill open".encode("utf-8"))
            actuation_ser.flush()
            time.sleep(float(input[2]))
            actuation_ser.write("ofill close".encode("utf-8"))
            actuation_ser.flush()
        else:
            actuation_ser.write(user_input.encode("utf-8"))
            actuation_ser.flush()
            loadcell_ser.write(user_input.encode("utf-8"))
            loadcell_ser.flush()
    
    actuation_read = actuation_ser.readline().decode("utf-8").strip()
    if actuation_read != "" and debug:
        print("actuation: " + actuation_read, flush=True)
    loadcell_read = loadcell_ser.readline().decode("utf-8").strip()
    if loadcell_read != "" and debug:
        print("loadcell: " + loadcell_read, flush=True)
    return (user_input, actuation_read, loadcell_read)


def main():
    ACTUATION_SERIAL_PORT = "/dev/ttyACM0"
    LOADCELL_SERIAL_PORT = "/dev/ttyACM1"
    BAUD_RATE = 115200
    actuation_ser = serial.Serial(ACTUATION_SERIAL_PORT, BAUD_RATE, timeout=0.05)
    actuation_ser.setDTR(False)
    time.sleep(0.1)
    actuation_ser.setDTR(True)
    loadcell_ser = serial.Serial(LOADCELL_SERIAL_PORT, BAUD_RATE, timeout=0.05)
    loadcell_ser.setDTR(False)
    time.sleep(0.1)
    loadcell_ser.setDTR(True)
    mtv = MTV(15, 33, servo2_offset=0)
    mtv.command(-40)
    time.sleep(3)
    mtv.command(0)
    try:
        while True:
            manual_control(actuation_ser, loadcell_ser, mtv, debug=True)
    except KeyboardInterrupt:
        mtv.stop()
        GPIO.cleanup()
        sys.exit()

if __name__ == "__main__":
    main()
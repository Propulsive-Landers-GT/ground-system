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
from mtv_module import MTV, PFS_Profile
from click_checks import manual_control

class RCS_Procedure:
    # 0 angle is closed, 180 angle is open
    def __init__(self, pfs_profile, pin1=15, pin2=33, servo2_offset=0, debug=False):
        self.ACTUATION_SERIAL_PORT = "/dev/ttyACM0"
        self.LOADCELL_SERIAL_PORT = "/dev/ttyACM1"
        self.BAUD_RATE = 115200
        
        GPIO.setmode(GPIO.BOARD)

        # pin1, pin2, servo_offset
        self.mtv = MTV(pin1, pin2, servo2_offset=servo2_offset)
        self.mtv.command(90) # TODO: set this to open

        self.actuation_ser = serial.Serial(self.ACTUATION_SERIAL_PORT, self.BAUD_RATE, timeout=0.05)
        self.actuation_ser.setDTR(False)
        time.sleep(0.1)
        self.actuation_ser.setDTR(True)

        self.loadcell_ser = serial.Serial(self.LOADCELL_SERIAL_PORT, self.BAUD_RATE, timeout=0.05)
        self.loadcell_ser.setDTR(False)
        time.sleep(0.1)
        self.loadcell_ser.setDTR(True)
        
        self.pfs_profile = pfs_profile
        self.debug = debug
    
    def shutdown(self):
        print("Setting MTV to zero", flush=True)
        self.mtv.command(0)
        time.sleep(2)
        print("Shutting down", flush=True)
        self.mtv.stop()
        GPIO.cleanup()
        sys.exit()
    
    def run(self, on_restart=False):
        # Read from MTV start pin once we begin
        # Read over Serial usb
        # Output an angle profile!
        
        if not on_restart:
            print("Starting...", flush=True)

            actuation_ready = False
            loadcell_ready = False
            while True:
                actuation_read = self.actuation_ser.readline().decode("utf-8").strip()
                loadcell_read = self.loadcell_ser.readline().decode("utf-8").strip()
                if actuation_read != "":
                    print("actuation: " + actuation_read, flush=True)
                if loadcell_read != "":
                    print("loadcell: " + loadcell_read, flush=True)
                if actuation_read == "connected":
                    actuation_ready = True
                if loadcell_read == "connected":
                    loadcell_ready = True
                if actuation_ready and loadcell_ready:
                    break

            print("Connected to Arduino", flush=True)

            print("Setting up rcs loadcell", flush=True)
            self.loadcell_ser.write("rcs setup".encode("utf-8"))
            self.loadcell_ser.flush()
            while True:
                read = self.loadcell_ser.readline().decode("utf-8").strip()
                if read != "":
                    print(read, flush=True)
                if read == "rcs loadcell ready":
                    break
            print("rcs loadcell setup complete", flush=True)
        else:
            actuation_ready = True
            loadcell_ready = True

            time.sleep(0.2)
            self.loadcell_ser.reset_input_buffer()
            self.loadcell_ser.write("rcs end".encode("utf-8"))
            self.loadcell_ser.flush()
            time.sleep(0.2)

        
        print("Ready for procedure start", flush=True)

        try: 
            while True:
                user_input, actuation_read, loadcell_read = manual_control(self.actuation_ser, self.loadcell_ser, self.mtv, debug=True)
                if not user_input is None:
                    if user_input == "start":
                        break
        except KeyboardInterrupt:
            self.shutdown()

        print("Procedure start", flush=True)

        self.actuation_ser.write("sync high".encode("utf-8"))
        self.actuation_ser.flush()
        sync_time = datetime.now()

        time.sleep(0.2)
        
        file = open('rcs_log.csv', mode='w', newline='')

        writer = csv.writer(file)
        writer.writerow(["timestamp", "rcs_value"])
        file.flush()
        
        self.pfs_profile.start()
        while (not self.pfs_profile.done()):
            if select.select([sys.stdin], [], [], 0)[0]:
                user_input = sys.stdin.readline().strip()

                if user_input == "exit" or user_input == "e":
                    self.actuation_ser.reset_input_buffer()
                    self.actuation_ser.write("shutdown begin".encode("utf-8"))
                    self.actuation_ser.flush()
                    time.sleep(0.2)
                    self.actuation_ser.reset_input_buffer()
                    self.actuation_ser.write("sync low".encode("utf-8"))
                    self.actuation_ser.flush()
                    self.pfs_profile.restart()
                    self.run(on_restart=True)
                    self.shutdown()
                    break
            
            while self.loadcell_ser.in_waiting:
                if self.debug:
                    actuation_read_value = self.actuation_ser.readline().decode("utf-8").strip()
                    if actuation_read_value:
                        print("actuation: " + actuation_read_value, flush=True)

                loadcell_read_value = self.loadcell_ser.readline().decode("utf-8").strip()
                if loadcell_read_value:
                    if self.debug:
                        print("loadcell: " + loadcell_read_value, flush=True)

                    loadcell_values = loadcell_read_value.split(" ")
                    rcs_value = 0.0
                    for i in loadcell_values:
                        if "rcs" in i:
                            rcs_value = i.split(":")[1]

                    # Get system time
                    timestamp = (datetime.now() - sync_time).total_seconds()

                    # Write to CSV
                    writer.writerow([timestamp, rcs_value])
                    file.flush()

            serial_command = self.pfs_profile.update()
            if serial_command is not None:
                if self.debug:
                    print(serial_command, flush=True)
                self.actuation_ser.reset_input_buffer()
                self.actuation_ser.write(serial_command.encode("utf-8"))
                self.actuation_ser.flush()
                self.loadcell_ser.reset_input_buffer()
                self.loadcell_ser.write(serial_command.encode("utf-8"))
                self.loadcell_ser.flush()
        
        file.flush()
        file.close()


        time.sleep(0.2)
        
        self.actuation_ser.reset_input_buffer()
        self.actuation_ser.write("sync low".encode("utf-8"))
        self.actuation_ser.flush()

        time.sleep(0.2)

        print("Manual Control Enabled", flush=True)

        try: 
            while True:
                user_input, actuation_read, loadcell_read = manual_control(self.actuation_ser, self.loadcell_ser, self.mtv, debug=True)
                if not user_input is None:
                    if user_input == "end":
                        break
        except KeyboardInterrupt:
            self.shutdown()

        self.shutdown()

def main():
    pfs_commands = [("rcs begin", 0.0), ("rcs begin", 0.1), ("puiso open", 1.0), ("pumv open", 1.5),  ("puiso close", 4.5), ("pumv close", 5.5),  ("rcs end", 5.6), ("rcs end", 5.7)]

    pfs_profile = PFS_Profile(pfs_commands)

    procedure = RCS_Procedure(pfs_profile, 15, 33, 0, debug=True)
    procedure.run()

if __name__ == "__main__":
    main()

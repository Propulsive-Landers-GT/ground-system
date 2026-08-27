import serial
import csv
import time
from datetime import datetime

# Adjust this to your actual UART device

SERIAL_PORT = "/dev/ttyACM0"
BAUD_RATE = 115200

def main():
    ser = serial.Serial(SERIAL_PORT, BAUD_RATE, timeout=1)

    try :
        while True:
            read = ser.readline().decode("utf-8").strip()
            if read != "":
                print(read)
    except KeyboardInterrupt:
        ser.write("omv high".encode())
        while True:
            read = ser.readline().decode("utf-8").strip()
            if read != "":
                print(read)
            if read == "Ending...":
                break
    # Open CSV file for writing

    # with open("serial_log.csv", "a", newline="") as csvfile:
    #     writer = csv.writer(csvfile)
    #     writer.writerow(["timestamp", "value"]) # header row

    #     while True:
    #         if ser.in_waiting > 0:
    #             line = ser.readline().decode("utf-8").strip()

    #             # Get system time

    #             timestamp = datetime.now().isoformat()

    #             # Write to CSV

    #             writer.writerow([timestamp, line])
    #             csvfile.flush()

    #             print(f"{timestamp}, {line}")

if __name__ == "__main__":
    main()
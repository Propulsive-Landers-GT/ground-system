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
from mtv_module import MTV

def main():
    GPIO.setmode(GPIO.BOARD)
    mtv = MTV(15, 33, angle_limit=160, servo2_offset=0)

    # for i in range(180):
    #     mtv.command(i)
    #     time.sleep(0.1)

    time.sleep(5)

    mtv.command(0)

    time.sleep(5)

    mtv.command(90)

    time.sleep(5)

    mtv.command(180)

    time.sleep(5)

    mtv.command(0)

    time.sleep(5)
    
    mtv.stop()
    GPIO.cleanup()

if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        GPIO.cleanup()
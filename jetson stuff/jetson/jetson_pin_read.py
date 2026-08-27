import Jetson.GPIO as GPIO
# import gpiod
# from hx711 import HX711
import os
import time
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"


GPIO.setmode(GPIO.BOARD)

input_pin = 19
GPIO.setup(input_pin, GPIO.IN)

print("Reading from Pin 1...")

try:
    while True:

        pin_value = GPIO.input(input_pin)
        print("Pin " , input_pin, "is ", pin_value)

        time.sleep(0.5)
except KeyboardInterrupt:
    print("Exiting...")
finally:
    GPIO.cleanup()

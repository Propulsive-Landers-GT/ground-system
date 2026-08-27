import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import Jetson.GPIO as GPIO
import gpiod
import time
import numpy as np


class Servo:
    # pins 15 and 33 are pwm enabled
    # low and high are the ends of the pwm range, range itself is the degree range of the servo
    def __init__(self, pin, hz=333, low=500, high=2500, range=180):
        self.pin = pin
        self.hz = hz
        self.low = low
        self.high = high
        self.range = range
        
        self.pwm = None
        self.start()
    
    def sweep(self):
        for i in range(0, 101):
            self.pwm.ChangeDutyCycle(i)
            time.sleep(0.1)

    def command(self, angle):
        self.pwm.ChangeDutyCycle(self._get_pwm(angle))
    
    def _get_pwm(self, angle):
        angle = np.clip(angle, 0, self.range)
        return round((self.low + (self.high - self.low) * (angle / self.range)) / (10000/self.hz), 2)
    
    def start(self):
        GPIO.setmode(GPIO.BOARD)
        GPIO.setup(self.pin, GPIO.OUT)

        self.pwm = GPIO.PWM(self.pin, self.hz)
        self.pwm.start(self._get_pwm(self.range / 2))
        time.sleep(0.5)

    def stop(self):
        self.pwm.stop()
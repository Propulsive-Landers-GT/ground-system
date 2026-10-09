import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import Jetson.GPIO as GPIO
import math
import time
from procedure_io import cleanup


class Servo:
    # pins 15 and 33 are pwm enabled
    # low and high are the ends of the pwm range, range itself is the degree range of the servo
    def __init__(self, pin, hz=333, low=500, high=2500, range=180, initial_angle=0):
        if not all(math.isfinite(v) for v in (hz, low, high, range)) or hz <= 0 or range <= 0 or not 0 <= low < high <= 1e6 / hz:
            raise ValueError("Invalid servo configuration")
        self.pin = pin
        self.hz = hz
        self.low = low
        self.high = high
        self.range = range
        self.initial_angle = initial_angle
        
        self.pwm = None
        self.start()
    
    def sweep(self):
        for i in range(0, 101):
            self.command(self.range * i / 100)
            time.sleep(0.1)

    def command(self, angle):
        self.pwm.ChangeDutyCycle(self._get_pwm(angle))
    
    def _get_pwm(self, angle):
        if not math.isfinite(angle) or not 0 <= angle <= self.range:
            raise ValueError("Servo angle out of range")
        return round((self.low + (self.high - self.low) * (angle / self.range)) / (10000/self.hz), 2)
    
    def start(self):
        duty_cycle = self._get_pwm(self.initial_angle)
        try:
            GPIO.setmode(GPIO.BOARD)
            GPIO.setup(self.pin, GPIO.OUT)
            self.pwm = GPIO.PWM(self.pin, self.hz)
            self.pwm.start(duty_cycle)
            time.sleep(0.5)
        except BaseException:
            cleanup(self.stop, lambda: GPIO.cleanup(self.pin))
            raise

    def stop(self):
        if self.pwm is not None:
            self.pwm.stop()

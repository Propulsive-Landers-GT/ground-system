import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import Jetson.GPIO as GPIO
import math
import time
from mtv_servo import Servo
from procedure_io import cleanup

DEFAULT_CLOSE_ANGLE = 44.0
DEFAULT_SERVO2_OFFSET = 0.0
DEFAULT_GEAR_RATIO = 2
DEFAULT_SERVO_RANGE = 355


class MTV:
    # Pins 15, 32, and 33 are pwm enabled
    # Pins 6, 34, and 39 are ground
    def __init__(self, pin1, pin2, close_angle=None, gear_ratio=DEFAULT_GEAR_RATIO,
                 servo2_offset=None, servo_range=DEFAULT_SERVO_RANGE):
        # close_angle uses valve degrees; servo2_offset uses servo degrees.
        close_angle = DEFAULT_CLOSE_ANGLE if close_angle is None else close_angle
        servo2_offset = DEFAULT_SERVO2_OFFSET if servo2_offset is None else servo2_offset
        if pin1 == pin2:
            raise ValueError("Servos must use different pins")
        if not all(math.isfinite(v) for v in (close_angle, gear_ratio, servo2_offset, servo_range)) or gear_ratio <= 0 or servo_range <= 0:
            raise ValueError("Invalid MTV calibration")
        endpoints = [(close_angle + angle) * gear_ratio + offset
                     for angle in (0, 90) for offset in (0, servo2_offset)]
        if not all(0 <= angle <= servo_range for angle in endpoints):
            raise ValueError("Calibration must allow the full 0–90 degree valve stroke on both servos")
        self.offset = servo2_offset
        self.gear_ratio = gear_ratio
        self.close_angle = close_angle * self.gear_ratio
        self.servo1 = self.servo2 = None
        try:
            GPIO.setmode(GPIO.BOARD)
            self.servo1 = Servo(pin1, range=servo_range, initial_angle=self.close_angle)
            self.servo2 = Servo(pin2, range=servo_range, initial_angle=self.close_angle + self.offset)
        except BaseException:
            cleanup(self.stop, GPIO.cleanup)
            raise
    
    def command(self, angle):
        if not math.isfinite(angle) or not 0 <= angle <= 90:
            raise ValueError("MTV angle must be between 0 and 90 degrees")
        command_angle = self.close_angle + angle * self.gear_ratio
        self.servo1._get_pwm(command_angle)
        self.servo2._get_pwm(command_angle + self.offset)
        try:
            self.servo1.command(command_angle)
        finally:
            self.servo2.command(command_angle + self.offset)

    def stop(self):
        try:
            if self.servo1 is not None:
                self.servo1.stop()
        finally:
            if self.servo2 is not None:
                self.servo2.stop()

class Angle_Profile:
    # Angles in degrees, times is in seconds
    def __init__(self, commands, initial_angle=0, final_angle=0):
        self.commands = list(commands)
        self.initial_angle = initial_angle
        self.final_angle = final_angle
        if not self.commands or not all(math.isfinite(a) and 0 <= a <= 90 for a in (initial_angle, final_angle)):
            raise ValueError("Invalid angle profile")
        end = 0
        for command in self.commands:
            if not command or command[0] not in ("hold", "ramp") or len(command) != (4 if command[0] == "hold" else 5):
                raise ValueError("Invalid angle command")
            if not all(math.isfinite(v) for v in command[1:]) or not end <= command[-2] < command[-1]:
                raise ValueError("Angle intervals must be ordered, non-overlapping, and positive")
            if not all(0 <= angle <= 90 for angle in command[1:-2]):
                raise ValueError("Angle out of range")
            end = command[-1]
        self.start_time = 0

    def get_init_angle(self):
        return self.initial_angle
    
    def start(self, start_time=None):
        self.start_time = time.monotonic() if start_time is None else start_time
        return self.update()

    def update(self):
        elapsed = time.monotonic() - self.start_time
        angle = self.initial_angle
        for command in self.commands:
            if elapsed < command[-2]:
                return angle
            if elapsed < command[-1]:
                return self.parse_command(command, elapsed)[0]
            angle = command[1] if command[0] == "hold" else command[2]
        return self.final_angle
    
    def parse_command(self, command, time):
        # ("hold", angle, time1, time2)
        if command[0] == "hold":
            return (command[1], (time >= command[2] and time < command[3]))
        # ("ramp", angle1, angle2, time1, time2)
        elif command[0] == "ramp":
            return (command[1] + (command[2] - command[1]) * (time - command[3]) / (command[4] - command[3]), (time >= command[3] and time < command[4]))

    def done(self):
        return time.monotonic() - self.start_time >= self.commands[-1][-1]

class PFS_Profile:
    # commands are in the form ([command], [time]), with times being in seconds
    def __init__(self, commands):

        self.commands = list(commands)
        previous = 0
        for command, deadline in self.commands:
            if not isinstance(command, str) or not command or not math.isfinite(deadline) or deadline < previous:
                raise ValueError("PFS commands must have ordered, nonnegative times")
            previous = deadline

        self.commands_run = [False] * len(self.commands)
        
        self.start_time = 0
    
    def start(self, start_time=None):
        self.restart()
        self.start_time = time.monotonic() if start_time is None else start_time

    def restart(self):
        self.commands_run = [False] * len(self.commands)

    def update(self):
        elapsed = time.monotonic() - self.start_time
        for i in range(len(self.commands)):
            if elapsed >= self.commands[i][1] and not self.commands_run[i]:
                self.commands_run[i] = True
                return self.commands[i][0]

    def done(self):
        return all(self.commands_run)


def main():
    servo1 = servo2 = None
    try:
        servo1 = Servo(15)
        servo2 = Servo(33)
        servo1.command(0)
        servo2.command(0)
        time.sleep(3)
    finally:
        for servo in (servo1, servo2):
            if servo is not None:
                cleanup(servo.stop)
        cleanup(GPIO.cleanup)


if __name__ == "__main__":
    main()

import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import Jetson.GPIO as GPIO
import gpiod
import time
import numpy as np
from mtv_servo import Servo

class MTV:
    # Pins 15, 32, and 33 are pwm enabled
    # Pins 6, 34, and 39 are ground
    def __init__(self, pin1, pin2, close_angle=44, gear_ratio=2, servo2_offset=0, servo_range=355):
        GPIO.setmode(GPIO.BOARD)
        self.servo1 = Servo(pin1, range=servo_range)
        self.servo2 = Servo(pin2, range=servo_range)
        self.offset = servo2_offset
        self.gear_ratio = gear_ratio
        self.close_angle = close_angle * self.gear_ratio
    
    def command(self, angle):
        # command_angle = np.clip(angle, 0, 90)
        command_angle = angle
        command_angle *= self.gear_ratio
        self.servo1.command(self.close_angle + command_angle)
        self.servo2.command(self.close_angle + command_angle + self.offset)

    def stop(self):
        self.servo1.stop()
        self.servo2.stop()

class Angle_Profile:
    # Angles in degrees, times is in seconds
    def __init__(self, commands):

        self.commands = commands
        
        self.start_time = 0

    def get_init_angle(self):
        return self.parse_command(self.commands[0], 0)[0]
    
    def start(self):
        self.start_time = time.time()
        return self.get_init_angle()

    def update(self):
        for command in self.commands:
            angle, valid = self.parse_command(command, time.time() - self.start_time)
            if valid:
                return angle
        return self.get_init_angle()
    
    def parse_command(self, command, time):
        # ("hold", angle, time1, time2)
        if command[0] == "hold":
            return (command[1], (time >= command[2] and time < command[3]))
        # ("ramp", angle1, angle2, time1, time2)
        elif command[0] == "ramp":
            return (command[1] + (command[2] - command[1]) * (time - command[3]) / (command[4] - command[3]), (time >= command[3] and time < command[4]))

    def done(self):
        if time.time() - self.start_time > self.commands[-1][-1]:
            return True
        return False

class PFS_Profile:
    # commands are in the form ([command], [time]), with times being in seconds
    def __init__(self, commands):

        self.commands = commands

        self.commands_run = [False] * len(self.commands)
        
        self.start_time = 0
    
    def start(self):
        self.start_time = time.time()

    def restart(self):
        self.commands_run = [False] * len(self.commands)

    def update(self):
        # for i in range(len(self.commands)):
        #     if time.time() - self.start_time > self.commands[len(self.commands) - 1 - i][1] and not self.commands_run[len(self.commands) - 1 - i]:
        #         # print(self.commands[len(self.commands) - 1 - i])
        #         self.commands_run[len(self.commands) - 1 - i] = True
        #         return self.commands[len(self.commands) - 1 - i][0]

        for i in range(len(self.commands)):
            if time.time() - self.start_time > self.commands[i][1] and not self.commands_run[i]:
                # print(self.commands[i])
                self.commands_run[i] = True
                return self.commands[i][0]

    def done(self):
        if time.time() - self.start_time > self.commands[-1][1]:
            return True
        return False


def main():
    GPIO.setmode(GPIO.BOARD)

    # GPIO.setup(15, GPIO.OUT)
    # pwm = GPIO.PWM(15, 333)
    # pwm.start(50)
    # time.sleep(5)
    # pwm.stop()
    # GPIO.cleanup()

    servo1 = Servo(15)
    servo2 = Servo(33)

    servo1.command(0)
    servo2.command(0)

    time.sleep(3)

    # servo1.sweep()
    # servo2.sweep()

    # servo1.command(0)
    # servo2.command(0)

    # time.sleep(3)

    # servo1.command(0)
    # servo2.command(0)

    # time.sleep(3)

    # servo1.command(90)
    # servo2.command(90)

    # time.sleep(3)

    # servo1.command(180)
    # servo2.command(180)

    # time.sleep(3)

    # servo1.command(0)
    # servo2.command(0)

    # time.sleep(3)

    servo1.stop()
    servo2.stop()

    GPIO.cleanup()

if __name__ == "__main__":
    main()
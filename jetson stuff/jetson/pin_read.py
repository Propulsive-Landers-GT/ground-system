import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import Jetson.GPIO as GPIO
import matplotlib.pyplot as plt
import matplotlib.animation as animation
import time

PIN = 31
GPIO.setmode(GPIO.BOARD)
GPIO.setup(PIN, GPIO.IN)

fig, ax = plt.subplots()
xdata, ydata = [], []
line, = ax.plot([], [], 'r-')

def init():
    ax.set_xlim(0, 100)
    ax.set_ylim(-0.5, 1.5) # Binary view for PWM discovery
    line.set_data([], [])
    return line,

def update(frame):
    # Read the digital state
    val = GPIO.input(PIN)
    ydata.append(val)
    xdata.append(len(ydata))

    if len(xdata) > 1:
        left = xdata[0]
        right = xdata[-1]

        # Prevent singular transformation if left == right
        if left == right:
            right = left + 1
        
        ax.set_xlim(left, right)
    
    # Ensure Y-axis also has a range
    # If reading digital/PWM, 0 to 1 is safe.
    # If reading Analog, use 0 to 1024.
    ax.set_ylim(-0.1, 1.1)

    print(ydata[-1])

    line.set_data(xdata, ydata)
    return line,

try:
    print(f"Graphing pin {PIN}. move the servo by hand to see changes.")
    ani = animation.FuncAnimation(fig, update, frames=None, init_func=init, blit=True, interval=10)
    plt.show()

    GPIO.cleanup()
except KeyboardInterrupt:
    GPIO.cleanup()
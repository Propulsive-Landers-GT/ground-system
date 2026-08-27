import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import Jetson.GPIO as GPIO
import gpiod
import time



INPUT_PIN = 31

class PWMReader:
    def __init__(self, pin):
        self.pin = pin
        self.start_time = 0
        self.pulse_width = 0

        GPIO.setmode(GPIO.BOARD)
        GPIO.setup(self.pin, GPIO.IN)

        # Add event detection for both rising and falling edges
        GPIO.add_event_detect(self.pin, GPIO.BOTH, callback=self._handle_edge)

    def _handle_edge(self, channel):
        current_time = time.time()
        if GPIO.input(self.pin) == GPIO.HIGH:
            self.start_time = current_time
        else:
            if self.start_time > 0:
                # Convert to milliseconds
                self.pulse_width = (current_time - self.start_time) * 1_000_000
        
    def get_micros(self):
        return self.pulse_width

def main():
    reader = PWMReader(INPUT_PIN)
    print(f"Reading PWM on Pin {INPUT_PIN}... Press Ctrl+C to stop.")

    try:
        while True:
            # Output the pulse width (Neutral should be ~1520)
            print(f"Pulse Width: {reader.get_micros():.2f} us")
            time.sleep(0.1)
    except KeyboardInterrupt:
        GPIO.cleanup()

if __name__ == "__main__":
    main()
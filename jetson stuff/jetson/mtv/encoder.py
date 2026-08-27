import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import Jetson.GPIO as GPIO
import gpiod
import time
import csv
import numpy as np

class Encoder:
    def __init__(self, encoder_pins=[11, 13, 19, 21, 23, 29, 31, 35], translation_path="encoder_translation.csv", debug=False):
        GPIO.setmode(GPIO.BOARD)
        self.encoder_pins = encoder_pins
        for pin in self.encoder_pins:
            GPIO.setup(pin, GPIO.IN)
        self.debug = debug
        self.lookup_table = {}
        self.load_table(translation_path)
    
    def load_table(self, translation_path):
        if not os.path.exists(translation_path):
            print(f"Error: {translation_path} not found!")
            return
        
        with open(translation_path, mode='r') as file:
            reader = csv.DictReader(file)
            for row in reader:
                dec_val = int(row['decimal_number'])
                pos_val = int(row['position'])
                self.lookup_table[dec_val] = pos_val

    def read(self):
        binary = ""
        for pin in self.encoder_pins:
            binary = str(GPIO.input(pin)) + binary
        dec = int(binary, 2)
        pos = self.lookup_table.get(dec, None)
        
        if self.debug:
            print("binary: " + binary)
            print("decimal: " + str(dec))
            print("position: " + str(pos))

        return pos

if __name__ == "__main__":
    encoder = Encoder(debug=True)
    try:
        while True:
            encoder.read()
    except KeyboardInterrupt:
        GPIO.cleanup
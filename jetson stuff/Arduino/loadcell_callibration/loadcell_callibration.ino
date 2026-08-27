#include<Arduino.h>
#include "HX711.h"

const int LOADCELL_DOUT_PIN = 6;
const int LOADCELL_SCK_PIN = 7;
float weight = 0.60822;

HX711 scale;

void setup() {
  // put your setup code here, to run once:
  Serial.begin(115200);
  scale.begin(LOADCELL_DOUT_PIN, LOADCELL_SCK_PIN);
  scale.set_scale(7550);
  scale.tare();
}

void loop() {
  // put your main code here, to run repeatedly:
  while (!scale.is_ready()) {
    ;
  }
  if (Serial.available() > 0) {
    weight = Serial.parseFloat();
    while (Serial.available() > 0) {
      Serial.read();
    }
  }
  float zero_factor = scale.read();
  Serial.print("Zero factor: ");
  Serial.println(zero_factor);
  Serial.print("Scale factor: ");
  Serial.println((zero_factor - scale.get_offset()) / weight, 10);
  Serial.print("With units: ");
  Serial.println(scale.get_units());
  Serial.println("");
  Serial.println("");

  delay(100);
}

#ifndef HX711_H
#define HX711_H

#include <JetsonGPIO.h>
#include <chrono>
#include <thread>

class HX711 {
private:
    int dout_pin;
    int pd_sck_pin;
    int gain;
    long offset;
    float scale;

public:
    HX711(int dout, int pd_sck, int gain = 128);
    ~HX711();

    void begin();
    bool is_ready();
    long read();
    long read_average(int times = 10);
    void set_gain(int gain = 128);
    void tare(int times = 10);
    void set_scale(float scale = 1.0);
    float get_units(int times = 1);
    void power_down();
    void power_up();
};

#endif
#include "hx711.h"

HX711::HX711(int dout, int pd_sck, int gain) 
    : dout_pin(dout), pd_sck_pin(pd_sck), gain(gain), offset(0), scale(1.0) {
}

HX711::~HX711() {
    GPIO::cleanup();
}

void HX711::begin() {
    GPIO::setmode(GPIO::BOARD);
    GPIO::setup(pd_sck_pin, GPIO::OUT);
    GPIO::setup(dout_pin, GPIO::IN);
    GPIO::output(pd_sck_pin, GPIO::LOW);
    set_gain(gain);
}

bool HX711::is_ready() {
    return GPIO::input(dout_pin) == GPIO::LOW;
}

long HX711::read() {
    // Wait for the chip to be ready
    while (!is_ready()) {
        std::this_thread::sleep_for(std::chrono::microseconds(1));
    }
    
    unsigned long value = 0;
    
    // Pulse the clock pin 24 times to read data
    for (int i = 0; i < 24; i++) {
        GPIO::output(pd_sck_pin, GPIO::HIGH);
        std::this_thread::sleep_for(std::chrono::microseconds(1));
        value = value << 1;
        GPIO::output(pd_sck_pin, GPIO::LOW);
        std::this_thread::sleep_for(std::chrono::microseconds(1));
        
        if (GPIO::input(dout_pin)) {
            value++;
        }
    }
    
    // Set gain for next reading
    for (int i = 0; i < gain; i++) {
        GPIO::output(pd_sck_pin, GPIO::HIGH);
        std::this_thread::sleep_for(std::chrono::microseconds(1));
        GPIO::output(pd_sck_pin, GPIO::LOW);
        std::this_thread::sleep_for(std::chrono::microseconds(1));
    }
    
    // Convert to signed value (24-bit two's complement)
    if (value & 0x800000) {
        value |= 0xFF000000;
    }
    
    return static_cast<long>(value);
}

long HX711::read_average(int times) {
    long sum = 0;
    for (int i = 0; i < times; i++) {
        sum += read();
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
    return sum / times;
}

void HX711::set_gain(int gain) {
    switch (gain) {
        case 128:  // Channel A, gain 128
            this->gain = 1;
            break;
        case 64:   // Channel A, gain 64
            this->gain = 3;
            break;
        case 32:   // Channel B, gain 32
            this->gain = 2;
            break;
        default:
            this->gain = 1;
    }
    
    GPIO::output(pd_sck_pin, GPIO::LOW);
    read();  // Perform a read to set the gain
}

void HX711::tare(int times) {
offset = read_average(times);
}

void HX711::set_scale(float scale) {
    this->scale = scale;
}

float HX711::get_units(int times) {
    return (read_average(times) - offset) / scale;
}

void HX711::power_down() {
    GPIO::output(pd_sck_pin, GPIO::LOW);
    GPIO::output(pd_sck_pin, GPIO::HIGH);
    std::this_thread::sleep_for(std::chrono::microseconds(60));
}

void HX711::power_up() {
    GPIO::output(pd_sck_pin, GPIO::LOW);
}

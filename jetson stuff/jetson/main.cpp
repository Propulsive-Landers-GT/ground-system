#include <iostream>
#include <iomanip>
#include "hx711.h"

int main() {
    // Pin numbers (BOARD mode)
    const int DOUT_PIN = 29;
    const int SCK_PIN = 31;
    
    HX711 scale(DOUT_PIN, SCK_PIN);
    scale.begin();
    
    std::cout << "Initializing scale..." << std::endl;
    std::this_thread::sleep_for(std::chrono::seconds(1));
    
    std::cout << "Taring... Remove any weight from the scale." << std::endl;
    std::cin.get();  // Wait for user
    scale.tare(10);
    
    std::cout << "Tare complete. Place known weight for calibration." << std::endl;
    std::cin.get();
    
    long reading = scale.read_average(10);
    float known_weight = 100.0;  // grams
    float calibration_factor = reading / known_weight;
    scale.set_scale(calibration_factor);
    
    std::cout << "Calibration factor: " << calibration_factor << std::endl;
    std::cout << "Reading weight... Press Ctrl+C to exit." << std::endl;
    
    try {
        while (true) {
            if (scale.is_ready()) {
                float weight = scale.get_units(5);
                std::cout << std::fixed << std::setprecision(2) 
                         << "Weight: " << weight << " g" << std::endl;
            }
            std::this_thread::sleep_for(std::chrono::milliseconds(500));
        }
    } catch (...) {
        std::cout << "Exiting..." << std::endl;
    }
    
    return 0;
}

cmake_minimum_required(VERSION 3.10)
project(HX711_Reader)

set(CMAKE_CXX_STANDARD 11)

find_package(JetsonGPIO REQUIRED)

add_executable(hx711_reader main.cpp hx711.cpp)
target_link_libraries(hx711_reader JetsonGPIO::JetsonGPIO)

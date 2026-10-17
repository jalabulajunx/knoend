# OpenPager firmware: one universal image for ESP32-S3 boards with octal PSRAM. What each board
# has (display, touch, pins) comes from its board definition in the "board" partition, written at
# install time (firmware/boards/definitions/*.json, spec/schemas/board.schema.json).
#   idf.py -B build/universal build
cmake_minimum_required(VERSION 3.16)
set(SDKCONFIG "${CMAKE_BINARY_DIR}/sdkconfig")
include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(openpager)

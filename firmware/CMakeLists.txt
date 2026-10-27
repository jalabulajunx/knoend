# KnoEnd firmware: one universal image for ESP32-S3 boards with octal PSRAM. What each board
# has (display, touch, pins) comes from its board definition in the "board" partition, written at
# install time (firmware/boards/definitions/*.json, spec/schemas/board.schema.json).
#   idf.py -B build/universal build
cmake_minimum_required(VERSION 3.16)
set(SDKCONFIG "${CMAKE_BINARY_DIR}/sdkconfig")
include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(knoend)
# LVGL allocates from PSRAM first (main/lv_mem_psram.h says why). Plain names: CMake drops
# function-like definitions.
idf_component_get_property(lvgl_lib lvgl__lvgl COMPONENT_LIB)
target_include_directories(${lvgl_lib} PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/main")
target_sources(${lvgl_lib} PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/main/lv_mem_psram.c")
target_compile_definitions(${lvgl_lib} PRIVATE
    LV_MEM_CUSTOM_ALLOC=knoend_lv_malloc LV_MEM_CUSTOM_REALLOC=knoend_lv_realloc LV_MEM_CUSTOM_FREE=knoend_lv_free)

# Windows Xtensa GCC can crash in IRA while compiling IDF's RGB driver at -Og.
# Keep this workaround on that SDK source file; the ONX uses the SPI driver.
# Upstream report: https://github.com/espressif/esp-idf/issues/12180
if(CMAKE_HOST_WIN32 AND CMAKE_C_COMPILER_ID STREQUAL "GNU")
    idf_component_get_property(knoend_lcd_lib esp_lcd COMPONENT_LIB)
    set_property(SOURCE "$ENV{IDF_PATH}/components/esp_lcd/rgb/esp_lcd_panel_rgb.c"
                 TARGET_DIRECTORY ${knoend_lcd_lib}
                 APPEND PROPERTY COMPILE_OPTIONS "-O0")
endif()

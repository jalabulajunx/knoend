// The contract every board profile implements. The firmware core includes only this header.
//
// Display and touch are handed to LVGL through esp_lvgl_port: after board_init() returns, the
// core draws with LVGL, taking lvgl_port_lock() around every LVGL call made outside an LVGL
// callback. Screen coordinates are landscape, (0,0) top-left, BOARD_* size from board_info().
#pragma once

#include <stdbool.h>
#include <stdint.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    const char *id;     // profile id, e.g. "onx2432g028"; used in the identity packet
    const char *name;   // human-readable board name
    uint16_t width;     // landscape screen width in pixels
    uint16_t height;    // landscape screen height in pixels
    bool has_speaker;
    bool has_native_usb; // true when the USB-C port can enumerate as a USB keyboard
} board_info_t;

// Power rails, buses, display, touch and LVGL. Leaves the backlight off; call
// board_set_backlight() once the first screen is drawn.
esp_err_t board_init(void);

const board_info_t *board_info(void);

// 0 = off, 100 = full.
void board_set_backlight(uint8_t percent);

#ifdef __cplusplus
}
#endif

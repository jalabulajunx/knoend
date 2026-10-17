// The contract the board support implements (components/board_generic, driven by a board
// definition). The firmware core includes only this header.
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

// Which line the companion talks to over USB.
typedef enum {
    BOARD_SERIAL_UART0,           // a USB-serial bridge (CH340, CP210x) on UART0
    BOARD_SERIAL_USB_SERIAL_JTAG, // the ESP32-S3's own USB port
} board_serial_t;

// Power rails, buses, display, touch and LVGL. Leaves the backlight off; call
// board_set_backlight() once the first screen is drawn.
// Returns an error (and the board runs without a display) if no usable board definition exists.
esp_err_t board_init(void);

// The serial line named by the board definition (UART0 when there is none).
board_serial_t board_serial(void);

const board_info_t *board_info(void);

// 0 = off, 100 = full.
void board_set_backlight(uint8_t percent);

#ifdef __cplusplus
}
#endif

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
    uint16_t width;     // screen width in pixels, as the board sits now
    uint16_t height;    // screen height in pixels, as the board sits now
    bool has_speaker;
    bool has_native_usb; // true when the USB-C port can enumerate as a USB keyboard
    bool has_mic;        // a microphone named by the board definition ("mic")
    uint16_t inset;      // keep content this far from every edge: a screen with rounded corners
    bool oled;           // an OLED/AMOLED screen: still pictures burn in, so the UI dims and shifts
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

// Turns the screen `turn` quarter turns clockwise from the board's usual landscape (0-3); touch
// follows. board_info() then gives the new width and height. Call with the LVGL lock held.
void board_set_turn(int turn);

// Physical buttons the definition names, and what each is for. board_button_presses() returns
// the presses since it was last called, one bit per action (a GPIO button pressed to ground, or the
// power key of an AXP2101 power chip); call it every 50-100 ms.
#define BOARD_BUTTON_PAGE   (1u << 0) // next page of keys
#define BOARD_BUTTON_SCREEN (1u << 1) // screen off, or back on
uint32_t board_button_presses(void);

// The microphone, when the definition names one: mono 16-bit samples at `sample_rate` Hz.
// board_mic_read() waits up to `timeout_ms` and returns how many samples it put in `samples`.
esp_err_t board_mic_start(uint32_t sample_rate);
size_t board_mic_read(int16_t *samples, size_t count, uint32_t timeout_ms);
void board_mic_stop(void);

#ifdef __cplusplus
}
#endif

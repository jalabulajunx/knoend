// Bluetooth LE keyboard transport: the board appears as an ordinary keyboard, no companion needed.
#pragma once

#include <stdbool.h>
#include <stdint.h>
#include "esp_err.h"

// Starts advertising as a BLE HID keyboard under `name`. Call once, after nvs_flash_init().
esp_err_t ble_keyboard_start(const char *name);

// True while a computer is connected (safe to call from any task).
bool ble_keyboard_connected(void);

// Sends one key press and release: `usage` is a USB HID keyboard usage ID (0x04 = 'a',
// 0x1E = '1', 0x68 = F13), `modifiers` the HID modifier bits (0x01 = left Ctrl, 0x02 = left
// Shift, 0x04 = left Alt, 0x08 = left GUI). Returns ESP_ERR_INVALID_STATE when not connected.
esp_err_t ble_keyboard_tap(uint8_t modifiers, uint8_t usage);

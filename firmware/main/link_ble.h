// Bluetooth LE data transport (spec/protocol.md section 2): the KnoEnd data service beside the HID
// keyboard, on the same encrypted connection. The companion writes packets to the "to board"
// characteristic and subscribes to "from board"; packets are `\n`-terminated JSON split across
// writes and notifications.
#pragma once

#include <stdbool.h>
#include "esp_err.h"
#include "esp_gatts_api.h"

#define LINK_BLE_MAX_LINE 4096

// `line` is NUL-terminated, without the newline; `too_long` as for the serial link.
typedef void (*link_ble_line_cb)(const char *line, bool too_long);
// The companion subscribed to "from board": a session has started.
typedef void (*link_ble_session_cb)(void);

// Registers the data service with the Bluetooth stack (ble_keyboard_start calls it, once the
// stack runs and GATT events reach link_ble_gatts_event).
esp_err_t link_ble_register(void);

// Where received packets and new sessions go (protocol_start).
void link_ble_start(link_ble_line_cb on_line, link_ble_session_cb on_session);

// The GATT events for the data service; ble_keyboard.c passes on every event, and those for other
// services (the keyboard's) come back false.
bool link_ble_gatts_event(esp_gatts_cb_event_t event, esp_gatt_if_t gatts_if, esp_ble_gatts_cb_param_t *param);

// A companion is subscribed to "from board".
bool link_ble_session_active(void);

// Sends `line` plus a newline as notifications, split to fit the connection's MTU.
void link_ble_send(const char *line);

// OpenPager protocol v0 (spec/protocol.md): packets in and out of the board.
#pragma once

#include <stdbool.h>
#include <stdint.h>
#include "esp_err.h"

// Starts the serial link and announces the board with openpager.identity.
esp_err_t protocol_start(void);

// Sends openpager.key for a tap on a key whose action needs the companion. `page` is the page
// the key was on (1 for a layout with one page).
void protocol_send_key(int slot, const char *layout_id, int page, uint32_t seq);

// Sends openpager.key carrying the keystroke itself (HID modifiers + usage), for a shortcut key
// tapped while no Bluetooth keyboard host is connected: the companion types it instead.
void protocol_send_keystroke(int slot, const char *layout_id, int page, uint32_t seq, uint8_t modifiers, uint8_t usage);

// Sends openpager.page: the person asked for the next (or previous) page of `layout_id`.
void protocol_send_page(const char *layout_id, bool next);

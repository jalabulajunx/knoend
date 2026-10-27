// KnoEnd protocol v0 (spec/protocol.md): packets in and out of the board.
#pragma once

#include <stdbool.h>
#include <stdint.h>
#include "esp_err.h"

// Starts the serial link and announces the board with knoend.identity.
esp_err_t protocol_start(void);

// Sends knoend.key for a tap on a key whose action needs the companion. `page` is the page
// the key was on (1 for a layout with one page).
void protocol_send_key(int slot, const char *layout_id, int page, uint32_t seq);

// Sends knoend.key carrying the keystroke itself (HID modifiers + usage), for a shortcut key
// tapped while no Bluetooth keyboard host is connected: the companion types it instead.
void protocol_send_keystroke(int slot, const char *layout_id, int page, uint32_t seq, uint8_t modifiers, uint8_t usage);

// Sends knoend.page: the person asked for the next (or previous) page of `layout_id`.
void protocol_send_page(const char *layout_id, bool next);

// A new Wi-Fi session starts: answer it in the current message prefix until its companion shows
// which one it uses ("knoend." or the older "openpager.").
void protocol_wifi_session_started(void);

// Relative movement from an explicitly enabled touch surface; no mouse button actions.
void protocol_send_pointer(const char *layout_id, int dx, int dy, uint32_t seq);

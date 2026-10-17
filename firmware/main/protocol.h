// OpenPager protocol v0 (spec/protocol.md): packets in and out of the board.
#pragma once

#include <stdint.h>
#include "esp_err.h"

// Starts the serial link and announces the board with openpager.identity.
esp_err_t protocol_start(void);

// Sends openpager.key for a tap on a key whose action needs the companion.
void protocol_send_key(int slot, const char *layout_id, uint32_t seq);

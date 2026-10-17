// USB-C serial transport (spec/protocol.md section 2): one JSON packet per line on UART0, which
// is also the console. Lines that do not start with '{' are log output and are ignored.
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include "esp_err.h"

#define LINK_SERIAL_MAX_LINE 4096

// `line` is NUL-terminated, without the newline. `too_long` is set (and `line` empty) when a
// line exceeded LINK_SERIAL_MAX_LINE and was discarded.
typedef void (*link_serial_line_cb)(const char *line, bool too_long);

esp_err_t link_serial_start(link_serial_line_cb on_line);

// Writes `line` plus a newline in one UART write, so log output cannot split a packet.
void link_serial_send(const char *line);

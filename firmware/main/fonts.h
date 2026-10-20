// Fonts the person chose, sent by the companion (spec/protocol.md 3.6e) and kept in the "fonts"
// partition: one per slot, TrueType, read in place (mapped from flash) and drawn with tiny_ttf.
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "esp_err.h"

typedef enum { FONT_SLOT_KEYS, FONT_SLOT_START, FONT_SLOTS } font_slot_t;

#define FONT_NAME_BYTES 65

// Finds the partition and maps the fonts already stored. Call once, after nvs_flash_init().
void fonts_init(void);

// "keys" / "start"; false for anything else.
bool fonts_slot_parse(const char *name, font_slot_t *slot);
const char *fonts_slot_name(font_slot_t slot);

// The stored font in `slot` (NULL when there is none): its TrueType bytes, length, name and CRC-32.
const uint8_t *fonts_data(font_slot_t slot, size_t *len, const char **name, uint32_t *crc);

// The largest font a slot holds, in bytes.
size_t fonts_slot_capacity(void);

// Receiving a font: begin (erases what the font needs; the slot is empty until finish), write the
// pieces in order, finish (checks the length and CRC-32, then maps it). The UI must not be using
// the slot's font while it is replaced: call ui_fonts_release() first and ui_fonts_reload() after.
esp_err_t fonts_begin(font_slot_t slot, size_t total, const char *name);
esp_err_t fonts_write(font_slot_t slot, size_t offset, const uint8_t *data, size_t len);
esp_err_t fonts_finish(font_slot_t slot, uint32_t crc);

// Empties the slot (back to the built-in font).
esp_err_t fonts_clear(font_slot_t slot);

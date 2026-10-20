// LVGL's memory (LV_MEM_CUSTOM): PSRAM first, internal RAM only when PSRAM is full. With plain
// malloc, LVGL's many small allocations (TrueType glyphs, the rotation buffer) took internal RAM
// first, until the SPI display driver could not get the internal buffer it sends each update
// from: the update failed silently and LVGL waited forever (the ONX froze, 2026-10-20).
#pragma once

#include <stddef.h>

void *knoend_lv_malloc(size_t size);
void *knoend_lv_realloc(void *ptr, size_t size);
void knoend_lv_free(void *ptr);

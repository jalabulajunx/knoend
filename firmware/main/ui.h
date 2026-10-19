// The key grid, the status bar and the layout currently on the board.
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define UI_KEYS        16 // the largest grid, 4x4
#define UI_LABEL_BYTES 97 // 24 characters of UTF-8, plus NUL
#define UI_ICON_BYTES  66 // a Material Icons name (spec/icons.txt) or a sent image ("app:firefox"), plus NUL
#define UI_DETAIL_BYTES 241 // 240 characters of text, plus NUL

typedef enum {
    UI_SEND_NONE,  // blank slot
    UI_SEND_HID,   // the board types modifiers + usage over the keyboard link
    UI_SEND_EVENT, // the board sends openpager.key and the companion acts
} ui_send_t;

typedef struct {
    ui_send_t send;
    char label[UI_LABEL_BYTES];
    char icon[UI_ICON_BYTES]; // empty: text only
    bool confirm;      // risky: needs a second tap within 3 s
    uint8_t modifiers; // UI_SEND_HID only
    uint8_t usage;     // UI_SEND_HID only
} ui_key_t;

typedef struct {
    char id[65];
    char name[129];
    bool reviewed;
    uint8_t cols, rows;  // this layout's grid, within ui_max_grid
    uint8_t page, pages; // page `page` of `pages` (1 of 1 for most layouts); the companion turns pages
    // Text shown in place of the middle rows (all but the top and bottom), e.g. what a permission
    // request is for. Ignored with fewer than 3 rows.
    char detail[UI_DETAIL_BYTES];
    ui_key_t keys[UI_KEYS]; // index 0 = slot 1, left to right, top to bottom
} ui_layout_t;

typedef enum { UI_STATUS_INFO, UI_STATUS_ATTENTION, UI_STATUS_ALERT } ui_status_level_t;

// Draws the grid with the default layout. Call once after board_init().
void ui_start(void);

// The largest grid this screen draws legibly (each 1-4), from its size.
void ui_max_grid(int *cols, int *rows);

// Replaces the keys. Safe to call from any task.
void ui_apply_layout(const ui_layout_t *layout);

// Keeps a picture sent by the companion (openpager.image) under `name` ("app:firefox"), for keys
// whose icon names it. `pixels` is w*h pixels of RGB565 (byte-swapped, as LVGL draws) + alpha, 3
// bytes each, allocated with heap_caps_malloc; the UI owns it from here on (also on failure).
// Safe to call from any task.
bool ui_add_image(const char *name, int w, int h, uint8_t *pixels, size_t len);

// Shows `text` on the status bar for `ttl_seconds` (0 = until replaced); empty text clears it.
// Safe to call from any task.
void ui_show_status(const char *text, ui_status_level_t level, uint32_t ttl_seconds);

// How the board looks: its theme and which way it sits (spec/protocol.md 3.6d). Kept in NVS with
// the pairing and Wi-Fi settings, read once at start-up.
#pragma once

#include <stdbool.h>

#include "esp_err.h"

typedef enum { DISPLAY_THEME_DARK, DISPLAY_THEME_PAPER } display_theme_t;

typedef struct {
    display_theme_t theme;
    int turn; // quarter turns clockwise from the board's usual landscape, 0-3
} display_settings_t;

// The stored settings (dark, turn 0 when none are stored). Call after nvs_flash_init().
const display_settings_t *display_settings(void);

// "dark" / "paper"; false for anything else.
const char *display_theme_name(display_theme_t theme);
bool display_theme_parse(const char *name, display_theme_t *theme);

// Stores `settings` for the next start.
esp_err_t display_settings_store(const display_settings_t *settings);

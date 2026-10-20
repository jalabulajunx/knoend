#include "display.h"

#include <string.h>

#include "nvs.h"

#define NVS_NAMESPACE "openpager" // storage name kept from before the rename

static display_settings_t current;
static bool loaded;

const display_settings_t *display_settings(void)
{
    if (!loaded) {
        loaded = true;
        nvs_handle_t nvs;
        if (nvs_open(NVS_NAMESPACE, NVS_READONLY, &nvs) == ESP_OK) {
            uint8_t theme = 0, turn = 0;
            if (nvs_get_u8(nvs, "theme", &theme) == ESP_OK && theme <= DISPLAY_THEME_PAPER) {
                current.theme = (display_theme_t)theme;
            }
            if (nvs_get_u8(nvs, "turn", &turn) == ESP_OK && turn <= 3) {
                current.turn = turn;
            }
            nvs_close(nvs);
        }
    }
    return &current;
}

display_settings_t *display_settings_mut(void)
{
    display_settings();
    return &current;
}

const char *display_theme_name(display_theme_t theme)
{
    return theme == DISPLAY_THEME_PAPER ? "paper" : "dark";
}

bool display_theme_parse(const char *name, display_theme_t *theme)
{
    if (name != NULL && strcmp(name, "dark") == 0) {
        *theme = DISPLAY_THEME_DARK;
    } else if (name != NULL && strcmp(name, "paper") == 0) {
        *theme = DISPLAY_THEME_PAPER;
    } else {
        return false;
    }
    return true;
}

esp_err_t display_settings_store(const display_settings_t *settings)
{
    nvs_handle_t nvs;
    esp_err_t err = nvs_open(NVS_NAMESPACE, NVS_READWRITE, &nvs);
    if (err != ESP_OK) {
        return err;
    }
    err = nvs_set_u8(nvs, "theme", (uint8_t)settings->theme);
    if (err == ESP_OK) {
        err = nvs_set_u8(nvs, "turn", (uint8_t)settings->turn);
    }
    if (err == ESP_OK) {
        err = nvs_commit(nvs);
    }
    nvs_close(nvs);
    return err;
}

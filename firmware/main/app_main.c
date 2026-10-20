// KnoEnd firmware: start-up only. The grid lives in ui.c, the protocol in protocol.c.
//
// Every tap prints one console line so a bench test can count presses on the board, the screen
// and the computer:
//   KEY <index 1-9> seq=<n> t=<ms since boot> <hid-sent|hid-not-connected|event-sent|...>
// Protocol packets share the same serial line; they are the lines that start with '{'.

#include <stdio.h>

#include "ble_keyboard.h"
#include "board.h"
#include "esp_app_desc.h"
#include "esp_heap_caps.h"
#include "fonts.h"
#include "nvs_flash.h"
#include "protocol.h"
#include "ui.h"

#define DEVICE_NAME "KnoEnd"

void app_main(void)
{
    esp_err_t err = nvs_flash_init();
    if (err == ESP_ERR_NVS_NO_FREE_PAGES || err == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        err = nvs_flash_init();
    }
    ESP_ERROR_CHECK(err);

    // Without a usable board definition there is no display, but Bluetooth, serial and Wi-Fi
    // still run, so the companion can write a definition over USB.
    fonts_init();
    const bool has_display = board_init() == ESP_OK;
    if (has_display) {
        ui_start();
        board_set_backlight(100);
    } else {
        printf("BOARD no definition: running without a display\n");
        fflush(stdout);
    }

    ESP_ERROR_CHECK(ble_keyboard_start(DEVICE_NAME));

    printf("READY knoend %s board=%s\n", esp_app_get_description()->version, board_info()->id);
    fflush(stdout);
    ESP_ERROR_CHECK(protocol_start());

    // Bluetooth, Wi-Fi and LVGL share internal RAM; this is the number to watch.
    printf("HEAP internal free=%u min=%u, psram free=%u\n",
           (unsigned)heap_caps_get_free_size(MALLOC_CAP_INTERNAL),
           (unsigned)heap_caps_get_minimum_free_size(MALLOC_CAP_INTERNAL),
           (unsigned)heap_caps_get_free_size(MALLOC_CAP_SPIRAM));
    fflush(stdout);
}

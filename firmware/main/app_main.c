// OpenPager firmware: start-up only. The grid lives in ui.c, the protocol in protocol.c.
//
// Every tap prints one console line so a bench test can count presses on the board, the screen
// and the computer:
//   KEY <index 1-9> seq=<n> t=<ms since boot> <hid-sent|hid-not-connected|event-sent|...>
// Protocol packets share the same serial line; they are the lines that start with '{'.

#include <stdio.h>

#include "ble_keyboard.h"
#include "board.h"
#include "esp_app_desc.h"
#include "nvs_flash.h"
#include "protocol.h"
#include "ui.h"

#define DEVICE_NAME "OpenPager"

void app_main(void)
{
    esp_err_t err = nvs_flash_init();
    if (err == ESP_ERR_NVS_NO_FREE_PAGES || err == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        err = nvs_flash_init();
    }
    ESP_ERROR_CHECK(err);

    ESP_ERROR_CHECK(board_init());
    ui_start();
    board_set_backlight(100);

    ESP_ERROR_CHECK(ble_keyboard_start(DEVICE_NAME));

    printf("READY openpager %s board=%s\n", esp_app_get_description()->version, board_info()->id);
    fflush(stdout);
    ESP_ERROR_CHECK(protocol_start());
}

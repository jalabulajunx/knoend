// OpenPager firmware core: draws the key grid and reports each tap.
//
// This stage has no transports yet. Every tap prints one line on the console so a bench test
// can count presses on the board, the screen and the computer:
//   KEY <index 1-9> seq=<n> t=<ms since boot>

#include <inttypes.h>
#include <stdio.h>

#include "board.h"
#include "esp_app_desc.h"
#include "esp_log.h"
#include "esp_lvgl_port.h"
#include "esp_timer.h"
#include "lvgl.h"
#include "nvs_flash.h"

#define GRID_COLS      3
#define GRID_ROWS      3
#define GRID_GAP       4
#define STATUS_HEIGHT  40

static const char *TAG = "openpager";

static lv_obj_t *status_label;
static uint32_t tap_count;

static void key_event(lv_event_t *event)
{
    const int index = (int)(intptr_t)lv_event_get_user_data(event);

    // One tap per press: CLICKED fires once on release, however long the key was held.
    tap_count++;
    printf("KEY %d seq=%" PRIu32 " t=%" PRIu64 "\n", index, tap_count,
           (uint64_t)(esp_timer_get_time() / 1000));
    fflush(stdout);
    lv_label_set_text_fmt(status_label, "Key %d  -  %" PRIu32 " taps", index, tap_count);
}

static void build_screen_locked(void)
{
    const board_info_t *board = board_info();
    lv_obj_t *screen = lv_scr_act();
    lv_obj_set_style_bg_color(screen, lv_color_hex(0x101418), 0);
    lv_obj_clear_flag(screen, LV_OBJ_FLAG_SCROLLABLE);

    const int grid_h = board->height - STATUS_HEIGHT;
    const int key_w = (board->width - GRID_GAP * (GRID_COLS + 1)) / GRID_COLS;
    const int key_h = (grid_h - GRID_GAP * (GRID_ROWS + 1)) / GRID_ROWS;

    for (int row = 0; row < GRID_ROWS; row++) {
        for (int col = 0; col < GRID_COLS; col++) {
            const int index = row * GRID_COLS + col + 1;
            lv_obj_t *key = lv_btn_create(screen);
            lv_obj_set_size(key, key_w, key_h);
            lv_obj_set_pos(key, GRID_GAP + col * (key_w + GRID_GAP),
                           GRID_GAP + row * (key_h + GRID_GAP));
            lv_obj_set_style_bg_color(key, lv_color_hex(0x2A3440), 0);
            lv_obj_set_style_bg_color(key, lv_color_hex(0x3D7EFF), LV_STATE_PRESSED);
            lv_obj_set_style_radius(key, 8, 0);
            lv_obj_add_event_cb(key, key_event, LV_EVENT_CLICKED, (void *)(intptr_t)index);

            lv_obj_t *label = lv_label_create(key);
            lv_label_set_text_fmt(label, "%d", index);
            lv_obj_set_style_text_font(label, &lv_font_montserrat_28, 0);
            lv_obj_center(label);
        }
    }

    status_label = lv_label_create(screen);
    lv_obj_set_width(status_label, board->width - 2 * GRID_GAP);
    lv_obj_set_style_text_font(status_label, &lv_font_montserrat_14, 0);
    lv_obj_set_style_text_color(status_label, lv_color_hex(0xC0C8D0), 0);
    lv_label_set_long_mode(status_label, LV_LABEL_LONG_DOT);
    lv_obj_align(status_label, LV_ALIGN_BOTTOM_LEFT, GRID_GAP, -(STATUS_HEIGHT - 14) / 2);
    lv_label_set_text_fmt(status_label, "OpenPager %s  -  %s  -  tap a key",
                          esp_app_get_description()->version, board->name);
}

void app_main(void)
{
    esp_err_t err = nvs_flash_init();
    if (err == ESP_ERR_NVS_NO_FREE_PAGES || err == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        err = nvs_flash_init();
    }
    ESP_ERROR_CHECK(err);

    ESP_ERROR_CHECK(board_init());

    if (lvgl_port_lock(1000)) {
        build_screen_locked();
        lvgl_port_unlock();
    } else {
        ESP_LOGE(TAG, "could not take the LVGL lock to draw the first screen");
    }
    board_set_backlight(100);

    printf("READY openpager %s board=%s\n", esp_app_get_description()->version, board_info()->id);
    fflush(stdout);
}

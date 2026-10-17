#include "ui.h"

#include <inttypes.h>
#include <stdio.h>
#include <string.h>

#include "ble_keyboard.h"
#include "board.h"
#include "esp_log.h"
#include "esp_lvgl_port.h"
#include "esp_timer.h"
#include "lvgl.h"
#include "protocol.h"

#define GRID_COLS      3
#define GRID_ROWS      3
#define GRID_GAP       4
#define STATUS_HEIGHT  40
#define CONFIRM_MS     3000
#define HID_USAGE_1    0x1E // USB HID usage IDs for '1'..'9' are consecutive

#define COLOR_BG        0x101418
#define COLOR_KEY       0x2A3440
#define COLOR_KEY_BLANK 0x181E25
#define COLOR_PRESSED   0x3D7EFF
#define COLOR_CONFIRM   0xC98A1B
#define COLOR_TEXT      0xC0C8D0
#define COLOR_ATTENTION 0x5A4512
#define COLOR_ALERT     0x6B1C1C

static const char *TAG = "ui";

static ui_layout_t layout;
static lv_obj_t *key_buttons[UI_KEYS];
static lv_obj_t *key_labels[UI_KEYS];
static lv_obj_t *status_bar;
static lv_obj_t *status_label;

static uint32_t tap_count;
static int last_key;

static char status_text[129];
static ui_status_level_t status_level;
static int64_t status_expires_us; // 0 = no expiry

static int confirm_slot = -1;     // index awaiting its second tap, or -1
static int64_t confirm_expires_us;

static void default_layout(ui_layout_t *out)
{
    memset(out, 0, sizeof(*out));
    strcpy(out->id, "default.digits");
    strcpy(out->name, "Digits");
    out->reviewed = true;
    for (int i = 0; i < UI_KEYS; i++) {
        out->keys[i].send = UI_SEND_HID;
        snprintf(out->keys[i].label, UI_LABEL_BYTES, "%d", i + 1);
        out->keys[i].usage = HID_USAGE_1 + i;
    }
}

static void update_status_locked(void)
{
    const int64_t now = esp_timer_get_time();
    if (status_text[0] != '\0' && status_expires_us != 0 && now >= status_expires_us) {
        status_text[0] = '\0';
    }
    if (confirm_slot >= 0 && now >= confirm_expires_us) {
        lv_obj_set_style_bg_color(key_buttons[confirm_slot], lv_color_hex(COLOR_KEY), 0);
        confirm_slot = -1;
    }

    uint32_t bar_color = COLOR_BG;
    if (confirm_slot >= 0) {
        lv_label_set_text_fmt(status_label, "Tap again to confirm: %s",
                              layout.keys[confirm_slot].label);
        bar_color = COLOR_ATTENTION;
    } else if (status_text[0] != '\0') {
        lv_label_set_text(status_label, status_text);
        bar_color = status_level == UI_STATUS_ALERT       ? COLOR_ALERT
                    : status_level == UI_STATUS_ATTENTION ? COLOR_ATTENTION
                                                          : COLOR_BG;
    } else {
        const char *link = ble_keyboard_connected() ? "Bluetooth connected"
                                                    : "Pair \"OpenPager\" over Bluetooth";
        const char *unreviewed = layout.reviewed ? "" : "Unreviewed - ";
        if (last_key == 0) {
            lv_label_set_text_fmt(status_label, "%s%s  -  %s", unreviewed, layout.name, link);
        } else {
            lv_label_set_text_fmt(status_label, "%s%s  -  %s  -  key %d, %" PRIu32 " taps",
                                  unreviewed, layout.name, link, last_key, tap_count);
        }
    }
    lv_obj_set_style_bg_color(status_bar, lv_color_hex(bar_color), 0);
}

static void status_timer(lv_timer_t *timer)
{
    update_status_locked();
}

static void act(int index)
{
    const ui_key_t *key = &layout.keys[index];
    const char *result = "none";

    if (key->send == UI_SEND_HID && ble_keyboard_connected()) {
        const esp_err_t sent = ble_keyboard_tap(key->modifiers, key->usage);
        result = sent == ESP_OK ? "hid-sent" : "hid-error";
    } else if (key->send == UI_SEND_HID) {
        // No Bluetooth host: hand the keystroke to the companion (over Wi-Fi or USB), which types
        // it on the computer. The ONX's USB-C cannot act as a keyboard itself.
        protocol_send_keystroke(index + 1, layout.id, tap_count, key->modifiers, key->usage);
        result = "hid-via-companion";
    } else if (key->send == UI_SEND_EVENT) {
        protocol_send_key(index + 1, layout.id, tap_count);
        result = "event-sent";
    }
    printf("KEY %d seq=%" PRIu32 " t=%" PRIu64 " %s\n", index + 1, tap_count,
           (uint64_t)(esp_timer_get_time() / 1000), result);
    fflush(stdout);
}

// LVGL callback, so the LVGL lock is already held. One tap per press: CLICKED fires once on
// release, however long the key was held.
static void key_event(lv_event_t *event)
{
    const int index = (int)(intptr_t)lv_event_get_user_data(event);
    const ui_key_t *key = &layout.keys[index];
    if (key->send == UI_SEND_NONE) {
        return;
    }
    tap_count++;
    last_key = index + 1;

    if (key->confirm && confirm_slot != index) {
        if (confirm_slot >= 0) {
            lv_obj_set_style_bg_color(key_buttons[confirm_slot], lv_color_hex(COLOR_KEY), 0);
        }
        confirm_slot = index;
        confirm_expires_us = esp_timer_get_time() + CONFIRM_MS * 1000LL;
        lv_obj_set_style_bg_color(key_buttons[index], lv_color_hex(COLOR_CONFIRM), 0);
        printf("KEY %d seq=%" PRIu32 " t=%" PRIu64 " confirm-pending\n", index + 1, tap_count,
               (uint64_t)(esp_timer_get_time() / 1000));
        fflush(stdout);
    } else {
        if (confirm_slot == index) {
            lv_obj_set_style_bg_color(key_buttons[index], lv_color_hex(COLOR_KEY), 0);
            confirm_slot = -1;
        }
        act(index);
    }
    update_status_locked();
}

static void draw_keys_locked(void)
{
    for (int i = 0; i < UI_KEYS; i++) {
        const ui_key_t *key = &layout.keys[i];
        const bool blank = key->send == UI_SEND_NONE;
        lv_obj_set_style_bg_color(key_buttons[i], lv_color_hex(blank ? COLOR_KEY_BLANK : COLOR_KEY), 0);
        lv_label_set_text(key_labels[i], blank ? "" : key->label);
        lv_obj_set_style_text_font(key_labels[i],
                                   strlen(key->label) <= 2 ? &lv_font_montserrat_28
                                                           : &lv_font_montserrat_20, 0);
        if (blank) {
            lv_obj_add_state(key_buttons[i], LV_STATE_DISABLED);
        } else {
            lv_obj_clear_state(key_buttons[i], LV_STATE_DISABLED);
        }
    }
}

void ui_start(void)
{
    default_layout(&layout);
    if (!lvgl_port_lock(1000)) {
        ESP_LOGE(TAG, "could not take the LVGL lock to draw the first screen");
        return;
    }

    const board_info_t *board = board_info();
    lv_obj_t *screen = lv_scr_act();
    lv_obj_set_style_bg_color(screen, lv_color_hex(COLOR_BG), 0);
    lv_obj_clear_flag(screen, LV_OBJ_FLAG_SCROLLABLE);

    const int grid_h = board->height - STATUS_HEIGHT;
    const int key_w = (board->width - GRID_GAP * (GRID_COLS + 1)) / GRID_COLS;
    const int key_h = (grid_h - GRID_GAP * (GRID_ROWS + 1)) / GRID_ROWS;

    for (int i = 0; i < UI_KEYS; i++) {
        const int row = i / GRID_COLS;
        const int col = i % GRID_COLS;
        lv_obj_t *button = lv_btn_create(screen);
        lv_obj_set_size(button, key_w, key_h);
        lv_obj_set_pos(button, GRID_GAP + col * (key_w + GRID_GAP), GRID_GAP + row * (key_h + GRID_GAP));
        lv_obj_set_style_bg_color(button, lv_color_hex(COLOR_PRESSED), LV_STATE_PRESSED);
        lv_obj_set_style_radius(button, 8, 0);
        lv_obj_add_event_cb(button, key_event, LV_EVENT_CLICKED, (void *)(intptr_t)i);

        lv_obj_t *label = lv_label_create(button);
        lv_obj_set_width(label, key_w - 8);
        lv_label_set_long_mode(label, LV_LABEL_LONG_WRAP);
        lv_obj_set_style_text_align(label, LV_TEXT_ALIGN_CENTER, 0);
        lv_obj_center(label);
        key_buttons[i] = button;
        key_labels[i] = label;
    }

    status_bar = lv_obj_create(screen);
    lv_obj_remove_style_all(status_bar);
    lv_obj_set_size(status_bar, board->width, STATUS_HEIGHT);
    lv_obj_align(status_bar, LV_ALIGN_BOTTOM_LEFT, 0, 0);
    lv_obj_set_style_bg_opa(status_bar, LV_OPA_COVER, 0);

    status_label = lv_label_create(status_bar);
    lv_obj_set_width(status_label, board->width - 2 * GRID_GAP);
    lv_obj_set_style_text_font(status_label, &lv_font_montserrat_14, 0);
    lv_obj_set_style_text_color(status_label, lv_color_hex(COLOR_TEXT), 0);
    lv_label_set_long_mode(status_label, LV_LABEL_LONG_DOT);
    lv_obj_align(status_label, LV_ALIGN_LEFT_MID, GRID_GAP, 0);

    draw_keys_locked();
    update_status_locked();
    lv_timer_create(status_timer, 250, NULL);
    lvgl_port_unlock();
}

void ui_apply_layout(const ui_layout_t *new_layout)
{
    if (status_label == NULL) {
        return; // no display (no board definition)
    }
    if (!lvgl_port_lock(1000)) {
        ESP_LOGE(TAG, "could not take the LVGL lock to apply a layout");
        return;
    }
    layout = *new_layout;
    if (confirm_slot >= 0) {
        confirm_slot = -1;
    }
    draw_keys_locked();
    update_status_locked();
    lvgl_port_unlock();
}

void ui_show_status(const char *text, ui_status_level_t level, uint32_t ttl_seconds)
{
    if (status_label == NULL) {
        return; // no display (no board definition)
    }
    if (!lvgl_port_lock(1000)) {
        ESP_LOGE(TAG, "could not take the LVGL lock to show a status");
        return;
    }
    snprintf(status_text, sizeof(status_text), "%s", text);
    status_level = level;
    status_expires_us = ttl_seconds ? esp_timer_get_time() + (int64_t)ttl_seconds * 1000000 : 0;
    update_status_locked();
    lvgl_port_unlock();
}

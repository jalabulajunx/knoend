#include "ui.h"
#include "icons.h"

#include <inttypes.h>
#include <stdio.h>
#include <string.h>

#include "ble_keyboard.h"
#include "board.h"
#include "esp_heap_caps.h"
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
static lv_obj_t *key_icons[UI_KEYS];
static lv_obj_t *status_bar;
static lv_obj_t *detail_box;   // over the middle row when the layout has a detail
static lv_obj_t *detail_label;

// Pictures sent by the companion (app and site icons), least recently used replaced first. The
// pixels live in PSRAM.
#define IMAGE_SLOTS 48
typedef struct {
    char name[UI_ICON_BYTES];
    lv_img_dsc_t dsc;
    uint8_t *pixels;
    uint32_t used;
} sent_image_t;
static sent_image_t images[IMAGE_SLOTS];
static uint32_t image_clock;

static const lv_img_dsc_t *sent_image(const char *name)
{
    for (int i = 0; i < IMAGE_SLOTS; i++) {
        if (images[i].pixels != NULL && strcmp(images[i].name, name) == 0) {
            images[i].used = ++image_clock;
            return &images[i].dsc;
        }
    }
    return NULL;
}
// Grid geometry, set in ui_start: rows shrink to `compact_h` while a detail is shown.
static int key_w, key_h, compact_h, detail_h;
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
    const bool detail = layout.detail[0] != '\0';
    // With a detail, the top and bottom rows get shorter (label only) and the text takes the
    // middle, scrollable by swiping.
    const int row_h = detail ? compact_h : key_h;
    const int row_y[GRID_ROWS] = {GRID_GAP, GRID_GAP + row_h + GRID_GAP,
                                  detail ? GRID_GAP + row_h + GRID_GAP + detail_h + GRID_GAP : GRID_GAP + 2 * (row_h + GRID_GAP)};
    if (detail) {
        lv_label_set_text(detail_label, layout.detail);
        lv_obj_set_pos(detail_box, GRID_GAP, row_y[1]);
        lv_obj_set_height(detail_box, detail_h);
        lv_obj_scroll_to_y(detail_box, 0, LV_ANIM_OFF);
        lv_obj_clear_flag(detail_box, LV_OBJ_FLAG_HIDDEN);
    } else {
        lv_obj_add_flag(detail_box, LV_OBJ_FLAG_HIDDEN);
    }
    for (int i = 0; i < UI_KEYS; i++) {
        const int row = i / GRID_COLS;
        lv_obj_set_pos(key_buttons[i], GRID_GAP + (i % GRID_COLS) * (key_w + GRID_GAP), row_y[row]);
        lv_obj_set_height(key_buttons[i], row_h);
        // The middle row gives way to the detail text.
        if (detail && row == 1) {
            lv_obj_add_flag(key_buttons[i], LV_OBJ_FLAG_HIDDEN);
        } else {
            lv_obj_clear_flag(key_buttons[i], LV_OBJ_FLAG_HIDDEN);
        }
        const ui_key_t *key = &layout.keys[i];
        const bool blank = key->send == UI_SEND_NONE;
        lv_obj_set_style_bg_color(key_buttons[i], lv_color_hex(blank ? COLOR_KEY_BLANK : COLOR_KEY), 0);
        lv_label_set_text(key_labels[i], blank ? "" : key->label);
        // A sent picture ("app:...", in colour) or a Material icon (white).
        const bool sent = strchr(key->icon, ':') != NULL;
        const lv_img_dsc_t *icon = blank || detail ? NULL : sent ? sent_image(key->icon) : icons_find(key->icon);
        lv_obj_set_style_img_recolor_opa(key_icons[i], sent ? LV_OPA_TRANSP : LV_OPA_COVER, 0);
        if (icon != NULL) {
            // Icon above, a small label under it.
            lv_img_set_src(key_icons[i], icon);
            lv_obj_clear_flag(key_icons[i], LV_OBJ_FLAG_HIDDEN);
            lv_obj_align(key_icons[i], LV_ALIGN_CENTER, 0, -10);
            lv_obj_set_style_text_font(key_labels[i], &lv_font_montserrat_14, 0);
            lv_label_set_long_mode(key_labels[i], LV_LABEL_LONG_DOT);
            lv_obj_align(key_labels[i], LV_ALIGN_CENTER, 0, 18);
        } else {
            lv_obj_add_flag(key_icons[i], LV_OBJ_FLAG_HIDDEN);
            lv_obj_set_style_text_font(key_labels[i],
                                       detail ? &lv_font_montserrat_14
                                       : strlen(key->label) <= 2 ? &lv_font_montserrat_28
                                                                 : &lv_font_montserrat_20, 0);
            lv_label_set_long_mode(key_labels[i], LV_LABEL_LONG_WRAP);
            lv_obj_center(key_labels[i]);
        }
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
    key_w = (board->width - GRID_GAP * (GRID_COLS + 1)) / GRID_COLS;
    key_h = (grid_h - GRID_GAP * (GRID_ROWS + 1)) / GRID_ROWS;
    compact_h = key_h * 2 / 3;
    detail_h = grid_h - 2 * compact_h - GRID_GAP * (GRID_ROWS + 1);

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
        lv_obj_t *icon = lv_img_create(button);
        lv_obj_set_style_img_recolor(icon, lv_color_white(), 0);
        lv_obj_set_style_img_recolor_opa(icon, LV_OPA_COVER, 0);
        lv_obj_add_flag(icon, LV_OBJ_FLAG_HIDDEN);
        lv_obj_clear_flag(icon, LV_OBJ_FLAG_CLICKABLE);
        key_buttons[i] = button;
        key_labels[i] = label;
        key_icons[i] = icon;
    }

    detail_box = lv_obj_create(screen);
    lv_obj_remove_style_all(detail_box);
    lv_obj_set_size(detail_box, board->width - 2 * GRID_GAP, key_h);
    lv_obj_set_pos(detail_box, GRID_GAP, GRID_GAP + (key_h + GRID_GAP));
    lv_obj_set_style_bg_opa(detail_box, LV_OPA_COVER, 0);
    lv_obj_set_style_bg_color(detail_box, lv_color_hex(COLOR_KEY_BLANK), 0);
    lv_obj_set_style_radius(detail_box, 8, 0);
    lv_obj_set_style_pad_all(detail_box, 6, 0);
    lv_obj_set_scroll_dir(detail_box, LV_DIR_VER);
    lv_obj_set_scrollbar_mode(detail_box, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_add_flag(detail_box, LV_OBJ_FLAG_HIDDEN);
    detail_label = lv_label_create(detail_box);
    lv_obj_set_width(detail_label, lv_pct(100));
    lv_label_set_long_mode(detail_label, LV_LABEL_LONG_WRAP);
    lv_obj_set_style_text_font(detail_label, &lv_font_montserrat_14, 0);
    lv_obj_set_style_text_color(detail_label, lv_color_white(), 0);
    lv_obj_align(detail_label, LV_ALIGN_TOP_LEFT, 0, 0);

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

bool ui_add_image(const char *name, int w, int h, uint8_t *pixels, size_t len)
{
    if (status_label == NULL || len != (size_t)w * h * 3 || strlen(name) >= UI_ICON_BYTES) {
        heap_caps_free(pixels);
        return false;
    }
    if (!lvgl_port_lock(1000)) {
        heap_caps_free(pixels);
        return false;
    }
    int slot = 0;
    for (int i = 0; i < IMAGE_SLOTS; i++) {
        if (images[i].pixels != NULL && strcmp(images[i].name, name) == 0) {
            slot = i; // replace the same name
            break;
        }
        if (images[i].used < images[slot].used) {
            slot = i;
        }
    }
    sent_image_t *image = &images[slot];
    lv_img_cache_invalidate_src(&image->dsc);
    heap_caps_free(image->pixels);
    snprintf(image->name, sizeof(image->name), "%s", name);
    image->pixels = pixels;
    image->used = ++image_clock;
    image->dsc = (lv_img_dsc_t){
        .header = { .cf = LV_IMG_CF_TRUE_COLOR_ALPHA, .w = w, .h = h },
        .data_size = len,
        .data = pixels,
    };
    draw_keys_locked(); // a key may already name it
    lvgl_port_unlock();
    return true;
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

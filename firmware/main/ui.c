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

#define GRID_GAP       4
#define GRID_MAX       4  // cols and rows
#define MIN_KEY_W      64 // smallest key a finger hits reliably, in pixels
#define MIN_KEY_H      44
#define STATUS_HEIGHT  40
#define PAGER_WIDTH    64 // the page indicator at the right end of the status bar
#define ICON_GAP       4  // between a key's icon and its label
#define ICON_MARGIN    3  // empty border inside a 32 px Material icon; a two-line label may use it
#define LINE_SQUEEZE   2  // how much closer a two-line label's lines are
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
static lv_obj_t *detail_box;   // over the middle rows when the layout has a detail
static lv_obj_t *detail_label;
static lv_obj_t *pager;        // "2/3 >" at the right end of the status bar; a tap turns the page
static lv_obj_t *pager_label;

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
// The area keys are drawn in, set in ui_start; each layout divides it into its own grid.
static int grid_w, grid_h;
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
    out->cols = out->rows = 3;
    out->page = out->pages = 1;
    for (int i = 0; i < 9; i++) {
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
        protocol_send_keystroke(index + 1, layout.id, layout.page, tap_count, key->modifiers, key->usage);
        result = "hid-via-companion";
    } else if (key->send == UI_SEND_EVENT) {
        protocol_send_key(index + 1, layout.id, layout.page, tap_count);
        result = "event-sent";
    }
    printf("KEY %d seq=%" PRIu32 " t=%" PRIu64 " %s\n", index + 1, tap_count,
           (uint64_t)(esp_timer_get_time() / 1000), result);
    fflush(stdout);
}

// Asks the companion for the next or previous page; it answers with that page's layout.
static void turn_page(bool next)
{
    if (layout.pages < 2) {
        return;
    }
    if (confirm_slot >= 0) {
        lv_obj_set_style_bg_color(key_buttons[confirm_slot], lv_color_hex(COLOR_KEY), 0);
        confirm_slot = -1;
    }
    protocol_send_page(layout.id, next);
    printf("PAGE %s from %d/%d t=%" PRIu64 "\n", next ? "next" : "prev", layout.page, layout.pages,
           (uint64_t)(esp_timer_get_time() / 1000));
    fflush(stdout);
}

// A swipe anywhere on the screen (LVGL works it out from the touch points, whatever the chip):
// left for the next page, right for the previous one.
static void gesture_event(lv_event_t *event)
{
    lv_indev_t *indev = lv_indev_get_act();
    if (indev == NULL || layout.pages < 2) {
        return;
    }
    const lv_dir_t dir = lv_indev_get_gesture_dir(indev);
    if (dir != LV_DIR_LEFT && dir != LV_DIR_RIGHT) {
        return;
    }
    lv_indev_wait_release(indev); // LVGL would also send CLICKED to the key the swipe started on
    turn_page(dir == LV_DIR_LEFT);
}

static void pager_event(lv_event_t *event)
{
    turn_page(true);
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
    const int cols = layout.cols, rows = layout.rows;
    const bool detail = layout.detail[0] != '\0' && rows >= 3;
    const int key_w = (grid_w - GRID_GAP * (cols + 1)) / cols;
    const int key_h = (grid_h - GRID_GAP * (rows + 1)) / rows;
    // With a detail, the top and bottom rows get shorter (label only, two thirds of a 3x3 row) and
    // the text takes the rows between them, scrollable by swiping.
    const int compact_h = (grid_h - GRID_GAP * 4) / 3 * 2 / 3;
    const int detail_h = grid_h - 2 * compact_h - GRID_GAP * 4;
    if (detail) {
        lv_label_set_text(detail_label, layout.detail);
        lv_obj_set_pos(detail_box, GRID_GAP, GRID_GAP + compact_h + GRID_GAP);
        lv_obj_set_size(detail_box, grid_w - 2 * GRID_GAP, detail_h);
        lv_obj_scroll_to_y(detail_box, 0, LV_ANIM_OFF);
        lv_obj_clear_flag(detail_box, LV_OBJ_FLAG_HIDDEN);
    } else {
        lv_obj_add_flag(detail_box, LV_OBJ_FLAG_HIDDEN);
    }
    for (int i = 0; i < UI_KEYS; i++) {
        const int row = i / cols;
        // Keys past the grid, and the middle rows under a detail, are hidden.
        if (i >= cols * rows || (detail && row > 0 && row < rows - 1)) {
            lv_obj_add_flag(key_buttons[i], LV_OBJ_FLAG_HIDDEN);
            continue;
        }
        lv_obj_clear_flag(key_buttons[i], LV_OBJ_FLAG_HIDDEN);
        const int h = detail ? compact_h : key_h;
        const int y = !detail ? GRID_GAP + row * (key_h + GRID_GAP)
                      : row == 0 ? GRID_GAP
                                 : GRID_GAP + compact_h + GRID_GAP + detail_h + GRID_GAP;
        lv_obj_set_pos(key_buttons[i], GRID_GAP + (i % cols) * (key_w + GRID_GAP), y);
        lv_obj_set_size(key_buttons[i], key_w, h);
        lv_obj_set_width(key_labels[i], key_w - 8);

        const ui_key_t *key = &layout.keys[i];
        const bool blank = key->send == UI_SEND_NONE;
        const bool big = key_w >= 140 && h >= 90;  // 1x1, 1x2, 2x2
        const bool small = key_w < 90 || h < 50;   // 4 columns or 4 rows
        lv_obj_set_style_bg_color(key_buttons[i], lv_color_hex(blank ? COLOR_KEY_BLANK : COLOR_KEY), 0);
        lv_label_set_text(key_labels[i], blank ? "" : key->label);
        // A sent picture ("app:...", in colour) or a Material icon (white).
        const bool sent = strchr(key->icon, ':') != NULL;
        const lv_img_dsc_t *icon = blank || detail ? NULL : sent ? sent_image(key->icon) : icons_find(key->icon);
        lv_obj_set_style_img_recolor_opa(key_icons[i], sent ? LV_OPA_TRANSP : LV_OPA_COVER, 0);
        if (icon != NULL && h < 56) {
            // No room for both: the icon alone.
            lv_img_set_src(key_icons[i], icon);
            lv_obj_clear_flag(key_icons[i], LV_OBJ_FLAG_HIDDEN);
            lv_obj_center(key_icons[i]);
            lv_label_set_text(key_labels[i], "");
        } else if (icon != NULL) {
            // Icon above, the label under it, the pair centred on the key; a larger label on big
            // keys. Icons stay 32 px: LVGL 8 draws 4-bit alpha images line by line and cannot zoom
            // them (it reads a one-line buffer as the whole image, past its end).
            const int icon_px = 32;
            const lv_font_t *font = big ? &lv_font_montserrat_20 : &lv_font_montserrat_14;
            const int line_px = lv_font_get_line_height(font);
            // A label too long for one line gets two where they fit: lines a little closer, the
            // text up into the icon's empty margin (3x3: 59 of 61 px). Otherwise one line and "...".
            lv_point_t text;
            lv_txt_get_size(&text, key->label, font, 0, 0, key_w - 8, LV_TEXT_FLAG_NONE);
            const bool two = text.y > line_px && icon_px - ICON_MARGIN + 2 * line_px - LINE_SQUEEZE <= h - 2;
            const int label_h = two ? 2 * line_px - LINE_SQUEEZE : line_px;
            const int block = icon_px + (two ? -ICON_MARGIN : ICON_GAP) + label_h;
            lv_img_set_src(key_icons[i], icon);
            lv_obj_clear_flag(key_icons[i], LV_OBJ_FLAG_HIDDEN);
            lv_obj_align(key_icons[i], LV_ALIGN_CENTER, 0, icon_px / 2 - block / 2);
            lv_obj_set_style_text_font(key_labels[i], font, 0);
            lv_obj_set_style_text_line_space(key_labels[i], two ? -LINE_SQUEEZE : 0, 0);
            lv_label_set_long_mode(key_labels[i], LV_LABEL_LONG_DOT);
            lv_obj_set_height(key_labels[i], label_h);
            lv_obj_align(key_labels[i], LV_ALIGN_CENTER, 0, block / 2 - label_h / 2);
        } else {
            lv_obj_add_flag(key_icons[i], LV_OBJ_FLAG_HIDDEN);
            lv_obj_set_style_text_font(key_labels[i],
                                       detail || small            ? &lv_font_montserrat_14
                                       : big || strlen(key->label) <= 2 ? &lv_font_montserrat_28
                                                                        : &lv_font_montserrat_20, 0);
            lv_obj_set_style_text_line_space(key_labels[i], 0, 0);
            lv_label_set_long_mode(key_labels[i], LV_LABEL_LONG_WRAP);
            lv_obj_set_height(key_labels[i], LV_SIZE_CONTENT);
            lv_obj_center(key_labels[i]);
        }
        if (blank) {
            lv_obj_add_state(key_buttons[i], LV_STATE_DISABLED);
        } else {
            lv_obj_clear_state(key_buttons[i], LV_STATE_DISABLED);
        }
    }
    // The page indicator takes the right end of the status bar while there are pages.
    if (layout.pages > 1) {
        lv_label_set_text_fmt(pager_label, "%d/%d " LV_SYMBOL_RIGHT, layout.page, layout.pages);
        lv_obj_clear_flag(pager, LV_OBJ_FLAG_HIDDEN);
        lv_obj_set_width(status_label, grid_w - 2 * GRID_GAP - PAGER_WIDTH);
    } else {
        lv_obj_add_flag(pager, LV_OBJ_FLAG_HIDDEN);
        lv_obj_set_width(status_label, grid_w - 2 * GRID_GAP);
    }
}

void ui_max_grid(int *cols, int *rows)
{
    const board_info_t *board = board_info();
    *cols = board->width / MIN_KEY_W;
    *rows = (board->height - STATUS_HEIGHT) / MIN_KEY_H;
    if (board->width == 0) {
        *cols = *rows = 3; // no display (no board definition): what the default layout uses
    }
    *cols = *cols < 1 ? 1 : *cols > GRID_MAX ? GRID_MAX : *cols;
    *rows = *rows < 1 ? 1 : *rows > GRID_MAX ? GRID_MAX : *rows;
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
    // Keys, the status bar and the detail panel pass swipes up to the screen (LVGL's default).
    lv_obj_add_event_cb(screen, gesture_event, LV_EVENT_GESTURE, NULL);

    grid_w = board->width;
    grid_h = board->height - STATUS_HEIGHT;

    // Every key the largest grid needs; draw_keys_locked places and sizes them per layout.
    for (int i = 0; i < UI_KEYS; i++) {
        lv_obj_t *button = lv_btn_create(screen);
        lv_obj_set_style_bg_color(button, lv_color_hex(COLOR_PRESSED), LV_STATE_PRESSED);
        lv_obj_set_style_radius(button, 8, 0);
        lv_obj_add_event_cb(button, key_event, LV_EVENT_CLICKED, (void *)(intptr_t)i);

        lv_obj_t *label = lv_label_create(button);
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

    pager = lv_obj_create(status_bar);
    lv_obj_remove_style_all(pager);
    lv_obj_set_size(pager, PAGER_WIDTH, STATUS_HEIGHT);
    lv_obj_align(pager, LV_ALIGN_RIGHT_MID, 0, 0);
    lv_obj_set_style_bg_color(pager, lv_color_hex(COLOR_PRESSED), LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(pager, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_add_flag(pager, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(pager, pager_event, LV_EVENT_CLICKED, NULL);
    lv_obj_add_flag(pager, LV_OBJ_FLAG_HIDDEN);
    pager_label = lv_label_create(pager);
    lv_obj_set_style_text_font(pager_label, &lv_font_montserrat_14, 0);
    lv_obj_set_style_text_color(pager_label, lv_color_white(), 0);
    lv_obj_center(pager_label);

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

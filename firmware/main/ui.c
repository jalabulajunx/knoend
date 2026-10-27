#include "ui.h"
#include "icons.h"

#include <inttypes.h>
#include <stdio.h>
#include <string.h>

#include "ble_keyboard.h"
#include "board.h"
#include "display.h"
#include "fonts.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_lvgl_port.h"
#include "esp_timer.h"
#include "esp_random.h"
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

// The themes (spec/protocol.md 3.6d). Dark: light text on dark keys, colour for the states.
// White and Sepia: e-ink looks, dark ink on white or on warm paper, high contrast; states are
// shades of the ink and borders.
typedef struct {
    uint32_t bg, key, key_blank, key_border, pressed, confirm, confirm_text;
    uint32_t key_text, icon, bar_text, bar_text_highlighted, attention, alert, detail_text;
    int border; // key outline, px
} theme_t;

static const theme_t DARK = {
    .bg = 0x101418, .key = 0x2A3440, .key_blank = 0x181E25, .key_border = 0x2A3440,
    .pressed = 0x3D7EFF, .confirm = 0xC98A1B, .confirm_text = 0xFFFFFF,
    .key_text = 0xFFFFFF, .icon = 0xFFFFFF, .bar_text = 0xC0C8D0, .bar_text_highlighted = 0xFFFFFF,
    .attention = 0x5A4512, .alert = 0x6B1C1C, .detail_text = 0xFFFFFF, .border = 0,
};

static const theme_t WHITE = {
    .bg = 0xFFFFFF, .key = 0xFFFFFF, .key_blank = 0xF2F2F2, .key_border = 0x000000,
    .pressed = 0xBDBDBD, .confirm = 0x000000, .confirm_text = 0xFFFFFF,
    .key_text = 0x000000, .icon = 0x000000, .bar_text = 0x000000, .bar_text_highlighted = 0xFFFFFF,
    .attention = 0x555555, .alert = 0x000000, .detail_text = 0x000000, .border = 2,
};

// Strong enough to read as sepia on a 16-bit panel: the first try (a pale cream, 0xF4ECD8) looked
// like dimmed white on the ONX.
static const theme_t SEPIA = {
    .bg = 0xE3CC9E, .key = 0xEEDDB8, .key_blank = 0xD8BE8A, .key_border = 0x4A2F14,
    .pressed = 0xC7A66C, .confirm = 0x4A2F14, .confirm_text = 0xEEDDB8,
    .key_text = 0x3A2410, .icon = 0x3A2410, .bar_text = 0x3A2410, .bar_text_highlighted = 0xEEDDB8,
    .attention = 0x7A5530, .alert = 0x4A2F14, .detail_text = 0x3A2410, .border = 2,
};

static const theme_t *theme_for(display_theme_t which)
{
    return which == DISPLAY_THEME_WHITE ? &WHITE : which == DISPLAY_THEME_SEPIA ? &SEPIA : &DARK;
}

static const theme_t *theme = &DARK;
static int shift_step; // OLED screens: where the picture is in its slow shift (screen_guard)

static const char *TAG = "ui";

static ui_layout_t layout;
static lv_obj_t *key_buttons[UI_KEYS];
static lv_obj_t *key_labels[UI_KEYS];
static lv_obj_t *key_icons[UI_KEYS];
static lv_obj_t *status_bar;
static lv_obj_t *detail_box;   // over the middle rows when the layout has a detail
static lv_obj_t *detail_label;
static lv_obj_t *touchpad;
static lv_obj_t *touchpad_label;
static bool pointer_down;
static lv_point_t pointer_last;
static int pointer_dx, pointer_dy;
static int64_t pointer_sent_at;
static uint32_t pointer_seq;

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

// A key's colours: normal, or waiting for its confirming second tap.
static void paint_key(int index, bool confirming)
{
    const bool blank = layout.keys[index].send == UI_SEND_NONE;
    lv_obj_set_style_bg_color(key_buttons[index], lv_color_hex(confirming ? theme->confirm : blank ? theme->key_blank : theme->key), 0);
    lv_obj_set_style_border_width(key_buttons[index], blank ? 0 : theme->border, 0);
    const uint32_t text = confirming ? theme->confirm_text : theme->key_text;
    lv_obj_set_style_text_color(key_labels[index], lv_color_hex(text), 0);
    lv_obj_set_style_img_recolor(key_icons[index], lv_color_hex(confirming ? theme->confirm_text : theme->icon), 0);
}

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
        paint_key(confirm_slot, false);
        confirm_slot = -1;
    }

    uint32_t bar_color = theme->bg;
    if (confirm_slot >= 0) {
        lv_label_set_text_fmt(status_label, "Tap again to confirm: %s",
                              layout.keys[confirm_slot].label);
        bar_color = theme->attention;
    } else if (status_text[0] != '\0') {
        lv_label_set_text(status_label, status_text);
        bar_color = status_level == UI_STATUS_ALERT       ? theme->alert
                    : status_level == UI_STATUS_ATTENTION ? theme->attention
                                                          : theme->bg;
    } else {
        const char *link = ble_keyboard_connected() ? "Bluetooth connected"
                                                    : "Pair \"KnoEnd\" over Bluetooth";
        const char *unreviewed = layout.reviewed ? "" : "Unreviewed - ";
        if (last_key == 0) {
            lv_label_set_text_fmt(status_label, "%s%s  -  %s", unreviewed, layout.name, link);
        } else {
            lv_label_set_text_fmt(status_label, "%s%s  -  %s  -  key %d, %" PRIu32 " taps",
                                  unreviewed, layout.name, link, last_key, tap_count);
        }
    }
    lv_obj_set_style_bg_color(status_bar, lv_color_hex(bar_color), 0);
    const bool highlighted = bar_color != theme->bg;
    const uint32_t bar_text = highlighted ? theme->bar_text_highlighted : theme->bar_text;
    lv_obj_set_style_text_color(status_label, lv_color_hex(bar_text), 0);
    lv_obj_set_style_text_color(pager_label, lv_color_hex(highlighted ? theme->bar_text_highlighted : theme->key_text), 0);
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
        paint_key(confirm_slot, false);
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

// A finger drag is relative movement. Lifting and touching again only anchors
// a new stroke; it never jumps the PC pointer or clicks/advances a slide.
static void touchpad_event(lv_event_t *event)
{
    if (!layout.touchpad) return;
    lv_indev_t *indev = lv_indev_get_act();
    if (!indev) return;
    const lv_event_code_t code = lv_event_get_code(event);
    lv_point_t point;
    lv_indev_get_point(indev, &point);
    if (code == LV_EVENT_PRESSED) {
        pointer_down = true;
        pointer_last = point;
        pointer_dx = pointer_dy = 0;
        pointer_sent_at = esp_timer_get_time();
    } else if ((code == LV_EVENT_PRESSING || code == LV_EVENT_RELEASED || code == LV_EVENT_PRESS_LOST) && pointer_down) {
        pointer_dx += point.x - pointer_last.x;
        pointer_dy += point.y - pointer_last.y;
        pointer_last = point;
        const int64_t now = esp_timer_get_time();
        const bool released = code == LV_EVENT_RELEASED || code == LV_EVENT_PRESS_LOST;
        if (released || now - pointer_sent_at >= 33000) {
            // Bound each report; retain the remainder so fast strokes are not lost.
            int dx = pointer_dx < -127 ? -127 : pointer_dx > 127 ? 127 : pointer_dx;
            int dy = pointer_dy < -127 ? -127 : pointer_dy > 127 ? 127 : pointer_dy;
            if (dx || dy) protocol_send_pointer(layout.id, dx, dy, ++pointer_seq);
            pointer_dx -= dx; pointer_dy -= dy;
            pointer_sent_at = now;
        }
        if (released) pointer_down = false;
    }
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
            paint_key(confirm_slot, false);
        }
        confirm_slot = index;
        confirm_expires_us = esp_timer_get_time() + CONFIRM_MS * 1000LL;
        paint_key(index, true);
        printf("KEY %d seq=%" PRIu32 " t=%" PRIu64 " confirm-pending\n", index + 1, tap_count,
               (uint64_t)(esp_timer_get_time() / 1000));
        fflush(stdout);
    } else {
        if (confirm_slot == index) {
            paint_key(index, false);
            confirm_slot = -1;
        }
        act(index);
    }
    update_status_locked();
}

// The widest piece of `text` (its first `len` bytes) LVGL keeps on one line: a word, or a word up
// to a break character such as '-' (CONFIG_LV_TXT_BREAK_CHARS), in pixels at `font`.
static lv_coord_t widest_piece(const char *text, uint32_t len, const lv_font_t *font)
{
    lv_coord_t widest = 0;
    const char *start = text;
    for (const char *p = text;; p++) {
        const bool end = p == text + len;
        const bool space = end || *p == ' ' || *p == '\n';
        const bool breaker = !end && strchr(",.;:-_", *p) != NULL;
        if (!space && !breaker) {
            continue;
        }
        const uint32_t piece = (uint32_t)(p - start) + (breaker ? 1 : 0);
        if (piece > 0) {
            const lv_coord_t w = lv_txt_get_width(start, piece, font, 0, LV_TEXT_FLAG_NONE);
            widest = w > widest ? w : widest;
        }
        if (end) {
            return widest;
        }
        start = p + 1;
    }
}

// The font for a key label: `preferred`, or one size smaller when a word would not fit `width`,
// never below 12 px (smaller cannot be read on these screens). Words still too wide are cut.
// Key label fonts, largest first: 28, 20, 14 and 12 px. The built-in Montserrat, or the font the
// person chose for keys (fonts.c, drawn by tiny_ttf), each falling back to Montserrat of the same
// size for a character the chosen font lacks.
enum { FONT_28, FONT_20, FONT_14, FONT_12, LABEL_FONTS };
static const lv_font_t *const BUILT_IN[LABEL_FONTS] = {&lv_font_montserrat_28, &lv_font_montserrat_20,
                                                       &lv_font_montserrat_14, &lv_font_montserrat_12};
static const int LABEL_PX[LABEL_FONTS] = {28, 20, 14, 12};
static const lv_font_t *label_fonts[LABEL_FONTS] = {&lv_font_montserrat_28, &lv_font_montserrat_20,
                                                    &lv_font_montserrat_14, &lv_font_montserrat_12};
static lv_font_t *chosen[LABEL_FONTS]; // tiny_ttf fonts, when the person chose one

static const lv_font_t *fitting_font(const char *text, const lv_font_t *preferred, lv_coord_t width)
{
    for (size_t i = 0; i < LABEL_FONTS; i++) {
        if (label_fonts[i] == preferred) {
            const bool fits = widest_piece(text, strlen(text), preferred) <= width;
            return fits || i + 1 == LABEL_FONTS ? preferred : label_fonts[i + 1];
        }
    }
    return preferred;
}

// `text` with every word too wide for `width` at `font` cut to fit and ended with "...", so a word
// is never split over two lines ("Sourcecontrol" on a narrow key: "Sourceco..."). Words that fit
// are kept and wrap between words as usual.
static void cut_long_words(const char *text, const lv_font_t *font, lv_coord_t width, char *out, size_t size)
{
    size_t n = 0;
    const char *p = text;
    while (*p != '\0' && n + 1 < size) {
        if (*p == ' ' || *p == '\n') {
            out[n++] = *p++;
            continue;
        }
        const char *end = p;
        while (*end != '\0' && *end != ' ' && *end != '\n') {
            end++;
        }
        uint32_t keep = (uint32_t)(end - p);
        const bool cut = widest_piece(p, keep, font) > width;
        if (cut) {
            // Measured with the dots attached: the last letter's spacing depends on what follows.
            char candidate[UI_LABEL_BYTES + 4];
            while (keep > 1) {
                const uint32_t len = keep < UI_LABEL_BYTES ? keep : UI_LABEL_BYTES - 1;
                memcpy(candidate, p, len);
                memcpy(candidate + len, "...", 4);
                if (lv_txt_get_width(candidate, len + 3, font, 0, LV_TEXT_FLAG_NONE) <= width) {
                    break;
                }
                keep--;
                while (keep > 1 && ((unsigned char)p[keep] & 0xC0) == 0x80) {
                    keep--; // not inside a UTF-8 character
                }
            }
        }
        for (uint32_t k = 0; k < keep && n + 1 < size; k++) {
            out[n++] = p[k];
        }
        for (int k = 0; cut && k < 3 && n + 1 < size; k++) {
            out[n++] = '.';
        }
        p = end;
    }
    out[n] = '\0';
}

static void draw_keys_locked(void)
{
    const int cols = layout.cols, rows = layout.rows;
    const bool detail = !layout.touchpad && layout.detail[0] != '\0' && rows >= 3;
    const int key_w = (grid_w - GRID_GAP * (cols + 1)) / cols;
    const int key_h = (grid_h - GRID_GAP * (rows + 1)) / rows;
    pointer_down = false; pointer_dx = pointer_dy = 0;
    if (layout.touchpad) {
        lv_obj_set_pos(touchpad, GRID_GAP, GRID_GAP);
        lv_obj_set_size(touchpad, grid_w - 2 * GRID_GAP, grid_h - key_h - 3 * GRID_GAP);
        lv_obj_set_style_bg_color(touchpad, lv_color_hex(theme->key_blank), 0);
        lv_obj_set_style_text_color(touchpad_label, lv_color_hex(theme->key_text), 0);
        lv_obj_clear_flag(touchpad, LV_OBJ_FLAG_HIDDEN);
    } else { lv_obj_add_flag(touchpad, LV_OBJ_FLAG_HIDDEN); }

    // With a detail, the top and bottom rows get shorter (label only, two thirds of a 3x3 row) and
    // the text takes the rows between them, scrollable by swiping.
    const int compact_h = (grid_h - GRID_GAP * 4) / 3 * 2 / 3;
    const int detail_h = grid_h - 2 * compact_h - GRID_GAP * 4;
    // A top row with no keys is left out: the text takes its place too (one panel, keys below).
    bool top_empty = detail;
    for (int i = 0; detail && i < cols; i++) {
        top_empty = top_empty && layout.keys[i].send == UI_SEND_NONE;
    }
    if (detail) {
        lv_label_set_text(detail_label, layout.detail);
        lv_obj_set_pos(detail_box, GRID_GAP, top_empty ? GRID_GAP : GRID_GAP + compact_h + GRID_GAP);
        lv_obj_set_size(detail_box, grid_w - 2 * GRID_GAP, top_empty ? detail_h + compact_h + GRID_GAP : detail_h);
        lv_obj_scroll_to_y(detail_box, 0, LV_ANIM_OFF);
        lv_obj_clear_flag(detail_box, LV_OBJ_FLAG_HIDDEN);
    } else {
        lv_obj_add_flag(detail_box, LV_OBJ_FLAG_HIDDEN);
    }
    for (int i = 0; i < UI_KEYS; i++) {
        const int row = i / cols;
        // Keys past the grid, the middle rows under a detail, and empty slots beside a detail are
        // hidden (they read as blank tiles around the text).
        if (i >= cols * rows || (layout.touchpad && row < rows - 1) || (detail && row > 0 && row < rows - 1)
            || (detail && layout.keys[i].send == UI_SEND_NONE)) {
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
        paint_key(i, confirm_slot == i);
        lv_label_set_text(key_labels[i], blank ? "" : key->label);
        // A sent picture ("app:...", as sent) or a Material icon (in the theme's colour).
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
            const lv_font_t *font = fitting_font(key->label, big ? label_fonts[FONT_20] : label_fonts[FONT_14], key_w - 8);
            const int line_px = lv_font_get_line_height(font);
            char shown[UI_LABEL_BYTES * 2];
            cut_long_words(key->label, font, key_w - 8, shown, sizeof(shown));
            lv_label_set_text(key_labels[i], shown);
            // A label too long for one line gets two where they fit: lines a little closer, the
            // text up into the icon's empty margin (3x3: 59 of 61 px). Otherwise one line and "...".
            lv_point_t text;
            lv_txt_get_size(&text, shown, font, 0, 0, key_w - 8, LV_TEXT_FLAG_NONE);
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
            const lv_font_t *font = detail || small                   ? label_fonts[FONT_14]
                                    : big || strlen(key->label) <= 2 ? label_fonts[FONT_28]
                                                                     : label_fonts[FONT_20];
            const lv_font_t *fitted = fitting_font(key->label, font, key_w - 8);
            char shown[UI_LABEL_BYTES * 2];
            cut_long_words(key->label, fitted, key_w - 8, shown, sizeof(shown));
            lv_label_set_text(key_labels[i], blank ? "" : shown);
            lv_obj_set_style_text_font(key_labels[i], fitted, 0);
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
    *cols = (board->width - 2 * board->inset) / MIN_KEY_W;
    *rows = (board->height - STATUS_HEIGHT - 2 * board->inset) / MIN_KEY_H;
    if (board->width == 0) {
        *cols = *rows = 3; // no display (no board definition): what the default layout uses
    }
    *cols = *cols < 1 ? 1 : *cols > GRID_MAX ? GRID_MAX : *cols;
    *rows = *rows < 1 ? 1 : *rows > GRID_MAX ? GRID_MAX : *rows;
}

// Everything the theme colours that is not redrawn with the keys and the status bar.
static void apply_theme_locked(void)
{
    lv_obj_set_style_bg_color(lv_scr_act(), lv_color_hex(theme->bg), 0);
    for (int i = 0; i < UI_KEYS; i++) {
        lv_obj_set_style_bg_color(key_buttons[i], lv_color_hex(theme->pressed), LV_STATE_PRESSED);
        lv_obj_set_style_border_color(key_buttons[i], lv_color_hex(theme->key_border), 0);
    }
    lv_obj_set_style_bg_color(detail_box, lv_color_hex(theme->key_blank), 0);
    lv_obj_set_style_border_width(detail_box, theme->border, 0);
    lv_obj_set_style_border_color(detail_box, lv_color_hex(theme->key_border), 0);
    lv_obj_set_style_text_color(detail_label, lv_color_hex(theme->detail_text), 0);
    lv_obj_set_style_bg_color(pager, lv_color_hex(theme->pressed), LV_STATE_PRESSED);
}

// The key area and the status bar for the screen's size as the board sits now.
static void size_screen_locked(void)
{
    const board_info_t *board = board_info();
    // On a screen with rounded corners everything sits inside the inset (children are placed in
    // the screen's content area, inside its padding).
    const int inset = board->inset;
    lv_obj_set_style_pad_all(lv_scr_act(), inset, 0);
    shift_step = 0; // a turn starts the picture shift again from the middle
    grid_w = board->width - 2 * inset;
    grid_h = board->height - STATUS_HEIGHT - 2 * inset;
    lv_obj_set_size(status_bar, grid_w, STATUS_HEIGHT);
    lv_obj_align(status_bar, LV_ALIGN_BOTTOM_LEFT, 0, 0);
    lv_obj_set_width(status_label, grid_w - 2 * GRID_GAP);
}

// Back to the built-in key font; the chosen one is freed. Called with the LVGL lock held.
static void release_key_font_locked(void)
{
    for (int i = 0; i < LABEL_FONTS; i++) {
        label_fonts[i] = BUILT_IN[i];
    }
    draw_keys_locked(); // nothing may still point at the fonts freed below
    for (int i = 0; i < LABEL_FONTS; i++) {
        if (chosen[i] != NULL) {
            lv_tiny_ttf_destroy(chosen[i]);
            chosen[i] = NULL;
        }
    }
}

// The person's key font, if one is stored. Called with the LVGL lock held.
static void load_key_font_locked(void)
{
    size_t len;
    const uint8_t *data = fonts_data(FONT_SLOT_KEYS, &len, NULL, NULL);
    if (data == NULL) {
        return;
    }
    for (int i = 0; i < LABEL_FONTS; i++) {
        chosen[i] = lv_tiny_ttf_create_data(data, len, LABEL_PX[i]);
        if (chosen[i] == NULL) {
            ESP_LOGW(TAG, "could not use the chosen key font at %d px", LABEL_PX[i]);
            release_key_font_locked();
            return;
        }
        chosen[i]->fallback = BUILT_IN[i];
        label_fonts[i] = chosen[i];
    }
}

void ui_fonts_release(void)
{
    if (!lvgl_port_lock(1000)) {
        return;
    }
    release_key_font_locked();
    lvgl_port_unlock();
}

static void settled(lv_timer_t *timer)
{
    fonts_settled();
}

void ui_fonts_reload(void)
{
    if (!lvgl_port_lock(1000)) {
        return;
    }
    release_key_font_locked();
    load_key_font_locked();
    draw_keys_locked();
    lvgl_port_unlock();
    // Drawn once (LVGL draws in its own task, a moment later): then it is kept.
    lv_timer_t *settle = lv_timer_create(settled, 2000, NULL);
    lv_timer_set_repeat_count(settle, 1);
}

// The name shown for a moment at start-up, in the start-up font the person chose (else Montserrat).
#define SPLASH_MS 1800
static lv_obj_t *splash;
static lv_font_t *splash_font;

static void splash_done(lv_timer_t *timer)
{
    fonts_settled(); // the chosen fonts have drawn: keep them
    lv_obj_del(splash);
    splash = NULL;
    if (splash_font != NULL) {
        lv_tiny_ttf_destroy(splash_font);
        splash_font = NULL;
    }
    lv_timer_del(timer);
}

static void show_splash_locked(void)
{
    splash = lv_obj_create(lv_layer_top());
    lv_obj_remove_style_all(splash);
    lv_obj_set_size(splash, LV_PCT(100), LV_PCT(100));
    lv_obj_set_style_bg_opa(splash, LV_OPA_COVER, 0);
    lv_obj_set_style_bg_color(splash, lv_color_hex(theme->bg), 0);
    lv_obj_t *name = lv_label_create(splash);
    lv_label_set_text(name, "Kn\xC2\xB0" "End");
    lv_obj_set_style_text_color(name, lv_color_hex(theme->key_text), 0);
    size_t len;
    const uint8_t *data = fonts_data(FONT_SLOT_START, &len, NULL, NULL);
    // tiny_ttf's default 4 KB glyph cache divides by zero above 64 px (4096 / px^2 hash buckets):
    // room for the name's 6 glyphs at any size.
    const int px = display_settings()->start_size;
    splash_font = data != NULL ? lv_tiny_ttf_create_data_ex(data, len, px, (size_t)px * px * 8) : NULL;
    if (splash_font != NULL) {
        splash_font->fallback = &lv_font_montserrat_28; // the degree sign, if the font has none
    }
    lv_obj_set_style_text_font(name, splash_font != NULL ? splash_font : &lv_font_montserrat_28, 0);
    lv_obj_center(name);
    lv_timer_create(splash_done, SPLASH_MS, NULL);
}

void ui_show_start(void)
{
    if (!lvgl_port_lock(1000)) {
        return;
    }
    if (splash == NULL) {
        show_splash_locked();
    }
    lvgl_port_unlock();
}

void ui_set_turn(int turn)
{
    if (!lvgl_port_lock(1000)) {
        ESP_LOGE(TAG, "could not take the LVGL lock to turn the screen");
        return;
    }
    board_set_turn(turn);
    size_screen_locked();
    // The layout on the board may not fit the new shape: until the companion sends one that does,
    // its keys are drawn within the largest grid that fits.
    int cols, rows;
    ui_max_grid(&cols, &rows);
    if (layout.cols > cols || layout.rows > rows) {
        ui_layout_t fitted = layout;
        fitted.cols = layout.cols > cols ? cols : layout.cols;
        fitted.rows = layout.rows > rows ? rows : layout.rows;
        layout = fitted;
    }
    draw_keys_locked();
    update_status_locked();
    lvgl_port_unlock();
}

void ui_set_theme(display_theme_t which)
{
    if (!lvgl_port_lock(1000)) {
        ESP_LOGE(TAG, "could not take the LVGL lock to change the theme");
        return;
    }
    theme = theme_for(which);
    apply_theme_locked();
    draw_keys_locked();
    update_status_locked();
    lvgl_port_unlock();
}

// ---- OLED screens: still pictures burn in ----
// A keypad shows the same keys for hours. On an OLED screen (board_info()->oled) the screen dims
// after a minute without a touch and goes dark after ten; the first tap on a dark screen only
// wakes it (a key nobody can see is never pressed). New keys or an alert from the computer wake
// it too. Every few minutes the whole picture moves a pixel or two, inside the corner margin.
#define DIM_AFTER_MS   (60 * 1000)
#define DARK_AFTER_MS  (10 * 60 * 1000)
#define DIM_PERCENT    30
#define SHIFT_EVERY_MS (3 * 60 * 1000)

static lv_obj_t *wake_cover;
static int brightness_now = 100;
static bool dark_by_button; // the screen button turned it off: stays off until a tap, the button or news
static uint32_t shifted_at;

static void screen_on_locked(void)
{
    dark_by_button = false;
    lv_obj_add_flag(wake_cover, LV_OBJ_FLAG_HIDDEN);
    if (brightness_now != 100) {
        board_set_backlight(100);
        brightness_now = 100;
    }
}

static void wake_cover_event(lv_event_t *e)
{
    screen_on_locked(); // the tap itself counts as activity, so the screen does not dim again at once
}

static void screen_off_locked(void)
{
    dark_by_button = true;
    board_set_backlight(0);
    brightness_now = 0;
    lv_obj_clear_flag(wake_cover, LV_OBJ_FLAG_HIDDEN);
}

// The screen's padding: the inset, moved by (dx, dy).
static void pad_screen_locked(int dx, int dy)
{
    const int inset = board_info()->inset;
    lv_obj_t *screen = lv_scr_act();
    lv_obj_set_style_pad_left(screen, inset + dx, 0);
    lv_obj_set_style_pad_right(screen, inset - dx, 0);
    lv_obj_set_style_pad_top(screen, inset + dy, 0);
    lv_obj_set_style_pad_bottom(screen, inset - dy, 0);
}

static void screen_guard(lv_timer_t *timer)
{
    if (dark_by_button || !board_info()->oled) {
        return;
    }
    const uint32_t idle = lv_disp_get_inactive_time(NULL);
    const int wanted = idle >= DARK_AFTER_MS ? 0 : idle >= DIM_AFTER_MS ? DIM_PERCENT : 100;
    if (wanted != brightness_now) {
        board_set_backlight((uint8_t)wanted);
        brightness_now = wanted;
        if (wanted == 0) {
            lv_obj_clear_flag(wake_cover, LV_OBJ_FLAG_HIDDEN);
        } else {
            lv_obj_add_flag(wake_cover, LV_OBJ_FLAG_HIDDEN);
        }
    }
    static const int8_t steps[][2] = { { 0, 0 }, { 2, 0 }, { 2, 2 }, { 0, 2 }, { -2, 2 }, { -2, 0 }, { -2, -2 }, { 0, -2 }, { 2, -2 } };
    if (board_info()->inset >= 2 && lv_tick_elaps(shifted_at) >= SHIFT_EVERY_MS) {
        shifted_at = lv_tick_get();
        shift_step = (shift_step + 1) % (int)(sizeof(steps) / sizeof(steps[0]));
        pad_screen_locked(steps[shift_step][0], steps[shift_step][1]);
    }
}

// Something new from the computer: an OLED screen wakes for it (called with the LVGL lock held).
static void wake_locked(void)
{
    if (wake_cover != NULL) {
        lv_disp_trig_activity(NULL);
        screen_on_locked();
    }
}

// The board's buttons (board_button_presses): next page, and the screen off or back on.
static void button_timer(lv_timer_t *timer)
{
    const uint32_t presses = board_button_presses();
    if (presses & BOARD_BUTTON_SCREEN) {
        if (brightness_now == 0) {
            lv_disp_trig_activity(NULL);
            screen_on_locked();
        } else {
            screen_off_locked();
        }
    }
    if ((presses & BOARD_BUTTON_PAGE) && brightness_now > 0 && layout.pages > 1) {
        lv_disp_trig_activity(NULL);
        turn_page(true);
    }
}

static void start_screen_guard_locked(void)
{
    wake_cover = lv_obj_create(lv_layer_top());
    lv_obj_remove_style_all(wake_cover);
    lv_obj_set_size(wake_cover, LV_PCT(100), LV_PCT(100));
    lv_obj_add_flag(wake_cover, LV_OBJ_FLAG_CLICKABLE | LV_OBJ_FLAG_HIDDEN);
    lv_obj_add_event_cb(wake_cover, wake_cover_event, LV_EVENT_PRESSED, NULL);
    shifted_at = lv_tick_get();
    lv_timer_create(screen_guard, 1000, NULL);
    lv_timer_create(button_timer, 50, NULL);
}

void ui_start(void)
{
    default_layout(&layout);
    if (!lvgl_port_lock(1000)) {
        ESP_LOGE(TAG, "could not take the LVGL lock to draw the first screen");
        return;
    }

    lv_obj_t *screen = lv_scr_act();
    lv_obj_clear_flag(screen, LV_OBJ_FLAG_SCROLLABLE);
    // Keys, the status bar and the detail panel pass swipes up to the screen (LVGL's default).
    lv_obj_add_event_cb(screen, gesture_event, LV_EVENT_GESTURE, NULL);


    // Every key the largest grid needs; draw_keys_locked places and sizes them per layout.
    for (int i = 0; i < UI_KEYS; i++) {
        lv_obj_t *button = lv_btn_create(screen);
        lv_obj_set_style_radius(button, 8, 0);
        lv_obj_set_style_shadow_width(button, 0, 0);
        lv_obj_add_event_cb(button, key_event, LV_EVENT_CLICKED, (void *)(intptr_t)i);

        lv_obj_t *label = lv_label_create(button);
        lv_label_set_long_mode(label, LV_LABEL_LONG_WRAP);
        lv_obj_set_style_text_align(label, LV_TEXT_ALIGN_CENTER, 0);
        lv_obj_center(label);
        lv_obj_t *icon = lv_img_create(button);
        lv_obj_set_style_img_recolor(icon, lv_color_hex(theme->icon), 0);
        lv_obj_set_style_img_recolor_opa(icon, LV_OPA_COVER, 0);
        lv_obj_add_flag(icon, LV_OBJ_FLAG_HIDDEN);
        lv_obj_clear_flag(icon, LV_OBJ_FLAG_CLICKABLE);
        key_buttons[i] = button;
        key_labels[i] = label;
        key_icons[i] = icon;
    }

    pointer_seq = esp_random();
    touchpad = lv_obj_create(screen);
    lv_obj_clear_flag(touchpad, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_GESTURE_BUBBLE);
    lv_obj_set_style_border_width(touchpad, 0, 0);
    lv_obj_set_style_radius(touchpad, 8, 0);
    lv_obj_add_event_cb(touchpad, touchpad_event, LV_EVENT_PRESSED, NULL);
    lv_obj_add_event_cb(touchpad, touchpad_event, LV_EVENT_PRESSING, NULL);
    lv_obj_add_event_cb(touchpad, touchpad_event, LV_EVENT_RELEASED, NULL);
    lv_obj_add_event_cb(touchpad, touchpad_event, LV_EVENT_PRESS_LOST, NULL);
    touchpad_label = lv_label_create(touchpad);
    lv_label_set_text(touchpad_label, "Drag to move laser");
    lv_obj_set_style_text_font(touchpad_label, &lv_font_montserrat_14, 0);
    lv_obj_center(touchpad_label);
    lv_obj_add_flag(touchpad, LV_OBJ_FLAG_HIDDEN);

    detail_box = lv_obj_create(screen);
    lv_obj_remove_style_all(detail_box);
    lv_obj_set_style_bg_opa(detail_box, LV_OPA_COVER, 0);
    lv_obj_set_style_radius(detail_box, 8, 0);
    lv_obj_set_style_pad_all(detail_box, 6, 0);
    lv_obj_set_scroll_dir(detail_box, LV_DIR_VER);
    lv_obj_set_scrollbar_mode(detail_box, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_add_flag(detail_box, LV_OBJ_FLAG_HIDDEN);
    detail_label = lv_label_create(detail_box);
    lv_obj_set_width(detail_label, lv_pct(100));
    lv_label_set_long_mode(detail_label, LV_LABEL_LONG_WRAP);
    lv_obj_set_style_text_font(detail_label, &lv_font_montserrat_14, 0);
    lv_obj_align(detail_label, LV_ALIGN_TOP_LEFT, 0, 0);

    status_bar = lv_obj_create(screen);
    lv_obj_remove_style_all(status_bar);

    lv_obj_set_style_bg_opa(status_bar, LV_OPA_COVER, 0);

    status_label = lv_label_create(status_bar);
    lv_obj_set_style_text_font(status_label, &lv_font_montserrat_14, 0);
    lv_label_set_long_mode(status_label, LV_LABEL_LONG_DOT);
    lv_obj_align(status_label, LV_ALIGN_LEFT_MID, GRID_GAP, 0);

    pager = lv_obj_create(status_bar);
    lv_obj_remove_style_all(pager);
    lv_obj_set_size(pager, PAGER_WIDTH, STATUS_HEIGHT);
    lv_obj_align(pager, LV_ALIGN_RIGHT_MID, 0, 0);
    lv_obj_set_style_bg_opa(pager, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_add_flag(pager, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(pager, pager_event, LV_EVENT_CLICKED, NULL);
    lv_obj_add_flag(pager, LV_OBJ_FLAG_HIDDEN);
    pager_label = lv_label_create(pager);
    lv_obj_set_style_text_font(pager_label, &lv_font_montserrat_14, 0);
    lv_obj_center(pager_label);

    theme = theme_for(display_settings()->theme);
    apply_theme_locked();
    board_set_turn(display_settings()->turn);
    size_screen_locked();
    load_key_font_locked();
    draw_keys_locked();
    show_splash_locked();
    update_status_locked();
    lv_timer_create(status_timer, 250, NULL);
    start_screen_guard_locked();
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
    wake_locked();
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
    if (level != UI_STATUS_INFO) {
        wake_locked(); // an agent waiting, a failure: worth lighting the screen for
    }
    lvgl_port_unlock();
}

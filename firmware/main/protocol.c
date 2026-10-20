#include "protocol.h"

#include <stdbool.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>

#include "board.h"
#include "cJSON.h"
#include "display.h"
#include "fonts.h"
#include "esp_system.h"
#include "esp_app_desc.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "mbedtls/base64.h"
#include "esp_mac.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "link_serial.h"
#include "link_wifi.h"
#include "nvs.h"
#include "ui.h"

#define PROTOCOL_VERSION 0
#define MAX_COMPANIONS   4
#define KEY_BYTES        32

static const char *TAG = "protocol";

// Only what this firmware actually handles is advertised.
static const char *const incoming_types[] = {
    "identity", "layout", "status", "pair", "wifi",
    "wifi.scan", "ping", "image", "display", "font",
};
static const char *const outgoing_types[] = {
    "identity", "key", "page", "layout.ack", "pair.ack",
    "wifi.ack", "wifi.networks", "ping", "error", "image.ack", "display.ack", "font.ack",
};
static const char *const transports[] = { "ble-hid", "serial", "wifi" };

typedef enum {
    TRANSPORT_SERIAL = 1,
    TRANSPORT_WIFI = 2,
    TRANSPORT_ALL = TRANSPORT_SERIAL | TRANSPORT_WIFI,
} transport_t;

static SemaphoreHandle_t send_lock;
static uint32_t next_id_serial;
static uint32_t next_id_wifi; // restarts at 0 for each Wi-Fi session
static char device_id[13];

// ---- sending ----

// Takes ownership of `body`. `re` < 0 means "not a reply". Replies go back over the transport
// the request came on; unsolicited packets (taps, the boot identity) go to every open link.
// Message names travel as "<prefix>.<name>". The prefix is "knoend"; companions from before the
// rename (v0.1.x, "OpenPager") use "openpager". Each link answers in the prefix its companion last
// used, so old and new companions both work with this board (spec/protocol.md section 2).
static const char *prefix_serial = "knoend";
static const char *prefix_wifi = "knoend";

void protocol_wifi_session_started(void)
{
    prefix_wifi = "knoend";
}

static void send_packet(const char *name, cJSON *body, int64_t re, transport_t to)
{
    xSemaphoreTake(send_lock, portMAX_DELAY);
    cJSON *packet = cJSON_CreateObject();
    cJSON *id = cJSON_AddNumberToObject(packet, "id", 0);
    if (re >= 0) {
        cJSON_AddNumberToObject(packet, "re", (double)re);
    }
    cJSON *type = cJSON_AddStringToObject(packet, "type", "");
    cJSON_AddItemToObject(packet, "body", body);
    char full[48];

    if (to & TRANSPORT_SERIAL) {
        cJSON_SetNumberValue(id, next_id_serial++);
        snprintf(full, sizeof(full), "%s.%s", prefix_serial, name);
        cJSON_SetValuestring(type, full);
        char *text = cJSON_PrintUnformatted(packet);
        if (text != NULL) {
            link_serial_send(text);
            cJSON_free(text);
        }
    }
    if ((to & TRANSPORT_WIFI) && link_wifi_session_active()) {
        cJSON_SetNumberValue(id, next_id_wifi++);
        snprintf(full, sizeof(full), "%s.%s", prefix_wifi, name);
        cJSON_SetValuestring(type, full);
        char *text = cJSON_PrintUnformatted(packet);
        if (text != NULL) {
            link_wifi_send(text);
            cJSON_free(text);
        }
    }
    cJSON_Delete(packet);
    xSemaphoreGive(send_lock);
}

static void send_error(int64_t re, const char *code, const char *message, transport_t to)
{
    cJSON *body = cJSON_CreateObject();
    cJSON_AddStringToObject(body, "code", code);
    cJSON_AddStringToObject(body, "message", message);
    send_packet("error", body, re, to);
}

static cJSON *string_array(const char *const *items, size_t count)
{
    cJSON *array = cJSON_CreateArray();
    for (size_t i = 0; i < count; i++) {
        cJSON_AddItemToArray(array, cJSON_CreateString(items[i]));
    }
    return array;
}

// Message names with the prefix of the link they are sent on.
static cJSON *type_array(const char *const *names, size_t count, transport_t to)
{
    const char *prefix = (to & TRANSPORT_WIFI) && !(to & TRANSPORT_SERIAL) ? prefix_wifi : prefix_serial;
    cJSON *array = cJSON_CreateArray();
    char full[48];
    for (size_t i = 0; i < count; i++) {
        snprintf(full, sizeof(full), "%s.%s", prefix, names[i]);
        cJSON_AddItemToArray(array, cJSON_CreateString(full));
    }
    return array;
}

static void send_identity(int64_t re, transport_t to)
{
    const board_info_t *board = board_info();
    cJSON *body = cJSON_CreateObject();
    cJSON_AddStringToObject(body, "deviceId", device_id);
    cJSON_AddStringToObject(body, "deviceName", "KnoEnd");
    cJSON_AddStringToObject(body, "deviceType", "board");
    cJSON_AddNumberToObject(body, "protocolVersion", PROTOCOL_VERSION);
    cJSON_AddStringToObject(body, "firmware", esp_app_get_description()->version);
    // Why the board last started: a crash is reported, so the companion (and its assistant) can see
    // a board that keeps restarting without reading the serial line.
    const esp_reset_reason_t reason = esp_reset_reason();
    cJSON_AddStringToObject(body, "lastStart",
                            reason == ESP_RST_POWERON ? "power-on"
                            : reason == ESP_RST_SW ? "restart"
                            : reason == ESP_RST_PANIC ? "crash"
                            : reason == ESP_RST_INT_WDT || reason == ESP_RST_TASK_WDT || reason == ESP_RST_WDT ? "watchdog"
                            : reason == ESP_RST_BROWNOUT ? "brownout"
                            : reason == ESP_RST_DEEPSLEEP ? "wake"
                            : "other");

    cJSON *b = cJSON_AddObjectToObject(body, "board");
    cJSON_AddStringToObject(b, "profile", board->id);
    cJSON_AddNumberToObject(b, "width", board->width);
    cJSON_AddNumberToObject(b, "height", board->height);
    // The largest grid this screen draws legibly; layouts choose their own up to this.
    int max_cols, max_rows;
    ui_max_grid(&max_cols, &max_rows);
    cJSON *grid = cJSON_AddObjectToObject(b, "grid");
    cJSON_AddNumberToObject(grid, "cols", max_cols);
    cJSON_AddNumberToObject(grid, "rows", max_rows);
    cJSON_AddBoolToObject(b, "statusBar", true);
    const display_settings_t *look = display_settings();
    cJSON *display = cJSON_AddObjectToObject(b, "display");
    cJSON_AddStringToObject(display, "theme", display_theme_name(look->theme));
    cJSON_AddNumberToObject(display, "turn", look->turn);
    cJSON_AddNumberToObject(display, "startSize", look->start_size);
    // The fonts the person chose that this board holds (spec/protocol.md 3.6e).
    cJSON *fonts = cJSON_AddObjectToObject(b, "fonts");
    cJSON_AddNumberToObject(fonts, "capacity", fonts_slot_capacity());
    if (fonts_refused()[0] != '\0') {
        cJSON_AddStringToObject(fonts, "refused", fonts_refused());
    }
    for (int slot = 0; slot < FONT_SLOTS; slot++) {
        const char *font_name;
        uint32_t crc;
        if (fonts_data(slot, NULL, &font_name, &crc) != NULL) {
            cJSON *held = cJSON_AddObjectToObject(fonts, fonts_slot_name(slot));
            cJSON_AddStringToObject(held, "name", font_name);
            cJSON_AddNumberToObject(held, "crc", crc);
        } else {
            cJSON_AddNullToObject(fonts, fonts_slot_name(slot));
        }
    }

    cJSON *features = cJSON_AddObjectToObject(body, "features");
    cJSON_AddBoolToObject(features, "speaker", board->has_speaker);
    cJSON_AddBoolToObject(features, "battery", false);
    cJSON_AddBoolToObject(features, "nativeUsb", board->has_native_usb);
    cJSON_AddItemToObject(features, "transports",
                          string_array(transports, sizeof(transports) / sizeof(transports[0])));

    cJSON_AddItemToObject(body, "incoming",
                          type_array(incoming_types, sizeof(incoming_types) / sizeof(incoming_types[0]), to));
    cJSON_AddItemToObject(body, "outgoing",
                          type_array(outgoing_types, sizeof(outgoing_types) / sizeof(outgoing_types[0]), to));
    send_packet("identity", body, re, to);
}

void protocol_send_key(int slot, const char *layout_id, int page, uint32_t seq)
{
    cJSON *body = cJSON_CreateObject();
    cJSON_AddNumberToObject(body, "key", slot);
    cJSON_AddStringToObject(body, "layout", layout_id);
    cJSON_AddNumberToObject(body, "page", page);
    cJSON_AddNumberToObject(body, "seq", seq);
    send_packet("key", body, -1, TRANSPORT_ALL);
}

void protocol_send_keystroke(int slot, const char *layout_id, int page, uint32_t seq, uint8_t modifiers, uint8_t usage)
{
    cJSON *body = cJSON_CreateObject();
    cJSON_AddNumberToObject(body, "key", slot);
    cJSON_AddStringToObject(body, "layout", layout_id);
    cJSON_AddNumberToObject(body, "page", page);
    cJSON_AddNumberToObject(body, "seq", seq);
    cJSON *hid = cJSON_AddObjectToObject(body, "hid");
    cJSON_AddNumberToObject(hid, "modifiers", modifiers);
    cJSON_AddNumberToObject(hid, "usage", usage);
    send_packet("key", body, -1, TRANSPORT_ALL);
}

void protocol_send_page(const char *layout_id, bool next)
{
    cJSON *body = cJSON_CreateObject();
    cJSON_AddStringToObject(body, "layout", layout_id);
    cJSON_AddStringToObject(body, "to", next ? "next" : "prev");
    send_packet("page", body, -1, TRANSPORT_ALL);
}

// ---- helpers ----

static bool get_int(const cJSON *object, const char *name, int min, int max, int *out)
{
    const cJSON *item = cJSON_GetObjectItemCaseSensitive(object, name);
    if (!cJSON_IsNumber(item) || item->valuedouble != (double)(int)item->valuedouble) {
        return false;
    }
    if (item->valueint < min || item->valueint > max) {
        return false;
    }
    *out = item->valueint;
    return true;
}

static const char *get_string(const cJSON *object, const char *name, size_t max_bytes)
{
    const cJSON *item = cJSON_GetObjectItemCaseSensitive(object, name);
    if (!cJSON_IsString(item) || item->valuestring[0] == '\0' || strlen(item->valuestring) > max_bytes) {
        return NULL;
    }
    return item->valuestring;
}

static size_t utf8_chars(const char *text)
{
    size_t count = 0;
    for (; *text; text++) {
        count += ((unsigned char)*text & 0xC0) != 0x80;
    }
    return count;
}

// ---- handlers ----

static void handle_layout(const cJSON *body, int64_t re, transport_t from)
{
    // On the heap: sixteen keys are about 3 KB, too much for the serial task's stack.
    ui_layout_t *layout = calloc(1, sizeof(*layout));
    if (layout == NULL) {
        send_error(re, "no-memory", "no memory for the layout", from);
        return;
    }
    char error[80] = "";

    const char *id = get_string(body, "layout", sizeof(layout->id) - 1);
    const char *name = get_string(body, "name", sizeof(layout->name) - 1);
    const cJSON *keys = cJSON_GetObjectItemCaseSensitive(body, "keys");
    const cJSON *reviewed = cJSON_GetObjectItemCaseSensitive(body, "reviewed");
    const cJSON *grid = cJSON_GetObjectItemCaseSensitive(body, "grid");
    int max_cols, max_rows, cols = 3, rows = 3, page = 1, pages = 1;
    ui_max_grid(&max_cols, &max_rows);

    if (id == NULL || name == NULL || !cJSON_IsArray(keys)) {
        snprintf(error, sizeof(error), "layout, name and keys are required");
    } else if (grid != NULL && !(get_int(grid, "cols", 1, max_cols, &cols) && get_int(grid, "rows", 1, max_rows, &rows))) {
        snprintf(error, sizeof(error), "grid must be within %dx%d", max_cols, max_rows);
    } else if ((cJSON_HasObjectItem(body, "pages") && !get_int(body, "pages", 1, 64, &pages)) ||
               (cJSON_HasObjectItem(body, "page") && !get_int(body, "page", 1, pages, &page))) {
        snprintf(error, sizeof(error), "page must be 1-pages, pages 1-64");
    } else if (cJSON_GetArraySize(keys) > cols * rows) {
        snprintf(error, sizeof(error), "too many keys");
    } else {
        strcpy(layout->id, id);
        strcpy(layout->name, name);
        layout->reviewed = !cJSON_IsFalse(reviewed);
        layout->cols = (uint8_t)cols;
        layout->rows = (uint8_t)rows;
        layout->page = (uint8_t)page;
        layout->pages = (uint8_t)pages;
        const char *detail = get_string(body, "detail", UI_DETAIL_BYTES - 1);
        snprintf(layout->detail, sizeof(layout->detail), "%s", detail != NULL ? detail : "");
    }

    const cJSON *key;
    cJSON_ArrayForEach(key, keys) {
        if (error[0] != '\0') {
            break;
        }
        int slot;
        const char *label = get_string(key, "label", UI_LABEL_BYTES - 1);
        const cJSON *send = cJSON_GetObjectItemCaseSensitive(key, "send");
        const cJSON *hid = cJSON_GetObjectItemCaseSensitive(send, "hid");
        const cJSON *event = cJSON_GetObjectItemCaseSensitive(send, "event");
        int modifiers, usage;

        if (!get_int(key, "key", 1, cols * rows, &slot)) {
            snprintf(error, sizeof(error), "key slot must be 1-%d", cols * rows);
            break;
        }
        ui_key_t *target = &layout->keys[slot - 1];
        if (target->send != UI_SEND_NONE) {
            snprintf(error, sizeof(error), "slot %d used twice", slot);
        } else if (label == NULL || utf8_chars(label) > 24) {
            snprintf(error, sizeof(error), "slot %d: label must be 1-24 characters", slot);
        } else if (hid != NULL && event == NULL && get_int(hid, "modifiers", 0, 15, &modifiers) &&
                   get_int(hid, "usage", 4, 0x73, &usage)) {
            target->send = UI_SEND_HID;
            target->modifiers = (uint8_t)modifiers;
            target->usage = (uint8_t)usage;
        } else if (event != NULL && hid == NULL && cJSON_IsTrue(event)) {
            target->send = UI_SEND_EVENT;
        } else {
            snprintf(error, sizeof(error), "slot %d: send must be hid or event", slot);
        }
        if (error[0] == '\0') {
            strcpy(target->label, label);
            // An icon name the board does not know is ignored (the label still shows).
            const char *icon = get_string(key, "icon", UI_ICON_BYTES - 1);
            target->icon[0] = '\0';
            if (icon != NULL && strspn(icon, "abcdefghijklmnopqrstuvwxyz0123456789_:.-") == strlen(icon)) {
                strcpy(target->icon, icon);
            }
            target->confirm = cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(key, "confirm"));
        }
    }

    cJSON *ack = cJSON_CreateObject();
    cJSON_AddStringToObject(ack, "layout", id != NULL ? id : "");
    cJSON_AddBoolToObject(ack, "ok", error[0] == '\0');
    if (error[0] != '\0') {
        cJSON_AddStringToObject(ack, "error", error);
    } else {
        ui_apply_layout(layout); // all-or-nothing: only a fully valid layout replaces the keys
    }
    free(layout);
    send_packet("layout.ack", ack, re, from);
}

// ---- pictures (knoend.image), sent in slices that fit a packet ----

static struct {
    char name[UI_ICON_BYTES];
    int w, h;
    size_t total, received;
    uint8_t *pixels;
} incoming_image;

static void image_ack(const char *name, const char *error, int re, transport_t transport)
{
    cJSON *ack = cJSON_CreateObject();
    cJSON_AddStringToObject(ack, "name", name != NULL ? name : "");
    cJSON_AddBoolToObject(ack, "ok", error == NULL);
    if (error != NULL) {
        cJSON_AddStringToObject(ack, "error", error);
    }
    send_packet("image.ack", ack, re, transport);
}

static void handle_image(const cJSON *body, int re, transport_t transport)
{
    const char *name = get_string(body, "name", UI_ICON_BYTES - 1);
    const char *data = get_string(body, "data", 4000);
    int w, h, offset, total;
    if (name == NULL || strspn(name, "abcdefghijklmnopqrstuvwxyz0123456789_:.-") != strlen(name) || strchr(name, ':') == NULL ||
        data == NULL || !get_int(body, "w", 1, 64, &w) || !get_int(body, "h", 1, 64, &h) ||
        !get_int(body, "offset", 0, 64 * 64 * 3, &offset) || !get_int(body, "total", 1, 64 * 64 * 3, &total) ||
        total != w * h * 3) {
        image_ack(name, "name, w, h, offset, total and data are required (total = w*h*3)", re, transport);
        return;
    }
    if (offset == 0) {
        heap_caps_free(incoming_image.pixels);
        incoming_image.pixels = heap_caps_malloc(total, MALLOC_CAP_SPIRAM);
        snprintf(incoming_image.name, sizeof(incoming_image.name), "%s", name);
        incoming_image.w = w;
        incoming_image.h = h;
        incoming_image.total = total;
        incoming_image.received = 0;
    }
    if (incoming_image.pixels == NULL || strcmp(incoming_image.name, name) != 0 ||
        (size_t)offset != incoming_image.received || (size_t)total != incoming_image.total) {
        image_ack(name, "slice out of order", re, transport);
        return;
    }
    size_t written = 0;
    if (mbedtls_base64_decode(incoming_image.pixels + offset, incoming_image.total - offset, &written,
                              (const unsigned char *)data, strlen(data)) != 0) {
        image_ack(name, "data is not base64 or too long", re, transport);
        return;
    }
    incoming_image.received += written;
    if (incoming_image.received < incoming_image.total) {
        return; // more slices to come
    }
    uint8_t *pixels = incoming_image.pixels;
    incoming_image.pixels = NULL;
    image_ack(name, ui_add_image(name, w, h, pixels, incoming_image.total) ? NULL : "could not keep it", re, transport);
}

static void handle_status(const cJSON *body)
{
    const cJSON *text = cJSON_GetObjectItemCaseSensitive(body, "text");
    if (!cJSON_IsString(text)) {
        return;
    }
    const char *level = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(body, "level"));
    int ttl = 0;
    get_int(body, "ttl", 1, 86400, &ttl);
    ui_status_level_t parsed = UI_STATUS_INFO;
    if (level != NULL && strcmp(level, "attention") == 0) {
        parsed = UI_STATUS_ATTENTION;
    } else if (level != NULL && strcmp(level, "alert") == 0) {
        parsed = UI_STATUS_ALERT;
    }
    ui_show_status(text->valuestring, parsed, (uint32_t)ttl);
}

// Wi-Fi keys, one per companion, oldest replaced first (spec/protocol.md section 4.1).
typedef struct {
    struct {
        char companion[65];
        uint8_t key[KEY_BYTES];
        uint32_t order; // 0 = empty slot
    } entries[MAX_COMPANIONS];
} pairings_t;

static esp_err_t store_pairing(const char *companion, const uint8_t key[KEY_BYTES])
{
    nvs_handle_t nvs;
    pairings_t pairings = { 0 };
    size_t size = sizeof(pairings);
    // The storage name is kept from before the rename, so pairings survive a firmware update.
    esp_err_t err = nvs_open("openpager", NVS_READWRITE, &nvs);
    if (err != ESP_OK) {
        return err;
    }
    nvs_get_blob(nvs, "pairings", &pairings, &size); // missing = start empty

    uint32_t newest = 0;
    int target = -1, oldest = 0;
    for (int i = 0; i < MAX_COMPANIONS; i++) {
        if (pairings.entries[i].order > newest) {
            newest = pairings.entries[i].order;
        }
        if (pairings.entries[i].order != 0 && strcmp(pairings.entries[i].companion, companion) == 0) {
            target = i;
        }
        if (pairings.entries[i].order < pairings.entries[oldest].order) {
            oldest = i;
        }
    }
    if (target < 0) {
        target = oldest; // an empty slot (order 0) is always the "oldest"
    }
    snprintf(pairings.entries[target].companion, sizeof(pairings.entries[target].companion), "%s", companion);
    memcpy(pairings.entries[target].key, key, KEY_BYTES);
    pairings.entries[target].order = newest + 1;

    err = nvs_set_blob(nvs, "pairings", &pairings, sizeof(pairings));
    if (err == ESP_OK) {
        err = nvs_commit(nvs);
    }
    nvs_close(nvs);
    memset(&pairings, 0, sizeof(pairings));
    return err;
}

static bool parse_key(const char *hex, uint8_t out[KEY_BYTES])
{
    if (hex == NULL || strlen(hex) != KEY_BYTES * 2) {
        return false;
    }
    for (int i = 0; i < KEY_BYTES; i++) {
        unsigned int byte;
        if (sscanf(hex + 2 * i, "%2x", &byte) != 1 ||
            strchr("0123456789abcdef", hex[2 * i]) == NULL || strchr("0123456789abcdef", hex[2 * i + 1]) == NULL) {
            return false;
        }
        out[i] = (uint8_t)byte;
    }
    return true;
}

static void handle_pair(const cJSON *body, int64_t re, transport_t transport)
{
    if (transport != TRANSPORT_SERIAL) {
        send_error(re, "unauthorised", "pairing is accepted over USB serial only", transport);
        return;
    }
    uint8_t key[KEY_BYTES];
    const char *companion = get_string(body, "companion", 64);
    const char *hex = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(body, "key"));

    cJSON *ack = cJSON_CreateObject();
    cJSON_AddStringToObject(ack, "companion", companion != NULL ? companion : "");
    const char *error = NULL;
    if (companion == NULL || !parse_key(hex, key)) {
        error = "companion and a 64-hex-digit key are required";
    } else if (store_pairing(companion, key) != ESP_OK) {
        error = "could not store the key";
    }
    memset(key, 0, sizeof(key));
    cJSON_AddBoolToObject(ack, "ok", error == NULL);
    if (error != NULL) {
        cJSON_AddStringToObject(ack, "error", error);
    } else {
        ESP_LOGI(TAG, "paired with companion %s", companion); // never log the key
    }
    send_packet("pair.ack", ack, re, transport);
}

static bool lookup_key(const char *companion, uint8_t key[KEY_BYTES])
{
    nvs_handle_t nvs;
    pairings_t pairings = { 0 };
    size_t size = sizeof(pairings);
    bool found = false;
    if (nvs_open("openpager", NVS_READONLY, &nvs) != ESP_OK) { // storage name kept from before the rename
        return false;
    }
    if (nvs_get_blob(nvs, "pairings", &pairings, &size) == ESP_OK) {
        for (int i = 0; i < MAX_COMPANIONS; i++) {
            if (pairings.entries[i].order != 0 && strcmp(pairings.entries[i].companion, companion) == 0) {
                memcpy(key, pairings.entries[i].key, KEY_BYTES);
                found = true;
            }
        }
    }
    nvs_close(nvs);
    memset(&pairings, 0, sizeof(pairings));
    return found;
}

static void handle_wifi(const cJSON *body, int64_t re, transport_t transport)
{
    if (transport != TRANSPORT_SERIAL) {
        send_error(re, "unauthorised", "Wi-Fi settings are accepted over USB serial only", transport);
        return;
    }
    const char *ssid = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(body, "ssid"));
    const char *password = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(body, "password"));
    const esp_err_t err = link_wifi_set_credentials(ssid, password);

    cJSON *ack = cJSON_CreateObject();
    cJSON_AddBoolToObject(ack, "ok", err == ESP_OK);
    if (err != ESP_OK) {
        cJSON_AddStringToObject(ack, "error", err == ESP_ERR_INVALID_ARG
                                                  ? "ssid 1-32 bytes and password up to 64 are required"
                                                  : "could not store or join");
    }
    send_packet("wifi.ack", ack, re, transport);
}

static void handle_wifi_scan(int64_t re, transport_t transport)
{
    if (transport != TRANSPORT_SERIAL) {
        send_error(re, "unauthorised", "Wi-Fi scans are answered over USB serial only", transport);
        return;
    }
    link_wifi_network_t *networks = calloc(20, sizeof(*networks));
    const int count = networks ? link_wifi_scan(networks, 20) : -1;
    cJSON *body = cJSON_CreateObject();
    cJSON *list = cJSON_AddArrayToObject(body, "networks");
    for (int i = 0; i < count; i++) {
        cJSON *n = cJSON_CreateObject();
        cJSON_AddStringToObject(n, "ssid", networks[i].ssid);
        cJSON_AddNumberToObject(n, "rssi", networks[i].rssi);
        cJSON_AddNumberToObject(n, "channel", networks[i].channel);
        cJSON_AddBoolToObject(n, "secured", networks[i].secured);
        cJSON_AddItemToArray(list, n);
    }
    if (count < 0) {
        cJSON_AddStringToObject(body, "error", "scan failed");
    }
    free(networks);
    send_packet("wifi.networks", body, re, transport);
}

// "knoend.layout" or "openpager.layout" -> "layout"; NULL for anything else.
static const char *name_of(const char *type)
{
    if (strncmp(type, "knoend.", 7) == 0) {
        return type + 7;
    }
    if (strncmp(type, "openpager.", 10) == 0) {
        return type + 10;
    }
    return NULL;
}

// Whether `type` is one of ours; remembers which prefix this link's companion uses.
static bool message_name(const char *type, transport_t transport)
{
    if (name_of(type) == NULL) {
        return false;
    }
    const char *prefix = strncmp(type, "knoend.", 7) == 0 ? "knoend" : "openpager";
    if (transport == TRANSPORT_SERIAL) {
        prefix_serial = prefix;
    } else {
        prefix_wifi = prefix;
    }
    return true;
}

// The person's theme and turn (spec/protocol.md 3.6d): stored, drawn at once and answered with the
// screen's new size and largest grid.
static void handle_display(const cJSON *body, int64_t re, transport_t transport)
{
    display_settings_t wanted = *display_settings();
    const cJSON *theme = cJSON_GetObjectItemCaseSensitive(body, "theme");
    const cJSON *turn = cJSON_GetObjectItemCaseSensitive(body, "turn");
    const cJSON *start = cJSON_GetObjectItemCaseSensitive(body, "startSize");
    if (theme == NULL && turn == NULL && start == NULL) {
        send_error(re, "bad-packet", "theme, turn or startSize is required", transport);
        return;
    }
    if (start != NULL && !get_int(body, "startSize", DISPLAY_START_MIN, DISPLAY_START_MAX, &wanted.start_size)) {
        send_error(re, "bad-packet", "startSize must be 24-96", transport);
        return;
    }
    if (theme != NULL && !display_theme_parse(cJSON_GetStringValue(theme), &wanted.theme)) {
        send_error(re, "bad-packet", "theme must be dark, white or sepia", transport);
        return;
    }
    if (turn != NULL && !get_int(body, "turn", 0, 3, &wanted.turn)) {
        send_error(re, "bad-packet", "turn must be 0-3", transport);
        return;
    }
    display_settings_t *now = display_settings_mut();
    const bool new_theme = wanted.theme != now->theme;
    const bool new_turn = wanted.turn != now->turn;
    const bool new_start = wanted.start_size != now->start_size;
    if ((new_theme || new_turn || new_start) && display_settings_store(&wanted) != ESP_OK) {
        send_error(re, "busy", "could not store the display settings", transport);
        return;
    }
    if (new_theme) {
        now->theme = wanted.theme;
        ui_set_theme(wanted.theme);
        ESP_LOGI(TAG, "display: %s", display_theme_name(wanted.theme));
    }
    if (new_turn) {
        now->turn = wanted.turn;
        ui_set_turn(wanted.turn);
    }
    if (new_start) {
        now->start_size = wanted.start_size;
        ui_show_start(); // the person sees the new size at once
    }
    const board_info_t *board = board_info();
    int cols, rows;
    ui_max_grid(&cols, &rows);
    cJSON *ack = cJSON_CreateObject();
    cJSON_AddStringToObject(ack, "theme", display_theme_name(wanted.theme));
    cJSON_AddNumberToObject(ack, "turn", wanted.turn);
    cJSON_AddNumberToObject(ack, "startSize", wanted.start_size);
    cJSON_AddNumberToObject(ack, "width", board->width);
    cJSON_AddNumberToObject(ack, "height", board->height);
    cJSON *grid = cJSON_AddObjectToObject(ack, "grid");
    cJSON_AddNumberToObject(grid, "cols", cols);
    cJSON_AddNumberToObject(grid, "rows", rows);
    send_packet("display.ack", ack, re, transport);
}

static void font_ack(const char *slot, const char *name, const char *error, int64_t re, transport_t transport)
{
    cJSON *ack = cJSON_CreateObject();
    cJSON_AddStringToObject(ack, "slot", slot != NULL ? slot : "");
    if (name != NULL) {
        cJSON_AddStringToObject(ack, "name", name);
    }
    cJSON_AddBoolToObject(ack, "ok", error == NULL);
    if (error != NULL) {
        cJSON_AddStringToObject(ack, "error", error);
    }
    send_packet("font.ack", ack, re, transport);
}

// A font the person chose (spec/protocol.md 3.6e): slices in order, kept in the fonts partition,
// then used at once. {"slot", "clear": true} goes back to the built-in font.
static void handle_font(const cJSON *body, int64_t re, transport_t transport)
{
    const char *slot_name = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(body, "slot"));
    font_slot_t slot;
    if (!fonts_slot_parse(slot_name, &slot)) {
        font_ack(slot_name, NULL, "slot must be keys or start", re, transport);
        return;
    }
    if (cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(body, "clear"))) {
        if (slot == FONT_SLOT_KEYS) {
            ui_fonts_release();
        }
        const esp_err_t err = fonts_clear(slot);
        font_ack(slot_name, NULL, err == ESP_OK ? NULL : esp_err_to_name(err), re, transport);
        return;
    }
    const char *name = get_string(body, "name", FONT_NAME_BYTES - 1);
    const char *data = get_string(body, "data", 4000);
    const cJSON *crc = cJSON_GetObjectItemCaseSensitive(body, "crc");
    int offset, total;
    if (name == NULL || data == NULL || !cJSON_IsNumber(crc) || !get_int(body, "offset", 0, 0x7FFFFFFF, &offset) ||
        !get_int(body, "total", 1, 0x7FFFFFFF, &total)) {
        font_ack(slot_name, name, "name, total, offset, crc and data are required", re, transport);
        return;
    }
    if ((size_t)total > fonts_slot_capacity()) {
        font_ack(slot_name, name, "too big for the board: choose another font", re, transport);
        return;
    }
    if (offset == 0) {
        if (slot == FONT_SLOT_KEYS) {
            ui_fonts_release(); // the keys may not use a font being replaced
        }
        const esp_err_t err = fonts_begin(slot, total, name);
        if (err != ESP_OK) {
            font_ack(slot_name, name, esp_err_to_name(err), re, transport);
            return;
        }
    }
    static uint8_t piece[3000]; // not on the network task's stack
    size_t written = 0;
    if (mbedtls_base64_decode(piece, sizeof(piece), &written, (const unsigned char *)data, strlen(data)) != 0) {
        font_ack(slot_name, name, "data is not base64 or too long", re, transport);
        return;
    }
    if (fonts_write(slot, offset, piece, written) != ESP_OK) {
        font_ack(slot_name, name, "slice out of order", re, transport);
        return;
    }
    if ((size_t)offset + written < (size_t)total) {
        return; // more slices to come
    }
    const esp_err_t err = fonts_finish(slot, (uint32_t)crc->valuedouble);
    if (slot == FONT_SLOT_KEYS) {
        ui_fonts_reload();
    } else if (err == ESP_OK) {
        ui_show_start(); // the new start-up font, at once
    }
    ESP_LOGI(TAG, "font %s: %s %s", slot_name, name, err == ESP_OK ? "kept" : esp_err_to_name(err));
    font_ack(slot_name, name, err == ESP_OK ? NULL : "the font arrived damaged; send it again", re, transport);
}

static void handle_line(const char *line, transport_t transport)
{
    cJSON *packet = cJSON_Parse(line);
    const cJSON *id = cJSON_GetObjectItemCaseSensitive(packet, "id");
    const cJSON *type = cJSON_GetObjectItemCaseSensitive(packet, "type");
    const cJSON *body = cJSON_GetObjectItemCaseSensitive(packet, "body");
    const int64_t re = cJSON_IsNumber(id) && id->valuedouble >= 0 ? (int64_t)id->valuedouble : -1;

    if (packet == NULL) {
        send_error(-1, "bad-packet", "not valid JSON", transport);
    } else if (re < 0 || !cJSON_IsString(type) || !cJSON_IsObject(body)) {
        send_error(re, "bad-packet", "id, type and body are required", transport);
    } else if (!message_name(type->valuestring, transport)) {
        // Not a KnoEnd (or OpenPager) message: ignored, as the spec requires for unknown types.
    } else if (strcmp(name_of(type->valuestring), "identity") == 0) {
        const char *name = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(body, "deviceName"));
        ESP_LOGI(TAG, "companion says hello: %s", name != NULL ? name : "(no name)");
        send_identity(re, transport);
    } else if (strcmp(name_of(type->valuestring), "ping") == 0) {
        send_packet("ping", cJSON_CreateObject(), re, transport);
    } else if (strcmp(name_of(type->valuestring), "layout") == 0) {
        handle_layout(body, re, transport);
    } else if (strcmp(name_of(type->valuestring), "status") == 0) {
        handle_status(body);
    } else if (strcmp(name_of(type->valuestring), "image") == 0) {
        handle_image(body, re, transport);
    } else if (strcmp(name_of(type->valuestring), "pair") == 0) {
        handle_pair(body, re, transport);
    } else if (strcmp(name_of(type->valuestring), "wifi") == 0) {
        handle_wifi(body, re, transport);
    } else if (strcmp(name_of(type->valuestring), "wifi.scan") == 0) {
        handle_wifi_scan(re, transport);
    } else if (strcmp(name_of(type->valuestring), "display") == 0) {
        handle_display(body, re, transport);
    } else if (strcmp(name_of(type->valuestring), "font") == 0) {
        handle_font(body, re, transport);
    }
    // Anything else is ignored, as the spec requires for unknown types.
    cJSON_Delete(packet);
}

static void on_serial_line(const char *line, bool too_long)
{
    if (too_long) {
        send_error(-1, "too-large", "packet over 4096 bytes", TRANSPORT_SERIAL);
        return;
    }
    handle_line(line, TRANSPORT_SERIAL);
}

static void on_wifi_packet(const char *packet)
{
    handle_line(packet, TRANSPORT_WIFI);
}

// A companion just proved it holds the key: start its numbering at 0 and introduce the board.
static void on_wifi_session(void)
{
    xSemaphoreTake(send_lock, portMAX_DELAY);
    next_id_wifi = 0;
    xSemaphoreGive(send_lock);
    send_identity(-1, TRANSPORT_WIFI);
}

esp_err_t protocol_start(void)
{
    uint8_t mac[6];
    esp_efuse_mac_get_default(mac);
    snprintf(device_id, sizeof(device_id), "%02x%02x%02x%02x%02x%02x",
             mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);

    send_lock = xSemaphoreCreateMutex();
    if (send_lock == NULL) {
        return ESP_ERR_NO_MEM;
    }
    esp_err_t err = link_serial_start(on_serial_line);
    if (err == ESP_OK) {
        send_identity(-1, TRANSPORT_SERIAL);
        err = link_wifi_start(device_id, on_wifi_packet, lookup_key, on_wifi_session);
    }
    return err;
}

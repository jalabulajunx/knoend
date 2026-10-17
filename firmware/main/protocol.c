#include "protocol.h"

#include <stdbool.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>

#include "board.h"
#include "cJSON.h"
#include "esp_app_desc.h"
#include "esp_log.h"
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
    "openpager.identity", "openpager.layout", "openpager.status", "openpager.pair", "openpager.wifi",
    "openpager.wifi.scan", "openpager.ping",
};
static const char *const outgoing_types[] = {
    "openpager.identity", "openpager.key", "openpager.layout.ack", "openpager.pair.ack",
    "openpager.wifi.ack", "openpager.wifi.networks", "openpager.ping", "openpager.error",
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
static void send_packet(const char *type, cJSON *body, int64_t re, transport_t to)
{
    xSemaphoreTake(send_lock, portMAX_DELAY);
    cJSON *packet = cJSON_CreateObject();
    cJSON *id = cJSON_AddNumberToObject(packet, "id", 0);
    if (re >= 0) {
        cJSON_AddNumberToObject(packet, "re", (double)re);
    }
    cJSON_AddStringToObject(packet, "type", type);
    cJSON_AddItemToObject(packet, "body", body);

    if (to & TRANSPORT_SERIAL) {
        cJSON_SetNumberValue(id, next_id_serial++);
        char *text = cJSON_PrintUnformatted(packet);
        if (text != NULL) {
            link_serial_send(text);
            cJSON_free(text);
        }
    }
    if ((to & TRANSPORT_WIFI) && link_wifi_session_active()) {
        cJSON_SetNumberValue(id, next_id_wifi++);
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
    send_packet("openpager.error", body, re, to);
}

static cJSON *string_array(const char *const *items, size_t count)
{
    cJSON *array = cJSON_CreateArray();
    for (size_t i = 0; i < count; i++) {
        cJSON_AddItemToArray(array, cJSON_CreateString(items[i]));
    }
    return array;
}

static void send_identity(int64_t re, transport_t to)
{
    const board_info_t *board = board_info();
    cJSON *body = cJSON_CreateObject();
    cJSON_AddStringToObject(body, "deviceId", device_id);
    cJSON_AddStringToObject(body, "deviceName", "OpenPager");
    cJSON_AddStringToObject(body, "deviceType", "board");
    cJSON_AddNumberToObject(body, "protocolVersion", PROTOCOL_VERSION);
    cJSON_AddStringToObject(body, "firmware", esp_app_get_description()->version);

    cJSON *b = cJSON_AddObjectToObject(body, "board");
    cJSON_AddStringToObject(b, "profile", board->id);
    cJSON_AddNumberToObject(b, "width", board->width);
    cJSON_AddNumberToObject(b, "height", board->height);
    cJSON *grid = cJSON_AddObjectToObject(b, "grid");
    cJSON_AddNumberToObject(grid, "cols", 3);
    cJSON_AddNumberToObject(grid, "rows", 3);
    cJSON_AddBoolToObject(b, "statusBar", true);

    cJSON *features = cJSON_AddObjectToObject(body, "features");
    cJSON_AddBoolToObject(features, "speaker", board->has_speaker);
    cJSON_AddBoolToObject(features, "battery", false);
    cJSON_AddBoolToObject(features, "nativeUsb", board->has_native_usb);
    cJSON_AddItemToObject(features, "transports",
                          string_array(transports, sizeof(transports) / sizeof(transports[0])));

    cJSON_AddItemToObject(body, "incoming",
                          string_array(incoming_types, sizeof(incoming_types) / sizeof(incoming_types[0])));
    cJSON_AddItemToObject(body, "outgoing",
                          string_array(outgoing_types, sizeof(outgoing_types) / sizeof(outgoing_types[0])));
    send_packet("openpager.identity", body, re, to);
}

void protocol_send_key(int slot, const char *layout_id, uint32_t seq)
{
    cJSON *body = cJSON_CreateObject();
    cJSON_AddNumberToObject(body, "key", slot);
    cJSON_AddStringToObject(body, "layout", layout_id);
    cJSON_AddNumberToObject(body, "seq", seq);
    send_packet("openpager.key", body, -1, TRANSPORT_ALL);
}

void protocol_send_keystroke(int slot, const char *layout_id, uint32_t seq, uint8_t modifiers, uint8_t usage)
{
    cJSON *body = cJSON_CreateObject();
    cJSON_AddNumberToObject(body, "key", slot);
    cJSON_AddStringToObject(body, "layout", layout_id);
    cJSON_AddNumberToObject(body, "seq", seq);
    cJSON *hid = cJSON_AddObjectToObject(body, "hid");
    cJSON_AddNumberToObject(hid, "modifiers", modifiers);
    cJSON_AddNumberToObject(hid, "usage", usage);
    send_packet("openpager.key", body, -1, TRANSPORT_ALL);
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
    ui_layout_t layout = { .reviewed = true };
    char error[80] = "";

    const char *id = get_string(body, "layout", sizeof(layout.id) - 1);
    const char *name = get_string(body, "name", sizeof(layout.name) - 1);
    const cJSON *keys = cJSON_GetObjectItemCaseSensitive(body, "keys");
    const cJSON *reviewed = cJSON_GetObjectItemCaseSensitive(body, "reviewed");

    if (id == NULL || name == NULL || !cJSON_IsArray(keys)) {
        snprintf(error, sizeof(error), "layout, name and keys are required");
    } else if (cJSON_GetArraySize(keys) > UI_KEYS) {
        snprintf(error, sizeof(error), "too many keys");
    } else {
        strcpy(layout.id, id);
        strcpy(layout.name, name);
        layout.reviewed = !cJSON_IsFalse(reviewed);
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

        if (!get_int(key, "key", 1, UI_KEYS, &slot)) {
            snprintf(error, sizeof(error), "key slot must be 1-9");
            break;
        }
        ui_key_t *target = &layout.keys[slot - 1];
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
            target->confirm = cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(key, "confirm"));
        }
    }

    cJSON *ack = cJSON_CreateObject();
    cJSON_AddStringToObject(ack, "layout", id != NULL ? id : "");
    cJSON_AddBoolToObject(ack, "ok", error[0] == '\0');
    if (error[0] != '\0') {
        cJSON_AddStringToObject(ack, "error", error);
    } else {
        ui_apply_layout(&layout); // all-or-nothing: only a fully valid layout replaces the keys
    }
    send_packet("openpager.layout.ack", ack, re, from);
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
    send_packet("openpager.pair.ack", ack, re, transport);
}

static bool lookup_key(const char *companion, uint8_t key[KEY_BYTES])
{
    nvs_handle_t nvs;
    pairings_t pairings = { 0 };
    size_t size = sizeof(pairings);
    bool found = false;
    if (nvs_open("openpager", NVS_READONLY, &nvs) != ESP_OK) {
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
    send_packet("openpager.wifi.ack", ack, re, transport);
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
    send_packet("openpager.wifi.networks", body, re, transport);
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
    } else if (strcmp(type->valuestring, "openpager.identity") == 0) {
        const char *name = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(body, "deviceName"));
        ESP_LOGI(TAG, "companion says hello: %s", name != NULL ? name : "(no name)");
        send_identity(re, transport);
    } else if (strcmp(type->valuestring, "openpager.ping") == 0) {
        send_packet("openpager.ping", cJSON_CreateObject(), re, transport);
    } else if (strcmp(type->valuestring, "openpager.layout") == 0) {
        handle_layout(body, re, transport);
    } else if (strcmp(type->valuestring, "openpager.status") == 0) {
        handle_status(body);
    } else if (strcmp(type->valuestring, "openpager.pair") == 0) {
        handle_pair(body, re, transport);
    } else if (strcmp(type->valuestring, "openpager.wifi") == 0) {
        handle_wifi(body, re, transport);
    } else if (strcmp(type->valuestring, "openpager.wifi.scan") == 0) {
        handle_wifi_scan(re, transport);
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

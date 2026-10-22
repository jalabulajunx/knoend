// Bluetooth LE data transport on Bluedroid: one GATT service with two characteristics, beside the
// HID keyboard (spec/protocol.md section 2). Written from Espressif's GATT server API docs.
//
// Both characteristics need an encrypted link: only a computer the board is bonded with (paired as
// its Bluetooth keyboard) can reach them, which is the trust Bluetooth gives (spec section 4).

#include "link_ble.h"

#include <stdlib.h>
#include <string.h>

#include "esp_gatt_common_api.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/semphr.h"
#include "freertos/task.h"

static const char *TAG = "link_ble";

// An app id of our own; the HID helper uses 0x180F, 0x180A and 0x1812 (its service UUIDs).
#define DATA_APP_ID 0x5C51
#define LOCAL_MTU   247

// f0e35c51-f991-4cdb-b330-97d0615e00xx, little-endian as Bluedroid takes 128-bit UUIDs.
#define KNOEND_UUID(last) { last, 0x00, 0x5e, 0x61, 0xd0, 0x97, 0x30, 0xb3, 0xdb, 0x4c, 0x91, 0xf9, 0x51, 0x5c, 0xe3, 0xf0 }
static const uint8_t service_uuid[16] = KNOEND_UUID(0x01);
static const uint8_t to_board_uuid[16] = KNOEND_UUID(0x02);
static const uint8_t from_board_uuid[16] = KNOEND_UUID(0x03);

static const uint16_t primary_service = ESP_GATT_UUID_PRI_SERVICE;
static const uint16_t characteristic_declaration = ESP_GATT_UUID_CHAR_DECLARE;
static const uint16_t client_config = ESP_GATT_UUID_CHAR_CLIENT_CONFIG;
static const uint8_t write_props = ESP_GATT_CHAR_PROP_BIT_WRITE | ESP_GATT_CHAR_PROP_BIT_WRITE_NR;
static const uint8_t notify_props = ESP_GATT_CHAR_PROP_BIT_NOTIFY;
static uint8_t cccd_value[2];
static uint8_t empty[1];

enum { IDX_SERVICE, IDX_TO_DECL, IDX_TO_VALUE, IDX_FROM_DECL, IDX_FROM_VALUE, IDX_FROM_CCCD, IDX_COUNT };

#define ENCRYPTED_RW (ESP_GATT_PERM_READ_ENCRYPTED | ESP_GATT_PERM_WRITE_ENCRYPTED)
static const esp_gatts_attr_db_t table[IDX_COUNT] = {
    [IDX_SERVICE] = { { ESP_GATT_AUTO_RSP }, { ESP_UUID_LEN_16, (uint8_t *)&primary_service, ESP_GATT_PERM_READ,
                                               sizeof(service_uuid), sizeof(service_uuid), (uint8_t *)service_uuid } },
    [IDX_TO_DECL] = { { ESP_GATT_AUTO_RSP }, { ESP_UUID_LEN_16, (uint8_t *)&characteristic_declaration, ESP_GATT_PERM_READ,
                                               1, 1, (uint8_t *)&write_props } },
    // Written by the companion; handled in GATTS_WRITE_EVT (no stored value).
    [IDX_TO_VALUE] = { { ESP_GATT_RSP_BY_APP }, { ESP_UUID_LEN_128, (uint8_t *)to_board_uuid, ESP_GATT_PERM_WRITE_ENCRYPTED,
                                                  512, 0, empty } },
    [IDX_FROM_DECL] = { { ESP_GATT_AUTO_RSP }, { ESP_UUID_LEN_16, (uint8_t *)&characteristic_declaration, ESP_GATT_PERM_READ,
                                                 1, 1, (uint8_t *)&notify_props } },
    [IDX_FROM_VALUE] = { { ESP_GATT_AUTO_RSP }, { ESP_UUID_LEN_128, (uint8_t *)from_board_uuid, ESP_GATT_PERM_READ_ENCRYPTED,
                                                  1, 0, empty } },
    [IDX_FROM_CCCD] = { { ESP_GATT_AUTO_RSP }, { ESP_UUID_LEN_16, (uint8_t *)&client_config, ENCRYPTED_RW,
                                                 sizeof(cccd_value), sizeof(cccd_value), cccd_value } },
};

static link_ble_line_cb line_cb;
static link_ble_session_cb session_cb;
static esp_gatt_if_t data_if = ESP_GATT_IF_NONE;
static uint16_t handles[IDX_COUNT];
static uint16_t conn_id;
static bool subscribed, congested;
static uint16_t mtu = 23;
static char *line;
static size_t length;
static bool overflow;
static SemaphoreHandle_t send_lock;

// What the Bluetooth stack's task hands to ours: packets are handled (and answered) there, never in
// the stack's own task, which a reply waiting out congestion would block.
typedef struct {
    char *line;    // malloc'd; NULL for a new session
    bool too_long;
} inbound_t;
static QueueHandle_t inbox;

static void inbox_task(void *arg)
{
    inbound_t item;
    while (true) {
        if (xQueueReceive(inbox, &item, portMAX_DELAY) != pdTRUE) {
            continue;
        }
        if (item.line == NULL && !item.too_long) {
            if (session_cb != NULL) {
                session_cb();
            }
        } else if (line_cb != NULL) {
            line_cb(item.line != NULL ? item.line : "", item.too_long);
        }
        free(item.line);
    }
}

static void post(char *copy, bool too_long)
{
    const inbound_t item = { copy, too_long };
    if (xQueueSend(inbox, &item, 0) != pdTRUE) {
        ESP_LOGW(TAG, "inbox full; packet dropped");
        free(copy);
    }
}

bool link_ble_session_active(void)
{
    return subscribed;
}

// Bytes written by the companion: complete lines go to the protocol, as on the serial link.
static void received(const uint8_t *data, size_t size)
{
    for (size_t i = 0; i < size; i++) {
        const char c = (char)data[i];
        if (c == '\n') {
            if (overflow) {
                post(NULL, true);
            } else if (length > 0 && line[0] == '{') {
                line[length] = '\0';
                char *copy = strdup(line);
                if (copy != NULL) {
                    post(copy, false);
                }
            }
            length = 0;
            overflow = false;
        } else if (c != '\r' && length < LINK_BLE_MAX_LINE) {
            line[length++] = c;
        } else if (c != '\r') {
            overflow = true;
        }
    }
}

void link_ble_send(const char *text)
{
    if (!subscribed) {
        return;
    }
    // The line and its newline, in notifications of up to MTU - 3 bytes.
    const size_t size = strlen(text) + 1;
    char *framed = malloc(size);
    if (framed == NULL) {
        return;
    }
    memcpy(framed, text, size - 1);
    framed[size - 1] = '\n';
    xSemaphoreTake(send_lock, portMAX_DELAY);
    const size_t chunk = mtu > 3 ? mtu - 3 : 20;
    for (size_t sent = 0; sent < size && subscribed;) {
        const size_t n = size - sent < chunk ? size - sent : chunk;
        // While the stack's queue is full (congested), wait a little at a time.
        for (int wait = 0; congested && wait < 200; wait++) {
            vTaskDelay(pdMS_TO_TICKS(5));
        }
        esp_err_t err = esp_ble_gatts_send_indicate(data_if, conn_id, handles[IDX_FROM_VALUE], n, (uint8_t *)framed + sent, false);
        if (err != ESP_OK) {
            vTaskDelay(pdMS_TO_TICKS(20));
            err = esp_ble_gatts_send_indicate(data_if, conn_id, handles[IDX_FROM_VALUE], n, (uint8_t *)framed + sent, false);
        }
        if (err != ESP_OK) {
            ESP_LOGW(TAG, "notification failed (%s); packet dropped", esp_err_to_name(err));
            break;
        }
        sent += n;
    }
    xSemaphoreGive(send_lock);
    free(framed);
}

bool link_ble_gatts_event(esp_gatts_cb_event_t event, esp_gatt_if_t gatts_if, esp_ble_gatts_cb_param_t *param)
{
    if (event == ESP_GATTS_REG_EVT) {
        if (param->reg.app_id != DATA_APP_ID) {
            return false;
        }
        data_if = gatts_if;
        esp_ble_gatts_create_attr_tab(table, gatts_if, IDX_COUNT, 0);
        return true;
    }
    if (gatts_if != data_if || data_if == ESP_GATT_IF_NONE) {
        return false;
    }
    switch (event) {
    case ESP_GATTS_CREAT_ATTR_TAB_EVT:
        if (param->add_attr_tab.status == ESP_GATT_OK && param->add_attr_tab.num_handle == IDX_COUNT) {
            memcpy(handles, param->add_attr_tab.handles, sizeof(handles));
            esp_ble_gatts_start_service(handles[IDX_SERVICE]);
            ESP_LOGI(TAG, "data service ready");
        } else {
            ESP_LOGE(TAG, "data service table failed (status %d)", param->add_attr_tab.status);
        }
        break;
    case ESP_GATTS_CONNECT_EVT:
        conn_id = param->connect.conn_id;
        mtu = 23;
        break;
    case ESP_GATTS_DISCONNECT_EVT:
        subscribed = congested = false;
        length = 0;
        break;
    case ESP_GATTS_MTU_EVT:
        mtu = param->mtu.mtu;
        break;
    case ESP_GATTS_CONGEST_EVT:
        congested = param->congest.congested;
        break;
    case ESP_GATTS_WRITE_EVT:
        if (param->write.handle == handles[IDX_FROM_CCCD] && param->write.len == 2) {
            const bool now = param->write.value[0] & 0x01;
            if (now && !subscribed) {
                subscribed = true;
                printf("BLE data session\n");
                post(NULL, false);
            }
            subscribed = now;
        } else if (param->write.handle == handles[IDX_TO_VALUE]) {
            received(param->write.value, param->write.len);
            if (param->write.need_rsp) {
                esp_ble_gatts_send_response(gatts_if, param->write.conn_id, param->write.trans_id, ESP_GATT_OK, NULL);
            }
        }
        break;
    default:
        break;
    }
    return true;
}

esp_err_t link_ble_register(void)
{
    line = malloc(LINK_BLE_MAX_LINE + 1);
    send_lock = xSemaphoreCreateMutex();
    inbox = xQueueCreate(8, sizeof(inbound_t));
    if (line == NULL || send_lock == NULL || inbox == NULL) {
        return ESP_ERR_NO_MEM;
    }
    if (xTaskCreate(inbox_task, "link_ble", 6144, NULL, 5, NULL) != pdPASS) {
        return ESP_ERR_NO_MEM;
    }
    esp_ble_gatt_set_local_mtu(LOCAL_MTU);
    return esp_ble_gatts_app_register(DATA_APP_ID);
}

void link_ble_start(link_ble_line_cb on_line, link_ble_session_cb on_session)
{
    line_cb = on_line;
    session_cb = on_session;
}

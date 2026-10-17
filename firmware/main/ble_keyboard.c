// Bluetooth LE HID keyboard on Bluedroid, using Espressif's esp_hid_gap helper for advertising
// and pairing (components/esp_hid_gap, Unlicense OR CC0-1.0).

#include "ble_keyboard.h"

#include <stdio.h>
#include <string.h>

#include "esp_check.h"
#include "esp_gap_ble_api.h"
#include "esp_gatts_api.h"
#include "esp_hid_gap.h"
#include "esp_hidd.h"
#include "esp_log.h"

static const char *TAG = "ble_keyboard";

// Keyboard report map from the ESP-IDF v5.4.4 esp_hid_device example (Unlicense OR CC0-1.0),
// with one change: the key array accepts usages up to 0x73 (was 0x65) so F13-F24 can be sent.
// Report ID 1 input: modifiers, reserved, 5 key slots. Output: 5 LED bits + padding.
static const uint8_t report_map[] = {
    0x05, 0x01,        // Usage Page (Generic Desktop Ctrls)
    0x09, 0x06,        // Usage (Keyboard)
    0xA1, 0x01,        // Collection (Application)
    0x85, 0x01,        //   Report ID (1)
    0x05, 0x07,        //   Usage Page (Kbrd/Keypad)
    0x19, 0xE0,        //   Usage Minimum (0xE0)
    0x29, 0xE7,        //   Usage Maximum (0xE7)
    0x15, 0x00,        //   Logical Minimum (0)
    0x25, 0x01,        //   Logical Maximum (1)
    0x75, 0x01,        //   Report Size (1)
    0x95, 0x08,        //   Report Count (8)
    0x81, 0x02,        //   Input (Data,Var,Abs)
    0x95, 0x01,        //   Report Count (1)
    0x75, 0x08,        //   Report Size (8)
    0x81, 0x03,        //   Input (Const,Var,Abs)
    0x95, 0x05,        //   Report Count (5)
    0x75, 0x01,        //   Report Size (1)
    0x05, 0x08,        //   Usage Page (LEDs)
    0x19, 0x01,        //   Usage Minimum (Num Lock)
    0x29, 0x05,        //   Usage Maximum (Kana)
    0x91, 0x02,        //   Output (Data,Var,Abs)
    0x95, 0x01,        //   Report Count (1)
    0x75, 0x03,        //   Report Size (3)
    0x91, 0x03,        //   Output (Const,Var,Abs)
    0x95, 0x05,        //   Report Count (5)
    0x75, 0x08,        //   Report Size (8)
    0x15, 0x00,        //   Logical Minimum (0)
    0x25, 0x73,        //   Logical Maximum (0x73, F24)
    0x05, 0x07,        //   Usage Page (Kbrd/Keypad)
    0x19, 0x00,        //   Usage Minimum (0x00)
    0x29, 0x73,        //   Usage Maximum (0x73, F24)
    0x81, 0x00,        //   Input (Data,Array,Abs)
    0xC0,              // End Collection
};

#define KEYBOARD_REPORT_ID  1
#define KEYBOARD_REPORT_LEN 7 // modifiers, reserved, 5 keys

static esp_hid_raw_report_map_t report_maps[] = {
    { .data = report_map, .len = sizeof(report_map) },
};

static esp_hidd_dev_t *hid_dev;
static volatile bool connected;

bool ble_keyboard_connected(void)
{
    return connected;
}

esp_err_t ble_keyboard_tap(uint8_t modifiers, uint8_t usage)
{
    if (!connected) {
        return ESP_ERR_INVALID_STATE;
    }
    uint8_t report[KEYBOARD_REPORT_LEN] = { modifiers, 0, usage };
    ESP_RETURN_ON_ERROR(esp_hidd_dev_input_set(hid_dev, 0, KEYBOARD_REPORT_ID, report, sizeof(report)),
                        TAG, "key down");
    memset(report, 0, sizeof(report));
    return esp_hidd_dev_input_set(hid_dev, 0, KEYBOARD_REPORT_ID, report, sizeof(report));
}

// Called by esp_hid_gap.c when pairing finishes, successfully or not.
void ble_hid_task_start_up(void)
{
    printf("BLE pairing complete\n");
    fflush(stdout);
}

static void hid_event(void *args, esp_event_base_t base, int32_t id, void *data)
{
    esp_hidd_event_data_t *param = (esp_hidd_event_data_t *)data;

    switch ((esp_hidd_event_t)id) {
    case ESP_HIDD_START_EVENT:
        ESP_LOGI(TAG, "advertising");
        esp_hid_ble_gap_adv_start();
        break;
    case ESP_HIDD_CONNECT_EVENT:
        connected = true;
        printf("BLE connected\n");
        fflush(stdout);
        break;
    case ESP_HIDD_DISCONNECT_EVENT:
        connected = false;
        printf("BLE disconnected: %s\n",
               esp_hid_disconnect_reason_str(esp_hidd_dev_transport_get(param->disconnect.dev),
                                             param->disconnect.reason));
        fflush(stdout);
        esp_hid_ble_gap_adv_start();
        break;
    default:
        break;
    }
}

esp_err_t ble_keyboard_start(const char *name)
{
    static esp_hid_device_config_t config = {
        .vendor_id = 0x16C0,  // as in the ESP-IDF example
        .product_id = 0x05DF,
        .version = 0x0100,
        .manufacturer_name = "OpenPager",
        .serial_number = "0001",
        .report_maps = report_maps,
        .report_maps_len = 1,
    };
    config.device_name = name;

    ESP_RETURN_ON_ERROR(esp_hid_gap_init(HIDD_BLE_MODE), TAG, "gap init");
    ESP_RETURN_ON_ERROR(esp_hid_ble_gap_adv_init(ESP_HID_APPEARANCE_KEYBOARD, name), TAG, "adv init");

    // Pairing mode: "Just Works" (LE Secure Connections, bonded, no MITM). The helper above asks
    // for numeric comparison, but the board does not show the code yet and desktop agents leave
    // the prompt unanswered, so pairing never completed. The link is still encrypted; what this
    // gives up is protection against an active attacker during pairing itself. Revisit once the
    // board can show the code on screen.
    esp_ble_auth_req_t auth_req = ESP_LE_AUTH_REQ_SC_BOND;
    esp_ble_io_cap_t io_cap = ESP_IO_CAP_NONE;
    ESP_RETURN_ON_ERROR(esp_ble_gap_set_security_param(ESP_BLE_SM_AUTHEN_REQ_MODE, &auth_req, 1),
                        TAG, "auth req");
    ESP_RETURN_ON_ERROR(esp_ble_gap_set_security_param(ESP_BLE_SM_IOCAP_MODE, &io_cap, 1),
                        TAG, "io cap");
    ESP_RETURN_ON_ERROR(esp_ble_gatts_register_callback(esp_hidd_gatts_event_handler), TAG, "gatts");
    return esp_hidd_dev_init(&config, ESP_HID_TRANSPORT_BLE, hid_event, &hid_dev);
}

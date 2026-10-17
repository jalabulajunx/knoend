# Unmodified copy of the GAP helper from ESP-IDF v5.4.4 examples/bluetooth/esp_hid_device
# (Espressif Systems, Unlicense OR CC0-1.0). It calls ble_hid_task_start_up(), which
# main/ble_keyboard.c provides.
idf_component_register(SRCS "esp_hid_gap.c"
                       INCLUDE_DIRS "."
                       REQUIRES bt esp_hid nvs_flash)

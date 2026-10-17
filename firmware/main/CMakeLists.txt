idf_component_register(SRCS "app_main.c" "ble_keyboard.c" "link_serial.c" "link_wifi.c" "protocol.c" "ui.c"
                       INCLUDE_DIRS "."
                       REQUIRES board_api board_onx2432g028 board_fnk0104b
                       PRIV_REQUIRES nvs_flash bt esp_hid esp_hid_gap esp_driver_uart json esp_wifi esp_netif esp_http_server mbedtls espressif__mdns esp_app_format esp_timer
                                     lvgl espressif__esp_lvgl_port)

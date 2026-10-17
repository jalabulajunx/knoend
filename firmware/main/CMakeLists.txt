idf_component_register(SRCS "app_main.c"
                       INCLUDE_DIRS "."
                       REQUIRES board_api board_onx2432g028 board_fnk0104b
                       PRIV_REQUIRES nvs_flash esp_app_format esp_timer
                                     lvgl espressif__esp_lvgl_port)

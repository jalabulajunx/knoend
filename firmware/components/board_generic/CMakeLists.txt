# Universal board support, driven by the board definition in the "board" partition.
idf_component_register(SRCS "board_generic.c"
                       REQUIRES board_api
                       PRIV_REQUIRES esp_driver_i2c esp_driver_spi esp_driver_ledc esp_driver_gpio esp_lcd
                                     esp_partition json lvgl espressif__esp_lvgl_port espressif__esp_lcd_ili9341)

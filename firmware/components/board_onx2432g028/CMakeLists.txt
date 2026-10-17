# Board profile: ONX2432G028. Compiled only when this board is selected in Kconfig.
set(srcs "")
if(CONFIG_OPENPAGER_BOARD_ONX2432G028)
    set(srcs "board_onx2432g028.c")
endif()

idf_component_register(SRCS ${srcs}
                       REQUIRES board_api
                       PRIV_REQUIRES esp_driver_i2c esp_driver_spi esp_driver_ledc esp_lcd
                                     lvgl espressif__esp_lvgl_port)

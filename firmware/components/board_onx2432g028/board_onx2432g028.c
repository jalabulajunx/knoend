// Board profile for the ONX2432G028 (Nextion Open G-series, ESP32-S3, 2.8" 320x240).
//
// Written for OpenPager from the board's schematic (V1.3) and Espressif's esp_lcd /
// esp_lvgl_port components. Pin numbers and controller registers are hardware facts; no
// vendor example code is used.
//
//   I2C0  SDA 8, SCL 7 (internal pull-ups) - PCF8574 expander 0x38, CST826 touch 0x15
//   SPI2  SCLK 5, MOSI 1, DC 3, CS 2       - ST7789 panel, native 240x320 portrait
//   GPIO6 backlight (LEDC PWM)
//   PCF8574 outputs: P0 amp enable (low = muted), P1 RTC INT, P2 CHG#, P3 camera PWDN,
//                    P4 touch INT, P5 LCD reset (low = reset), P6 microSD CS, P7 unused

#include "board.h"

#include "driver/i2c_master.h"
#include "driver/ledc.h"
#include "driver/spi_master.h"
#include "esp_check.h"
#include "esp_lcd_panel_io.h"
#include "esp_lcd_panel_ops.h"
#include "esp_lcd_panel_vendor.h"
#include "esp_log.h"
#include "esp_lvgl_port.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "lvgl.h"

static const char *TAG = "board_onx";

#define I2C_SDA 8
#define I2C_SCL 7

#define EXPANDER_ADDR       0x38
#define EXPANDER_AMP_ENABLE (1u << 0)
#define EXPANDER_LCD_RESET  (1u << 5)

#define TOUCH_ADDR          0x15
#define TOUCH_REG_POINTS    0x02 // finger count, then X high/low, Y high/low
#define TOUCH_REG_CHIP_ID   0xAA

#define LCD_SPI_HOST  SPI2_HOST
#define LCD_SCLK      5
#define LCD_MOSI      1
#define LCD_DC        3
#define LCD_CS        2
#define LCD_PCLK_HZ   (80 * 1000 * 1000)
#define LCD_NATIVE_W  240 // the panel is portrait; LVGL rotates to landscape in software
#define LCD_NATIVE_H  320

#define BACKLIGHT_GPIO     6
#define BACKLIGHT_TIMER    LEDC_TIMER_1
#define BACKLIGHT_CHANNEL  LEDC_CHANNEL_0
#define BACKLIGHT_BITS     LEDC_TIMER_10_BIT

static const board_info_t info = {
    .id = "onx2432g028",
    .name = "ONX2432G028",
    .width = 320,
    .height = 240,
    .has_speaker = true,
    .has_native_usb = false, // USB-C goes only to the CH340K serial chip
};

static i2c_master_bus_handle_t i2c_bus;
static i2c_master_dev_handle_t expander;
static i2c_master_dev_handle_t touch;
static uint8_t expander_state = 0xFF;

const board_info_t *board_info(void)
{
    return &info;
}

static esp_err_t expander_write(uint8_t state)
{
    expander_state = state;
    return i2c_master_transmit(expander, &state, 1, 50);
}

static esp_err_t i2c_init(void)
{
    const i2c_master_bus_config_t bus_config = {
        .i2c_port = I2C_NUM_0,
        .sda_io_num = I2C_SDA,
        .scl_io_num = I2C_SCL,
        .clk_source = I2C_CLK_SRC_DEFAULT,
        .glitch_ignore_cnt = 7,
        .flags.enable_internal_pullup = true,
    };
    ESP_RETURN_ON_ERROR(i2c_new_master_bus(&bus_config, &i2c_bus), TAG, "i2c bus");

    const i2c_device_config_t expander_config = {
        .dev_addr_length = I2C_ADDR_BIT_LEN_7,
        .device_address = EXPANDER_ADDR,
        .scl_speed_hz = 100 * 1000,
    };
    ESP_RETURN_ON_ERROR(i2c_master_bus_add_device(i2c_bus, &expander_config, &expander),
                        TAG, "expander");

    const i2c_device_config_t touch_config = {
        .dev_addr_length = I2C_ADDR_BIT_LEN_7,
        .device_address = TOUCH_ADDR,
        .scl_speed_hz = 400 * 1000,
    };
    return i2c_master_bus_add_device(i2c_bus, &touch_config, &touch);
}

// All expander outputs high except the amplifier, which stays muted until audio needs it.
// Then pulse the panel's reset line.
static esp_err_t expander_init(void)
{
    ESP_RETURN_ON_ERROR(expander_write(0xFF & ~EXPANDER_AMP_ENABLE), TAG, "expander write");
    ESP_RETURN_ON_ERROR(expander_write(expander_state & ~EXPANDER_LCD_RESET), TAG, "lcd reset");
    vTaskDelay(pdMS_TO_TICKS(20));
    ESP_RETURN_ON_ERROR(expander_write(expander_state | EXPANDER_LCD_RESET), TAG, "lcd reset");
    vTaskDelay(pdMS_TO_TICKS(120));
    return ESP_OK;
}

static esp_err_t display_init(esp_lcd_panel_io_handle_t *io_out, esp_lcd_panel_handle_t *panel_out)
{
    const spi_bus_config_t bus_config = {
        .sclk_io_num = LCD_SCLK,
        .mosi_io_num = LCD_MOSI,
        .miso_io_num = -1,
        .quadwp_io_num = -1,
        .quadhd_io_num = -1,
        .max_transfer_sz = LCD_NATIVE_W * 40 * sizeof(uint16_t),
    };
    ESP_RETURN_ON_ERROR(spi_bus_initialize(LCD_SPI_HOST, &bus_config, SPI_DMA_CH_AUTO), TAG, "spi");

    const esp_lcd_panel_io_spi_config_t io_config = {
        .dc_gpio_num = LCD_DC,
        .cs_gpio_num = LCD_CS,
        .pclk_hz = LCD_PCLK_HZ,
        .lcd_cmd_bits = 8,
        .lcd_param_bits = 8,
        .spi_mode = 0,
        .trans_queue_depth = 10,
    };
    ESP_RETURN_ON_ERROR(esp_lcd_new_panel_io_spi((esp_lcd_spi_bus_handle_t)LCD_SPI_HOST,
                                                 &io_config, io_out), TAG, "panel io");

    const esp_lcd_panel_dev_config_t panel_config = {
        .reset_gpio_num = -1, // reset is on the expander, pulsed in expander_init()
        .rgb_ele_order = LCD_RGB_ELEMENT_ORDER_RGB,
        .bits_per_pixel = 16,
    };
    ESP_RETURN_ON_ERROR(esp_lcd_new_panel_st7789(*io_out, &panel_config, panel_out), TAG, "st7789");
    ESP_RETURN_ON_ERROR(esp_lcd_panel_reset(*panel_out), TAG, "panel reset");
    ESP_RETURN_ON_ERROR(esp_lcd_panel_init(*panel_out), TAG, "panel init");
    ESP_RETURN_ON_ERROR(esp_lcd_panel_invert_color(*panel_out, false), TAG, "invert");
    ESP_RETURN_ON_ERROR(esp_lcd_panel_swap_xy(*panel_out, false), TAG, "swap");
    ESP_RETURN_ON_ERROR(esp_lcd_panel_mirror(*panel_out, false, true), TAG, "mirror");
    return esp_lcd_panel_disp_on_off(*panel_out, true);
}

static esp_err_t backlight_init(void)
{
    const ledc_timer_config_t timer = {
        .speed_mode = LEDC_LOW_SPEED_MODE,
        .timer_num = BACKLIGHT_TIMER,
        .duty_resolution = BACKLIGHT_BITS,
        .freq_hz = 10000,
        .clk_cfg = LEDC_AUTO_CLK,
    };
    ESP_RETURN_ON_ERROR(ledc_timer_config(&timer), TAG, "backlight timer");

    const ledc_channel_config_t channel = {
        .speed_mode = LEDC_LOW_SPEED_MODE,
        .channel = BACKLIGHT_CHANNEL,
        .timer_sel = BACKLIGHT_TIMER,
        .gpio_num = BACKLIGHT_GPIO,
        .duty = 0,
    };
    return ledc_channel_config(&channel);
}

void board_set_backlight(uint8_t percent)
{
    if (percent > 100) {
        percent = 100;
    }
    const uint32_t max_duty = (1u << BACKLIGHT_BITS) - 1;
    ledc_set_duty(LEDC_LOW_SPEED_MODE, BACKLIGHT_CHANNEL, max_duty * percent / 100);
    ledc_update_duty(LEDC_LOW_SPEED_MODE, BACKLIGHT_CHANNEL);
}

// LVGL polls this every LV_INDEV_DEF_READ_PERIOD. Coordinates are reported in the panel's
// native portrait frame; LVGL applies the display rotation.
static void touch_read(lv_indev_drv_t *drv, lv_indev_data_t *data)
{
    uint8_t reg = TOUCH_REG_POINTS;
    uint8_t buf[5];

    data->state = LV_INDEV_STATE_REL;
    if (i2c_master_transmit_receive(touch, &reg, 1, buf, sizeof(buf), 20) != ESP_OK) {
        return;
    }
    if ((buf[0] & 0x0F) == 0) {
        return;
    }
    uint16_t x = ((buf[1] & 0x0F) << 8) | buf[2];
    uint16_t y = ((buf[3] & 0x0F) << 8) | buf[4];
    if (x >= LCD_NATIVE_W || y >= LCD_NATIVE_H) {
        return;
    }
    data->point.x = x;
    data->point.y = y;
    data->state = LV_INDEV_STATE_PR;
}

static void touch_log_chip_id(void)
{
    uint8_t reg = TOUCH_REG_CHIP_ID;
    uint8_t id = 0;
    esp_err_t err = i2c_master_transmit_receive(touch, &reg, 1, &id, 1, 50);
    if (err == ESP_OK) {
        ESP_LOGI(TAG, "touch controller id 0x%02x", id);
    } else {
        ESP_LOGW(TAG, "touch controller not answering: %s", esp_err_to_name(err));
    }
}

static esp_err_t lvgl_init(esp_lcd_panel_io_handle_t io, esp_lcd_panel_handle_t panel)
{
    const lvgl_port_cfg_t port_config = ESP_LVGL_PORT_INIT_CONFIG();
    ESP_RETURN_ON_ERROR(lvgl_port_init(&port_config), TAG, "lvgl port");

    const lvgl_port_display_cfg_t display_config = {
        .io_handle = io,
        .panel_handle = panel,
        .buffer_size = LCD_NATIVE_W * LCD_NATIVE_H / 8,
        .double_buffer = true,
        .hres = LCD_NATIVE_W,
        .vres = LCD_NATIVE_H,
        .flags = {
            .buff_spiram = true,
            .sw_rotate = true,
        },
    };
    lv_disp_t *display = lvgl_port_add_disp(&display_config);
    ESP_RETURN_ON_FALSE(display != NULL, ESP_FAIL, TAG, "lvgl display");

    ESP_RETURN_ON_FALSE(lvgl_port_lock(0), ESP_FAIL, TAG, "lvgl lock");
    lv_disp_set_rotation(display, LV_DISP_ROT_90);
    static lv_indev_drv_t touch_driver;
    lv_indev_drv_init(&touch_driver);
    touch_driver.type = LV_INDEV_TYPE_POINTER;
    touch_driver.disp = display;
    touch_driver.read_cb = touch_read;
    lv_indev_t *indev = lv_indev_drv_register(&touch_driver);
    lvgl_port_unlock();
    ESP_RETURN_ON_FALSE(indev != NULL, ESP_FAIL, TAG, "lvgl touch");
    return ESP_OK;
}

esp_err_t board_init(void)
{
    esp_lcd_panel_io_handle_t io = NULL;
    esp_lcd_panel_handle_t panel = NULL;

    ESP_RETURN_ON_ERROR(i2c_init(), TAG, "i2c");
    ESP_RETURN_ON_ERROR(expander_init(), TAG, "expander");
    ESP_RETURN_ON_ERROR(backlight_init(), TAG, "backlight");
    ESP_RETURN_ON_ERROR(display_init(&io, &panel), TAG, "display");
    touch_log_chip_id();
    ESP_RETURN_ON_ERROR(lvgl_init(io, panel), TAG, "lvgl");
    return ESP_OK;
}

// Universal board support: one firmware image, driven by a board definition (JSON) stored in the
// "board" flash partition. Schema: spec/schemas/board.schema.json; examples:
// firmware/boards/definitions/. Written from schematics, datasheets and Espressif's esp_lcd /
// esp_lvgl_port; no board-vendor example code.
//
// Without a valid definition the board still boots "headless": Bluetooth, serial and Wi-Fi run,
// board_init() returns an error, and the companion can write a definition.

#include "board.h"

#include <stdlib.h>
#include <string.h>

#include "cJSON.h"
#include "driver/gpio.h"
#include "driver/i2s_pdm.h"
#include "hal/usb_serial_jtag_ll.h"
#include "driver/i2c_master.h"
#include "driver/ledc.h"
#include "driver/spi_master.h"
#include "esp_check.h"
#include "esp_lcd_ili9341.h"
#include "esp_lcd_sh8601.h"
#include "esp_lcd_panel_io.h"
#include "esp_lcd_panel_ops.h"
#include "esp_lcd_panel_vendor.h"
#include <inttypes.h>

#include "esp_log.h"
#include "esp_timer.h"
#include "esp_lvgl_port.h"
#include "esp_partition.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "lvgl.h"

static const char *TAG = "board";

#define DEFINITION_SUBTYPE 0x40
#define DEFINITION_MAX     4096
#define TOUCH_REG_POINTS   0x02 // FocalTech layout (CST816, CST826, FT6336, FT5x06): count, then X high/low, Y high/low
#define GT911_REG_STATUS   0x814E // bit 7: new data ready; bits 0-3: number of points
#define GT911_REG_POINT1   0x8150 // X low, X high, Y low, Y high
#define MAX_INIT_CMDS      32
#define MAX_INIT_BYTES     16

#define BACKLIGHT_TIMER   LEDC_TIMER_1
#define BACKLIGHT_CHANNEL LEDC_CHANNEL_0
#define BACKLIGHT_BITS    LEDC_TIMER_10_BIT

// A control line is either a GPIO or a bit on the I/O expander.
typedef struct {
    int gpio;         // -1 if none
    int expander_bit; // -1 if none
} line_t;

typedef enum { TOUCH_FOCALTECH, TOUCH_GT911 } touch_kind_t;

typedef struct {
    uint8_t cmd;
    uint8_t len;
    uint8_t data[MAX_INIT_BYTES];
    uint16_t delay_ms;
} init_cmd_t;

typedef struct {
    char id[33];
    char name[65];
    board_serial_t serial;
    bool native_usb, speaker;
    int sda, scl;
    bool pullup;
    bool has_expander;
    int expander_address, expander_initial;
    char display_driver[16];
    int sclk, mosi, dc, cs;
    bool qspi;              // four data lines (AMOLED panels: SH8601, CO5300); commands are framed
    int data[4];            // QSPI data lines 0-3
    int x_gap, y_gap;       // where the visible area starts in the panel's memory
    bool even_areas;        // the panel only takes updates that start and end on even pixels
    bool dcs_brightness;    // no backlight: brightness is the panel's own command (0x51)
    int corner_radius;      // a screen with rounded corners (most AMOLED watches), px
    line_t lcd_reset;
    bool reset_active_high;
    init_cmd_t init_cmds[MAX_INIT_CMDS];
    int init_count;
    int width, height, pclk_hz;
    bool bgr, invert, mirror_x, mirror_y, swap_xy;
    int rotation;
    int backlight_gpio;
    bool backlight_active_low;
    char touch_driver[16];
    touch_kind_t touch_kind;
    int touch_address;
    line_t touch_reset;
    int touch_max_x, touch_max_y;
    bool touch_swap_xy, touch_mirror_x, touch_mirror_y;
    int mic_clk, mic_data;  // a PDM microphone; -1 when there is none
    bool mic_right;         // the microphone answers in the right slot
    bool mic_slot_fixed;    // the definition names the slot; otherwise the louder one is used
} definition_t;

static definition_t def;
static board_info_t info = { .id = "unknown", .name = "No board definition" };
static bool backlight_ready;
static i2c_master_bus_handle_t i2c_bus;
static i2c_master_dev_handle_t expander, touch;
static uint8_t expander_state;

const board_info_t *board_info(void)
{
    return &info;
}

// ---- reading and checking the definition ----

static bool pin_ok(int pin)
{
    // -1 = not connected. 22-25 do not exist on the S3; 26-32 are the SPI flash, 33-37 the octal PSRAM.
    return pin == -1 || (pin >= 0 && pin <= 48 && !(pin >= 22 && pin <= 37));
}

static int get_int(const cJSON *object, const char *name, int fallback)
{
    const cJSON *item = cJSON_GetObjectItemCaseSensitive(object, name);
    return cJSON_IsNumber(item) ? item->valueint : fallback;
}

static bool get_bool(const cJSON *object, const char *name)
{
    return cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(object, name));
}

static void get_str(const cJSON *object, const char *name, char *out, size_t size)
{
    const char *value = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(object, name));
    snprintf(out, size, "%s", value ? value : "");
}

static line_t get_line(const cJSON *object, const char *name)
{
    const cJSON *item = cJSON_GetObjectItemCaseSensitive(object, name);
    if (cJSON_IsNumber(item)) {
        return (line_t){ .gpio = item->valueint, .expander_bit = -1 };
    }
    if (cJSON_IsObject(item)) {
        return (line_t){ .gpio = -1, .expander_bit = get_int(item, "expander_bit", -1) };
    }
    return (line_t){ .gpio = -1, .expander_bit = -1 };
}

static esp_err_t parse(const char *text)
{
    cJSON *root = cJSON_Parse(text);
    ESP_RETURN_ON_FALSE(root != NULL, ESP_ERR_INVALID_ARG, TAG, "definition is not JSON");
    const cJSON *i2c = cJSON_GetObjectItemCaseSensitive(root, "i2c");
    const cJSON *expander_json = cJSON_GetObjectItemCaseSensitive(root, "expander");
    const cJSON *display = cJSON_GetObjectItemCaseSensitive(root, "display");
    const cJSON *backlight = cJSON_GetObjectItemCaseSensitive(display, "backlight");
    const cJSON *touch_json = cJSON_GetObjectItemCaseSensitive(root, "touch");
    const cJSON *mic_json = cJSON_GetObjectItemCaseSensitive(root, "mic");
    char serial[24], mic_driver[8], mic_slot[8];

    memset(&def, 0, sizeof(def));
    get_str(root, "id", def.id, sizeof(def.id));
    get_str(root, "name", def.name, sizeof(def.name));
    get_str(root, "serial", serial, sizeof(serial));
    def.serial = strcmp(serial, "usb-serial-jtag") == 0 ? BOARD_SERIAL_USB_SERIAL_JTAG : BOARD_SERIAL_UART0;
    def.native_usb = get_bool(root, "native_usb");
    def.speaker = get_bool(root, "speaker");
    def.sda = get_int(i2c, "sda", -1);
    def.scl = get_int(i2c, "scl", -1);
    def.pullup = get_bool(i2c, "pullup");
    def.has_expander = cJSON_IsObject(expander_json);
    def.expander_address = get_int(expander_json, "address", 0);
    def.expander_initial = get_int(expander_json, "initial", 0xFF);
    get_str(display, "driver", def.display_driver, sizeof(def.display_driver));
    def.sclk = get_int(display, "sclk", -1);
    def.mosi = get_int(display, "mosi", -1);
    def.dc = get_int(display, "dc", -1);
    def.cs = get_int(display, "cs", -1);
    def.lcd_reset = get_line(display, "reset");
    char bus[8];
    get_str(display, "bus", bus, sizeof(bus));
    def.qspi = strcmp(bus, "qspi") == 0;
    const cJSON *data_pins = cJSON_GetObjectItemCaseSensitive(display, "data");
    for (int i = 0; i < 4; i++) {
        const cJSON *pin = cJSON_GetArrayItem(data_pins, i);
        def.data[i] = cJSON_IsNumber(pin) ? pin->valueint : -1;
    }
    def.x_gap = get_int(display, "x_gap", 0);
    def.corner_radius = get_int(display, "corner_radius", 0);
    def.y_gap = get_int(display, "y_gap", 0);
    def.even_areas = get_bool(display, "even_areas");
    def.dcs_brightness = strcmp(cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(display, "brightness")) ?: "", "panel") == 0;
    def.reset_active_high = get_bool(display, "reset_active_high");
    const cJSON *cmds = cJSON_GetObjectItemCaseSensitive(display, "init_cmds");
    const cJSON *cmd;
    cJSON_ArrayForEach(cmd, cmds) {
        if (def.init_count >= MAX_INIT_CMDS) {
            break;
        }
        init_cmd_t *c = &def.init_cmds[def.init_count++];
        c->cmd = (uint8_t)get_int(cmd, "cmd", 0);
        c->delay_ms = (uint16_t)get_int(cmd, "delay_ms", 0);
        const cJSON *byte;
        cJSON_ArrayForEach(byte, cJSON_GetObjectItemCaseSensitive(cmd, "data")) {
            if (c->len < MAX_INIT_BYTES && cJSON_IsNumber(byte)) {
                c->data[c->len++] = (uint8_t)byte->valueint;
            }
        }
    }
    def.width = get_int(display, "width", 0);
    def.height = get_int(display, "height", 0);
    def.pclk_hz = get_int(display, "pclk_hz", 40000000);
    def.bgr = get_bool(display, "bgr");
    def.invert = get_bool(display, "invert");
    def.mirror_x = get_bool(display, "mirror_x");
    def.mirror_y = get_bool(display, "mirror_y");
    def.swap_xy = get_bool(display, "swap_xy");
    def.rotation = get_int(display, "rotation", 0);
    def.backlight_gpio = get_int(backlight, "gpio", -1);
    def.backlight_active_low = get_bool(backlight, "active_low");
    get_str(touch_json, "driver", def.touch_driver, sizeof(def.touch_driver));
    def.touch_address = get_int(touch_json, "address", 0);
    def.touch_reset = get_line(touch_json, "reset");
    def.touch_max_x = get_int(touch_json, "max_x", def.width);
    def.touch_max_y = get_int(touch_json, "max_y", def.height);
    def.touch_swap_xy = get_bool(touch_json, "swap_xy");
    def.touch_mirror_x = get_bool(touch_json, "mirror_x");
    def.touch_mirror_y = get_bool(touch_json, "mirror_y");
    get_str(mic_json, "driver", mic_driver, sizeof(mic_driver));
    get_str(mic_json, "slot", mic_slot, sizeof(mic_slot));
    const bool pdm = strcmp(mic_driver, "pdm") == 0;
    def.mic_clk = pdm ? get_int(mic_json, "clk", -1) : -1;
    def.mic_data = pdm ? get_int(mic_json, "data", -1) : -1;
    def.mic_right = strcmp(mic_slot, "right") == 0;
    def.mic_slot_fixed = mic_slot[0] != '\0';
    if (cJSON_IsObject(mic_json) && !pdm) {
        ESP_LOGW(TAG, "microphone driver \"%s\" not supported; running without one", mic_driver);
    }
    cJSON_Delete(root);

    // The schema is checked by the companion before writing; this repeats what could harm the
    // board or the chip if it were wrong.
    const int pins[] = { def.sda, def.scl, def.sclk, def.mosi, def.dc, def.cs, def.backlight_gpio,
                         def.lcd_reset.gpio, def.touch_reset.gpio, def.mic_clk, def.mic_data,
                         def.data[0], def.data[1], def.data[2], def.data[3] };
    for (size_t i = 0; i < sizeof(pins) / sizeof(pins[0]); i++) {
        ESP_RETURN_ON_FALSE(pin_ok(pins[i]), ESP_ERR_INVALID_ARG, TAG, "GPIO %d is not usable on the ESP32-S3", pins[i]);
    }
    ESP_RETURN_ON_FALSE(def.id[0] != '\0' && def.width >= 64 && def.height >= 64, ESP_ERR_INVALID_ARG, TAG, "incomplete definition");
    const bool amoled = strcmp(def.display_driver, "sh8601") == 0;
    ESP_RETURN_ON_FALSE(strcmp(def.display_driver, "st7789") == 0 || strcmp(def.display_driver, "ili9341") == 0 || amoled,
                        ESP_ERR_NOT_SUPPORTED, TAG, "display driver \"%s\" not supported", def.display_driver);
    ESP_RETURN_ON_FALSE(!amoled || (def.qspi && def.data[0] >= 0 && def.data[1] >= 0 && def.data[2] >= 0 && def.data[3] >= 0),
                        ESP_ERR_INVALID_ARG, TAG, "sh8601 needs the qspi bus and four data pins");
    if (strcmp(def.touch_driver, "gt911") == 0) {
        def.touch_kind = TOUCH_GT911;
    } else if (strcmp(def.touch_driver, "cst826") == 0 || strcmp(def.touch_driver, "cst816") == 0 ||
               strcmp(def.touch_driver, "ft6336") == 0 || strcmp(def.touch_driver, "ft5x06") == 0) {
        def.touch_kind = TOUCH_FOCALTECH;
    } else {
        ESP_LOGE(TAG, "touch driver \"%s\" not supported", def.touch_driver);
        return ESP_ERR_NOT_SUPPORTED;
    }
    ESP_RETURN_ON_FALSE(def.rotation % 90 == 0 && def.rotation >= 0 && def.rotation < 360, ESP_ERR_INVALID_ARG, TAG, "rotation");
    return ESP_OK;
}

static esp_err_t load_definition(void)
{
    const esp_partition_t *partition = esp_partition_find_first(ESP_PARTITION_TYPE_DATA, DEFINITION_SUBTYPE, "board");
    ESP_RETURN_ON_FALSE(partition != NULL, ESP_ERR_NOT_FOUND, TAG, "no \"board\" partition");
    char *text = calloc(1, DEFINITION_MAX + 1);
    ESP_RETURN_ON_FALSE(text != NULL, ESP_ERR_NO_MEM, TAG, "memory");
    esp_err_t err = esp_partition_read(partition, 0, text, DEFINITION_MAX);
    if (err == ESP_OK) {
        // Erased flash reads 0xFF: the definition ends at the first 0xFF or NUL.
        for (int i = 0; i < DEFINITION_MAX; i++) {
            if ((unsigned char)text[i] == 0xFF || text[i] == '\0') {
                text[i] = '\0';
                break;
            }
        }
        err = text[0] == '{' ? parse(text) : ESP_ERR_NOT_FOUND;
    }
    free(text);
    return err;
}

// ---- hardware ----

static esp_err_t expander_write(uint8_t state)
{
    expander_state = state;
    return i2c_master_transmit(expander, &state, 1, 50);
}

static esp_err_t set_line(line_t line, int level)
{
    if (line.gpio >= 0) {
        return gpio_set_level(line.gpio, level);
    }
    if (line.expander_bit >= 0 && expander != NULL) {
        const uint8_t bit = 1u << line.expander_bit;
        return expander_write(level ? (expander_state | bit) : (expander_state & ~bit));
    }
    return ESP_OK;
}

static esp_err_t pulse_reset(line_t line, bool active_high)
{
    if (line.gpio < 0 && line.expander_bit < 0) {
        return ESP_OK;
    }
    if (line.gpio >= 0) {
        const gpio_config_t config = { .pin_bit_mask = 1ULL << line.gpio, .mode = GPIO_MODE_OUTPUT };
        ESP_RETURN_ON_ERROR(gpio_config(&config), TAG, "reset gpio");
    }
    ESP_RETURN_ON_ERROR(set_line(line, active_high ? 1 : 0), TAG, "reset assert");
    vTaskDelay(pdMS_TO_TICKS(20));
    ESP_RETURN_ON_ERROR(set_line(line, active_high ? 0 : 1), TAG, "reset release");
    vTaskDelay(pdMS_TO_TICKS(120));
    return ESP_OK;
}

static esp_err_t i2c_init(void)
{
    if (def.sda < 0 || def.scl < 0) {
        return ESP_OK;
    }
    const i2c_master_bus_config_t bus = {
        .i2c_port = I2C_NUM_0, .sda_io_num = def.sda, .scl_io_num = def.scl,
        .clk_source = I2C_CLK_SRC_DEFAULT, .glitch_ignore_cnt = 7, .flags.enable_internal_pullup = def.pullup,
    };
    ESP_RETURN_ON_ERROR(i2c_new_master_bus(&bus, &i2c_bus), TAG, "i2c bus");
    if (def.has_expander) {
        const i2c_device_config_t config = { .dev_addr_length = I2C_ADDR_BIT_LEN_7, .device_address = def.expander_address, .scl_speed_hz = 100000 };
        ESP_RETURN_ON_ERROR(i2c_master_bus_add_device(i2c_bus, &config, &expander), TAG, "expander");
        ESP_RETURN_ON_ERROR(expander_write((uint8_t)def.expander_initial), TAG, "expander write");
    }
    return ESP_OK;
}

// Touch is attached after the display has started: on some boards (S3-BOX-3) the display reset
// also resets the touch controller, and a GT911 picks its address (0x5D or 0x14) at reset.
static esp_err_t touch_attach(void)
{
    ESP_RETURN_ON_FALSE(i2c_bus != NULL, ESP_ERR_INVALID_STATE, TAG, "touch needs the I2C pins in the definition");
    ESP_RETURN_ON_ERROR(pulse_reset(def.touch_reset, false), TAG, "touch reset");
    int address = def.touch_address;
    if (def.touch_kind == TOUCH_GT911 && i2c_master_probe(i2c_bus, address, 50) != ESP_OK) {
        address = address == 0x5D ? 0x14 : 0x5D;
    }
    ESP_RETURN_ON_ERROR(i2c_master_probe(i2c_bus, address, 50), TAG, "no touch controller answers at 0x%02x", address);
    const i2c_device_config_t config = { .dev_addr_length = I2C_ADDR_BIT_LEN_7, .device_address = address, .scl_speed_hz = 400000 };
    return i2c_master_bus_add_device(i2c_bus, &config, &touch);
}

// LVGL draws into buffers in PSRAM; each update is copied into internal memory for the SPI
// transfer, so a buffer must fit what is free there: an eighth of the screen, at most 12000 pixels
// (24 KB). A 410x502 AMOLED's eighth (51 KB) failed every update ("spi transmit color failed").
#define DRAW_BUFFER_MAX_PIXELS 12000
static int draw_buffer_pixels(void)
{
    const int eighth = def.width * def.height / 8;
    return eighth < DRAW_BUFFER_MAX_PIXELS ? eighth : DRAW_BUFFER_MAX_PIXELS;
}

static esp_lcd_panel_io_handle_t panel_io;
static sh8601_lcd_init_cmd_t amoled_cmds[MAX_INIT_CMDS];

// An AMOLED on four data lines (SH8601 and CO5300, which takes the same commands), with Espressif's
// esp_lcd_sh8601 (Apache-2.0). Its start-up commands come from the definition: the panel's driver
// sends them framed for QSPI, which a plain command write here would not be.
static esp_err_t amoled_init(esp_lcd_panel_io_handle_t *io, esp_lcd_panel_handle_t *panel)
{
    const spi_bus_config_t bus = SH8601_PANEL_BUS_QSPI_CONFIG(def.sclk, def.data[0], def.data[1], def.data[2], def.data[3],
                                                              (draw_buffer_pixels() + 2 * def.width) * sizeof(uint16_t));
    ESP_RETURN_ON_ERROR(spi_bus_initialize(SPI2_HOST, &bus, SPI_DMA_CH_AUTO), TAG, "qspi");
    esp_lcd_panel_io_spi_config_t io_config = SH8601_PANEL_IO_QSPI_CONFIG(def.cs, NULL, NULL);
    io_config.pclk_hz = def.pclk_hz;
    ESP_RETURN_ON_ERROR(esp_lcd_new_panel_io_spi((esp_lcd_spi_bus_handle_t)SPI2_HOST, &io_config, io), TAG, "panel io");
    for (int i = 0; i < def.init_count; i++) {
        amoled_cmds[i] = (sh8601_lcd_init_cmd_t){ .cmd = def.init_cmds[i].cmd, .data = def.init_cmds[i].data,
                                                  .data_bytes = def.init_cmds[i].len, .delay_ms = def.init_cmds[i].delay_ms };
    }
    sh8601_vendor_config_t vendor = { .init_cmds = def.init_count ? amoled_cmds : NULL, .init_cmds_size = def.init_count,
                                      .flags = { .use_qspi_interface = 1 } };
    const esp_lcd_panel_dev_config_t panel_config = {
        .reset_gpio_num = -1, // pulsed by pulse_reset()
        .rgb_ele_order = def.bgr ? LCD_RGB_ELEMENT_ORDER_BGR : LCD_RGB_ELEMENT_ORDER_RGB,
        .bits_per_pixel = 16,
        .vendor_config = &vendor,
    };
    ESP_RETURN_ON_ERROR(pulse_reset(def.lcd_reset, def.reset_active_high), TAG, "lcd reset");
    ESP_RETURN_ON_ERROR(esp_lcd_new_panel_sh8601(*io, &panel_config, panel), TAG, "sh8601");
    ESP_RETURN_ON_ERROR(esp_lcd_panel_reset(*panel), TAG, "panel reset");
    ESP_RETURN_ON_ERROR(esp_lcd_panel_init(*panel), TAG, "panel init");
    ESP_RETURN_ON_ERROR(esp_lcd_panel_set_gap(*panel, def.x_gap, def.y_gap), TAG, "gap");
    if (def.invert) {
        ESP_RETURN_ON_ERROR(esp_lcd_panel_invert_color(*panel, true), TAG, "invert");
    }
    if (def.mirror_x) {
        ESP_RETURN_ON_ERROR(esp_lcd_panel_mirror(*panel, true, false), TAG, "mirror");
    }
    return esp_lcd_panel_disp_on_off(*panel, true);
}

static esp_err_t display_init(esp_lcd_panel_io_handle_t *io, esp_lcd_panel_handle_t *panel)
{
    if (def.qspi) {
        return amoled_init(io, panel);
    }
    const spi_bus_config_t bus = {
        .sclk_io_num = def.sclk, .mosi_io_num = def.mosi, .miso_io_num = -1, .quadwp_io_num = -1, .quadhd_io_num = -1,
        .max_transfer_sz = def.width * 40 * sizeof(uint16_t),
    };
    ESP_RETURN_ON_ERROR(spi_bus_initialize(SPI2_HOST, &bus, SPI_DMA_CH_AUTO), TAG, "spi");
    const esp_lcd_panel_io_spi_config_t io_config = {
        .dc_gpio_num = def.dc, .cs_gpio_num = def.cs, .pclk_hz = def.pclk_hz,
        .lcd_cmd_bits = 8, .lcd_param_bits = 8, .spi_mode = 0, .trans_queue_depth = 10,
    };
    ESP_RETURN_ON_ERROR(esp_lcd_new_panel_io_spi((esp_lcd_spi_bus_handle_t)SPI2_HOST, &io_config, io), TAG, "panel io");
    const esp_lcd_panel_dev_config_t panel_config = {
        .reset_gpio_num = -1, // reset pulsed by pulse_reset(), which also handles expander lines
        .rgb_ele_order = def.bgr ? LCD_RGB_ELEMENT_ORDER_BGR : LCD_RGB_ELEMENT_ORDER_RGB,
        .bits_per_pixel = 16,
    };
    ESP_RETURN_ON_ERROR(pulse_reset(def.lcd_reset, def.reset_active_high), TAG, "lcd reset");
    if (strcmp(def.display_driver, "ili9341") == 0) {
        ESP_RETURN_ON_ERROR(esp_lcd_new_panel_ili9341(*io, &panel_config, panel), TAG, "ili9341");
    } else {
        ESP_RETURN_ON_ERROR(esp_lcd_new_panel_st7789(*io, &panel_config, panel), TAG, "st7789");
    }
    ESP_RETURN_ON_ERROR(esp_lcd_panel_reset(*panel), TAG, "panel reset");
    ESP_RETURN_ON_ERROR(esp_lcd_panel_init(*panel), TAG, "panel init");
    // Board-specific registers (gamma, power, timing) after the driver's own start-up, and before
    // orientation, which rewrites the address-mode register.
    for (int i = 0; i < def.init_count; i++) {
        const init_cmd_t *c = &def.init_cmds[i];
        ESP_RETURN_ON_ERROR(esp_lcd_panel_io_tx_param(*io, c->cmd, c->len ? c->data : NULL, c->len), TAG, "init cmd 0x%02x", c->cmd);
        if (c->delay_ms) {
            vTaskDelay(pdMS_TO_TICKS(c->delay_ms));
        }
    }
    ESP_RETURN_ON_ERROR(esp_lcd_panel_invert_color(*panel, def.invert), TAG, "invert");
    ESP_RETURN_ON_ERROR(esp_lcd_panel_swap_xy(*panel, def.swap_xy), TAG, "swap");
    ESP_RETURN_ON_ERROR(esp_lcd_panel_mirror(*panel, def.mirror_x, def.mirror_y), TAG, "mirror");
    return esp_lcd_panel_disp_on_off(*panel, true);
}

static esp_err_t backlight_init(void)
{
    if (def.dcs_brightness) {
        backlight_ready = true; // the panel's own brightness, set once the panel is up
        return ESP_OK;
    }
    if (def.backlight_gpio < 0) {
        return ESP_OK;
    }
    const ledc_timer_config_t timer = {
        .speed_mode = LEDC_LOW_SPEED_MODE, .timer_num = BACKLIGHT_TIMER, .duty_resolution = BACKLIGHT_BITS,
        .freq_hz = 10000, .clk_cfg = LEDC_AUTO_CLK,
    };
    ESP_RETURN_ON_ERROR(ledc_timer_config(&timer), TAG, "backlight timer");
    const ledc_channel_config_t channel = {
        .speed_mode = LEDC_LOW_SPEED_MODE, .channel = BACKLIGHT_CHANNEL, .timer_sel = BACKLIGHT_TIMER,
        .gpio_num = def.backlight_gpio, .duty = 0, .flags.output_invert = def.backlight_active_low,
    };
    ESP_RETURN_ON_ERROR(ledc_channel_config(&channel), TAG, "backlight channel");
    backlight_ready = true;
    return ESP_OK;
}

void board_set_backlight(uint8_t percent)
{
    if (!backlight_ready) {
        return;
    }
    if (percent > 100) {
        percent = 100;
    }
    if (def.dcs_brightness) {
        // Write Display Brightness (0x51), framed for QSPI as the panel's driver frames commands.
        if (panel_io != NULL) {
            const uint8_t level = (uint8_t)(percent * 255 / 100);
            esp_lcd_panel_io_tx_param(panel_io, (0x02 << 24) | (0x51 << 8), &level, 1);
        }
        return;
    }
    const uint32_t max_duty = (1u << BACKLIGHT_BITS) - 1;
    ledc_set_duty(LEDC_LOW_SPEED_MODE, BACKLIGHT_CHANNEL, max_duty * percent / 100);
    ledc_update_duty(LEDC_LOW_SPEED_MODE, BACKLIGHT_CHANNEL);
}

// GT911 (from its datasheet's register map): new frames are flagged in 0x814E and must be
// acknowledged by writing 0 back. Between frames the last state is kept, so a finger held down
// does not flicker between pressed and released.
static bool gt911_read(int *x, int *y)
{
    static bool pressed;
    static int last_x, last_y;
    uint8_t reg[2] = { GT911_REG_STATUS >> 8, GT911_REG_STATUS & 0xFF };
    uint8_t status = 0;
    if (touch == NULL) {
        return false;
    }
    if (i2c_master_transmit_receive(touch, reg, 2, &status, 1, 20) != ESP_OK) {
        return false;
    }
    if (status & 0x80) {
        pressed = (status & 0x0F) > 0;
        if (pressed) {
            uint8_t point_reg[2] = { GT911_REG_POINT1 >> 8, GT911_REG_POINT1 & 0xFF };
            uint8_t p[4];
            if (i2c_master_transmit_receive(touch, point_reg, 2, p, sizeof(p), 20) == ESP_OK) {
                last_x = p[0] | (p[1] << 8);
                last_y = p[2] | (p[3] << 8);
            }
        }
        const uint8_t clear[3] = { GT911_REG_STATUS >> 8, GT911_REG_STATUS & 0xFF, 0 };
        i2c_master_transmit(touch, clear, sizeof(clear), 20);
    }
    *x = last_x;
    *y = last_y;
    return pressed;
}

static bool focaltech_read(int *x, int *y)
{
    if (touch == NULL) {
        return false;
    }
    uint8_t reg = TOUCH_REG_POINTS;
    uint8_t buf[5];
    if (i2c_master_transmit_receive(touch, &reg, 1, buf, sizeof(buf), 20) != ESP_OK || (buf[0] & 0x0F) == 0) {
        return false;
    }
    *x = ((buf[1] & 0x0F) << 8) | buf[2];
    *y = ((buf[3] & 0x0F) << 8) | buf[4];
    return true;
}

// LVGL polls this; coordinates go in the panel's native frame and LVGL applies the rotation.
static void touch_read(lv_indev_drv_t *drv, lv_indev_data_t *data)
{
    int x, y;
    data->state = LV_INDEV_STATE_REL;
    if (!(def.touch_kind == TOUCH_GT911 ? gt911_read(&x, &y) : focaltech_read(&x, &y))) {
        return;
    }
    if (def.touch_swap_xy) {
        const int t = x;
        x = y;
        y = t;
    }
    if (def.touch_mirror_x) {
        x = def.width - 1 - x;
    }
    if (def.touch_mirror_y) {
        y = def.height - 1 - y;
    }
    if (x < 0 || y < 0 || x >= def.width || y >= def.height) {
        return;
    }
    data->point.x = x;
    data->point.y = y;
    data->state = LV_INDEV_STATE_PR;
}

static lv_disp_t *display;

// LVGL waits for the panel to report each update done before the next. On 2026-10-20 the ONX lost
// that report after 14 min (display task spinning in draw_buf_flush, Wi-Fi starved). A lost report
// must cost a redraw, not the board: after 2 s the update counts as done and the whole screen is
// drawn again. Not sooner: in portrait every update is turned in software first, and a whole new
// layout took longer than the first limit (300 ms), which left the screen half drawn.
#define FLUSH_GIVE_UP_US (2000 * 1000)
static bool redraw_all;
// When the update being sent began: LVGL calls wait_cb only while it waits, never when an update
// ends, so the clock starts in the send step itself (a first version timed from the first wait
// ever, and declared ordinary updates lost every 2-3 s, garbling the screen).
static void (*port_flush)(lv_disp_drv_t *, const lv_area_t *, lv_color_t *);
static int64_t flush_started;

static void timed_flush(lv_disp_drv_t *drv, const lv_area_t *area, lv_color_t *pixels)
{
    flush_started = esp_timer_get_time();
    port_flush(drv, area, pixels);
}

static void flush_wait(lv_disp_drv_t *drv)
{
    static uint32_t given_up;
    lv_disp_draw_buf_t *buf = drv->draw_buf;
    if (buf == NULL || !buf->flushing || flush_started == 0) {
        return;
    }
    if (esp_timer_get_time() - flush_started > FLUSH_GIVE_UP_US) {
        given_up++;
        ESP_LOGW(TAG, "display: an update never reported done; drawing everything again (%" PRIu32 " so far)", given_up);
        flush_started = 0;
        redraw_all = true;
        lv_disp_flush_ready(drv);
    }
}

// After a lost update, everything is drawn again (called from the LVGL task, between refreshes).
static void redraw_check(lv_timer_t *timer)
{
    if (redraw_all) {
        redraw_all = false;
        lv_obj_invalidate(lv_scr_act());
        lv_obj_invalidate(lv_layer_top());
    }
}

// Panels that only take even-aligned updates (AMOLED): every area grows to even edges.
static void even_rounder(lv_disp_drv_t *drv, lv_area_t *area)
{
    area->x1 &= ~1;
    area->y1 &= ~1;
    area->x2 |= 1;
    area->y2 |= 1;
}

static const lv_disp_rot_t rotations[] = { LV_DISP_ROT_NONE, LV_DISP_ROT_90, LV_DISP_ROT_180, LV_DISP_ROT_270 };

// The screen size for a rotation of the native panel (90 and 270 swap width and height).
static void set_size(int rotation)
{
    const bool swap = rotation == 90 || rotation == 270;
    info.width = swap ? def.height : def.width;
    info.height = swap ? def.width : def.height;
}

void board_set_turn(int turn)
{
    if (display == NULL || turn < 0 || turn > 3) {
        return;
    }
    const int rotation = (def.rotation + 90 * turn) % 360;
    lv_disp_set_rotation(display, rotations[rotation / 90]);
    set_size(rotation);
    ESP_LOGI(TAG, "turned %d (rotation %d): %dx%d", turn, rotation, info.width, info.height);
}

static esp_err_t lvgl_init(esp_lcd_panel_io_handle_t io, esp_lcd_panel_handle_t panel)
{
    const lvgl_port_cfg_t port = ESP_LVGL_PORT_INIT_CONFIG();
    ESP_RETURN_ON_ERROR(lvgl_port_init(&port), TAG, "lvgl port");
    const lvgl_port_display_cfg_t display_config = {
        .io_handle = io, .panel_handle = panel,
        .buffer_size = draw_buffer_pixels(), .double_buffer = true,
        .hres = def.width, .vres = def.height,
        // lvgl_port_add_disp() re-applies swap/mirror from this config when it registers the display,
        // overwriting what display_init() set: give it the definition's values.
        .rotation = { .swap_xy = def.swap_xy, .mirror_x = def.mirror_x, .mirror_y = def.mirror_y },
        // Always: the person may turn the screen later (board_set_turn).
        .flags = { .buff_spiram = true, .sw_rotate = true },
    };
    display = lvgl_port_add_disp(&display_config);
    ESP_RETURN_ON_FALSE(display != NULL, ESP_FAIL, TAG, "lvgl display");

    ESP_RETURN_ON_FALSE(lvgl_port_lock(0), ESP_FAIL, TAG, "lvgl lock");
    port_flush = display->driver->flush_cb;
    display->driver->flush_cb = timed_flush;
    display->driver->wait_cb = flush_wait;
    if (def.even_areas) {
        display->driver->rounder_cb = even_rounder;
    }
    lv_timer_create(redraw_check, 100, NULL);
    lv_disp_set_rotation(display, rotations[def.rotation / 90]);
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

board_serial_t board_serial(void)
{
    return def.serial;
}

esp_err_t board_init(void)
{
    esp_err_t err = load_definition();
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "no usable board definition (%s); running without a display", esp_err_to_name(err));
        return err;
    }
    info = (board_info_t){
        .id = def.id, .name = def.name,
        .has_speaker = def.speaker, .has_native_usb = def.native_usb,
        .has_mic = def.mic_clk >= 0 && def.mic_data >= 0,
        // A rectangle's corner clears a rounded corner of radius r when it sits r(1 - 1/sqrt 2)
        // (0.29 r) in from both edges; a little more, for the keys' own edges.
        .inset = (uint16_t)((def.corner_radius * 3 + 9) / 10),
    };
    set_size(def.rotation);
    ESP_LOGI(TAG, "definition %s: %s display, %s touch", def.id, def.display_driver, def.touch_driver);

    esp_lcd_panel_io_handle_t io = NULL;
    esp_lcd_panel_handle_t panel = NULL;
    ESP_RETURN_ON_ERROR(i2c_init(), TAG, "i2c");
    ESP_RETURN_ON_ERROR(backlight_init(), TAG, "backlight");
    ESP_RETURN_ON_ERROR(display_init(&io, &panel), TAG, "display");
    panel_io = io;
    // A screen without touch still shows the board's state; a touch that does not answer is logged
    // rather than leaving the screen dark.
    err = touch_attach();
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "touch did not start (%s); running without touch", esp_err_to_name(err));
        touch = NULL;
    }
    return lvgl_init(io, panel);
}

// ---- microphone ----

// A PDM microphone on the I2S peripheral (ESP-IDF's PDM RX mode, which decodes to PCM), read in
// stereo: boards like the Nextion Dual MIC carry two microphones, and which slot answers varies.
// The first half second picks the slot with sound in it. Opened only while someone listens.
static i2s_chan_handle_t mic_chan;
static int mic_slot = -1; // 0 left, 1 right, -1 not chosen yet
static int16_t mic_pairs[2 * 512];
static int64_t mic_energy[2];
static int mic_pick_samples;

esp_err_t board_mic_start(uint32_t sample_rate)
{
    ESP_RETURN_ON_FALSE(info.has_mic, ESP_ERR_NOT_SUPPORTED, TAG, "this board has no microphone");
    if (mic_chan != NULL) {
        return ESP_OK;
    }
    // GPIO 19 and 20 are the S3's own USB lines, held by its USB serial port (the image's second log
    // console) until released. A board that talks through a USB-serial chip does not use them.
    const bool on_usb_pins = def.mic_clk == 19 || def.mic_clk == 20 || def.mic_data == 19 || def.mic_data == 20;
    if (on_usb_pins) {
        ESP_RETURN_ON_FALSE(def.serial == BOARD_SERIAL_UART0, ESP_ERR_INVALID_STATE, TAG,
                            "the microphone is on GPIO 19/20, which this board's USB serial line needs");
        usb_serial_jtag_ll_phy_enable_pad(false);
    }
    i2s_chan_config_t chan_cfg = I2S_CHANNEL_DEFAULT_CONFIG(I2S_NUM_0, I2S_ROLE_MASTER);
    ESP_RETURN_ON_ERROR(i2s_new_channel(&chan_cfg, NULL, &mic_chan), TAG, "microphone channel");
    i2s_pdm_rx_config_t cfg = {
        .clk_cfg = I2S_PDM_RX_CLK_DEFAULT_CONFIG(sample_rate),
        .slot_cfg = I2S_PDM_RX_SLOT_DEFAULT_CONFIG(I2S_DATA_BIT_WIDTH_16BIT, I2S_SLOT_MODE_STEREO),
        .gpio_cfg = { .clk = def.mic_clk, .din = def.mic_data },
    };
    esp_err_t err = i2s_channel_init_pdm_rx_mode(mic_chan, &cfg);
    if (err == ESP_OK) {
        err = i2s_channel_enable(mic_chan);
    }
    if (err != ESP_OK) {
        i2s_del_channel(mic_chan);
        mic_chan = NULL;
        ESP_LOGE(TAG, "microphone: %s", esp_err_to_name(err));
        return err;
    }
    mic_slot = def.mic_slot_fixed ? (def.mic_right ? 1 : 0) : -1;
    mic_energy[0] = mic_energy[1] = 0;
    mic_pick_samples = 0;
    ESP_LOGI(TAG, "microphone on (PDM, clock GPIO %d, data GPIO %d, %" PRIu32 " Hz)", def.mic_clk, def.mic_data, sample_rate);
    return ESP_OK;
}

size_t board_mic_read(int16_t *samples, size_t count, uint32_t timeout_ms)
{
    if (mic_chan == NULL) {
        return 0;
    }
    if (count > 512) {
        count = 512;
    }
    size_t bytes = 0;
    i2s_channel_read(mic_chan, mic_pairs, count * 2 * sizeof(int16_t), &bytes, timeout_ms);
    const size_t pairs = bytes / (2 * sizeof(int16_t));
    if (mic_slot < 0 && pairs > 0) {
        // Energy of each slot around its own average, and what the raw values look like.
        int16_t lo[2] = { INT16_MAX, INT16_MAX }, hi[2] = { INT16_MIN, INT16_MIN };
        int64_t sum[2] = { 0, 0 };
        for (size_t i = 0; i < pairs; i++) {
            for (int c = 0; c < 2; c++) {
                const int16_t v = mic_pairs[2 * i + c];
                sum[c] += v;
                lo[c] = v < lo[c] ? v : lo[c];
                hi[c] = v > hi[c] ? v : hi[c];
            }
        }
        for (int c = 0; c < 2; c++) {
            const int32_t mean = (int32_t)(sum[c] / (int64_t)pairs);
            for (size_t i = 0; i < pairs; i++) {
                const int32_t d = mic_pairs[2 * i + c] - mean;
                mic_energy[c] += (int64_t)d * d;
            }
        }
        if (mic_pick_samples == 0 || mic_pick_samples % 4096 < (int)pairs) {
            ESP_LOGI(TAG, "microphone raw: left %d..%d, right %d..%d", lo[0], hi[0], lo[1], hi[1]);
        }
        mic_pick_samples += pairs;
        if (mic_pick_samples >= 8000) {
            mic_slot = mic_energy[1] > mic_energy[0] ? 1 : 0;
            ESP_LOGI(TAG, "microphone: using the %s slot (energy left %lld, right %lld)",
                     mic_slot ? "right" : "left", mic_energy[0], mic_energy[1]);
        }
    }
    const int c = mic_slot < 0 ? 0 : mic_slot;
    for (size_t i = 0; i < pairs; i++) {
        samples[i] = mic_pairs[2 * i + c];
    }
    return pairs;
}

void board_mic_stop(void)
{
    if (mic_chan != NULL) {
        i2s_channel_disable(mic_chan);
        i2s_del_channel(mic_chan);
        mic_chan = NULL;
        ESP_LOGI(TAG, "microphone off");
    }
}

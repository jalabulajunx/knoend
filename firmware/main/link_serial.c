#include "link_serial.h"

#include <stdlib.h>
#include <string.h>

#include "driver/uart.h"
#include "driver/uart_vfs.h"
#include "esp_check.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#define UART_PORT  UART_NUM_0 // the console UART; the CH340K carries it to USB-C
#define RX_BUFFER  4096
#define TX_BUFFER  4096

static const char *TAG = "link_serial";

static link_serial_line_cb line_cb;

static void reader_task(void *arg)
{
    char *line = malloc(LINK_SERIAL_MAX_LINE + 1);
    uint8_t chunk[256];
    size_t length = 0;
    bool overflow = false;

    while (true) {
        int count = uart_read_bytes(UART_PORT, chunk, sizeof(chunk), portMAX_DELAY);
        for (int i = 0; i < count; i++) {
            const char c = (char)chunk[i];
            if (c == '\n') {
                if (overflow) {
                    line[0] = '\0';
                    line_cb(line, true);
                } else if (length > 0 && line[0] == '{') {
                    line[length] = '\0';
                    line_cb(line, false);
                }
                length = 0;
                overflow = false;
            } else if (c == '\r') {
                continue;
            } else if (length < LINK_SERIAL_MAX_LINE) {
                line[length++] = c;
            } else {
                overflow = true;
            }
        }
    }
}

esp_err_t link_serial_start(link_serial_line_cb on_line)
{
    line_cb = on_line;
    ESP_RETURN_ON_ERROR(uart_driver_install(UART_PORT, RX_BUFFER, TX_BUFFER, 0, NULL, 0), TAG,
                        "uart driver");
    // Route stdout/stderr through the driver too, so console writes and packets share one lock.
    uart_vfs_dev_use_driver(UART_PORT);
    BaseType_t ok = xTaskCreate(reader_task, "link_serial", 4096, NULL, 5, NULL);
    return ok == pdPASS ? ESP_OK : ESP_ERR_NO_MEM;
}

void link_serial_send(const char *line)
{
    const size_t length = strlen(line);
    char *buffer = malloc(length + 2);
    if (buffer == NULL) {
        return;
    }
    memcpy(buffer, line, length);
    buffer[length] = '\n';
    buffer[length + 1] = '\0';
    uart_write_bytes(UART_PORT, buffer, length + 1);
    free(buffer);
}

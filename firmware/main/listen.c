#include "listen.h"

#include <string.h>

#include "board.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "pitch.h"

#define MIC_RATE 16000
#define RATE     8000 // what the pitch search looks at: every two microphone samples, averaged
#define HOLD 3 // frames (about 100 ms) a note must hold before it counts

static const char *TAG = "listen";
static volatile bool running, stop_asked;
static volatile int64_t until_us;
static volatile float quiet;
static listen_cb_t callback;

static void report(int off, int on, int velocity, float hz, bool ended)
{
    const listen_event_t event = { .off = off, .on = on, .velocity = velocity, .hz = hz, .ended = ended };
    if (callback != NULL) {
        callback(&event);
    }
}

// `count` samples at RATE: the microphone's, two at a time, averaged (which also filters out what
// lies above the new rate's reach).
static void fill(int16_t *out, size_t count)
{
    static int16_t raw[2 * PITCH_HOP];
    size_t done = 0;
    while (done < count && !stop_asked) {
        size_t want = 2 * (count - done);
        if (want > sizeof(raw) / sizeof(raw[0])) {
            want = sizeof(raw) / sizeof(raw[0]);
        }
        size_t got = 0;
        while (got < want && !stop_asked) {
            got += board_mic_read(raw + got, want - got, 200);
        }
        for (size_t i = 0; i + 1 < got; i += 2) {
            out[done++] = (int16_t)(((int32_t)raw[i] + raw[i + 1]) / 2);
        }
    }
}

static void listen_task(void *arg)
{
    (void)arg;
    int16_t *frame = heap_caps_malloc(PITCH_FRAME * sizeof(int16_t), MALLOC_CAP_INTERNAL | MALLOC_CAP_8BIT);
    pitch_tracker_t tracker;
    pitch_tracker_init(&tracker, HOLD);
    if (frame == NULL || board_mic_start(MIC_RATE) != ESP_OK) {
        ESP_LOGE(TAG, "could not start the microphone");
        heap_caps_free(frame);
        running = false;
        report(-1, -1, 0, 0, true);
        vTaskDelete(NULL);
        return;
    }
    // Fill the first frame, then slide by a hop at a time.
    fill(frame, PITCH_FRAME);
    int64_t last_level_log = 0, busy_us = 0;
    float loudest = -120;
    while (!stop_asked && esp_timer_get_time() < until_us) {
        const int64_t started = esp_timer_get_time();
        const pitch_frame_t heard = pitch_detect(frame, RATE, quiet);
        busy_us += esp_timer_get_time() - started;
        if (heard.level > loudest) {
            loudest = heard.level;
        }
        const pitch_change_t change = pitch_tracker_feed(&tracker, heard, quiet);
        if (change.off >= 0 || change.on >= 0) {
            char name[8] = "-";
            pitch_name(change.on, name, sizeof(name));
            if (change.on >= 0) {
                ESP_LOGI(TAG, "note %s (%d, %.1f Hz, %.0f dB, velocity %d)", name, change.on, heard.hz, heard.level, change.velocity);
            }
            report(change.off, change.on, change.velocity, heard.hz, false);
        }
        // Once a second, how loud it was: shows the microphone works, and what "quiet" should be.
        const int64_t now = esp_timer_get_time();
        if (now - last_level_log > 1000000) {
            ESP_LOGI(TAG, "loudest %.0f dB in the last second (quiet below %.0f); pitch search %lld ms of it",
                     loudest, quiet, busy_us / 1000);
            last_level_log = now;
            busy_us = 0;
            loudest = -120;
        }
        memmove(frame, frame + PITCH_HOP, (PITCH_FRAME - PITCH_HOP) * sizeof(int16_t));
        fill(frame + PITCH_FRAME - PITCH_HOP, PITCH_HOP);
    }
    board_mic_stop();
    heap_caps_free(frame);
    ESP_LOGI(TAG, "stopped");
    running = false;
    report(tracker.current, -1, 0, 0, true);
    vTaskDelete(NULL);
}

esp_err_t listen_start(uint32_t seconds, float quiet_db, listen_cb_t on_event)
{
    if (!board_info()->has_mic) {
        return ESP_ERR_NOT_SUPPORTED;
    }
    until_us = esp_timer_get_time() + (int64_t)seconds * 1000000;
    quiet = quiet_db;
    callback = on_event;
    stop_asked = false;
    if (running) {
        return ESP_OK;
    }
    running = true;
    // Below the UI and the links.
    if (xTaskCreatePinnedToCore(listen_task, "listen", 4096, NULL, 3, NULL, 1) != pdPASS) {
        running = false;
        return ESP_ERR_NO_MEM;
    }
    return ESP_OK;
}

void listen_stop(void)
{
    stop_asked = true;
}

bool listen_active(void)
{
    return running;
}

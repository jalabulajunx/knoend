// Listening for notes: while on, the microphone is read and each note someone sings or plays is
// reported as it starts and stops (spec/protocol.md 3.6f). The sound itself never leaves the board.
#pragma once

#include <stdbool.h>
#include <stdint.h>
#include "esp_err.h"

typedef struct {
    int off;      // a note that stopped (MIDI number), or -1
    int on;       // a note that started, or -1
    int velocity; // 1-127 for `on`
    float hz;     // the frequency heard for `on`
    bool ended;   // listening stopped (asked, or its time ran out)
} listen_event_t;

typedef void (*listen_cb_t)(const listen_event_t *event);

// Starts listening for `seconds` (then it stops by itself); sounds quieter than `quiet_db` (dB
// below full scale, for example -50) are silence. Already listening: the time starts again.
esp_err_t listen_start(uint32_t seconds, float quiet_db, listen_cb_t on_event);
void listen_stop(void);
bool listen_active(void);

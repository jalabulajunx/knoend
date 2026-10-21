// Finding the note someone sings or plays: one voice at a time, from 16-bit samples. Plain C with
// no ESP-IDF calls, so it is tested on the computer (firmware/test/pitch_test.c).
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define PITCH_FRAME 1024 // samples looked at each time (64 ms at 16 kHz)
#define PITCH_HOP   512  // a new look every 512 samples (32 ms at 16 kHz)

typedef struct {
    bool voiced;   // a clear pitch was found
    float hz;      // its frequency (0 when not voiced)
    float level;   // loudness, dB below full scale (0 = loudest)
} pitch_frame_t;

// The pitch in `frame` (PITCH_FRAME samples) at `sample_rate`, between 65 Hz (C2) and 1100 Hz (C#6).
// Anything quieter than `quiet_db` (for example -50) counts as silence.
pitch_frame_t pitch_detect(const int16_t *frame, uint32_t sample_rate, float quiet_db);

// A frequency as a MIDI note number (A4 = 440 Hz = 69), rounded to the nearest note.
int pitch_midi(float hz);

// "C4", "F#3": the note's name and octave (MIDI 60 = C4). `out` holds at least 5 bytes.
void pitch_name(int midi, char *out, size_t size);

// Turns frame-by-frame pitches into notes that start and stop: a note starts after it has held for
// `hold` frames running and stops after `hold` frames of silence or of another note.
typedef struct {
    int hold;
    int current;   // the note sounding now, or -1
    int candidate; // the note being heard, not yet held long enough
    int count;     // frames the candidate has held
    int quiet;     // frames of silence in a row
    float level;   // loudness of the candidate's loudest frame
} pitch_tracker_t;

typedef struct {
    int off; // a note that stopped, or -1
    int on;  // a note that started, or -1
    int velocity; // 1-127 for `on`, from its loudness
} pitch_change_t;

void pitch_tracker_init(pitch_tracker_t *t, int hold);
pitch_change_t pitch_tracker_feed(pitch_tracker_t *t, pitch_frame_t frame, float quiet_db);

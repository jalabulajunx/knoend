// Checks pitch.c on the computer with made-up sounds: pure tones, voice-like tones (harmonics,
// vibrato, noise), silence and noise. Build and run: firmware/test/run_pitch_test.sh
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../main/pitch.h"

#define RATE 8000
static int failures;

static void check(int ok, const char *what)
{
    if (!ok) {
        failures++;
    }
    printf("%s %s\n", ok ? "ok  " : "FAIL", what);
}

// `seconds` of a tone at `hz`: `harmonics` overtones falling off, vibrato of `vib` semitones at
// 5.5 Hz, white noise at `noise` (share of full scale), at `gain`.
static int16_t *tone(float hz, float seconds, int harmonics, float fundamental, float vib, float noise, float gain, int *n)
{
    *n = (int)(seconds * RATE);
    int16_t *s = malloc(*n * sizeof(int16_t));
    double phase = 0;
    srand(7);
    for (int i = 0; i < *n; i++) {
        const double t = (double)i / RATE;
        const double f = hz * pow(2, vib / 12.0 * sin(2 * M_PI * 5.5 * t));
        phase += 2 * M_PI * f / RATE;
        double v = fundamental * sin(phase);
        for (int h = 2; h <= harmonics + 1; h++) {
            v += sin(h * phase) / h;
        }
        v = v * gain + noise * ((double)rand() / RAND_MAX * 2 - 1);
        s[i] = (int16_t)fmax(-32767, fmin(32767, v * 32767));
    }
    return s;
}

// The notes the tracker reports for a sound, as "on:60 off:60 ...".
static void notes(const int16_t *s, int n, char *out, size_t size)
{
    pitch_tracker_t t;
    pitch_tracker_init(&t, 3);
    out[0] = '\0';
    for (int at = 0; at + PITCH_FRAME <= n; at += PITCH_HOP) {
        pitch_change_t c = pitch_tracker_feed(&t, pitch_detect(s + at, RATE, -50), -50);
        char part[24];
        if (c.off >= 0) {
            snprintf(part, sizeof(part), "off:%d ", c.off);
            strncat(out, part, size - strlen(out) - 1);
        }
        if (c.on >= 0) {
            snprintf(part, sizeof(part), "on:%d ", c.on);
            strncat(out, part, size - strlen(out) - 1);
        }
    }
}

static void midi_of(float hz, int harmonics, float fundamental, float vib, float noise, float gain, int want, const char *what)
{
    int n;
    int16_t *s = tone(hz, 0.5f, harmonics, fundamental, vib, noise, gain, &n);
    char got[256], expect[32], line[400];
    notes(s, n, got, sizeof(got));
    snprintf(expect, sizeof(expect), "on:%d ", want);
    snprintf(line, sizeof(line), "%s: %.1f Hz -> want %s got \"%s\"", what, hz, expect, got);
    check(strcmp(got, expect) == 0, line);
    free(s);
}

int main(void)
{
    char name[8];
    pitch_name(60, name, sizeof(name));
    check(strcmp(name, "C4") == 0, "MIDI 60 is C4");
    pitch_name(69, name, sizeof(name));
    check(strcmp(name, "A4") == 0, "MIDI 69 is A4");
    check(pitch_midi(440) == 69 && pitch_midi(261.63f) == 60 && pitch_midi(452) == 69 && pitch_midi(453) == 70,
          "frequencies round to the nearest note");

    // Pure tones across the range.
    const int range[] = { 36, 40, 45, 48, 52, 55, 60, 64, 67, 69, 72, 76, 81, 84 };
    for (size_t i = 0; i < sizeof(range) / sizeof(range[0]); i++) {
        midi_of(440 * powf(2, (range[i] - 69) / 12.0f), 0, 1, 0, 0, 0.5f, range[i], "pure tone");
    }
    // Voice-like: overtones, vibrato of a third of a semitone, a little noise, quiet.
    const int voice[] = { 48, 53, 57, 60, 62, 65, 67, 72 };
    for (size_t i = 0; i < sizeof(voice) / sizeof(voice[0]); i++) {
        midi_of(440 * powf(2, (voice[i] - 69) / 12.0f), 6, 1, 0.33f, 0.01f, 0.1f, voice[i], "voice-like");
    }
    // A weak fundamental (some voices, a guitar's low string): must not jump an octave up.
    midi_of(110, 6, 0.3f, 0, 0.005f, 0.2f, 45, "weak fundamental");
    midi_of(196, 6, 0.3f, 0, 0.005f, 0.2f, 55, "weak fundamental");

    // Silence and noise: no notes.
    int n;
    char got[256];
    int16_t *s = tone(440, 0.5f, 0, 1, 0, 0, 0.0005f, &n);
    notes(s, n, got, sizeof(got));
    check(got[0] == '\0', "a tone under the quiet threshold is silence");
    free(s);
    s = tone(440, 0.5f, 0, 0, 0, 0.3f, 0, &n);
    notes(s, n, got, sizeof(got));
    check(got[0] == '\0', "white noise gives no note");
    free(s);

    // Two notes in a row, then silence: on, then off and on, then off.
    int n1, n2, n3;
    int16_t *a = tone(261.63f, 0.4f, 4, 1, 0, 0.005f, 0.2f, &n1);
    int16_t *b = tone(329.63f, 0.4f, 4, 1, 0, 0.005f, 0.2f, &n2);
    int16_t *q = tone(1, 0.3f, 0, 0, 0, 0, 0, &n3);
    int16_t *all = malloc((n1 + n2 + n3) * sizeof(int16_t));
    memcpy(all, a, n1 * 2);
    memcpy(all + n1, b, n2 * 2);
    memcpy(all + n1 + n2, q, n3 * 2);
    notes(all, n1 + n2 + n3, got, sizeof(got));
    char line[400];
    snprintf(line, sizeof(line), "C4 then E4 then silence -> \"%s\"", got);
    check(strcmp(got, "on:60 off:60 on:64 off:64 ") == 0, line);

    printf("%d failure(s)\n", failures);
    return failures != 0;
}

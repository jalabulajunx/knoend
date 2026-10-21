// YIN pitch detection (de Cheveigné and Kawahara, "YIN, a fundamental frequency estimator for
// speech and music", JASA 2002), written for this firmware: the difference function, its
// cumulative mean normalisation, the first dip under a threshold, and a parabola through that dip
// for a frequency between whole sample lags.
#include "pitch.h"

#include <math.h>
#include <stdio.h>

#define LOW_HZ    65.0f
#define HIGH_HZ   1100.0f
#define THRESHOLD 0.15f // how clear a dip must be: lower is stricter
#define MAX_LAG   (PITCH_FRAME / 2)

pitch_frame_t pitch_detect(const int16_t *frame, uint32_t sample_rate, float quiet_db)
{
    pitch_frame_t out = { .voiced = false, .hz = 0, .level = -120 };
    static float x[PITCH_FRAME];
    static float d[MAX_LAG];

    // Without its average (a microphone's offset), and how loud it is.
    float mean = 0;
    for (int i = 0; i < PITCH_FRAME; i++) {
        mean += frame[i];
    }
    mean /= PITCH_FRAME;
    float power = 0;
    for (int i = 0; i < PITCH_FRAME; i++) {
        x[i] = (frame[i] - mean) / 32768.0f;
        power += x[i] * x[i];
    }
    const float rms = sqrtf(power / PITCH_FRAME);
    out.level = rms > 1e-6f ? 20.0f * log10f(rms) : -120;
    if (out.level < quiet_db) {
        return out;
    }

    int min_lag = (int)(sample_rate / HIGH_HZ);
    int max_lag = (int)(sample_rate / LOW_HZ);
    if (max_lag >= MAX_LAG) {
        max_lag = MAX_LAG - 1;
    }
    const int window = PITCH_FRAME - max_lag;

    // Difference function and its cumulative mean normalisation.
    d[0] = 1;
    float running = 0;
    for (int lag = 1; lag <= max_lag; lag++) {
        float sum = 0;
        for (int i = 0; i < window; i++) {
            const float delta = x[i] - x[i + lag];
            sum += delta * delta;
        }
        running += sum;
        d[lag] = running > 0 ? sum * lag / running : 1;
    }

    // The first dip under the threshold, followed to its bottom.
    int lag = -1;
    for (int t = min_lag; t <= max_lag; t++) {
        if (d[t] < THRESHOLD) {
            while (t + 1 <= max_lag && d[t + 1] < d[t]) {
                t++;
            }
            lag = t;
            break;
        }
    }
    if (lag < 0) {
        return out; // no clear pitch: breath, noise, a consonant
    }

    // A parabola through the dip and its neighbours.
    float best = lag;
    if (lag > 1 && lag < max_lag) {
        const float a = d[lag - 1], b = d[lag], c = d[lag + 1];
        const float bend = a - 2 * b + c;
        if (bend > 0) {
            best = lag + 0.5f * (a - c) / bend;
        }
    }
    out.voiced = true;
    out.hz = sample_rate / best;
    return out;
}

int pitch_midi(float hz)
{
    return (int)lroundf(69.0f + 12.0f * log2f(hz / 440.0f));
}

void pitch_name(int midi, char *out, size_t size)
{
    static const char *const names[] = { "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B" };
    if (midi < 0 || midi > 127) {
        snprintf(out, size, "-");
        return;
    }
    snprintf(out, size, "%s%d", names[midi % 12], midi / 12 - 1);
}

void pitch_tracker_init(pitch_tracker_t *t, int hold)
{
    *t = (pitch_tracker_t){ .hold = hold, .current = -1, .candidate = -1 };
}

static int velocity(float level, float quiet_db)
{
    // From the quiet threshold (soft) to 6 dB below full scale (loud).
    float share = (level - quiet_db) / (-6.0f - quiet_db);
    share = share < 0 ? 0 : share > 1 ? 1 : share;
    return 20 + (int)(share * 107);
}

pitch_change_t pitch_tracker_feed(pitch_tracker_t *t, pitch_frame_t frame, float quiet_db)
{
    pitch_change_t change = { .off = -1, .on = -1 };
    const int heard = frame.voiced ? pitch_midi(frame.hz) : -1;
    if (heard < 0) {
        t->candidate = -1;
        t->count = 0;
        if (t->current >= 0 && ++t->quiet >= t->hold) {
            change.off = t->current;
            t->current = -1;
        }
        return change;
    }
    t->quiet = 0;
    if (heard == t->current) {
        t->candidate = -1;
        t->count = 0;
        return change;
    }
    if (heard != t->candidate) {
        t->candidate = heard;
        t->count = 0;
        t->level = frame.level;
    }
    if (frame.level > t->level) {
        t->level = frame.level;
    }
    if (++t->count >= t->hold) {
        change.off = t->current;
        change.on = heard;
        change.velocity = velocity(t->level, quiet_db);
        t->current = heard;
        t->candidate = -1;
        t->count = 0;
    }
    return change;
}

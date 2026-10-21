#!/bin/sh
# Builds and runs the pitch detector's tests on this computer (no board, no ESP-IDF).
set -e
here=$(dirname "$0")
out=${TMPDIR:-/tmp}/knoend_pitch_test
cc -O2 -Wall -Wextra -o "$out" "$here/pitch_test.c" "$here/../main/pitch.c" -lm
"$out"

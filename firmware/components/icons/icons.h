// Material Icons for keys (Apache-2.0, see LICENSE), by the names in spec/icons.txt.
#pragma once

#include "lvgl.h"

#define ICONS_SIZE 32

// The icon called `name` (a 4-bit alpha image: draw it with img_recolor), or NULL.
const lv_img_dsc_t *icons_find(const char *name);

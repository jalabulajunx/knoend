#include "lv_mem_psram.h"

#include "esp_heap_caps.h"

#define PSRAM_FIRST (MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT)

void *knoend_lv_malloc(size_t size)
{
    return heap_caps_malloc_prefer(size, 2, PSRAM_FIRST, MALLOC_CAP_DEFAULT);
}

void *knoend_lv_realloc(void *ptr, size_t size)
{
    return heap_caps_realloc_prefer(ptr, size, 2, PSRAM_FIRST, MALLOC_CAP_DEFAULT);
}

void knoend_lv_free(void *ptr)
{
    heap_caps_free(ptr);
}

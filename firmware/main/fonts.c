#include "fonts.h"

#include <string.h>

#include "esp_flash.h"
#include "esp_log.h"
#include "esp_partition.h"
#include "esp_rom_crc.h"

#define FONTS_SUBTYPE 0x41
#define FONT_MAGIC    0x544E464B // "KFNT"
#define SECTOR        4096

static const char *TAG = "fonts";

// The first sector of a slot: what it holds. The font follows from the second sector on.
typedef struct {
    uint32_t magic;
    uint32_t len;
    uint32_t crc;
    char name[FONT_NAME_BYTES];
} header_t;

static const esp_partition_t *partition;
static size_t slot_size;

static struct {
    const uint8_t *data;
    esp_partition_mmap_handle_t map;
    bool mapped;
    header_t header;
    size_t received; // while receiving
    bool receiving;
} slots[FONT_SLOTS];

static size_t slot_offset(font_slot_t slot)
{
    return (size_t)slot * slot_size;
}

size_t fonts_slot_capacity(void)
{
    return slot_size > SECTOR ? slot_size - SECTOR : 0;
}

static void unmap(font_slot_t slot)
{
    if (slots[slot].mapped) {
        esp_partition_munmap(slots[slot].map);
        slots[slot].mapped = false;
    }
    slots[slot].data = NULL;
}

// Maps the slot's font if its header and CRC-32 are right.
static void map(font_slot_t slot)
{
    unmap(slot);
    header_t header;
    if (esp_partition_read(partition, slot_offset(slot), &header, sizeof(header)) != ESP_OK
        || header.magic != FONT_MAGIC || header.len == 0 || header.len > fonts_slot_capacity()) {
        memset(&slots[slot].header, 0, sizeof(header_t));
        return;
    }
    header.name[FONT_NAME_BYTES - 1] = '\0';
    const void *data;
    if (esp_partition_mmap(partition, slot_offset(slot) + SECTOR, header.len, ESP_PARTITION_MMAP_DATA, &data,
                           &slots[slot].map) != ESP_OK) {
        ESP_LOGW(TAG, "%s: could not map the font", fonts_slot_name(slot));
        return;
    }
    slots[slot].mapped = true;
    if (esp_rom_crc32_le(0, data, header.len) != header.crc) {
        ESP_LOGW(TAG, "%s: stored font is damaged; ignored", fonts_slot_name(slot));
        unmap(slot);
        return;
    }
    slots[slot].data = data;
    slots[slot].header = header;
    ESP_LOGI(TAG, "%s: %s, %u bytes", fonts_slot_name(slot), header.name, (unsigned)header.len);
}

void fonts_init(void)
{
    partition = esp_partition_find_first(ESP_PARTITION_TYPE_DATA, FONTS_SUBTYPE, NULL);
    if (partition == NULL) {
        ESP_LOGW(TAG, "no fonts partition: the built-in font only");
        return;
    }
    slot_size = (partition->size / FONT_SLOTS) & ~(size_t)(SECTOR - 1);
    for (int slot = 0; slot < FONT_SLOTS; slot++) {
        map(slot);
    }
}

bool fonts_slot_parse(const char *name, font_slot_t *slot)
{
    for (int s = 0; s < FONT_SLOTS; s++) {
        if (name != NULL && strcmp(name, fonts_slot_name(s)) == 0) {
            *slot = s;
            return true;
        }
    }
    return false;
}

const char *fonts_slot_name(font_slot_t slot)
{
    return slot == FONT_SLOT_START ? "start" : "keys";
}

const uint8_t *fonts_data(font_slot_t slot, size_t *len, const char **name, uint32_t *crc)
{
    if (slot >= FONT_SLOTS || slots[slot].data == NULL) {
        return NULL;
    }
    if (len) *len = slots[slot].header.len;
    if (name) *name = slots[slot].header.name;
    if (crc) *crc = slots[slot].header.crc;
    return slots[slot].data;
}

esp_err_t fonts_begin(font_slot_t slot, size_t total, const char *name)
{
    if (partition == NULL) {
        return ESP_ERR_NOT_SUPPORTED;
    }
    if (slot >= FONT_SLOTS || total == 0 || total > fonts_slot_capacity() || name == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    unmap(slot);
    // The header sector and as many sectors as the font needs.
    const size_t erase = SECTOR + ((total + SECTOR - 1) / SECTOR) * SECTOR;
    esp_err_t err = esp_partition_erase_range(partition, slot_offset(slot), erase);
    if (err != ESP_OK) {
        return err;
    }
    memset(&slots[slot].header, 0, sizeof(header_t));
    slots[slot].header.len = total;
    strncpy(slots[slot].header.name, name, FONT_NAME_BYTES - 1);
    slots[slot].received = 0;
    slots[slot].receiving = true;
    return ESP_OK;
}

esp_err_t fonts_write(font_slot_t slot, size_t offset, const uint8_t *data, size_t len)
{
    if (slot >= FONT_SLOTS || !slots[slot].receiving || offset != slots[slot].received
        || offset + len > slots[slot].header.len) {
        return ESP_ERR_INVALID_STATE;
    }
    esp_err_t err = esp_partition_write(partition, slot_offset(slot) + SECTOR + offset, data, len);
    if (err == ESP_OK) {
        slots[slot].received += len;
    }
    return err;
}

esp_err_t fonts_finish(font_slot_t slot, uint32_t crc)
{
    if (slot >= FONT_SLOTS || !slots[slot].receiving || slots[slot].received != slots[slot].header.len) {
        return ESP_ERR_INVALID_STATE;
    }
    slots[slot].receiving = false;
    header_t header = slots[slot].header;
    header.magic = FONT_MAGIC;
    header.crc = crc;
    esp_err_t err = esp_partition_write(partition, slot_offset(slot), &header, sizeof(header));
    if (err != ESP_OK) {
        return err;
    }
    map(slot);
    return slots[slot].data != NULL ? ESP_OK : ESP_ERR_INVALID_CRC;
}

esp_err_t fonts_clear(font_slot_t slot)
{
    if (partition == NULL || slot >= FONT_SLOTS) {
        return ESP_ERR_INVALID_ARG;
    }
    unmap(slot);
    slots[slot].receiving = false;
    memset(&slots[slot].header, 0, sizeof(header_t));
    return esp_partition_erase_range(partition, slot_offset(slot), SECTOR);
}

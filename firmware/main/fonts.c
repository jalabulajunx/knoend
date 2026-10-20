#include "fonts.h"

#include <string.h>

#include "esp_flash.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_partition.h"
#include "esp_rom_crc.h"
#include "nvs.h"
#include "esp_system.h"

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
    uint8_t *data; // a copy in PSRAM (not read in place from flash: a font's stray offset must not
                   // fault the flash mapping, and flash writes must not stop drawing)
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
    heap_caps_free(slots[slot].data);
    slots[slot].data = NULL;
}

// Hides the kerning tables (GPOS, kern) from the renderer: tiny_ttf always reads them, and a font
// whose GPOS points past its end crashed the board (WDXL Lubrifont SC, 2026-10-20). Kerning is
// invisible at key-label sizes. Only the table's name in the directory changes.
static void hide_kerning(uint8_t *font, size_t len)
{
    if (len < 12) {
        return;
    }
    const unsigned tables = (unsigned)font[4] << 8 | font[5];
    for (unsigned i = 0; i < tables && 12 + 16 * (i + 1) <= len; i++) {
        uint8_t *tag = font + 12 + 16 * i;
        if (memcmp(tag, "GPOS", 4) == 0 || memcmp(tag, "kern", 4) == 0) {
            tag[0] = 'x';
        }
    }
}

// Reads the slot's font into PSRAM if its header and CRC-32 are right.
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
    uint8_t *data = heap_caps_malloc(header.len, MALLOC_CAP_SPIRAM);
    if (data == NULL || esp_partition_read(partition, slot_offset(slot) + SECTOR, data, header.len) != ESP_OK) {
        ESP_LOGW(TAG, "%s: could not read the font", fonts_slot_name(slot));
        heap_caps_free(data);
        return;
    }
    if (esp_rom_crc32_le(0, data, header.len) != header.crc) {
        ESP_LOGW(TAG, "%s: stored font is damaged; ignored", fonts_slot_name(slot));
        heap_caps_free(data);
        return;
    }
    hide_kerning(data, header.len);
    slots[slot].data = data;
    slots[slot].header = header;
    ESP_LOGI(TAG, "%s: %s, %u bytes", fonts_slot_name(slot), header.name, (unsigned)header.len);
}

#define NVS_NAMESPACE "openpager" // storage name kept from before the rename

// A font that crashed the board must not crash it at every start: "font_trying" is set while the
// fonts are first used after a start, and cleared once they have drawn (fonts_settled). Found set
// at start, the stored fonts are dropped and their names kept in "font_refused" for the companion.
static char refused[2 * FONT_NAME_BYTES];

static void set_trying(uint8_t value)
{
    nvs_handle_t nvs;
    if (nvs_open(NVS_NAMESPACE, NVS_READWRITE, &nvs) == ESP_OK) {
        nvs_set_u8(nvs, "font_trying", value);
        nvs_commit(nvs);
        nvs_close(nvs);
    }
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
    nvs_handle_t nvs;
    uint8_t trying = 0;
    if (nvs_open(NVS_NAMESPACE, NVS_READWRITE, &nvs) == ESP_OK) {
        size_t size = sizeof(refused);
        nvs_get_u8(nvs, "font_trying", &trying);
        // Only a crash drops them: a freeze, a power cut, a restart or a flash can also leave the
        // flag set, and the fonts were not at fault then (a freeze cost Sundar's fonts once).
        const esp_reset_reason_t why = esp_reset_reason();
        const bool crashed = why == ESP_RST_PANIC || why == ESP_RST_INT_WDT || why == ESP_RST_TASK_WDT || why == ESP_RST_WDT;
        if (trying && !crashed) {
            ESP_LOGI(TAG, "the last start with these fonts did not finish, but the board did not crash: kept");
            trying = 0;
            nvs_set_u8(nvs, "font_trying", 0);
            nvs_commit(nvs);
        }
        if (trying) {
            refused[0] = '\0';
            for (int slot = 0; slot < FONT_SLOTS; slot++) {
                if (slots[slot].data != NULL) {
                    if (refused[0] != '\0') {
                        strlcat(refused, ", ", sizeof(refused));
                    }
                    strlcat(refused, slots[slot].header.name, sizeof(refused));
                    fonts_clear(slot);
                }
            }
            ESP_LOGW(TAG, "the last start with these fonts did not finish; dropped: %s", refused);
            nvs_set_str(nvs, "font_refused", refused);
            nvs_set_u8(nvs, "font_trying", 0);
            nvs_commit(nvs);
        } else if (nvs_get_str(nvs, "font_refused", refused, &size) != ESP_OK) {
            refused[0] = '\0';
        }
        nvs_close(nvs);
    }
    if (!trying && (slots[FONT_SLOT_KEYS].data != NULL || slots[FONT_SLOT_START].data != NULL)) {
        set_trying(1);
    }
}

void fonts_settled(void)
{
    set_trying(0);
}

const char *fonts_refused(void)
{
    return refused;
}

// A font the companion sends anew is no longer refused.
static void forget_refused(void)
{
    if (refused[0] == '\0') {
        return;
    }
    refused[0] = '\0';
    nvs_handle_t nvs;
    if (nvs_open(NVS_NAMESPACE, NVS_READWRITE, &nvs) == ESP_OK) {
        nvs_erase_key(nvs, "font_refused");
        nvs_commit(nvs);
        nvs_close(nvs);
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
    if (slots[slot].data != NULL) {
        forget_refused();
        if (slot == FONT_SLOT_KEYS) {
            set_trying(1); // drawn at once, until it has (fonts_settled); the start font draws at the next start
        }
    }
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

# firmware

ESP-IDF v5.4.4 project (C). One universal image for ESP32-S3 boards with octal PSRAM; each board's
display, touch and pins come from a JSON **board definition** in the 4 KB `board` partition
(0x310000), written at install time.

- `main/` — grid UI, keys, protocol, Bluetooth / Wi-Fi / serial links
- `components/board_generic/` — display (ST7789, ILI9341), touch (CST816, CST826, FT6336, GT911),
  backlight and expander, all driven by the definition
- `components/board_api/` — `board.h`; `components/esp_hid_gap/` — from Espressif's examples (Unlicense/CC0)
- `boards/catalog.json`, `boards/definitions/*.json` — boards and their definitions
  (schema: `spec/schemas/board.schema.json`)

Hardware rule: one change per flash, tested on the board before the next.

## Build and flash

```bash
. ~/esp/esp-idf/export.sh
idf.py -B build/universal build
idf.py -B build/universal -p /dev/ttyUSB0 flash monitor
```

`idf.py flash` does not write a board definition; without one the board boots headless
(`BOARD no definition`). Install once through the web app (`openpager serve`), or package with
`tools/release_firmware.py` and flash with `openpager flash`. Full guide: [`../docs/setup.md`](../docs/setup.md).

Each tap prints `KEY <n> ...` on the console (115200 baud on the ONX).

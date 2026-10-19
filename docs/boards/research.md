# Which ESP32-S3 touch boards can run OpenPager

As of 2026-10-18. Popular ESP32-S3 touchscreen boards, checked against what the universal image
supports today, and what each would need. Every fact below comes from the page linked beside it;
pins are taken only from manufacturer pages or the manufacturer's open-source board support.

**Today's universal image supports:** ESP32-S3 with **octal** PSRAM, 4 MB flash or more; **SPI**
displays with ST7789 or ILI9341 (with start-up commands from the definition); touch CST816, CST826,
FT6336 (FocalTech layout) and **GT911**; optional PCF8574 expander; active-low or active-high reset.

**Update, same day:** GT911, CST816, display start-up commands and active-high reset were added, and
**unverified** definitions were written for the Waveshare ESP32-S3-Touch-LCD-2 (pins from Waveshare's
demo code) and the ESP32-S3-BOX-3 (from Espressif's Apache-2.0 board support). The rows below are as
first researched; those two boards now install as "unverified".

## Summary

| Board | Display | Touch | PSRAM | Runs today? | Missing | Popularity signal |
| --- | --- | --- | --- | --- | --- | --- |
| ONX2432G028 | ST7789 SPI 240x320 | CST826 | octal | **Yes (tested)** | - | ours |
| Freenove FNK0104B | ILI9341 SPI 320x240 | FT6336 | octal (S3R8) | Should, once its definition exists | definition (Tom) | ours |
| Waveshare ESP32-S3-Touch-LCD-2 | ST7789T3 SPI 240x320 | CST816D | 8 MB (S3R8) | Probably | pins not published on its pages (in its demo code) | - |
| Waveshare ESP32-S3-Touch-LCD-2.8 | ST7789 SPI 240x320 | CST328 (V1), CST3530 (V2, shipping since 2026-06-05) | 8 MB (S3R8) | No | Hynitron CST3xx touch driver | - |
| Espressif ESP32-S3-BOX-3 | ILI9342C (ILI9341 driver) SPI 320x240 | GT911 (TT21100 on early units) | octal | No | GT911 driver; display init commands; active-high shared reset | esp-box 1.3k stars |
| Sunton ESP32-1732S019 | ST7789 SPI 170x320 | GT911 | not checked | No | GT911 driver; pins to confirm | Sunton family |
| Sunton ESP32-8048S043 / 8048S050 / 8048S070, Waveshare ESP32-S3-Touch-LCD-7, Guition JC8048W550C | ST7262 16-bit RGB 800x480 | GT911 | not checked | No | RGB panel driver, GT911, a layout for 800x480 | Sunton repo 229 stars |
| Sunton ESP32-4827S043 | ST7262 RGB 480x272 | GT911 or XPT2046 | not checked | No | RGB panel driver, GT911 | Sunton family |
| Guition JC4827W543C | NV3041A QSPI 480x272 | GT911 | not checked | No | QSPI panel driver, GT911 | - |
| LilyGo T-Display-S3 (Touch) | ST7789 8-bit parallel 170x320 | touch version: controller not named on the page | octal (OPI) | No | i80 parallel bus | 1.2k stars |
| M5Stack CoreS3 | ILI9342C 320x240 | FT6336U (0x38) | **quad** | No | quad-PSRAM image; AXP2101 power IC and AW9523B expander | - |
| "Cheap Yellow Display" ESP32-2432S028 and kin | ILI9341/ST7789 SPI | resistive or GT911 | - | **Never: plain ESP32, not S3** | - | 4.5k stars (most popular) |

## What this means

1. **GT911 is the most valuable next driver.** Every Sunton ESP32-S3 board in the community catalog,
   the ESP32-S3-BOX-3, and the Waveshare/Elecrow/Guition 7-inch boards use it. Espressif publishes
   an Apache-2.0 component for it (`esp_lcd_touch_gt911`).
2. **A 16-bit RGB panel driver is the second.** The popular 4.3-7 inch boards drive ST7262 panels
   over RGB, which ESP-IDF's `esp_lcd` supports; the board definition would carry the 16 data pins
   and timings. It also needs a bigger key layout (the UI is designed for 320x240).
3. **Definitions are cheap once drivers exist**, but a definition nobody has run is a guess. New
   definitions should be marked unverified until someone confirms the screen, colours and touch,
   ideally with the planned tap-the-corners calibration screen.
4. **Not every popular board is an S3.** The Cheap Yellow Display family is the most-starred by far
   but uses the original ESP32 (no native USB, different PSRAM), which the universal image does not
   target.
5. **Small definition-format additions** some boards need: display init command lists (ILI9342C on
   the S3-BOX-3), an active-high reset option, a reset line shared by display and touch.
6. **Dense screens (e.g. 800x600 at about this size) are not handled; not for the demo.** The board
   UI sizes everything in pixels: fonts 14/20/28 px, icons 32 px, status bar 40 px, smallest key
   64x44 px (which sets the largest grid). On a screen with 2.5x the pixel density all of it would be
   about half as big physically. Needed then: the screen's physical size in the definition, minimum
   key and bar sizes in mm, bigger fonts and icons pre-drawn at 64 px (LVGL 8 cannot zoom our 4-bit
   icons). Discussed by Tom on 2026-10-19 with grids and pages.

## Sources

- Waveshare ESP32-S3-Touch-LCD-2: [wiki](https://www.waveshare.com/wiki/ESP32-S3-Touch-LCD-2), [docs](https://docs.waveshare.com/ESP32-S3-Touch-LCD-2), [Arduino guide](https://docs.waveshare.com/ESP32-S3-Touch-LCD-2/Arduino)
- Waveshare ESP32-S3-Touch-LCD-2.8: [wiki](https://www.waveshare.com/wiki/ESP32-S3-Touch-LCD-2.8)
- ESP32-S3-BOX-3: Espressif board support, [esp-bsp/bsp/esp-box-3](https://github.com/espressif/esp-bsp/tree/master/bsp/esp-box-3) (`esp-box-3.h`, `display.h`, `esp-box-3.c`)
- Sunton/Guition/Waveshare S3 boards: [rzeldent/platformio-espressif32-sunton](https://github.com/rzeldent/platformio-espressif32-sunton)
- LilyGo T-Display-S3: [Xinyuan-LilyGO/T-Display-S3](https://github.com/Xinyuan-LilyGO/T-Display-S3)
- M5Stack CoreS3: [docs](https://docs.m5stack.com/en/core/CoreS3)
- Cheap Yellow Display: [witnessmenow/ESP32-Cheap-Yellow-Display](https://github.com/witnessmenow/ESP32-Cheap-Yellow-Display)
- Popularity: GitHub star counts read 2026-10-18 via the GitHub API.

# Running log

Shared memory for everyone working on OpenPager and their coding agents. Newest entry at the
bottom. Append only: correct a past entry with a new one. Rules: [AGENTS.md](../AGENTS.md).

Entry template:

```
## YYYY-MM-DD - <person> (<agent>) - <short title>

- Changed:
- Tested (hardware / OS, result):
- Decided:
- Open:
```

---

## 2026-10-16 - Sundar (Claude Code) - Repository, companion shape, ONX board profile

- Changed:
  - Monorepo scaffold: README, MIT licence, folders for `spec/`, `firmware/`, `companion/`, `web/`,
    `extension/`, `libraries/`, `hardware/`, `docs/`, `tools/`.
  - Firmware: `board_api` (`board.h`: init, info, backlight; Kconfig board choice; build with
    `-DOPENPAGER_BOARD=<board>`, one sdkconfig per build directory).
  - `board_onx2432g028` written from the schematic and Espressif `esp_lcd` / `esp_lvgl_port`.
    Facts: ST7789 on SPI2 (SCLK 5, MOSI 1, DC 3, CS 2, 80 MHz), native 240x320 portrait, panel
    mirror (x false, y true), LVGL software-rotated 90 degrees to landscape; PCF8574 expander at 0x38
    (P0 amplifier enable, low = muted; P5 LCD reset, low = reset); CST826 touch at 0x15, points from
    register 0x02, chip ID register 0xAA reads 0x11; backlight PWM on GPIO 6; I2C0 SDA 8, SCL 7.
  - `main`: 3x3 key grid with a 40 px status bar. One tap per press (sent on release). Console line
    per tap: `KEY <1-9> seq=<n> t=<ms>`.
  - `board_fnk0104b`: empty placeholder for Tom.
- Tested (ONX2432G028, Arch Linux): builds with no warnings; flashed; boot log shows
  `READY openpager 7e61233 board=onx2432g028`. On the board: landscape grid, correct colours, each of
  keys 1-9 highlights when touched; serial log `KEY 1`..`KEY 9`, seq 1-9 in order, no gaps or doubles;
  a 5-second hold on key 5 logged as one tap. Opening the serial port to log did not visibly reboot
  the board this time (single observation; the reset issue is not considered fixed).
- Decided:
  - Companion is a local Rust service plus a web app, not a desktop app. The service links to boards
    (BLE, serial, Wi-Fi), watches the active app, runs the library engine and agent hooks, and
    identifies and flashes boards (`espflash`). The web app is served by the service on 127.0.0.1 and
    also hosted publicly, where it can flash over Web Serial and run the simulator. The local API
    checks `Origin` and requires a pairing token.
  - The ONX board-support code used in the earlier bench prototype is the board maker's example code
    (copyright notice, no licence), so none of it is in this repository; the profile above is new.
  - Third-party agreement texts read during planning are kept outside the repository.
- Open:
  - Next flash on ONX: Bluetooth LE keyboard (Espressif `esp_hid_device` example, Unlicense/CC0).
  - Serial-reset behaviour needs a deliberate test.
  - `spec/` v0 (packets, identity, library schema) to write with Tom.
  - Tom: FNK0104B profile; confirm native USB enumerates as Espressif `303a:xxxx`.
  - E-ink-style theme is scheduled later; colour is the default for now.

## 2026-10-17 - Sundar (Claude Code) - Bluetooth LE keyboard on the ONX

- Changed:
  - `components/esp_hid_gap`: Espressif's GAP helper copied unchanged from ESP-IDF v5.4.4
    `examples/bluetooth/esp_hid_device` (Unlicense OR CC0-1.0).
  - `main/ble_keyboard.c`: BLE HID keyboard (Bluedroid), advertised as `OpenPager`. Report map from
    the same example, widened to usages up to 0x73 so F13-F24 can be sent later without re-pairing.
    One press + release per tap. Console line now ends `ble=<sent|not-connected|error>`.
  - Key N types the digit N until layouts exist. Status bar shows the Bluetooth state.
- Tested (ONX2432G028, Arch Linux, BlueZ + Blueman): build `a1ea8a8`. Paired with no prompt
  (`BLE GAP AUTH SUCCESS`, laptop shows paired + bonded). Tapping 1-9 then holding 5 for ~5 s typed
  `1234567895`; board log shows `KEY 1..9, 5` seq 1-10, all `ble=sent`.
- Found on the way:
  - The board's Bluetooth address is the base MAC + 2 (`80:B5:4E:DB:CD:BA`, base `...:B8`).
  - A stale bond from the earlier prototype made the laptop reconnect every ~2 s with a key the board
    no longer had (`remove bond, rsn 102`). Fix: remove the device on the computer and pair again.
  - With numeric comparison (the helper's default), pairing never completed: Blueman is the active
    agent, its confirm prompt went unanswered, SMP timed out every 30 s, and the computer refused key
    reports on the unencrypted link (`GATT_INSUF_AUTHENTICATION`) although the board logged them as sent.
  - Opening the serial port did not reboot the board on two more occasions.
- Decided: pairing mode is "Just Works" for now (LE Secure Connections, bonding, IO capability none),
  set in `ble_keyboard.c` after the helper's defaults. The link is encrypted; it gives up protection
  against an active attacker during pairing. Revisit by showing the code on the board's screen.
- Open:
  - Show a pairing code on the board (and confirm on the board) to restore MITM protection.
  - `spec/` v0 with Tom; data channel (BLE data service / Wi-Fi / serial) next on the ONX.
  - macOS pairing on Tom's Mac not yet tried.

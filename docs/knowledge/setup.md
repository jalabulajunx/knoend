# Setting up an OpenPager board: what we know

The one place for what has been learned getting boards working. Coding agents read it before
hardware work (see `AGENTS.md`); the companion's setup assistant has it built into its prompt.
When you learn something new on the bench, add it here (symptom, cause, fix, how it was confirmed).

## How the pieces fit

- **The board** (ESP32-S3 with a 320x240 touchscreen) runs one universal firmware image. A small
  **board definition** (JSON, in the 4 KB `board` partition at 0x310000) tells it which display,
  touch controller, pins and serial line it has. Without a definition it still boots, with no
  display ("BOARD no definition"), so it can be fixed over USB.
- **The companion service** (`openpager serve`) runs on the computer: it flashes and pairs boards
  over USB, then talks to the board over Wi-Fi (or USB serial), follows the focused app and pushes
  matching layouts. Its web app is at `http://127.0.0.1:7342/`.
- **Keys reach the computer two ways.** The board can type by itself as a Bluetooth keyboard (no
  software needed). If no Bluetooth host is connected, it sends the keystroke to the companion,
  which types it (X11 XTest on Linux; keyboard events on macOS).

## Boards we know

| Board | USB-C goes to | Serial line | Can be a USB keyboard? | Notes |
| --- | --- | --- | --- | --- |
| ONX2432G028 (Nextion Open G-series) | CH340K serial chip (USB `1a86:7522`) | UART0, 115200 | **No**: the S3's own USB pins go to the mic connector | ST7789 display, CST826 touch at 0x15, PCF8574 expander at 0x38, 16 MB flash, 8 MB octal PSRAM |
| Freenove FNK0104B | the ESP32-S3's own USB (Espressif `303a:xxxx`) | USB-Serial-JTAG | Yes (not built yet) | ILI9341 display, FT6336 touch at 0x38; definition not written yet |

Any ESP32-S3 board with octal PSRAM and at least 4 MB flash can run the universal image once it has
a definition. Quad-PSRAM boards need a second image variant (not built yet). Other chips (ESP32,
C3, C6) are not supported.

## Symptoms, causes, fixes

### Finding the board

- **No port listed.** Check, in order: the cable carries data (many USB-C cables are charge-only;
  try another); the board is powered (screen or LED on); on Linux, `dmesg` shows the CH340 or
  Espressif device; on macOS, recent versions include CH34x and Espressif drivers (older macOS may
  need the WCH driver, unverified). Try a different USB port, avoiding hubs.
- **Port listed but cannot be opened (permission denied).** On Linux the user must be in the group
  that owns the port (`uucp` on Arch, `dialout` on Debian/Ubuntu): `ls -l /dev/ttyUSB0`, then add the
  user to that group and log out and in. Also close anything else holding the port (serial
  monitors, `idf.py monitor`, another `openpager`).
- **Opening the port resets the ONX.** Its CH340 auto-reset circuit can restart the chip when a
  program opens the port (seen with the companion; not with pyserial). The companion therefore
  takes the board's start-up announcement as its identity. A board that restarts when you connect
  is normal.

### Identifying and flashing

- **"Could not reach the chip's bootloader".** Another program holds the port; or the cable or USB
  port is flaky; or the board's auto-reset does not work. Manual bootloader mode: hold BOOT (GPIO0),
  tap RESET, release BOOT, then try again.
- **Install failed part way ("communication error").** Retry once. Two known causes, both fixed in
  the companion: writing parts in separate sessions restarted the chip after the first part, and a
  short final part (the board definition) lost its last bytes; the definition is now padded to the
  full 4 KB partition. If it still fails: different cable or port, lower baud, then manual
  bootloader mode.
- **Verification failed on the board definition only.** Same padding issue as above; make sure the
  companion is current.
- **Erase or not?** A board that already runs OpenPager does not need erasing: updates keep its
  pairing keys and Wi-Fi settings (the settings partition never moves). Erase a board that ran other
  firmware, or one that misbehaves after an update. Erasing removes pairing and Wi-Fi; the person
  must agree first.
- **Wrong board chosen.** The universal image is the same; only the definition differs. Flash again
  with the right board; nothing is damaged by a wrong display definition, the screen just stays
  blank or garbled.

### After flashing

- **Screen stays dark, but the serial log shows READY.** Look for `BOARD no definition`: the
  definition is missing or invalid; reinstall with the right board. If the definition loaded
  (`board: definition <id>`), the backlight pin or display wiring in the definition is wrong.
- **Picture rotated, mirrored or colours swapped.** Definition values: `rotation`,
  `mirror_x`/`mirror_y`/`swap_xy`, `bgr`, `invert`. Touch landing on the wrong key: the touch
  `swap_xy`/`mirror_*` values. The ONX's verified values are in
  `firmware/boards/definitions/onx2432g028.json`.
- **Taps do nothing.** First decide whether the board registers them: the serial log prints
  `KEY n ...` per tap and the key flashes blue. If taps register:
  - `hid-sent` but nothing typed: Bluetooth is connected to a different device or the wrong host.
  - `hid-via-companion` but nothing typed: the companion is not running or not connected, or the
    key is not on the computer's keyboard layout (F13-F24 are often missing on Linux X11; they work
    over Bluetooth).
  - `event-sent`: the action needs the companion (open a page, run a command).

### Wi-Fi and pairing

- **Board never joins Wi-Fi.** The ESP32-S3 joins **2.4 GHz only**; a 5 GHz-only network, or a band
  steering router that hides 2.4 GHz, will fail (`WIFI disconnected reason=201` = network not found;
  reason 15 or 204 = wrong password). Captive-portal networks (hotels, offices) do not work.
- **Companion cannot find the board** although it joined: mDNS (`_openpager._tcp`) is blocked
  between the computer and the board (guest networks, client isolation, some mesh systems, a VPN
  on the computer). Same network, not a guest one; disconnect the VPN.
- **Found but the handshake fails ("board proof does not verify").** The board and this computer
  hold different keys (board erased, or paired from another computer). Pair again over USB.
- **Board restarted and the dashboard still says connected.** The companion pings every 10 s and
  reconnects after 25 s of silence; wait half a minute.

### Bluetooth keyboard

- **Laptop Bluetooth off or soft-blocked** (`rfkill list bluetooth` shows "Soft blocked: yes").
  Keys still work through the companion; Bluetooth is only needed without the companion.
  `rfkill unblock bluetooth`.
- **Pairing loops (connect, disconnect every few seconds).** The computer holds an old bond the
  board no longer has. Remove the device on the computer and pair again.
- **Pairing never completes with a number prompt.** The board uses "Just Works" (no number); if a
  desktop agent (e.g. Blueman on Linux) shows a confirmation, accept it. The board's Bluetooth
  address is its base MAC + 2.

### macOS

- **Keys typed by the companion do nothing on a Mac.** macOS only lets programs type into other apps
  with the Accessibility permission: System Settings > Privacy & Security > Accessibility, add the
  app that runs `openpager` (Terminal, or the `openpager` binary), then restart it. The companion
  reports this instead of failing silently. (Written without a Mac to test on; confirm on one.)
- **"openpager" cannot be opened because the developer cannot be verified.** The binary is not
  signed yet: right-click > Open once, or `xattr -d com.apple.quarantine openpager`.
- **Two ports per board** (`/dev/cu.*` and `/dev/tty.*`): the companion lists only the `cu.*` one,
  which is the one to use.

### The AI pieces

- **Configure fails** with "environment variable ... is not set" or 401/403: no API key, or a wrong
  one, for the chosen provider. Set it in the web app's AI settings.
- **Very slow drafts with a local model.** Ollama on a CPU-only machine runs at a few tokens per
  second (about 2 minutes per draft on our test laptop); use a hosted provider for a responsive
  setup, or a machine with a supported GPU.

## When the problem is new

1. Separate the layers: power and cable; USB port and permissions; the chip's bootloader; the
   firmware's serial log; Wi-Fi; the companion; the app being typed into. Find the first layer that
   fails and work there.
2. Prefer checks that change nothing (list ports, read the serial log, identify) before actions
   that change the board (install, erase). Erase only with the person's agreement.
3. Read the board's serial log: every stage prints a line (`READY`, `BOARD`, `WIFI ...`, `KEY ...`).
4. Change one thing at a time and look at the result before the next change.
5. Never ask for or repeat passwords or API keys in messages; they go only into the web app's own
   fields.
6. If the board is not one we know, the universal image may still run it once it has a definition:
   gather the board's name, its product page or schematic, the display and touch controller names
   and the pins, then draft a definition (`spec/schemas/board.schema.json`) and test it. Never use
   GPIO 26-37 on the ESP32-S3 (flash and PSRAM).
7. Say plainly what is known, what is a guess, and what to check next.

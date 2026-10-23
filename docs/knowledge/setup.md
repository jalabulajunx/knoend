# Setting up an KnoEnd board: what we know

The one place for what has been learned getting boards working. Coding agents read it before
hardware work (see `AGENTS.md`); the companion's setup assistant has it built into its prompt.
When you learn something new on the bench, add it here (symptom, cause, fix, how it was confirmed).

## How the pieces fit

- **The board** (ESP32-S3 with a 320x240 touchscreen) runs one universal firmware image. A small
  **board definition** (JSON, in the 4 KB `board` partition at 0x310000) tells it which display,
  touch controller, pins and serial line it has. Without a definition it still boots, with no
  display ("BOARD no definition"), so it can be fixed over USB.
- **The companion service** (`knoend serve`) runs on the computer: it flashes and pairs boards
  over USB, then talks to the board over Wi-Fi (when on the same 2.4 GHz network) or directly over
  USB serial (when plugged in by USB cable without Wi-Fi), follows the focused app and pushes
  matching layouts. Its web app is at `http://127.0.0.1:7342/`.
- **Three communication options.** During setup (`Set up a board`), the board is connected over
  **USB** first, paired with this computer over USB right after installing (or when clicking
  **Skip install** on a board that already runs KnoEnd), and **Keys and look** (Step 4) is
  configured immediately over USB. **Wi-Fi** (Step 5, 2.4 GHz wireless) and **Bluetooth** (Step 6,
  the `"KnoEnd"` BLE keyboard) are separate optional steps that can be configured during setup or
  skipped and configured later.
- **Keys reach the computer two ways.** The board can type by itself as a Bluetooth keyboard (no
  software needed). If no Bluetooth host is connected, it sends the keystroke to the companion over
  Wi-Fi or USB serial, which types it (X11 XTest on Linux; keyboard events on macOS; Win32
  `SendInput` on Windows).

## Boards we know

| Board | USB-C goes to | Serial line | Can be a USB keyboard? | Notes |
| --- | --- | --- | --- | --- |
| ONX2432G028 (Nextion Open G-series) | CH340K serial chip (USB `1a86:7522`) | UART0, 115200 | **No**: the S3's own USB pins go to the mic connector | ST7789 display, CST826 touch at 0x15, PCF8574 expander at 0x38, 16 MB flash, 8 MB octal PSRAM |
| Freenove FNK0104B | the ESP32-S3's own USB (Espressif `303a:xxxx`) | USB-Serial-JTAG | Yes (not built yet) | ILI9341 display (BGR, inverted, mirror_y), FT6336 touch at 0x38 (mirror_x + mirror_y); picture, colours and touch confirmed 2026-10-18 |
| Waveshare ESP32-S3-Touch-LCD-2 (**unverified**) | the S3's own USB | USB-Serial-JTAG | Yes | ST7789T3 SPI 240x320 (colour inversion on), CST816D touch at 0x15, backlight GPIO 1 |
| Espressif ESP32-S3-BOX-3 (**unverified**) | the S3's own USB | USB-Serial-JTAG | Yes | ILI9342C (ILI9341 driver + init commands) 320x240, BGR, mirrored x+y; GT911 touch at 0x5D or 0x14; active-high reset shared by display and touch on GPIO 48 |

**Unverified** boards have definitions written from the maker's documentation but nobody has run
them yet. If one shows a blank, rotated, mirrored or wrongly coloured screen, or touch lands in the
wrong place, the definition values are the first suspect (see "After flashing"); report what you
saw so the definition can be corrected and promoted to supported. More boards and what they would
need: `docs/boards/research.md`.

Supported drivers: displays ST7789 and ILI9341 over SPI (plus start-up commands from the definition);
touch CST816, CST826, FT6336 (FocalTech register layout) and GT911. Any ESP32-S3 board with octal
PSRAM, at least 4 MB flash and these parts can run the universal image once it has a definition. Quad-PSRAM boards need a second image variant (not built yet). Other chips (ESP32,
C3, C6) are not supported.

## Symptoms, causes, fixes

### Finding the board

- **No port listed.** Check, in order: the cable carries data (many USB-C cables are charge-only;
  try another); the board is powered (screen or LED on); on Linux, `dmesg` shows the CH340 or
  Espressif device; on macOS, see "USB device seen but no port" below. Try a different USB port,
  avoiding hubs. The wizard and `system_checks.usb_serial_devices` (Linux and macOS) show a
  USB-serial chip that is plugged in but has no port, so "no cable/power" and "no driver" can be
  told apart without commands.
- **Linux: chip plugged in, no port.** `usb_serial_devices` shows `has_serial_port: false`. With
  `drivers: ["usbfs"]` another program holds the chip: usually `brltty` (a Braille display service
  that some distributions, Ubuntu among them, ship and which grabs CH340s); remove or stop it, then
  replug. With no driver at all, the kernel module (`ch341`, `cp210x` or `cdc_acm`) is not loaded:
  `sudo modprobe ch341`. (Widely reported; not yet seen on our machines.)
- **Port listed but cannot be opened (permission denied).** On Linux the user must be in the group
  that owns the port (`uucp` on Arch, `dialout` on Debian/Ubuntu): `ls -l /dev/ttyUSB0`, then add the
  user to that group and log out and in. Also close anything else holding the port (serial
  monitors, `idf.py monitor`, another `knoend`).
- **Opening the port resets the ONX.** Its CH340 auto-reset circuit can restart the chip when a
  program opens the port (seen with the companion; not with pyserial). The companion therefore
  takes the board's start-up announcement as its identity. A board that restarts when you connect
  is normal.

### Identifying and flashing

- **A board KnoEnd does not know (#63).** Identify (and `knoend devices`, and the `identify_board`
  check) reads the chip, its flash size, and, without writing anything, the firmware on it: the
  partition table and each app's project, version and components. A vendor's firmware names the
  board's support package and its display and touch drivers, which identify the board when the chip
  cannot. A board KnoEnd lists with another flash size is never offered; when no listed board
  matches the chip, flash size and USB connection, the answer is "a board KnoEnd does not know" and
  nothing is chosen for the person. Seen 2026-10-21: an ESP32-S3 with 32 MB flash on native USB
  carried `phone_s3_box_3` (a demo's name, not the board) and `xiaozhi` 1.8.5 with components
  `waveshare__esp32_s3_touch_amoled_2_06` and `waveshare__esp_lcd_sh8601`: a Waveshare
  ESP32-S3-Touch-AMOLED-2.06. Its QSPI AMOLED is not a display KnoEnd's firmware drives (SPI LCDs
  with ST7789 or ILI9341; touch GT911, CST816, CST826, FT6336), so KnoEnd cannot run on it yet and
  must not be installed: the screen would stay blank. Before trying anything on such a board, back
  its flash up (`esptool.py read_flash 0 <flash size> backup.bin`); `write_flash 0 backup.bin`
  restores it.

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
- **Erase or not?** A board that already runs KnoEnd does not need erasing: updates keep its
  pairing keys and Wi-Fi settings (the settings partition never moves). Erase a board that ran other
  firmware, or one that misbehaves after an update. Erasing removes pairing and Wi-Fi; the person
  must agree first.
- **Wrong board chosen.** The universal image is the same; only the definition differs. Flash again
  with the right board; nothing is damaged by a wrong display definition, the screen just stays
  blank or garbled.
- **Boards on the S3's own USB look identical.** The Freenove FNK0104B, ESP32-S3-BOX-3 and Waveshare
  boards all show an Espressif `303a:1001` port, an ESP32-S3 and 16 MB flash, so the wizard cannot
  tell them apart. Since 2026-10-18 the wizard does not pre-select when the top boards tie; it asks
  the person to pick. Ask the person which board they have (name on the box or the back of the
  board) before installing. Seen 2026-10-17: a Freenove FNK0104B got the
  BOX-3 definition; it ran (Wi-Fi, pairing, keys) with a dark screen.
- **Picture mirrored, or taps landing on the opposite key** (seen on the Freenove FNK0104B,
  2026-10-18): firmware before `tom/fnk0104b` ignored a definition's `display` mirror/swap (the LVGL
  port re-applied its own). With the fix the picture follows `display.mirror_*`; touch has its own
  `touch.mirror_x/mirror_y/swap_xy` and must be flipped the same way. A definition alone can be
  rewritten with `esptool write_flash 0x310000 <definition.json>` (no firmware flash); confirm by
  tapping the corner keys. Once KnoEnd runs, the board
  reports its definition and identify ranks that board first, even if it was the wrong one.

### After flashing

- **Screen stays dark, but the serial log shows READY.** Look for `BOARD no definition`: the
  definition is missing or invalid; reinstall with the right board. If the definition loaded
  (`board: definition <id>`), the backlight pin or display wiring in the definition is wrong.
- **GT911 touch not found or touch dead on a GT911 board.** The GT911 picks its I2C address (0x5D or
  0x14) when it is reset; the firmware tries both after the display has started (some boards share
  the reset line). Early ESP32-S3-BOX-3 units use a TT21100 instead, which is not supported.
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

- **Asked to pair and join Wi-Fi again after installing new firmware.** Not needed when the board
  was installed without erasing: it keeps its pairing, Wi-Fi and settings, restarts and joins its
  network by itself within about 20 s. Setup now waits for it and skips pairing when it comes back
  (`find_boards_on_network` shows it with `paired_with_this_computer`). Only an erase, or a board
  paired from another computer, needs pairing again. Confirmed on the ONX, Linux, 2026-10-20.
- **"no knoend.wifi.ack from the board within 3s" seconds after installing.** The pairing step
  worked, then the board did not answer the Wi-Fi step in time: it had just restarted and was still
  joining its stored network. Doing it again a little later worked (ONX, Linux, 2026-10-20). The
  Wi-Fi step now waits 10 s and tries once more by itself. If it still fails: wait half a minute and
  check whether the board came back by itself before pairing again.

### Wi-Fi and pairing

- **Board never joins Wi-Fi.** The ESP32-S3 joins **2.4 GHz only**; a 5 GHz-only network, or a band
  steering router that hides 2.4 GHz, will fail (`WIFI disconnected reason=201` = network not found;
  reason 15 or 204 = wrong password). Captive-portal networks (hotels, offices) do not work.
- **Board worked, then stopped connecting after the router changed** (new router, new Wi-Fi
  password, moved house): the board keeps retrying the network it knows (`WIFI disconnected
  reason=201` network not found, 15 or 204 wrong password). Fix: plug it in with USB and use
  **Change Wi-Fi** on the dashboard (or the assistant's Change Wi-Fi button): pick the new network;
  pairing and settings stay. Pair again is not needed, and erasing is never needed for this. Over
  USB only for now (the board takes Wi-Fi details only over the cable), which also covers a
  computer that already moved to the new network. Confirmed from the code 2026-10-18.
- **Companion cannot find the board** although it joined: mDNS (`_knoend._tcp`) is blocked
  between the computer and the board (guest networks, client isolation, some mesh systems, a VPN
  on the computer). Same network, not a guest one; disconnect the VPN.
- **Found but the handshake fails ("board proof does not verify").** The board and this computer
  hold different keys (board erased, or paired from another computer). Pair again over USB.
- **Board restarted and the dashboard still says connected.** The companion pings every 10 s and
  reconnects after 25 s of silence; wait half a minute.

### How the board looks

- **Several boards (#15):** every paired board the computer reaches is connected at once, and they
  mirror each other: the same keys, a tap on any of them runs it. **Each board keeps its own look**
  (key size, theme, way round, fonts, start-up size; `board_looks` in the snapshot); a board paired
  later starts from the defaults (Dark, its usual way round, medium keys, built-in font), and the
  settings list's "board" says which board the look settings show. While several are connected,
  pages use the smallest of their key-size grids, so they show the same pages; each board draws
  them at its own size. **Way round is named for how each board usually sits**: an AMOLED watch is
  portrait at turn 0, so its "Portrait" is turn 0 and "Landscape" turn 1. **A board shows the wrong
  look after pairing:** check that the change named that board (`board` in the settings fix).

- **Key size, Look and Way round** are settings (dashboard, setup's "Keys and look" step, and the
  assistant's settings list). Key size is the grid every page uses (Big 2x2, Medium 3x3 by default,
  Small 4x4); an app's keys fill it in order and the rest go on further pages. The look (Dark,
  White, Sepia) and the way round (landscape, portrait, either upside down) are kept on the board
  and change at once, without a restart. Portrait on the ONX fits 3 keys across and 4 down.
- **"this firmware draws landscape only" or "has one look only".** The board's firmware is older
  than the setting: install the latest firmware (without erasing). The web app puts the choice back
  to what the board shows and says so.
- **Sepia looks like dimmed white.** The first sepia (a pale cream) did on the ONX's 16-bit panel;
  firmware from 2026-10-20 on uses a stronger tan and brown. Install the latest firmware.
- **Keys too small to read or hit.** Choose a bigger key size (Medium or Big), or White or Sepia
  for contrast.
- **Fonts** (settings `key_font`, `start_font`): any Google Fonts family, by its exact name. The
  service downloads it from Google when chosen (trimmed to the characters the board shows, as
  TrueType; no account or key; nothing stored on the computer) and sends it to the board, which
  keeps it in its own flash, so the start-up name shows before the computer connects. Each font
  must be under about 470 KB after trimming (`board.fonts.capacity`). A character the font lacks
  is drawn in Montserrat (Edu AU VIC WA NT Dots has no ° for "Kn°End"). Decorative or dotted fonts
  are hard to read on keys at 12-20 px: suggest them for the start-up name, plain ones
  (Atkinson Hyperlegible, Lexend) for keys. Fonts need firmware from 2026-10-20 on, flashed with
  the new partition table (any web-app install writes it; no erase needed).
- **The board restarts over and over after a font was chosen.** Seen 2026-10-20 with WDXL Lubrifont
  SC for keys: its kerning table points past the end of the font, and the renderer crashed reading
  it at every start (serial log: "Cache disabled but cached memory region accessed", backtrace in
  stb_truetype `GetGlyphKernAdvance`). Fixed in firmware after v0.1.1-110: kerning tables are hidden
  from the renderer, fonts are read from a copy in PSRAM, and a font the board did not get through
  a start with is dropped at the next start (`board.fonts.refused`; the service then does not send
  it again and says so). With firmware v0.1.1-110 itself: install the latest firmware (no erase).
- **Board stops answering on Wi-Fi, or freezes on a tap, with its screen still on; serial shows
  repeated "task_wdt: Task watchdog got triggered ... IDLE0 ... CPU 0: taskLVGL".** The board has
  not restarted (the number in brackets is milliseconds since it started): the display task is
  spinning while it waits for the screen to report an update done (`draw_buf_flush`), and Wi-Fi
  starves. Cause, found 2026-10-20: LVGL's memory came from plain malloc (since fonts,
  v0.1.1-110 to -124), whose small allocations take internal RAM first; the SPI display driver then
  could not get the internal buffer it sends each update from, the update failed silently and its
  "done" never came. Fixed in v0.1.1-126: LVGL allocates from PSRAM first
  (`firmware/main/lv_mem_psram.c`); a guard in `board_generic.c` gives up after 2 s and redraws
  everything ("display: an update never reported done"). Measured on the ONX: 30 lost updates in
  75 s before, none in 32 minutes after. Unplug and plug the board in to get a frozen one back.
  A watchdog line is not a crash; "Guru Meditation" or `rst:` with a panic is.
- **Edit keys says "page 1 is 4x4; this board draws at most 3x3".** Usually the board's firmware is
  older than v0.2.0 (v0.1.1 draws a fixed 3x3 and reports no grid, so the service assumes 3x3): an
  AI draft or recipe with a 4x4 page cannot be edited for it. Install the latest firmware without
  erasing (seen on Sundar's Windows machine, 2026-10-20). In portrait a v0.2.0 board draws at most
  3x4, so a 4x4 page cannot be edited there either: turn it to landscape to edit.
- **Several Claude questions in one prompt (#41).** Claude Code reports nothing until the last is
  answered. While the person is in the app Claude runs in (or once they started answering there)
  the board shows a panel listing the questions with "Answer here"; otherwise it shows the
  questions with their answer keys. It clears as soon as Claude has every answer.
- **"Could not get the font" or "could not reach Google Fonts".** The computer is offline or
  Google Fonts is blocked (some offices and schools); the name may also be misspelt: pick it from
  the list.

### Listening for notes (music apps)

A board with a microphone (the ONX2432G028 with the Nextion Dual MIC board on its MIC_PDM
connector) listens while a Listen key is on and sends each note it hears; the service passes them
to music apps as MIDI from a device called "KnoEnd". The board's own serial log, while it listens,
prints `listen: loudest N dB in the last second` and `listen: note C4 (...)`.

- **Symptom:** the board listens but hears nothing; the log shows `microphone raw: left -30935..-30935,
  right -30935..-30935` (one fixed number) and loudness stays near -97 dB. **Cause:** the
  microphone sends no data: its ribbon is not fully in the connector, or the ribbon or mic board is
  faulty. **Fix:** switch the board off and unplug it, open the connector latch, push the ribbon in
  squarely and fully (bare contacts toward the connector's pins), close the latch; else try another
  ribbon or mic board. Not the firmware. **Confirmed** 2026-10-21 on the ONX (Sundar): a reseated mic
  read live values and heard every note; the walkie-talkie project saw the same -30935 in July and
  fixed it the same way.
- **Symptom:** notes are heard (service log `note C4 ... sent as MIDI`) but nothing reaches the music
  app. **Cause:** the app reads another MIDI input. On Linux the service also sends into "Midi
  Through", which MuseScore reads by default; an app set to a different input needs "KnoEnd" chosen
  as its MIDI input. In MuseScore the notes are written only in note input (N) with a bar selected.
  **Confirmed** 2026-10-21, MuseScore 4.7 on Arch Linux (Sundar).
- **Symptom:** soft singing or a distant instrument gives no notes; the log shows loudness a little
  under the quiet level. **Fix:** lower the `listen_quiet` setting (for example -70). Raise it
  (-45) when room noise makes stray notes.
- Windows has no virtual MIDI device without a driver: the service sends to an output called
  "KnoEnd" made with loopMIDI, and says so when there is none (untested).

### Bluetooth keyboard

- **Bluetooth carries the keys and, with current firmware, the service's link too.** A board paired
  with the computer as a Bluetooth keyboard types its keys. The service also reaches it over the
  KnoEnd data service beside the keyboard (identity `transports` has `ble-data`) when the board is
  on neither Wi-Fi nor USB: the Board section then shows its address as "Bluetooth". **The board
  types but does not follow apps, and the Board section says "paired, not connected":** its
  firmware has no data service (update it), or it is not paired as a keyboard with this computer,
  or Bluetooth is off. Linux (BlueZ) tested 2026-10-21 (found in about 3 s, ping 90 ms); macOS
  needs the Bluetooth permission for the app the service runs from; Windows untested. Fonts over
  Bluetooth are slow (10-20 s).

- **Windows says "Try connecting your device again" when pairing "KnoEnd".** The Windows Bluetooth
  radio adapter or stack frequently hangs or holds a half-formed handshake attempt. Fix: toggle
  Bluetooth off and back on in Windows Settings (or press Win+A > Bluetooth), or remove "KnoEnd"
  from Bluetooth & devices if listed, then click it again. If the modern Settings flyout still
  fails, run `devicepairingwizard.exe` (Win+R > `devicepairingwizard`).
- **Laptop Bluetooth off or soft-blocked** (`rfkill list bluetooth` shows "Soft blocked: yes").
  Keys still work through the companion; Bluetooth is only needed without the companion.
  `rfkill unblock bluetooth`.
- **Pairing loops (connect, disconnect every few seconds).** The computer holds an old bond the
  board no longer has. Remove the device on the computer and pair again.
- **Pairing never completes with a number prompt.** The board uses "Just Works" (no number); if a
  desktop agent (e.g. Blueman on Linux) shows a confirmation, accept it. The board's Bluetooth
  address is its base MAC + 2.
- **The board takes minutes to connect, or the computer briefly shows a Bluetooth pairing prompt
  every 20 s (macOS focuses `BluetoothUIServer`; the board shows "Unsupported" for a moment).** A
  board paired with this computer earlier is not around (a friend's board, one since erased), and
  the service looks for it over Bluetooth. It cannot tell boards apart before opening them, so it
  opens the board that is connected over Wi-Fi too; a board not paired as a keyboard with this
  computer refuses (its serial log: `GATT_INSUF_AUTHENTICATION`, then `smp_rsp_timeout` about 30 s
  later), and the service waits that long. Seen on macOS with the FNK0104B, 2026-10-22: 2.5 minutes
  to the first connection. Workaround until the service is fixed: remove the absent board's pairing
  (a new set-up with that board pairs it again). Confirmed: with the stale pairing removed, the
  board connected in 2 s and no Bluetooth attempt followed.

### macOS

Confirmed on macOS 26.6 (Apple silicon) on 2026-10-17 with the ONX2432G028 and the Freenove
FNK0104B: install, pairing, Wi-Fi, layouts following the focused app, keys typed by the companion,
and drafting a layout with OpenAI. The assistant's snapshot (`system_checks`) shows the USB serial
devices and whether each has a port, the WCH driver state, and whether knoend may type.

- **USB device seen but no port** ("No board found"; `system_checks.usb_serial_devices` shows the
  device with `has_serial_port: false`). macOS sees the board's USB chip but no driver claimed it.
  Apple's built-in CH34x driver covers `1a86:7523` and `1a86:55d4` only; the ONX2432G028's CH340K is
  **`1a86:7522`**, which needs WCH's own driver (the same board works on Linux, whose `ch341` driver
  covers it). Fix: "Install the USB driver" button (or `CH34xVCPDriver.pkg` from
  github.com/WCHSoftGroup/ch34xser_macos, notarized, signed by Nanjing Qinheng Microelectronics,
  team 5JZGQTGU4W). The installer asks for the Mac password and may show errors yet finish.
- **Driver installed, still no port.** `system_checks.wch_driver_extension` (or
  `systemextensionsctl list`) says `activated waiting for user`: macOS has not been allowed to load
  it. System Settings > General > Login Items & Extensions > Driver Extensions (the "Open Driver
  Extensions" button), switch on CH34xVCPDriver, then unplug and replug the board. Working state:
  `activated enabled`, and a port `/dev/cu.wchusbserial...` appears.
- **Keys typed by the companion do nothing on a Mac.** The log says "macOS needs Accessibility
  permission". macOS only lets a program type into other apps with Accessibility, and it asks for
  it for the **app that launched `knoend`**, not the binary: Terminal, iTerm, or Visual Studio
  Code when started from VS Code's terminal or a coding agent inside it
  (`system_checks.accessibility.app_to_allow` names it). System Settings > Privacy & Security >
  Accessibility (the "Open Accessibility settings" button), switch that app on, then restart
  `knoend`; restarting the app itself was not needed on macOS 26.6.
- **The layout does not follow the app on a Mac** (it stays on the first app). Fixed in
  v0.1.1: `NSWorkspace.frontmostApplication` never updates in a program without a main run loop, so
  the watcher now asks the window server which app owns the front window. With v0.1.0, update.
- **"knoend" cannot be opened because the developer cannot be verified.** The binary is not
  signed yet: right-click > Open once, or `xattr -d com.apple.quarantine knoend`.
- **Two ports per board** (`/dev/cu.*` and `/dev/tty.*`): the companion lists only the `cu.*` one,
  which is the one to use.
- **Building the companion on a Mac** (developers only; not needed with the release binary):
  `cargo build` fails at linking with `xcrun: error: invalid active developer path` until the Xcode
  Command Line Tools are installed (`xcode-select --install`). If it then fails with `tapi error:
  malformed file ... MacOSX27.0.sdk ... unknown architecture`, an SDK newer than the installed
  linker is left over and `xcrun` picks it because it is the highest version. Seen 2026-10-17/18
  after Command Line Tools 26.5, 27.0 and 26.6 installed in one run: 26.6's linker (`ld -v`) stayed,
  27.0's SDK stayed with it, and no installed package owns that SDK (`pkgutil --file-info
  .../MacOSX27.0.sdk/usr/lib/libSystem.tbd` names none). Fix: remove the stray SDK, `sudo rm -rf
  /Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk
  /Library/Developer/CommandLineTools/SDKs/MacOSX27.sdk`, then `xcrun --kill-cache`; `xcrun
  --show-sdk-path` should name `MacOSX.sdk` and a plain `cargo build` links (confirmed on macOS 26.6).
  Prefixing one build with `SDKROOT=$(xcrun --sdk macosx26.5 --show-sdk-path)` only works for that
  command; the next shell fails again. `xcrun --kill-cache` alone does not help.

### Windows

- **Installing:** the release's `knoend-setup-<version>-x86_64.exe` installs for the person only (no
  administrator rights) to `%LOCALAPPDATA%\Programs\KnoEnd`, adds KnoEnd to the Start menu, and by
  default runs `knoend service install` and `knoend browser install`. **"Windows protected your PC" /
  "unknown publisher":** the installer is not code-signed yet; More info > Run anyway. The Start menu
  entry runs `knoend serve`, which opens the running KnoEnd's web app when the background one is
  already up. Uninstalling (Settings > Apps) removes the startup entry and the browser
  registrations and stops knoend; the settings in the person's config folder stay.

The assistant's snapshot (`system_checks`) shows `usb_serial_devices` from Windows SetupAPI (every
known USB-serial chip on the USB bus, whether or not a COM port driver is installed, and its bound
Windows service in `drivers`), `keys_follow_apps: true`, and `service_can_type: true`.

- **USB device seen but no COM port** (`system_checks.usb_serial_devices` shows `has_serial_port:
  false`). Native ESP32-S3 USB (`303a:1001`, Freenove FNK0104B) uses Windows's built-in `usbser`
  driver automatically. A CH340 (`1a86:7522` on the ONX2432G028, `1a86:7523`, `1a86:55d4`) or
  CP210x (`10c4:ea60`) may need its driver if Windows has not installed it yet: open Windows
  Settings > Windows Update > Advanced options > Optional updates > Driver updates (look for
  `wch.cn` Ports or Silicon Labs), or install WCH's `CH341SER` / Silicon Labs CP210x driver, then
  unplug and replug the board.
- **Port listed (`COM3`, `COM4`, ...) but cannot be opened ("Access is denied").** On Windows only
  one program can open a COM port at a time. Close any serial monitor (Arduino IDE, PuTTY, VS Code
  Serial Monitor, `idf.py monitor`, or another `knoend`) and try again. Also: rapid reconnects or
  Wi-Fi setup right after flashing/pairing on CH340 boards can briefly see `ERROR_ACCESS_DENIED` while
  the driver finishes closing the previous handle; `SerialLink::open` and `knoend_flash::connect`
  retry automatically with backoff (~6 attempts over 1.2 s).
- **Removing a board from the web app (#77).** Click "Remove board" on the main Board card (or "Remove"
  on any secondary board in the board list). The modal offers two choices: **Forget & unpair** (drops
  the active connection, removes the board from `companion.json` and clears its look) or **Remove look only**
  (resets custom display settings back to defaults while keeping the board connected). If a board was
  previously removed but returned upon refresh, update to v0.2.2 where `display.rs` preserves empty
  board maps.
- **Keys typed by the companion do nothing in one specific window ("SendInput sent 0 of ... events").**
  Windows needs no special permission to type into ordinary apps, but User Interface Privilege
  Isolation (UIPI) blocks a normal process from typing into an elevated ("Run as administrator")
  window unless `knoend` is also run as Administrator (or over the board's Bluetooth keyboard).
- **Started from a coding agent's sandboxed shell on Windows (Antigravity / Codex) and focus was
  empty (`GetForegroundWindow` returned 0).** Seen 2026-10-20 on Windows 11:
  `codex-windows-sandbox-service` runs child processes on an isolated non-interactive desktop where
  `GetForegroundWindow()` is 0 and `EnumWindows` lists no windows. Fixed in `focus/src/windows.rs`
  and `input/src/windows.rs`: the focus and input threads attach to the active interactive desktop
  via `OpenInputDesktop` + `SetThreadDesktop` before querying windows or calling `SendInput`.
- **Tapping an app or site on the launcher (while on the desktop) logs `launcher: opened` on
  Windows, but the window does not come to the front.** Seen and fixed 2026-10-20 on Windows 11:
  three causes overlapped. First, apps such as PowerPoint (`POWERPNT.EXE`) and Edge (`msedge.exe`)
  create visible helper/tool/cloaked top-level `HWND`s ahead of the main window in `EnumWindows`;
  `find_window` now filters with `is_main_window` (visible, not DWM-cloaked via `DWMWA_CLOAKED`,
  not `WS_EX_TOOLWINDOW` or `WS_EX_NOACTIVATE`, unowned or `WS_EX_APPWINDOW`, and non-empty title
  length). Second, the Windows Foreground Lock and "Show Desktop" (`WorkerW`) block a background
  `SetForegroundWindow` call; `bring_to_front` now attaches thread input, unlocks the foreground
  lock (`AllowSetForegroundWindow` + a synthetic `VK_NONAME` key event), restores/raises the window
  above `WorkerW`, and calls `SetForegroundWindow` + `SwitchToThisWindow`. Third, when `knoend` is
  started inside a sandboxed shell, `std::process::Command::spawn` inherits the hidden sandbox
  desktop; `spawn_on_input_desktop` launches newly opened apps and URLs via `CreateProcessW` with
  `lpDesktop = "Winsta0\\Default"`. Confirmed on Windows 11 with PowerPoint, WhatsApp, File
  Explorer, Microsoft Edge and Antigravity.
- **Running in the background on Windows:** `knoend service install` adds `KnoEnd` to
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` and starts `knoend serve --no-open` in the
  background with no console window; `knoend service status` checks it.
- **Browser extension on Windows:** `knoend browser install` writes the native messaging host
  manifests under `%USERPROFILE%\.config\knoend\native-messaging-hosts\` and registers them in
  `HKCU\Software\...\NativeMessagingHosts` for Chrome, Edge, Brave, Chromium and Firefox.

### Coding agents asking on the board

- **Claude Code's approvals and "needs input" on the board:** `knoend claude-hooks install` adds
  a PermissionRequest and a Notification hook to `~/.claude/settings.json` (backup:
  `settings.json.before-knoend` (`settings.json.before-openpager` if made before the rename); `knoend claude-hooks uninstall` removes them). They apply to
  new Claude Code sessions in the terminal and in the desktop app. A request shows on the board as
  Allow / Always / Deny / Stop with the tool and command on the status line; destructive commands
  need a second tap. Claude's own prompt stays: whichever is answered first applies; a request
  answered on screen leaves the board within a second; with no service running, the hook says
  nothing and Claude asks as usual.
- **Nothing reaches the board from Claude:** the session was started before the hooks were installed
  (restart it), or the running `knoend serve` is older than the hooks (no `agents.sock` in
  `$XDG_RUNTIME_DIR/knoend/`): restart it.
  Also: Claude Code's auto-approval (or a command it allows by itself, such as `ls`) asks nothing,
  so no request is made and nothing reaches the board. Switch the session to manual approval and
  ask for a command that needs it (seen on macOS 2026-10-18; manual approval then worked: Deny,
  Allow and questions from the board).
- **Keys for the program in a terminal tab (Claude Code, `less`/`man`):** in a terminal app
  (Terminal, iTerm2, Ghostty, Xfce Terminal...) the board shows keys for the program running in
  the active tab, and the terminal's own keys at a shell prompt. The active tab is found from the
  window's title first (the terminal shows the active tab's title; fish and zsh put the running
  command and the folder in it), then from the tab typed in last. The `last_terminal` check shows
  what the service saw. **The board shows the wrong tab's keys:** the person switched tabs without
  typing and the title does not tell the tabs apart (bash sets the same title in every tab of one
  folder); typing anything in the tab fixes it, and on Linux it can take up to 8 s. **No change at
  all in VS Code's terminal pane:** by design, an editor keeps its own keys (the board cannot tell
  the editor from the pane). No permission is needed (seen on macOS 26.6 with Terminal.app,
  2026-10-20; Linux with Xfce Terminal and fish, 2026-10-21).
- **A program in a terminal tab with no keys of its own (`pacman`, `htop`):** the board shows the
  terminal's keys plus a **Configure** key (on the last page when the terminal's keys fill the
  first), and its status line says "No keys for pacman yet". Tapping it drafts keys for that
  program with the AI; they apply to it in any terminal app. `sudo pacman` counts as pacman: sudo
  runs its command on a terminal of its own, which belongs to the tab. Only the program's name and
  its one-line manual description (`whatis`) go to the AI. Drafts are listed in Edit keys under
  "Programs in a terminal". (Linux, 2026-10-21: before this, `sudo pacman` showed only the
  terminal's keys and nothing offered to make pacman's.)
- **A Claude Code session in a terminal tab:** its hooks name the tab (`knoend_tty`), so several
  questions show the "Answer here" panel only while that tab is in front, not in any terminal.
- **Claude desktop's own screen is not readable yet:** with session accessibility on, the Electron app
  still exposes only its window frames; reading its content needs it launched with
  `--force-renderer-accessibility`. The hooks make that unnecessary for Claude Code sessions.

### Linux desktops: X11 and Wayland

- **Keys never change with the app, and keys from the board are not typed (Bluetooth off), on a
  Wayland session** (GNOME and KDE default to Wayland; `XDG_SESSION_TYPE=wayland`, shown in
  `system_checks.desktop_session`). Cause: following the focused window and typing keys use X11
  (EWMH `_NET_ACTIVE_WINDOW`, XTest); Wayland hides native Wayland windows from other programs and
  blocks synthetic input. Apps running through XWayland are visible, so focus may follow a few apps
  and not others. Not supported yet (planned). What works: the board, pairing, Wi-Fi, Claude Code
  questions and approvals, notifications, media, and typing through the board's Bluetooth keyboard.
  Workaround: choose an X11/Xorg session at the login screen (GNOME: "GNOME on Xorg"; KDE: "Plasma
  (X11)"), where everything works. Confirmed from the code (2026-10-18); not yet seen on a Wayland
  machine.

### Running in the background

- **Board stops following apps after the computer or Claude Code restarts.** `knoend serve` was
  started by hand (or by a coding agent's session) and stopped with it. Install it as a background
  service: `knoend service install` (systemd user service on Linux, LaunchAgent on macOS), then
  `knoend service status`. Never run a second copy beside it.
- **Service runs but the layout does not follow the focused app (Linux).** The systemd user manager
  lacks the graphical session's `DISPLAY`/`XAUTHORITY` (`systemctl --user show-environment`); most
  desktop sessions import them. Run `systemctl --user import-environment DISPLAY XAUTHORITY` once in
  the session, or add it to the window manager's start-up (i3: an `exec` line).

### The browser extension

- **Where it comes from:** each release has `knoend-chrome-<version>.zip` and
  `knoend-firefox-<version>.zip`; neither is in a browser store yet. Chrome, Edge, Brave: unzip,
  `chrome://extensions`, Developer mode, Load unpacked, the unzipped folder (the zip's manifest
  carries the key that fixes the extension id, so the native host accepts it). Firefox:
  `about:debugging` > This Firefox > Load Temporary Add-on > the zip; unsigned, so it is gone after a
  Firefox restart until the add-on is signed.

- **Popup says "not set up".** The browser could not start knoend: `knoend browser install` was
  not run, or the browser was not restarted afterwards, or the knoend binary moved (the
  registration names its full path; run install again). Chrome only lets the extension id
  `echibclfkpicikodohehemgjefbpakpj` start it; a copy loaded with another id cannot connect.
- **Popup says "knoend not running".** Start `knoend serve`; the extension reconnects by itself.
- **The board does not change for a site.** The site needs a layout (`match.web`), and the focused
  window must be the browser. A key that acts on the page answers "not-granted" until board buttons
  are turned on for that site in the popup.
- **Media keys report done but nothing plays.** A page script may not start playback (browsers need a
  click). On Linux, media keys therefore go to the browser's media player (MPRIS), which acts like the
  keyboard's media keys; the extension reports `blocked` if it had to try and the page refused.
- **"Make keys for this site" fails with "turn on board buttons first".** The page can only be read
  after the site is granted (popup switch). The keys are saved under `libraries/ai/site-<site>.toml`.
- **Testing in Chrome:** Google Chrome 137 and later ignore `--load-extension`; load it from
  `chrome://extensions` (Developer mode), or in automation with DevTools' `Extensions.loadUnpacked`
  (`--remote-debugging-pipe --enable-unsafe-extension-debugging`).

### The AI pieces

- **The assistant can propose a change, the person applies it.** Besides buttons, its reply may
  carry a "fix": a model, an API address, request options (null = leave out), a reply timeout, or
  which board to install for. The web app lists the change and does nothing until Apply; AI
  changes are saved and tested at once. Keys, commands and files are never part of a fix.
- **The wizard keeps looking by itself.** While no board is found it checks every 4 s (and when the
  page comes back into view); a changed state (driver installed, switched on) is shown and, in
  guided mode, sent to the assistant once; when the board appears it is identified without a click.
  After the Accessibility page is opened, the page notices when typing is allowed.

- **Configure fails** with "environment variable ... is not set" or 401/403: no API key, or a wrong
  one, for the chosen provider. Set it in the web app's AI settings.
- **The provider refuses a request setting** (400: "temperature does not support 0.2", "Unsupported
  parameter: 'x'", "Use 'y' instead", Gemini's `Unknown name "x"`). Providers change what each
  model accepts. Since 2026-10-18 the companion drops (or renames, when the error names the
  replacement) the refused setting and retries by itself; confirmed against Gemini with an unknown
  option. If a model needs something else, it is a setting, never a code change: web app > AI
  settings > Advanced > request options, a JSON object added to every request, where `null` leaves a
  setting out (`{"temperature": null, "reasoning_effort": "low"}`), and the API address for a
  provider that moved or a proxy. They are saved in `agent.toml` as `[options]` and `omit`. With
  v0.1.0 (first seen 2026-10-17 with OpenAI) only the temperature was handled.
- **Every AI error now ends with "What to do:"** for its kind: 401/403 key refused; 404 model name or
  address wrong (pick from the list, which comes from the provider); 429 rate limit or credit; 5xx
  provider trouble; a connection error (address, internet, or Ollama not running). Follow that
  before anything else.
- **"Claude Code is not installed" although the person uses it** (macOS, 2026-10-19; Windows,
  2026-10-20). Claude Code can live only inside an editor extension (VS Code, Cursor, Windsurf,
  Antigravity) or inside Claude Desktop on Windows: no `claude` command on any PATH. The extension
  carries its own copy in `~/.vscode/extensions/anthropic.claude-code-<version>-<os>/resources/native-binary/claude`,
  and Windows Claude Desktop carries it in `%APPDATA%\Claude\claude-code\<version>\claude.exe` (or
  `%LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming\Claude\claude-code\<version>\claude.exe` for
  the Microsoft Store app). Likewise, the Windows OpenAI Codex app keeps `codex.exe` in
  `%LOCALAPPDATA%\OpenAI\Codex\bin\<hash>\codex.exe` and `%USERPROFILE%\.codex\plugins\.plugin-appserver\codex.exe`.
  The lookup checks all of those (newest version first, after any command on PATH), and the AI tools
  list (`GET /api/ai/tools`, the setup snapshot) shows the path it found. Confirmed on Tom's Mac
  (Claude Code 2.1.304 in VS Code) and on Windows 11 (Claude Code 2.1.271 in MSIX Claude Desktop
  and Codex CLI 0.163.0 in the OpenAI Codex Windows app). Note: Claude Desktop's bundled `claude.exe`
  has its own CLI login state (`claude auth status`), so if it reports `signed_in: false`, run that
  `claude.exe` in a terminal and type `/login`, or pick Codex CLI / another provider.
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
8. Use what the service can tell you before asking the person: the settings list (what can be
   changed, and to what), the checks it can run (the board on the network now, the full service
   log, the USB ports now) and the service log, which holds every line the service printed. These
   grow as KnoEnd grows; read them each time.
9. A board that keeps restarting: its identity says why it last started (`lastStart`: crash,
   watchdog, brownout), and the `board_serial_log` check reads its own serial output (the crash,
   its cause and backtrace) while it is not connected. The service's watcher raises such problems
   by itself and asks the assistant without the person asking ("noticed_by_the_service").
10. Before sending the person back to a step, find out whether it is needed (a board that kept its
   pairing needs none), and after a fix, check again that it worked.
11. Look below the port list. A board can be on the USB bus with no driver (macOS), or a permission
   can stop keys with every board-side step working: the companion log is the first place where
   the failing layer shows ("could not type", "no paired board", focus lines).

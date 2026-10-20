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
  over USB, then talks to the board over Wi-Fi (or USB serial), follows the focused app and pushes
  matching layouts. Its web app is at `http://127.0.0.1:7342/`.
- **Keys reach the computer two ways.** The board can type by itself as a Bluetooth keyboard (no
  software needed). If no Bluetooth host is connected, it sends the keystroke to the companion,
  which types it (X11 XTest on Linux; keyboard events on macOS).

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
- **"Claude Code is not installed" although the person uses it** (macOS, 2026-10-19). Claude Code
  can live only inside an editor extension (VS Code, Cursor, Windsurf): no `claude` command on any
  PATH, not even the login shell's. The extension carries its own copy in
  `~/.vscode/extensions/anthropic.claude-code-<version>-<os>/resources/native-binary/claude`. Since
  2026-10-19 the lookup also checks there (newest extension first, after any installed `claude`), and
  the AI tools list (`GET /api/ai/tools`, the setup snapshot) shows the path it found. Confirmed on
  Tom's Mac: Claude Code 2.1.304 from the VS Code extension drafted an app in 10 s through the
  service. If it is still not found: AI settings > "Look again"; otherwise install the command
  (claude.com/claude-code) or pick another provider.
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
9. Before sending the person back to a step, find out whether it is needed (a board that kept its
   pairing needs none), and after a fix, check again that it worked.
10. Look below the port list. A board can be on the USB bus with no driver (macOS), or a permission
   can stop keys with every board-side step working: the companion log is the first place where
   the failing layer shows ("could not type", "no paired board", focus lines).

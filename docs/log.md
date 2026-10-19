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

## 2026-10-17 - Sundar (Claude Code) - spec/ v0 draft for review

- Changed: `spec/protocol.md`, `spec/schemas/` (packet, library, Wi-Fi frame), `spec/examples/`
  (14 valid, 7 must-fail), `tools/validate_spec.py`. On branch `sundar/spec-v0`, **not merged**:
  needs Tom's review (spec/ is co-owned).
- Main choices, all open to review:
  - KDE-Connect-shaped packets `{id, type, body, re?}`; unknown types and fields ignored; 4096-byte cap.
  - Serial = one JSON packet per line (lines not starting with `{` are logs); Wi-Fi = WebSocket
    text frames; BLE data = custom GATT service (UUIDs `f0e35c51-...-97d0615e0001..3`) beside HID.
  - Layout keys carry `send`: either `hid` (board types it, works without companion; companion has
    already resolved Primary to Ctrl/Cmd) or `event` (board sends `openpager.key`).
  - Risky keys `confirm: true` = second tap within 3 s. Approval requests: no answer = no.
  - Wi-Fi frames: `{ctr, mac, packet}`, mac = HMAC-SHA256(K, ctr + "\n" + packet text). Key
    exchange not specified yet.
  - Library actions limited to shortcut / same-site open / element / named command.
- Tested: `python3 tools/validate_spec.py` (system Python 3.14, python-jsonschema 4.26): 14 valid
  pass, 7 invalid rejected, each for the intended reason (checked by reading the messages; one
  example first failed for the wrong reason and was fixed).
- Open: pairing key exchange; pages beyond 9 keys; icons; what desktop `match` patterns compare
  against per OS; BLE data service on macOS/Windows untested.

## 2026-10-17 - Sundar (Claude Code) - spec v0: pair over USB, session-bound Wi-Fi auth

- Changed (branch `sundar/spec-v0`, still not merged): `spec/protocol.md` section 4 rewritten;
  `openpager.pair` / `openpager.pair.ack` added; `wifi-frame.schema.json` replaced by
  `wifi-message.schema.json` (hello, proof, frame); test vector `examples/wifi-session.json`.
- Decided (Sundar's idea): **pair over USB.** While the board is cabled (including right after the
  companion flashes it) the companion generates a random 32-byte key and sends it in
  `openpager.pair` over serial; the board accepts that packet on serial only. No key in the
  firmware image (public, identical on every board). mDNS broadcasts only `id` and `pv`. On each
  Wi-Fi connection both sides prove the key over fresh nonces; frame MACs cover the session id
  (`Nb + Nc`) so frames cannot be replayed across sessions (the earlier draft's per-session counter
  allowed that). The 6-digit code stays as the no-cable fallback (exchange not specified).
- Tested: `python3 tools/validate_spec.py`: 16 valid pass; 10 invalid rejected, each for the
  intended reason (tampered frame, replayed handshake, repeated counter, short pairing key, plus the
  earlier six). The test vector was generated and checked by the same Python formulas, so it only
  proves consistency with the spec text; it becomes a real cross-check when the C and Rust code
  reproduce it.
- Open: where the companion stores keys per OS (keyring choice); NVS encryption on the board.

## 2026-10-17 - Sundar (Claude Code) - Serial protocol and Wi-Fi link on the ONX

- Changed (branch `sundar/serial-packets`, stacked on `sundar/spec-v0`; neither merged yet):
  - Firmware speaks spec v0 over USB serial (`link_serial`, `protocol`, `ui`): identity, ping,
    layout + ack (validated all-or-nothing), status, pair (serial only, key in NVS, up to 4
    companions, never logged), errors; keys send HID or `openpager.key`; confirm keys need a second
    tap within 3 s.
  - Wi-Fi link (`link_wifi`): credentials arrive over serial (`openpager.wifi`, never logged), mDNS
    `_openpager._tcp` + `openpager-<id>.local`, WebSocket `/ws`, handshake and session-bound
    HMAC frames per spec section 4. Replies return on the transport they came from; taps go to all.
  - Tools: `tools/serial_probe.py` (scripted serial checks + tap listener) and
    `tools/dummy_companion.py` (pair over USB, listen over Wi-Fi, attack tests).
- Tested (ONX2432G028, Arch Linux, home 2.4 GHz Wi-Fi):
  - Serial (build `c90fed3`): 16/16 probe checks; all 15 board packets valid against the schema.
    Taps: A, B, Shift C sent as HID; Hello and the long label sent `openpager.key`; Delete went
    pending then sent on the second tap (twice).
  - Wi-Fi (build `2756255`): Sundar ran `pair` (key + Wi-Fi over USB). The laptop found the board by
    mDNS, both proofs verified, identity arrived over the authenticated link, the Amazon layout was
    acked, ping answered. Taps on Search, Orders, Cart, Wish list arrived as keys 1-4 (seq 1-4);
    Buy now arrived as key 9 seq 6 after its confirm tap (seq 5, no packet). Every frame's MAC
    and counter verified and every packet passed the schema.
  - Attacks: unknown companion, replayed handshake, tampered frame were each refused; the board's
    log shows the intended reason for each (`not paired`, `proof does not verify`, `frame failed
    authentication`), with a genuine session opening normally in between.
  - Free internal RAM after start-up with BLE + Wi-Fi + LVGL: about 72 KB (min 71 KB).
  - Opening the serial port still did not reboot the board (now 5 observations).
- Bugs found and fixed on the way: replies held back until the next request (UART read waited for
  a full 256-byte chunk); Wi-Fi retry loop every 2 s with no network stored; the dummy companion's
  mDNS lookup ran inside zeroconf's callback and then inside the asyncio loop.
- Not yet confirmed: what A/B/Shift C actually typed on the laptop; a single Delete tap timing out.
- Open: layout not kept across reboots yet; approval prompts; the real companion (Rust) replaces
  the dummy; keys in an OS keyring; NVS encryption.

## 2026-10-17 - Sundar (Claude Code) - Confirmations from the serial bench test

- Tested (Sundar, by eye): A, B, Shift C typed `a`, `b`, `C` on the laptop over Bluetooth; a single
  tap on a confirm key turned it amber and timed out by itself after ~3 s without sending.
- Decided (Sundar): the layout follows the active app. For an app with no library the board shows
  "Unsupported app: <name>" with a Configure button; Configure asks the companion service to run
  its AI agent to set the app up. (Replaces the plan's automatic drafting on first use: the agent
  now runs only when the person asks.)

## 2026-10-17 - Sundar (Claude Code) - Rust companion: layout follows the focused app

- Changed (branch `sundar/companion-core`, stacked on `sundar/serial-packets`): Rust workspace in
  `companion/` with `openpager-core` (libraries, validation against the spec schema, matching,
  library -> layout), `openpager-transport` (mDNS discovery, Wi-Fi handshake + frames, serial
  pairing, key store `~/.config/openpager/companion.json` mode 600), `openpager-focus` (X11
  `_NET_ACTIVE_WINDOW` events, WM_CLASS) and the `openpager` binary (`pair`, `run`).
  First built-in library: `libraries/builtin/vscode.toml`.
- Tested:
  - `cargo test`: 11 tests. The spec's example libraries build exactly the spec's hand-written
    layout packets (a deliberate F5->F6 change made the test fail, then was reverted); invalid
    libraries rejected for the intended reasons; the Rust MAC/proof code reproduces
    `spec/examples/wifi-session.json`, which the Python tools generated (two independent
    implementations agree).
  - On the board (firmware `2756255`, Arch/i3): `openpager pair` over USB (Wi-Fi step skipped; the
    board already had the network), then `openpager run` found and authenticated the board over
    Wi-Fi. Over ~15 app switches (Claude, VS Code, Xfce terminal, Firefox, Antigravity) each switch
    pushed exactly one layout: VS Code got its library, the others the "Unsupported app: <name>"
    screen with a Configure key. Configure reached the service. Sundar: "super responsive and
    slick" (switch latency not measured in numbers).
- Decided: window titles are never logged or sent anywhere; only the app's WM_CLASS is used.
- Open: firmware card screen for unsupported apps (now a 1-key layout + status text); the AI agent
  behind Configure (any provider: OpenAI-compatible incl. OpenRouter, Gemini, Ollama; plus
  Anthropic); reconnect after the board reboots not yet exercised.

## 2026-10-17 - Sundar (Claude Code) - Branches merged to main

- Decided (Sundar): merge `sundar/spec-v0`, `sundar/serial-packets` and `sundar/companion-core`
  into `main` now; Tom reviews tomorrow and changes what he sees fit (spec/ included).
- Also confirmed by Sundar: the VS Code "Palette" key opened the command palette (typed over BLE).

## 2026-10-17 - Sundar (Claude Code) - End of day: handover state

- Changed: `docs/status.md`, a one-page map of what is on `main`, what is not started, and how to
  run it. The team's shared plan document was refreshed from the private plan (status table,
  decisions since Oct 15, who it is for, rules, a checklist for Tom).
- State: everything listed as "works" in `docs/status.md` was tested on the ONX today (see the
  entries above). Not started: FNK0104B, macOS/Windows focus, web app, local API, flashing,
  extension, simulator, agent-CLI hooks.
- Open for tomorrow: Tom reviews `spec/` and the code; firmware card for unsupported apps; layout
  kept across reboots; Eon Era access (promised at kickoff) not yet confirmed.

## 2026-10-18 - Sundar (Claude Code) - Web setup, universal image, keys without Bluetooth, setup assistant (PR #1)

- Changed (merged as PR #1, `main` 8ddb81b): `openpager serve` web app (wizard + dashboard), flashing
  (`crates/flash`, espflash), universal firmware image with board definitions (`board_generic`,
  `board` partition at 0x310000, `spec/schemas/board.schema.json`), keystrokes typed by the service
  when no Bluetooth host is connected (`crates/input`, X11 XTest), setup assistant with
  `docs/knowledge/setup.md` built in, AI-guided setup with locally stored API keys, Mac focus and
  typing backends, CI (spec, firmware, companion on Linux + macOS) and a release workflow.
- Tested (ONX2432G028, Arch Linux, home Wi-Fi):
  - From scratch, by Sundar in the web app: guided mode, Gemini key entered and tested in the page,
    identify, erase + install (4 parts verified), pair, join Wi-Fi, dashboard connected, VS Code key
    typed by the service with Bluetooth off. Service log confirms each step.
  - Universal image keeps pairing keys and Wi-Fi when updated without erasing (the settings
    partition did not move).
  - The assistant, given a real snapshot, found that focus was on Firefox and Bluetooth soft-blocked.
  - CI green on Linux and macOS (the Mac code compiles and its tests pass; behaviour untested).
- Found and fixed: separate espflash write sessions restart the chip after the first part; a short
  last part (517-byte definition) lost its final 5 bytes (now padded to the 4 KB partition); the
  identity probe missed the boot announcement; a restarted board went unnoticed (10 s heartbeat);
  CSS `display` overrode the `hidden` attribute.
- Decided: one universal image + per-board JSON definitions (Tasmota-style); keys must work without
  Bluetooth; the in-product assistant shares the knowledge base with coding agents.
- Open (UI, requested by Sundar): list models per provider instead of typing the name; scan and
  list Wi-Fi networks instead of typing the name. Also: first release (`v*` tag) not run yet;
  macOS behaviour on Tom's Mac; FNK0104B definition; dynamic tiles (PLAN backlog).

## 2026-10-18 - Sundar (Claude Code) - Board research; GT911 and two unverified board definitions

- Changed: `docs/boards/research.md` (12 popular ESP32-S3 touch boards vs the universal image, from
  manufacturer pages and board-support code). Firmware: GT911 touch reader (written from its
  register map; probes 0x5D and 0x14 after the display has started), `cst816` touch name,
  `init_cmds` and `reset_active_high` in board definitions. Two definitions marked **unverified**:
  `waveshare_s3_touch_lcd_2` (pins from Waveshare's demo zip, downloaded with Sundar's OK, read for
  pin numbers only) and `esp32_s3_box_3` (from Espressif's Apache-2.0 esp-bsp). New board status
  "unverified" in the catalog, manifest and wizard (installable, clearly marked, ranked after a
  supported match on equal evidence).
- Tested (ONX2432G028): the new firmware with the normal ONX definition (display, colours,
  app-following and keys confirmed by Sundar); a temporary definition using the `cst816` name and an
  extra start-up command (0x13) loaded and worked, then the normal definition was restored. GT911,
  active-high reset and both new definitions are untested (no such hardware here).
- Found: the release script and validator refused the S3-BOX-3 definition while one source note was
  longer than the schema's 300 characters.
- Open: run the two unverified boards (or any GT911 board); RGB-panel boards (4.3-7") need an RGB
  driver and a larger layout; Hynitron CST328/CST3530 for the Waveshare 2.8.

## 2026-10-18 - Sundar (Claude Code) - Setup guide; README and status brought up to date

- Changed: new `docs/setup.md` (set up a board: build the service, package firmware until the first
  release, `openpager serve` wizard; working on the code: ESP-IDF, crates, CLI, validator, CI,
  releases). README: status, boards (with unverified ones), what works today, getting started,
  layout. `docs/status.md`, `firmware/README.md`, `tools/README.md` rewritten to match main.
- Tested: commands and paths checked against the code (CLI flags, partition table, release script,
  CI); the guide itself has not been followed end to end on a fresh machine or on a Mac.
- Open: Tom following `docs/setup.md` on his Mac is the first real test of it.

## 2026-10-18 - Sundar (Claude Code) - First release v0.1.0

- PR #3 merged (GT911, unverified boards, setup guide). Tagged v0.1.0 on main with Sundar's go-ahead;
  the release workflow built and published the universal image, three board definitions, the
  manifest, and `openpager` for Linux x86_64 and macOS arm64/x86_64.
- Checked: every manifest part downloaded from releases/latest matches its SHA-256; the Linux binary
  reports `openpager 0.1.0`. Not yet done: installing a board from the release (default firmware
  source) and running the macOS binaries.
- Docs now point to the release instead of building from source.

## 2026-10-17 - Tom (Claude Code) - First run on a Mac: two boards, focus and OpenAI fixes, assistant learns macOS

- Changed (branch `tom/macos-setup`):
  - `agents/src/provider.rs`: OpenAI-style requests retry once without `temperature` when the
    provider rejects it (400 with `param: temperature`; newer OpenAI reasoning models accept only
    the default).
  - `focus/src/macos.rs`: the focused app now comes from the window server (owner of the frontmost
    layer-0 window, `CGWindowListCopyWindowInfo`, then `NSRunningApplication` by PID).
    `NSWorkspace.frontmostApplication` never changed in the service (no main run loop), so the
    layout stayed on the first app.
  - Setup assistant: the snapshot's `system_checks` on macOS lists USB serial devices with
    `has_serial_port`, the WCH driver extension state, `can_type_keys` and `app_to_allow`; new
    actions open Accessibility, open Driver Extensions, and download + open WCH's driver installer
    after a Gatekeeper check (`cli/src/macos_help.rs`, `POST /api/open`); the prompt tells it to
    read `system_checks` first and lead the person through steps only they can approve. Decision:
    [`decisions/2026-10-17-assistant-opens-settings.md`](decisions/2026-10-17-assistant-opens-settings.md).
  - The Accessibility error names the app that launched `openpager`.
  - Docs: `knowledge/setup.md` (macOS section rewritten from confirmed facts; identical native-USB
    boards; OpenAI temperature; building on a Mac), `setup.md`, `status.md`, and in `AGENTS.md` a
    section asking agents to turn each hand-made setup fix into knowledge, snapshot evidence and an
    assistant action, plus "lessons that cost time".
- Changed (branch `tom/fnk0104b`, not merged, needs a firmware build): `fnk0104b.json` definition from
  Freenove's 2.8" schematic (pin numbers only; LCD SPI CS 10, MOSI 11, SCK 12, DC 46, backlight 45
  active high, reset on CHIP_PU; I2C SDA 16, SCL 15; FT6336 at 0x38, reset 18), catalog status
  "unverified"; `board_generic.c` passes the definition's swap/mirror to `esp_lvgl_port`, because
  `lvgl_port_add_disp()` re-applies its own (all false) values when registering the display, so
  `mirror_*`/`swap_xy` in every definition have had no effect; ONX `mirror_y` set to false to keep
  its confirmed picture under that fix.
- Tested (macOS 26.6.1, Apple silicon, Wi-Fi "home"; service built from this branch):
  - v0.1.0 release binary runs (`openpager 0.1.0`), web app serves.
  - OpenAI (`gpt-6-luna`): the key test failed with v0.1.0 (400, temperature); after the fix a
    Firefox layout was drafted in 7.4 s, first attempt.
  - Freenove FNK0104B (native USB, `/dev/cu.usbmodem101`): the wizard chose the ESP32-S3-BOX-3
    definition (equal evidence, see knowledge); it installed, paired over USB-Serial-JTAG and joined
    Wi-Fi with a dark screen. With `fnk0104b.json` (local firmware folder, v0.1.0 image): display,
    backlight and touch work, picture mirrored left-right; `mirror_y: true` changed nothing even
    after erase, and reading the `board` partition back (esptool `read_flash 0x310000`) showed the
    new definition byte for byte, which led to the `esp_lvgl_port` finding above. The firmware fix
    is not built yet (no ESP-IDF on this Mac).
  - Typing: refused until Visual Studio Code (which launched `openpager`) was allowed under
    Accessibility; then VS Code's Terminal key and Firefox keys typed. Restarting `openpager` was
    enough.
  - Focus: with the new watcher, VS Code -> Firefox ("Unsupported", Configure) -> VS Code, layouts
    followed each switch.
  - ONX2432G028 (CH340K, `1a86:7522`): "No board found". The USB device was present but had no
    port: Apple's driver covers 7523 and 55d4 only. WCH's `CH34xVCPDriver.pkg` (notarized, team
    5JZGQTGU4W) installed with errors shown but succeeded; the extension stayed "activated waiting
    for user" until switched on under Driver Extensions; then `/dev/cu.wchusbserial10`, listed as
    `ch340`. Installed v0.1.0 for the ONX, paired, Wi-Fi, focus and keys work.
  - `POST /api/assist` with the new snapshot: `system_checks` showed the CH340K with its port, the
    driver `activated enabled`, typing allowed via Visual Studio Code, and the answer used them.
  - `cargo test`: 23 passed. Not tested: the three new assistant buttons pressed in the web app;
    Intel Macs.
- Building on the Mac: `cargo` needed the Xcode Command Line Tools, then failed with `tapi error ...
  MacOSX27.0.sdk ... unknown architecture` (a stale SDK newer than the linker); built with `SDKROOT`
  pointing at the 26.5 SDK. Recorded in the knowledge base.
- Third-party: no code copied. Freenove's schematic (CC BY-NC-SA repository) read for pin numbers
  only; esp-bsp sources (Apache-2.0) read to find the mirroring cause; WCH's driver installed on this
  Mac, not redistributed.
- Decided: the assistant may open settings pages and a verified driver installer (decision record);
  Freenove work on its own branch.
- Open:
  - Build the firmware with the mirror fix, flash the Freenove, confirm the picture and touch; Sundar
    to check the ONX picture with that firmware before `tom/fnk0104b` merges.
  - The wizard should ask, not pre-select, when native-USB boards tie.
  - Press the new assistant buttons in a real setup; a release with the focus and OpenAI fixes for
    Mac users.

## 2026-10-18 - Sundar (Claude Code) - PR #5 reviewed and merged; onboarding without rebuilds

- Reviewed and merged Tom's PR #5 (macOS focus, OpenAI temperature, assistant learns macOS).
  Follow-up from the pairing session: what Tom and Sundar fixed by editing Rust (provider
  parameters, API address) or by hand (Mac USB driver) should be the setup's job, not a developer's.
- Changed:
  - `agents/src/provider.rs`: any request setting the provider rejects is dropped, or renamed when
    the error names the replacement, and the request retried (up to 3; remembered per model for the
    run). Error styles handled: OpenAI `param`, quoted names, Anthropic "x: Extra inputs", Gemini
    (one-element list, `Unknown name "x"`, `fieldViolations`). `agent.toml` gains `[options]` and
    `omit`. Every error ends with "What to do:" by status.
  - Web app: AI settings > Advanced (API address for every provider, request options as JSON with
    `null` = leave out). "No board found" now tells "chip plugged in, no driver" from "nothing
    plugged in", on Linux (brltty/usbfs, missing module) and macOS (driver missing / not switched on,
    with the install and Driver Extensions buttons), also in manual mode. Boards that tie (native
    USB) are no longer pre-selected. The model field is now a combo box: clicking it (or the arrow)
    lists every model the provider offers, scrolled to the current one; typing filters anywhere in
    the name; arrows + Enter pick; any name can still be typed. (The old `<datalist>` was pre-filled
    with the current model, so it only ever offered that one and looked empty.) A 404 from the test
    reopens the list.
  - `flash/src/usb.rs`: USB-serial chips with or without a port, Linux (sysfs, bound drivers) and
    macOS (moved from Tom's diagnostics); `/api/ports` returns them; Linux `system_checks` too.
  - WCH installer downloaded into a fresh 0700 folder (not a fixed temp name).
  - Assistant prompt: never send the person to edit code or rebuild; provider problems are fixed in
    AI settings; ask which board when boards tie. AGENTS.md: "Never make a rebuild the fix".
- Tested (Arch Linux): 30 Rust tests (new: error styles incl. Gemini as received, agent.toml options,
  sysfs parsing). Live with Gemini: an unknown option in `agent.toml` was rejected, dropped and the
  draft succeeded (5.4 s); wrong model -> 404 with the "What to do"; unreachable address -> connection
  advice. Web app in the browser: Advanced section loads saved settings; the four no-port messages
  and buttons rendered from stand-in `/api/ports` replies; tie and clear-match candidates from a
  stand-in identify reply, with the firmware list fetched from the v0.1.0 release; the assistant,
  asked about an "unsupported parameter" error, pointed to Advanced settings, not a rebuild.
- Not tested: real no-port detection on hardware (the ONX is on Wi-Fi with a working port here),
  macOS build of the moved code (CI), saving options through the form against OpenAI.
- Open: a v0.1.1 release so Mac users get Tom's focus fix and these.

## 2026-10-18 - Sundar (Claude Code) - Assistant proposes fixes; the wizard re-checks by itself

- Merged PR #6 (onboarding without rebuilds). Then, as agreed with Sundar:
- Changed:
  - `agents/src/assistant.rs`: the reply may carry a `fix` (model, base_url, request options with
    null = leave out, timeout_secs, board). `check_fix` keeps only well-formed fields (plain names,
    http(s) addresses, scalar option values, no `model`/`messages`/`system`, timeout 10-3600) and
    drops anything else (keys, files, nested values). Prompt: offer a fix only on evidence; models
    from the provider's list (sent as `ai_models_available`); never claim a change before Apply.
  - Web app: the fix is listed under the answer with an Apply button; AI changes are saved through
    `/api/ai` and tested; a board choice is checked against the installable candidates. While no
    board is found the wizard re-checks every 4 s (and on becoming visible), redraws and asks the
    assistant only when something changed, and identifies a board that appears (guided mode).
    After opening Accessibility it polls the new `/api/typing` and says when typing is allowed.
  - `agent.toml` `timeout_secs` settable from the web app.
- Tested (Arch Linux, Gemini): 31 Rust tests (new: fix checking). In the browser with stand-in
  `/api/ports` and `/api/identify` replies (the ONX was not touched): no-port -> driver "waiting for
  user" -> port appears: one assistant call per state, then identified automatically and on to
  "Choose firmware". Real assistant, three native-USB boards tied, "I have the Freenove FNK0104B":
  it proposed `board: fnk0104b`; Apply selected it and enabled Install. After the prompt fix its
  answer says "Pressing Apply makes this selection" (before: "I have selected ... for you"). Apply
  of an AI-settings fix (stand-in proposal, same model) saved, tested, and left `agent.toml` byte
  for byte as before.
- Not tested: on a Mac (driver flow, `/api/typing` with Accessibility); the watcher pauses while the
  page is hidden, which the browser pane here is, so the test forced it visible.

## 2026-10-18 - Sundar (Claude Code) - PR #7 merged; release v0.1.1

- Merged PR #7 (assistant proposes fixes; the wizard re-checks by itself). Version 0.1.1
  (`companion/Cargo.toml`), tagged `v0.1.1` with Sundar's go-ahead. What it brings over v0.1.0:
  Tom's macOS fixes (layout follows the front window, Accessibility names the right app, assistant
  sees USB driver and permission state, opens settings and the verified WCH installer), provider
  quirks handled at run time with Advanced AI settings, a searchable model list, "no board"
  explained on Linux and macOS, no pre-selection when native-USB boards tie, assistant fixes applied
  with one click, and the wizard re-checking by itself. Firmware unchanged from v0.1.0 apart from
  the rebuild.
- Wi-Fi list redone after Sundar's screenshot (radio buttons stretched across each row, names
  pushed right and wrapping): the form's text-box style no longer applies to radios and
  checkboxes; networks are compact rows in one scrolling box (name, four signal bars, a lock for
  secured), strongest first, each name once. Checked in the browser at desktop and phone width
  with the 17 networks from the screenshot as a stand-in scan.
- Docs now point Mac users to v0.1.1 or later.
- Release check results: in the entry that follows, or in the PR.

## 2026-10-18 - Sundar (Claude Code) - v0.1.1 published and checked

- Release workflow for `v0.1.1` passed (firmware, Linux x86_64, macOS arm64 and x86_64, publish).
- Checked from `releases/latest`: manifest version 0.1.1; bootloader, partition table, app image and
  the three board definitions each match their SHA-256; 10 assets, not a draft or pre-release; the
  Linux binary reports `openpager 0.1.1` and contains the new web app (model combo box, Wi-Fi rows).
- Not yet done: installing a board from v0.1.1 end to end, and running the macOS binaries.

## 2026-10-18 - Sundar (Claude Code) - Browser extension: site-aware keys (first slice)

- Division of work agreed with Tom: Tom takes the dynamic board grid and pagination, per-app button
  editing in the web app, and terminal-aware buttons from running CLIs (mostly on his Mac); Sundar
  takes the Chrome/Firefox extension with website-aware keys, then app-specific dynamic UIs (Claude
  desktop waiting state, Codex, Antigravity, presentations, media players). Live key state (labels,
  badges) without re-sending a layout is a shared contract to agree next.
- Changed (branch `sundar/browser-extension`):
  - `spec/browser.md` (new): extension <-> native messaging host <-> service; tab, act, inventory,
    result messages; page inventory (controls only); actions; per-site permission. Decision:
    [`decisions/2026-10-18-browser-link-native-messaging.md`](decisions/2026-10-18-browser-link-native-messaging.md).
  - `spec/schemas/library.schema.json` + core: actions `focus` and `media`; libraries match sites by
    `match.web` (host or parent domain; `name.*` for any ending), before the app's own library.
  - `cli/src/browser.rs`: the service listens on `$XDG_RUNTIME_DIR/openpager/browser.sock` (0600, in
    a 0700 folder); `openpager` acts as the native host when a browser starts it; `openpager browser
    install|uninstall` registers it for Chrome, Chromium, Edge, Brave and Firefox. A board key whose
    action is a web action is sent to the extension; the result is logged.
  - `extension/` (new): MV3 for Chrome and Firefox from one source (`tools/build_extension.py`):
    tab reporting, popup with per-site on/off, page actions and inventory. Chrome id fixed by a
    public key (private key kept off the repo).
  - Built-in YouTube layout (`libraries/builtin/youtube.toml`).
- Tested (Arch Linux): Rust tests incl. framing, browser detection, web matching. The helper relayed
  a stand-in browser's messages to the real service. Chrome 152 headless with the extension loaded
  via `Extensions.loadUnpacked` and the real `openpager` host, against a stand-in service: hello and
  tab reports on navigation; inventory of a YouTube watch page = 35 controls with selectors and the
  site's shortcuts (k, m, c, t, f); media pause/forward/mute, focus search, open `/feed/trending`
  all ok; missing element -> `not-found`; `//evil.example/` -> `bad-action`.
- Found: Google Chrome 137+ ignores `--load-extension`; Edge loaded it once, then blocked it.
- Not tested: a real board key reaching the page (needs the extension in Sundar's own browser),
  Firefox, the popup's permission prompt. Next: drafting a site's keys from its inventory, teach
  mode, live key state (with Tom).

## 2026-10-18 - Sundar (Claude Code) - Browser extension in Firefox (Sundar's main browser)

- Ran `openpager browser install` on Sundar's machine (Firefox, Chrome, Chromium, Edge, Brave host
  files; the binary path is the dev build in `companion/target/debug`).
- Tested Firefox 156 headless, extension installed through WebDriver BiDi `webExtension.install`,
  real host from `~/.mozilla/native-messaging-hosts`, stand-in service: hello, tab reports,
  inventory of a YouTube watch page (52 controls, shortcuts k/m/c/t/f), media pause/forward/mute,
  focus search, open `/feed/trending` ok; `#does-not-exist` -> `not-found`; `//evil.example/` ->
  `bad-action`. With the shipped build (no site granted): `granted: false`, inventory and every page
  action -> `not-granted`, `open` ok. Popup no longer uses innerHTML (Mozilla's checker flags it).
- Keeping it in Firefox across restarts needs Mozilla's signing (self-distribution); steps in
  `extension/README.md`. Not done: that upload is Sundar's (his Mozilla account).

## 2026-10-18 - Sundar (Claude Code) - Media keys through MPRIS; "Make keys for this site with AI"

- Sundar tried the YouTube keys in his Firefox: after turning buttons on, every key reported "done in
  the browser" but only Search had an effect. Reproduced in headless Firefox reading the video's state
  before and after: `play()` from a page script is refused without a click (still paused), yet the
  extension said ok. He pointed at KDE Connect, which uses MPRIS.
- Changed:
  - New crate `openpager-media`: MPRIS players over D-Bus (zbus; list, play/pause, next, previous,
    seek). Firefox publishes its playing tab there (title, channel, state). The service polls every
    second; the board's status bar shows "Playing/Paused: title - artist" (ASCII only: the board's
    Montserrat font has no other glyphs) and the play/pause key is labelled Play or Pause. Media keys
    go to the focused app's player first; the extension is the fallback and now reports `blocked`
    when the page refuses to play.
  - `match.web` takes a path prefix; most specific wins. YouTube split into browsing keys and video
    keys (`builtin.youtube`, `builtin.youtube-watch`). `open` may now go to `/`.
  - Site drafting (`agents/src/site.rs`): from the page inventory, pinned to the site, every selector
    must be in the inventory (one retry with the errors). Triggered by the popup ("Make keys for this
    site with AI") or Configure on the board for a site with no keys; progress shown in both.
- Tested (Arch Linux): Rust tests (new: player matching, path patterns, site constraints and unknown
  selectors). The media crate lists Sundar's real Firefox player with title, channel and state; the
  service showed "Paused: ... - Sathya Jyothi Films" on the board. Gemini drafted Wikipedia keys from
  236 controls in 6.5 s, first attempt (Search, Random article, Current events, Main page, Recent
  changes, Talk, History, Help, Dark mode); in headless Firefox, Search focused the box and Random
  article navigated to a random article.
- Not tested: MPRIS commands (headless Firefox does not register a player until something plays;
  needs Sundar's board), the popup button end to end, Configure on the board for a site.

## 2026-10-18 - Sundar (Claude Code) - Site keys from the board

- Sundar: making keys for a site should be done from the board, not (only) the extension.
- Already there: on a site with no keys the board shows Configure, which runs the same drafting.
- Changed: when Configure or a page key needs a site that is not allowed yet, the service asks the
  extension (`grant`), which opens an OpenPager page next to the site with Allow / Not now (browsers
  only grant on a click); the board says "Allow <site> in your browser". On Allow it returns to the
  page and carries on (reads the page and drafts keys, or repeats the key). Spec: `grant` message.
- Tested (headless Firefox): the grant request opens `grant.html` next to the page with the site's
  origin. Not tested: pressing Allow / Not now (BiDi cannot evaluate in extension pages; Firefox's
  permission prompt cannot be clicked headless), the whole flow from the board.
- Mistake while testing: a stand-in service sent one `grant` to whichever extension connected last,
  which may have been Sundar's own Firefox (an "OpenPager: allow this site" tab; it closes itself
  after two minutes). The test now targets only the test browser's connection.
- Open (for Tom's dynamic grid): a way to redo keys from the board on a site that already has them
  (all 9 slots used), e.g. a last page that always offers "Make keys again".

## 2026-10-18 - Sundar (Claude Code) - Multi-page sites: page kinds, per-page keys, media row

- Sundar on his Navidrome (`/app/#/...` routes): keys drafted once never changed on the album page,
  and starting playback did not change the keys (only the status bar). Causes: routes after `#` were
  ignored (every page looked the same); each draft overwrote the one site library; media keys came
  only from drafts; several drafted selectors were positional.
- Changed:
  - Routes include an app's in-page route (`/app/#/album/<id>/show`); a page's kind replaces id-like
    segments with `*`. `match.web` routes match segment by segment with `*`.
  - Drafting has two scopes: the first draft covers the site; later drafts cover the kind of page
    (told the site-wide keys). On a site configured with the AI, a new kind of page is drafted in the
    background on first visit, once (`sites.json`). The prompt prefers `open` with a link's href,
    avoids positional selectors, and leaves media keys to the keypad.
  - Media row: while the focused app's player plays or is paused mid-track and the layout has no
    media keys, slots 7-9 become Prev / Play-Pause / Next (MPRIS; Prev/Next only if the player offers
    them). Board keys act on what is shown (layout + row).
  - Inventory links keep `#/` routes.
- Tested: Rust tests (routes, page kinds, page-scoped drafts). Live: the service sees Sundar's
  Firefox player on the Navidrome album route (paused, next/prev available). Not tested: the media
  row and per-page drafting on the board (needs Firefox focused on Navidrome).

## 2026-10-18 - Sundar (Claude Code) - Google Slides keys did nothing; Chrome re-check; open items

- Sundar drafted keys on a Google Slides presentation: all keys reported "done in the browser",
  none had an effect, and Present never started. Causes: Google's editors act on the mouse going
  down/up, not on a bare `click()`; and presenting goes full screen, which browsers only allow on a
  real key press or click by the person.
- Changed: `element` keys now dispatch a whole press (pointerdown, mousedown, pointerup, mouseup,
  click at the element's centre). The site-drafting prompt gets the operating system, prefers an
  app's documented keyboard shortcuts (Docs, Slides, Sheets, Gmail, Figma, Notion, GitHub) over
  toolbar clicks, and must use a shortcut for anything that presents, goes full screen or opens a file
  picker (shortcuts are typed as real key presses by the board or the service).
- Tested: headless Firefox, a local page with a button that only listens for `mousedown` (like
  Google's toolbar): before, no effect; now it fires once, and an ordinary button still gets exactly
  one click. Gemini, given a stand-in Slides control list (ids from Sundar's draft, names guessed):
  Present = Ctrl+F5, New slide = Ctrl+M, Comment = Ctrl+Alt+M, Undo/Redo = Ctrl+Z/Ctrl+Y, the rest
  presses. Not tested: real Google Slides (needs Sundar's signed-in browser; delete
  `libraries/ai/site-docs-google-com.toml` and its entry in `sites.json`, then configure again).
- Chrome re-check of the current extension build (headless, `Extensions.loadUnpacked`): inventory of
  34 controls on YouTube, media/focus/open actions and refusals as in Firefox.
- Open items:
  - **macOS has no MPRIS**: media keys, the media row and "Playing:" status are Linux-only; on a Mac
    media keys fall back to the extension (seek, mute and pause work; play from a script is refused;
    nothing is shown as playing). Needs a macOS Now Playing source (Tom's Mac; to agree who).
  - Redo keys from the board on a site that already has keys: needs a place on Tom's paged grid.
  - Firefox: keeping the extension across restarts needs Mozilla signing (Sundar's account); Chrome:
    Web Store listing (US$5, review) or Developer mode. The web app should guide both.
  - Not yet tried on the board: media row, per-page drafts, the Allow tab, Chrome/Edge/Brave with
    MPRIS.

## 2026-10-18 - Sundar (Claude Code) - Presenting: the board becomes a slide remote

- Sundar: after starting a presentation the board kept the editing keys. Google Slides presents in
  the same tab at the same address; only full screen changes.
- Changed: on allowed sites the extension registers `watch.js` (content script) that reports
  `fullscreenchange`; the tab message carries `fullscreen`. `match.web` patterns may end in
  `@fullscreen`. Built-in `google-slides-present` (`docs.google.com/presentation@fullscreen`):
  Prev, Next, Exit, First, Last, Laser, Black, White, Captions as Slides' own shortcuts. No drafting
  while full screen. Also: removed Sundar's old Slides draft and its `sites.json` entry on request.
- Tested: Rust tests (full-screen patterns: a presentation matches only while full screen and wins
  over the site keys). Not tested: the full-screen report itself: headless Firefox cannot go full
  screen even with `full-screen-api.allow-trusted-requests-only` off, and there is no Xvfb here (not
  installed; would ask first). Needs Sundar's Slides test.

## 2026-10-18 - Sundar (Claude Code) - Icons on the board; desktop notifications; Claude desktop prep

- Sundar: presenting worked (the Slides remote). Next: Claude desktop and other app-specific
  dynamic keys, and icons instead of text on keys (Material Icons). He chose to have me do the icons
  end to end (firmware included; Tom's grid rework merges with it) and to turn on accessibility now.
- Icons:
  - `spec/icons.txt`: 132 Material Icons names (Apache-2.0, google/material-design-icons), the one
    list for firmware, companion and AI. `tools/build_icons.py` renders them from the Material Icons
    font (32x32, 4-bit alpha) into `firmware/components/icons/` with Google's LICENSE and a NOTICE.
  - Spec: optional `icon` on layout keys (packet schema) and library keys; unknown names are
    rejected by the library validator and ignored by the board (label only).
  - Firmware: keys with a known icon draw it (white) with a small label under it; others as before.
    Image +66 KB (app 1.75 MB, 44% of the partition free).
  - Companion: layouts carry icons; media row and Configure key have icons; both drafting agents
    get the icon list and choose icons (Gemini on Wikipedia: search, shuffle, home, chat, history,
    public, schedule, explore, dark_mode). Built-in VS Code, YouTube (browse, video) and Slides
    remote have icons.
- Desktop notifications: new crate `openpager-desktop`; the service monitors `Notify` calls on the
  session D-Bus (read only) and puts notifications from Claude, Codex, Antigravity, ChatGPT and
  Cursor on the board's status bar (alert, 60 s). Tested with `notify-send` (app name, desktop entry,
  summary and body arrive).
- Claude desktop: it is Electron (`/usr/lib/claude-desktop`) and exposes nothing on the
  accessibility bus while session accessibility is off. With Sundar's go-ahead: `org.a11y.Status
  IsEnabled` set true (gsettings `toolkit-accessibility` was already true). Claude desktop must be
  restarted once to expose its tree; the accessibility adapter (working / waiting / approval,
  pressing its buttons, sending a message) comes after that.
- Tested on the ONX2432G028: icon firmware flashed (`idf.py flash`, hashes verified); the board came
  back and reconnected over Wi-Fi. Not yet seen by Sundar on the board.
- Generated icon preview checked (all 132 render as the Material designs).

## 2026-10-18 - Sundar (Claude Code) - Background service install

- After Sundar restarted Claude desktop, `openpager serve` (started inside the previous Claude Code
  session) was gone: no notifications or layouts reached the board. Sundar: running in the background
  should be part of installing the service.
- Changed: `openpager service install|uninstall|status`: a systemd user service on Linux
  (`openpager.service`, `Restart=always`, starts at login, journal log) and a LaunchAgent on macOS
  (`KeepAlive`, log in `~/Library/Logs/openpager.log`; Accessibility must then allow the `openpager`
  binary itself). Setup guide and knowledge base updated.
- Tested: unit tests for the unit/plist text; the generated unit passes `systemd-analyze --user
  verify`; `service status` reports "not installed". Not installed on Sundar's machine yet (he runs
  `serve` in a terminal; switching is his call). LaunchAgent untested (no Mac).
- Notifications design note (Sundar asked why the board kept the app's keys): notifications go to the
  status line and do not change keys; while Claude is in front it usually sends none, so its waiting
  state needs the accessibility adapter. Proposed: an agent notification also turns one key into
  "<app> needs you" (tap to switch to it).

## 2026-10-18 - Sundar (Claude Code) - Claude Code asks through the board (hooks)

- Sundar: tapping a Claude notification should show what Claude wants; Claude's "needs input"
  in Accept Edits and Manual modes must reach the board so he can act there.
- Found: after restarting Claude desktop with session accessibility on, the app is on the
  accessibility bus but only as two empty frames (Electron exposes page content only when launched
  with `--force-renderer-accessibility`). Chose Claude Code's hooks instead (Sundar: install for all
  his sessions). From the hooks docs: PermissionRequest decision `behavior` allow/deny,
  `updatedPermissions` (echo a `permission_suggestions` entry = always allow), `interrupt`; no output
  leaves the normal flow; in the desktop app hooks run alongside its dialog, first answer applies.
- Changed: `openpager hook permission|notification` (run by the hooks; relays over
  `$XDG_RUNTIME_DIR/openpager/agents.sock`, 0600; waits up to 300 s; prints nothing on any problem);
  service shows a request as layout `system.approve` (Allow, Always when offered, Deny, Stop; Allow
  needs a second tap for destructive commands; status line "Claude: <tool> <command or file>"),
  queues several, withdraws a request answered on screen (hook gone) within a second, then restores
  the app's keys; agent notifications (idle, needs input, completed) go to the status line.
  `openpager claude-hooks install|uninstall` edits `~/.claude/settings.json` (backup first; other
  settings untouched; installing twice changes nothing). Installed on Sundar's machine.
- Tested: unit tests (install/uninstall keep other hooks, summaries, destructive check); the hook
  against a stand-in service prints the documented JSON for allow, always (with the suggestion as
  `updatedPermissions`), deny and stop, and nothing when no service listens. Not tested: on the
  board with a real Claude session (needs the new service build and a new session).
- Overlap with Tom's terminal-aware work: the same hooks serve the Claude CLI; the board side
  (`system.approve`) and `openpager hook` are shared, terminal-specific keys stay his.

## 2026-10-18 - Sundar (Claude Code) - Permission requests explained on the board

- Sundar's photo: the request reached the board (Allow / Always / Deny / Stop) but the status line
  showed "Claude needs you - Pair OpenPager...": he could not tell what he was allowing, once or
  always. Cause: a Claude notification (desktop, or the Notification hook) replaced the request's
  status text and expired after 60 s.
- Changed: layouts may carry `detail` (spec: packet schema, up to 240 characters); the firmware shows
  it in place of the middle row (slots 4-6 hidden). A permission request's detail: the tool and its
  full input, the agent's description, "Allow = this time only", and what Always remembers and where,
  from the agent's suggestion (e.g. "Always = allow Bash(ls:*) from now on in this project").
  Notifications no longer overwrite the status line while a request is pending.
- Tested: unit test for the detail text; spec validator; firmware built (app 1.75 MB) and flashed to
  the ONX (hashes verified). Seen on the board: see the next entry.

## 2026-10-18 - Sundar (Claude Code) - Readable request details; requests answered in the app leave the board

- Sundar: the details worked but did not fit ("a small scroll could have helped by readjusting the
  button heights"); and a request answered with keyboard/mouse in Claude stayed on the board whenever
  he was on Claude.
- Changed (firmware, flashed to the ONX, hashes verified): with a detail, the top and bottom rows
  shrink to two thirds (label only, 14 px) and the detail panel takes the rest (about 104 px of the
  ONX's 200 px grid), wraps, and scrolls with a swipe (starts at the top for each new detail).
- Cause of the stuck request: Claude Code does not stop a PermissionRequest hook when the person
  answers in its own window, so the hook kept waiting (up to 300 s). Changed: `claude-hooks install`
  also adds PostToolUse, Stop and UserPromptSubmit hooks (`openpager hook event`, 5 s, report only).
  A session cannot move on while its permission question is open, so the same session finishing that
  tool (same tool and input), stopping, taking a new prompt or going idle withdraws its request and
  brings the app's keys back. Installing again no longer replaces the first settings backup (it had,
  once, on Sundar's machine: the backup now holds the first OpenPager hooks, not the original; uninstall
  still removes only ours).
- Tested: unit tests (settling rule: same session and tool/input, stop, other sessions ignored). Not
  yet on the board: needs Sundar to restart `openpager serve` and a new Claude Code session.

## 2026-10-18 - Sundar (Claude Code) - Merged to main; launcher and real app/site icons (built, not flashed)

- Merged PR #10 (browser extension) and #11 (icons, desktop notifications, Claude hooks, background
  service, request details) into main at Sundar's request. Both change `spec/`; they went in without
  Tom's review by Sundar's decision as owner. Tom: please review the spec changes on main
  (`spec/browser.md`, library actions focus/media, `match.web` routes and `@fullscreen`, key `icon`,
  layout `detail`, `openpager.image`).
- Sundar confirmed on the board: Claude's permission requests with the details panel; asked for
  shorter keys + a scrollable detail (flashed: compact rows, scrolling; his check pending) and
  reported a request answered on screen staying on the board (fixed with settle hooks; pending his
  check after restarting the service and a new session).
- New (branch `sundar/launcher-and-app-icons`):
  - Firmware: `openpager.image` (pictures in slices, RGB565 swapped + alpha, up to 64x64, kept in
    PSRAM, 48 least-recently-used); keys whose icon is `app:...`/`site:...` draw that picture in
    colour. Built (app 1.75 MB); NOT flashed: waiting for Sundar to report the previous flash.
  - Companion: `openpager-desktop::apps`: installed apps from `.desktop` entries, the icon from the
    GTK icon theme (Buuf here, then inherited themes, hicolor, pixmaps; PNG/SVG via `image`/`resvg`),
    shrunk to 32x32. Usage per app and site (seconds in front, one-week half-life, today bonus) in
    `usage.json` (local only). When nothing is focused: launcher `system.launcher` with the top 9 and
    their own icons (sent before the layout, once per connection); a tap runs `gtk-launch <id>` /
    `xdg-open https://site` (`open -b` / `open` on macOS). Site icons from the tab's favicon (the
    extension now sends `favIconUrl`), cached in `~/.cache/openpager/favicons/`.
  - Spec: `openpager.image` + ack, icon names for sent pictures, `detail`, the launcher
    (`protocol.md` 3.6a-c); example `image.json`.
- Tested: unit tests (colour packing: red -> F8 00 FF; image names; ranking; desktop entries; SVG
  render); real apps on this machine render from the Buuf theme (Firefox, Claude, Xfce Terminal,
  Thunar, Chrome, VLC; preview checked). Not tested: anything on the board (firmware not flashed),
  launching from the board, favicons from the extension.

## 2026-10-18 - Sundar (Claude Code) - Launcher on the board; macOS app and site icons; handoff to Tom

- Tested on the ONX (Arch Linux, i3/X11), Sundar's report:
  - Request details (previous flash): top and bottom rows shrink, the explanation below scrolls with
    a swipe. Confirmed.
  - PR #12 firmware flashed (hashes verified; boot log clean, Wi-Fi up, the service reconnected).
    On an empty workspace the board showed the launcher with the apps' own icons in colour; tapping
    one opened that app. Sites (GitHub, YouTube) show the generic globe icon until a favicon is
    saved (needs the rebuilt extension reloaded; not yet done).
  - Ranking is most used and most recent together (seconds in front, halved per week, +600 if
    used today); counting started when the service on this build started.
- Changed:
  - `openpager-desktop::picture` (new, every system): decoding/fitting PNG, ICO, JPEG, GIF, WebP,
    SVG and PNG encoding, moved out of the Linux-only `apps`. `image`/`resvg` are now plain
    dependencies. Before this, site icons (favicons) silently did nothing on macOS.
  - `openpager-desktop::mac_apps` (new): an app's icon from its bundle id: Spotlight (`mdfind
    kMDItemCFBundleIdentifier`, id checked to [A-Za-z0-9._-]), else `<name>.app` in /Applications,
    /System/Applications(/Utilities), ~/Applications; the `.icns` from Info.plist (`plutil`:
    CFBundleIconFile, CFBundleIconName, else AppIcon.icns, else any .icns); `sips` to PNG; cached for
    the service's life, misses too. Only tools macOS ships.
- Tested: unit tests (bundle id check, icns names, SVG render) on Linux; Windows type-check of the
  non-Linux paths. macOS: compiled in CI only; NOT run on a Mac.
- Decided: icons stay as pictures, not an icon font: the same 132 glyphs at 32 px as a 4-bit font
  hold about the same data (~66 KB flash), and app/site logos are colour pictures (3 KB each, 48 kept
  in PSRAM, about 150 KB of 8 MB).
- Open: Tom to try the launcher on his Mac (focus Finder's desktop with no window; app icons should
  appear; check `openpager serve` output for `sips`/`mdfind` problems). Rebuild + reload the
  extension for favicons (both browsers).

### Handoff for Tom's agents (dynamic layouts, terminal-aware actions)

What Sundar's side built that your work can reuse, and where it plugs in:

- **One place decides what the board shows:** `companion/crates/cli/src/service.rs`, `show()`.
  Order today: a pending agent request (`system.approve`) wins; nothing focused -> launcher
  (`system.launcher`); else the focused app's or site's library (`catalog.find`, most specific
  `match.web` first) plus the media row (slots 7-9 while a player plays). `shown_actions:
  HashMap<u8, KeyRef>` maps each visible slot to its action; key taps resolve through it. Pagination
  belongs here (build pages of `LayoutKey`s, keep the slot map per page) and in `firmware/main/ui.c`.
- **Layout packet extras** (`spec/protocol.md` 3.6a-c, `packet.schema.json`): key `icon` (a Material
  name from `spec/icons.txt`, or `app:`/`site:` + id for a picture sent with `openpager.image`),
  layout `detail` (<= 240 chars; middle row becomes a scrollable panel, top/bottom rows compact).
  The firmware keeps up to 48 pictures; the service tracks what it sent per connection
  (`sent_images`) and must resend after a reconnect.
- **Coding-agent state for terminal-aware keys:** `cli/src/agent_hooks.rs`. Claude Code hooks
  (installed with `openpager claude-hooks install`, all sessions, `~/.claude/settings.json`) run
  `openpager hook permission|notification|event`, which relay the hook's JSON (has `session_id`,
  `cwd`, `transcript_path`, tool name/input) to the service over `agents.sock` (0600; Linux
  `$XDG_RUNTIME_DIR/openpager/`, macOS `~/Library/Application Support/openpager/`). The service receives `Event::Agent(AgentEvent)`. So per terminal/session you
  already know: waiting for permission (PermissionRequest), idle / needs input (Notification),
  working (UserPromptSubmit, PostToolUse), finished (Stop). Matching a session to the focused
  terminal window is the missing piece (e.g. by `cwd` vs the terminal's working directory, or the
  process tree of the focused window). Reuse `summary`/`detail`/`destructive` for any agent.
- **Answers back to the agent:** PermissionRequest prints `{"hookSpecificOutput": {"hookEventName":
  "PermissionRequest", "decision": {"behavior": "allow"|"deny", "updatedPermissions": [..],
  "interrupt": ..}}}`; nothing printed = the agent asks as usual. Claude Code does not stop the hook
  when the person answers in its own UI: requests are settled by the session's next event
  (`settles`). Codex/others need their own `hook` entry point; the board side is shared.
- **AI that edits keys:** `agents/src/site.rs` is the pattern for "an agent configures buttons
  behind the scenes": the prompt gets the OS, the icon list and only verifiable facts (the page's
  controls); the draft is constrained (id, source, where it applies set by us) and every selector is
  checked against the inventory, one retry with the errors. Per-app/per-terminal drafting should do
  the same (`agents/src/lib.rs` draft for apps already gets the icon list). Drafts land in the AI
  library folder; `DraftedPages` (`sites.json`) remembers what was drafted per page kind.
- **Desktop notifications** (`openpager-desktop::watch_notifications`, Linux D-Bus only) are shown
  in the status line unless an agent request is pending. Claude desktop's accessibility tree is
  empty without `--force-renderer-accessibility`, so the hooks are the reliable channel.
- **macOS gaps your side may hit:** no MPRIS (media row never appears), no notification watcher,
  launcher app icons untested (above), Claude Code hooks untested on macOS (socket in
  `~/Library/Application Support/openpager/agents.sock`; Unix socket paths max ~104 bytes there).

## 2026-10-18 - Sundar (Claude Code) - Claude's questions answered from the board

- Sundar: a new Claude Code session asked him a question (options to pick); the board only showed
  "Claude needs your attention" in the status line, and he had to go to the computer to answer.
- Found: that session was a cloud session (claude.ai/code, "Default" environment). Cloud sessions do
  not read `~/.claude/settings.json`, and their hooks run on Anthropic's machine, which cannot reach
  this computer's `agents.sock`: the only signal is Claude desktop's notification (D-Bus), text
  only. Answering a cloud session from the board is not possible with hooks; local sessions (desktop
  app on this computer, or the `claude` CLI) are.
- First rejected a "Go to Claude" key that raises the app's window (Sundar: the point of the board
  is answering from wherever he is, e.g. another room); then agreed to it for sessions the board
  cannot answer (below).
- Changed: a local session's AskUserQuestion (it arrives as a PermissionRequest) is shown as the
  question: options on keys 1, 2, 3, 7, the question and each option's description in the detail
  panel, Decline on 9; several-choice questions tick options and Send on 8; several questions go one
  after another. The answer goes back as `decision.behavior = allow` with `updatedInput` = the
  agent's own input plus `answers` (question -> label; several labels joined with ", "), as the
  hooks docs describe for PermissionRequest.
- Tested: unit tests (reading questions, answers added to the input, tapping through two questions
  incl. ticks/untick/Send/Decline); the hook against a stand-in service prints
  `{"hookSpecificOutput":{"decision":{"behavior":"allow","updatedInput":{...,"answers":{"Which?":"B"}}},...}}`.
  Not tested: a real Claude Code session taking the answer (needs the new `serve` and a new local
  session).
- Checked the CLI for cloud sessions (Claude Code 2.1.283 + docs): `claude -p "msg" --cloud <id>`
  only queues a message (write-only; needs `claude auth login`, not signed in here); no status,
  attach or stream-json for an existing cloud session; `--teleport` makes a local copy. So the board
  cannot read a cloud session's question. Remote Control (local session, steered from phone/web)
  keeps hooks local and is the way to get questions and approvals on the board.
- Changed: a Claude desktop notification with no hook event within 3 s either side (= no local
  session behind it, e.g. a cloud session; also Claude chat notifications) puts "Go to Claude" on
  slot 9 of whatever the board shows (app keys or launcher), for up to 30 minutes or until Claude is
  focused. A tap brings Claude's window to the front: `openpager_focus::bring_to_front` sends EWMH
  `_NET_ACTIVE_WINDOW` as a pager (source 2), which i3 honours across workspaces; macOS uses
  `open -b/-a` (untested).
- Tested: unit tests (window class matching, the Go to key and when it applies). Not tested: on the
  board, and raising a window on another i3 workspace.
- Board test (ONX, i3 4.25.1): the first real cloud-session notification put "Go to Claude" on key 9
  and the service logged "brought Claude to the front", but Claude did not come forward; i3's log no
  longer covered that tap, so the cause is unknown. Retest with `notify-send -a Claude ...` from the
  terminal workspace: the key appeared, the tap switched to workspace 10 and focused Claude (i3 log:
  `_NET_ACTIVE_WINDOW ... should be activated`, workspace switch, focus in for Claude's window).
  If it fails again: `i3-msg 'debuglog on'`, tap, then `i3-dump-log | grep -A3 ACTIVE_WINDOW`.

## 2026-10-18 - Sundar (Claude Code) - Wayland: detected and explained, not supported yet (#13)

- Tested: a local Claude Code session's AskUserQuestion reached the board with its options (Sundar
  answered in the app; answering by a board tap is still to be seen).
- Decided (Sundar): no Wayland support for the hackathon; X11 (Linux) and macOS only. Enhancement
  issue: https://github.com/jalabulajunx/openpager/issues/13 (plan: one Rust "focus from a helper"
  input + uinput typing + wlroots foreign-toplevel; GNOME extension / KWin script installed by the
  setup assistant, so nothing needs a rebuild during setup).
- Changed: `system_checks.desktop_session` (Linux): session type, desktop, `keys_follow_apps`,
  `service_can_type`, and a plain note; the service prints the note at start on Wayland or with no
  DISPLAY. Knowledge entry "Linux desktops: X11 and Wayland"; assistant prompt rule: on Wayland, say
  plainly what does not work and that an Xorg session at login fixes it; no fault hunting.
- Tested: unit tests (session rules; prompt carries the rule and the knowledge entry). On this i3
  machine the snapshot reports x11. Not tested on a Wayland machine.
- Open, for Tom's terminal work: "a terminal program is waiting for input" (pacman/yay prompts).
  Measured on Linux 7.2: a program waiting at a prompt is the foreground process group of its pty
  (`/proc/<pid>/stat` field 8 tpgid == its pgrp), in a `read` on fd 0 (`/proc/<pid>/syscall` starts
  `0 0x0`, wchan `wait_woken`), fd 0 -> `/dev/pts/N`. Works for every terminal app (kernel level).
  Shells at their prompt look the same: tell them apart by name. macOS: `ps -o stat,wchan` shows
  `ttyin`. The prompt's text cannot be read from outside the terminal; `TIOCSTI` is off
  (`dev.tty.legacy_tiocsti = 0`), so answering means focusing that terminal and typing (X11/macOS).
  Never type into `sudo`/password prompts.
- Sundar: board taps answered Claude's questions in the earlier tests (confirmed on the board).
  Terminal-waiting detection waits for Tom's terminal work.

## 2026-10-18 - Sundar (Claude Code) - Recipes folder; Claude Code's screens as recipes

- Sundar: working layouts should live in one folder for every OS and website, the Claude ones too;
  all AI drafts made on his laptop work, record him as the tester for Linux. Leads to #15 and #16.
- Changed (decision: [`docs/decisions/2026-10-18-recipes.md`](decisions/2026-10-18-recipes.md)):
  - `libraries/builtin/` -> `recipes/` (`apps/`, `sites/<host>[/<page>].toml`, `agents/`); `build.rs`
    compiles in every `recipes/**/*.toml`; `tools/validate_spec.py` validates each (builtin source
    and id, unique ids).
  - Spec (needs Tom's review): `[[pack.tested]]` (os, by, date, note) and the `agent` action
    (`reply`); `spec/protocol.md` section 5 documents both, the folder, and agent screens. The
    action table there now also lists `focus` and `media` (already in the schema).
  - Promoted 15 AI drafts with "tested: linux, Sundar, 2026-10-18": Claude desktop, Firefox, Chrome,
    Thunar, Xfce Terminal; docs.google.com, Google Drive (+ recent), Facebook (+ Marketplace),
    GitHub (+ this repository's main, branches, projects and wiki pages). Not promoted: the three
    Navidrome drafts (home server LAN address, positional selectors). YouTube and Slides presenting
    got the same tested entry; VS Code has none (not seen tested).
  - `recipes/agents/claude-code-{permission,question,elsewhere}.toml` now decide the slots, labels and
    icons of those screens; the service fills in options, the explanation, Always only when offered,
    Done only for several-choice, and confirm for destructive commands. A user library with the same
    `match.agent` replaces a screen. Visible change: the several-choice key is "Done" (was "Send":
    the validator requires a second tap on anything named send).
  - Bug fixed: a tap on an unused key while a request was on the board removed the request
    without answering (`pop_front()` ran inside the `if let` tuple). Now only an answering tap does.
- Tested: workspace tests (new: every recipe loads; the permission screen matches the earlier
  hard-coded one key for key; unused keys answer nothing; questions step through options, ticks and
  Done; a user's recipe replaces the built-in screen; Go to Claude comes from its recipe); spec
  validator (22 recipes, examples and board definitions, 0 problems). Not yet on the board.
- Tom approved merging the recipes PR (#18) in a chat with Sundar on 2026-10-18; spec review of the
  `tested` field and `agent` action by Tom while it is on main.

## 2026-10-18 - Tom (Claude Code) - Review of #6-#18 after merge; recipes fixes; Mac build SDK

- Reviewed after merge: #6-#14 and #18 (code and `spec/`). The recipes additions (`[[pack.tested]]`,
  the `agent` action) are fine. Spec feedback for Sundar (what reaches the drafting AI in
  `browser.md` sections 3-5; the fixed `openpager.image` pixel format; 3x3 slot numbers now used by
  agent screens, `detail`, the media row and the launcher) left as a comment on #18 for him to
  decide; the slot numbers come into Tom's grid work.
- Changed (branch `tom/recipes-fixes`):
  - `OPENPAGER_BUILTIN` loads a recipes tree again: since #18 every recipe is in a subfolder and
    `Catalog::load` reads folders flat, so pointing it at `recipes/` loaded nothing. New
    `Catalog::folders`; the person's own library folder stays flat (its `ai/` is listed separately).
  - `spec/schemas/library.schema.json`: an `agent` key in a library without `match.agent` is
    rejected (it did nothing when tapped); `protocol.md` section 5 says so. New invalid example
    `library-agent-key-in-app-layout.toml`.
  - `docs/knowledge/setup.md`: the Mac build's stray-SDK failure, corrected. On 2026-10-17 it was
    worked around per command with `SDKROOT`, which did not last: today's shells failed again.
- Tested (macOS 26.6, Apple silicon): workspace tests pass (new: a recipes tree loads from its
  subfolders, agents/apps/site pages included; the new invalid example is rejected at
  `/key/0/action/kind`). `validate_spec.py`: 22 valid, 12 invalid examples, 0 problems (the new
  example rejected; the three agent recipes pass). It needed Python 3.13 from Homebrew and a
  virtualenv for `jsonschema` (this Mac had only Homebrew's 3.9); steps added to `docs/setup.md`.
  The service itself was not started with `OPENPAGER_BUILTIN`.
- Mac toolchain: `cargo` failed to link (`tapi error: malformed file ... MacOSX27.0.sdk ... unknown
  architecture arm64e.x1`). Cause: Command Line Tools 26.5, 27.0 and 26.6 installed together on
  2026-10-17; 26.6's linker stayed with 27.0's SDK, which no installed package owns, and `xcrun`
  picks the highest SDK (`xcrun --kill-cache` did not change it). Tom removed the stray SDK; plain
  `cargo test` links since.
- Open: Mac versions of the app recipes (bundle ids, shortcut check, Tom's `tested` entries).

## 2026-10-18 - Tom (Claude Code) - Freenove FNK0104B works: picture, colours, touch

- Changed (branch `tom/fnk0104b`): Freenove definition + catalog "supported"; firmware passes a
  definition's mirror/swap to `esp_lvgl_port` (it re-applied its own all-false values, so every
  definition's mirror settings were ignored); ONX definition `mirror_y` true -> false so its picture
  stays as it was; Freenove touch `mirror_x` + `mirror_y` (the picture flipped both ways, touch did
  not).
- Tested (Freenove FNK0104B, macOS 26.6): built with ESP-IDF v5.4.4 (installed today), flashed with
  `idf.py flash`, definition written with `esptool write_flash 0x310000`. Picture right way round,
  colours and icons right, taps land on the key under the finger (service log: keys 1, 2, 4, 5, 8 on
  VS Code typed). Not tested: the ONX with this firmware.
- Open: Sundar to check the ONX picture with this firmware, installed with its new definition, before
  merging.

## 2026-10-18 - Sundar (Claude Code) - PR #21 (Tom: FNK0104B, definitions' mirror settings) checked on the ONX

- Flashed Tom's branch `tom/fnk0104b` (f1fe331) to the ONX2432G028 with
  `idf.py flash` (#20, merged meanwhile, changes no firmware), then wrote its new `onx2432g028.json` (`mirror_y` false) to the `board` partition
  at 0x310000 as Tom's PR asks; all writes hash-verified. The previous definition was read back first
  (minified, `mirror_y` true) so it could be restored.
- Boot log: `board: definition onx2432g028: st7789 display, cst826 touch`, `READY openpager
  v0.1.1-32-gf1fe331`, Wi-Fi joined, the running service reconnected; no errors.
- Sundar's photo ([`images/2026-10-18-onx-pr21-mirror.jpg`](images/2026-10-18-onx-pr21-mirror.jpg)):
  picture the same as before (text upright, keys in order, status line at the bottom). Taps were not
  checked key by key; the ONX touch settings are unchanged by the PR. Merged at Sundar's request.
## 2026-10-18 - Tom (Claude Code) - Launcher works on macOS

- Changed (branch `tom/macos-launcher`): `focus/src/macos.rs` asks the Accessibility API for the
  focused app and its focused window (the permission typing already needs). A click on the desktop
  now counts as "nothing focused" and shows the launcher: the window list kept the last app's window
  in front, so the board never changed. On the desktop Finder's focused "window" has the role
  `AXScrollArea`; only `AXWindow` counts as a window. The Dock (focused for a moment on a Dock click)
  is skipped (Tom confirmed: Dock clicks no longer add "dock" to the launcher). Without the
  permission the window list is used as before.
- Tested (Freenove FNK0104B, macOS 26.6): desktop click -> launcher with the apps' own icons
  (Spotlight/`plutil`/`sips` worked, no errors); taps opened Firefox, VS Code and Finder; a Finder
  window shows Finder's Configure key; switching apps follows as before.

## 2026-10-18 - Tom (Claude Code) - Claude Code on the board works on macOS

- Tested (Freenove FNK0104B, macOS 26.6, `openpager claude-hooks install`, new VS Code Claude Code
  session with manual approval): permission requests appeared on the board; Deny and then Allow
  from the board reached Claude. A question (AskUserQuestion, four options) showed its options on the
  keys; the tapped option came back as Claude's answer.
- Note: with auto-approval on, Claude Code asks nothing, so nothing reaches the board.
- Not tested on macOS: "Go to Claude" (cloud sessions), the LaunchAgent.

## 2026-10-18 - Sundar (Claude Code) - Change Wi-Fi over USB (#17, first part)

- Decided (Sundar): #17 starts with "Change Wi-Fi" over USB, which covers every case including a
  computer that already moved to the new network. The cable-free route (a paired computer sending
  the network, encrypted with a key from the pairing key, with a confirm tap on the board) and a
  Bluetooth route for a board whose network is gone are later steps; both change `spec/`.
- Changed:
  - `provision::change_wifi`: identity, then the new network over serial and wait to join (30 s),
    without pairing again; says so when the board is not paired with this computer. The join wait is
    shared with setup (`join_wifi`); its error now names the board's last `WIFI disconnected` line.
  - `POST /api/wifi/change` {port, ssid, password} (name 1-32 bytes, password up to 64).
  - Web app: "Change Wi-Fi" on the dashboard's Board card opens the wizard as two steps (Find the
    board, Wi-Fi: the same scan-and-pick screen as setup); the Set up tab restores the full wizard.
    The assistant gets a `change_wifi` action and the wizard task in its context.
  - Knowledge: "Board worked, then stopped connecting after the router changed" -> Change Wi-Fi.
  - No firmware change: the board already applies a new network from `openpager.wifi` over serial
    and keeps its pairings (`link_wifi_set_credentials`).
- Tested: unit tests (assistant prompt carries the action and the knowledge entry); a separate copy
  of the service (empty config, own sockets, port 7399) in the browser pane: the dashboard button,
  the two-step wizard, the board's real Wi-Fi scan over USB (ONX, lists networks), the API's errors
  (missing port 502, blank name 400), and the switch back to the full setup wizard (a bug where
  steps 2-3 stayed hidden was found and fixed: `step()` rewrites class names). Not tested: an actual
  change of network (needs the person's Wi-Fi password).
- Sundar changed the board's Wi-Fi with it on the ONX (2026-10-18): worked.

## 2026-10-18 - Sundar (Claude Code) - Antigravity recipes; drafts told which product an app is

- Sundar: xed's AI keys work (promoted to `recipes/apps/xed.toml`, tested Linux, Sundar). Antigravity
  got the wrong keys: there are two products, the Antigravity IDE (VS Code-based, 1.107, window class
  `antigravity-ide`) and the Antigravity agent desktop app (2.23.1, "Agentic Desktop Application",
  class `antigravity`, like Claude or Codex). The draft matched the right window but the model, told
  only the name, wrote the IDE's shortcuts (Quick Open, Command Palette, Terminal).
- Changed:
  - `recipes/apps/antigravity.toml` (agent app): New Chat Ctrl+N, Chats Ctrl+K, Type Ctrl+L, Prev/Next
    Chat Alt+Up/Down, Find File Ctrl+P, New Window Ctrl+Shift+N, Close Esc. Sources: the "Navigation
    shortcuts" of antigravity.google/docs/getting-started (desktop app section) and the app's own
    menu code (New Window). Untested (no `tested` entry yet). As a built-in recipe it outranks the
    local AI draft for the same window.
  - `recipes/apps/antigravity-ide.toml`: the draft's IDE keys, matched to `antigravity-ide`. Untested.
  - Drafting: `app_facts::about` (Linux) finds the window's process (`_NET_WM_PID`,
    `openpager_focus::window_pid`) and tells the drafting agent the program file name and, for
    Electron apps, the package's name, version and description (`resources/app.asar` read directly,
    or `resources/app/product.json` for VS Code-style apps). No paths are sent. Prompt rule: products
    can share a name; draft for the one "About the app" names.
- Tested: unit tests (prompt carries the facts and the rule; VS Code-style and asar packages are
  described, with a hand-built asar); a one-off check on this machine read "Antigravity 2.23.1:
  Antigravity - Agentic Desktop Application" and "Claude 2.7032.0: Desktop application for Claude.ai"
  from the running windows. Not yet: the Antigravity keys on the board (Sundar's board still showed
  the draft: the running service predates this branch).
- Sundar: the Antigravity recipe's keys work on the board (2026-10-18); tested entry added.
- Added `recipes/sites/protege.lexis.com.toml` from Sundar's working AI draft (tested Linux,
  Sundar, Firefox): 7 keys (New chat, Ask, Skills, Library, Add options, Review tables, Workrooms).
  Left out the draft's two position-based keys (Search chats, Switch product); name "Protege" (the
  board's font is ASCII only, so "Protégé" showed as "Prot g"). Published with Sundar's OK.

## 2026-10-18 - Sundar (Claude Code) - Chrome: "Make keys for this site" said "Focus the browser window"

- Sundar, first time with the Chrome extension (linkedin.com, board buttons on): the popup's "Make
  keys for this site with AI" answered "Focus the browser window with the site first."
- Cause: while the extension's popup is open, Chrome reports its window as not focused
  (`windows.getLastFocused().focused` false); the extension then sent `tab` with `windowFocused:
  false`, the service cleared the page, and the `configure` request that followed found no site.
  Firefox does not do this, so it had not shown up there.
- Changed (`extension/src/background.js`): `tabMessage(forceFocused)`; on "configure" the extension
  sends the active tab as focused (the person is using our popup in this browser), then the request.
- Tested: `node --check`, extension rebuilt for both browsers. Not tested: Chrome with the fix (needs
  the extension reloaded in Sundar's Chrome).
- Sundar tried it: the fix works (every click reached the AI). His service log showed three more
  problems on linkedin.com:
  1. Bug: the site-wide draft (Search, Home, Messages, Alerts, Network, Jobs, Saved, Groups, Events)
     was overwritten. On the front page the background drafter treated "/" as a new kind of page,
     and `page_id(host, "/")` equals the site-wide id, so its draft replaced the site's keys.
     Fixed: the front page is the site (`site_scope` drafts the site there, `draft_new_page` skips
     "/"), and `site::constrain` refuses a "page" whose id would be the site's.
  2. Repeated "Make keys" on /feed re-drafted /feed each time with different results (the site
     already had keys, so the scope is the kind of page; models are not deterministic). The popup's
     result now says what the keys cover: "linkedin.com pages like /feed (the rest of the site keeps
     its keys)" or the whole site.
  3. Poor picks: LinkedIn's games (Zip, Wend, Patches, Mini Sudoku) and per-post buttons (React,
     Comment, Repost: they act on whichever post is first). Prompt rule added: leave out games, ads,
     promotions, suggested items and the buttons of one item in a list or feed.
  "Saved" showing other keys is by design (its own kind of page).
- Tested: workspace tests (new: the front page cannot overwrite site keys). Not tested: re-making
  LinkedIn's keys with the new rules (needs the service restarted on this branch).

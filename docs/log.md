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

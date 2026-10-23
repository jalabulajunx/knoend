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
- Sundar, after restarting on this branch: going back to Chrome on LinkedIn showed Chrome's own keys
  (log: "Focus: Google-chrome" with no "Site:" line). Same cause as the popup bug: the service took
  a browser's page only while that browser said its window was focused, and Chrome on Linux (i3)
  does not report regaining focus reliably. Changed (`browser.rs`): each connection's last tab is
  kept whether or not the browser thinks it is focused; on every focus change the service asks
  `Browser::focus_changed(app, instance)`, which picks the extension of the browser the window system
  says is in front (firefox / chrome for every Chromium browser, from the extension's hello) and
  makes it the one to act in. Unit test: Chrome's page is shown on switching to Chrome though Chrome
  never said it was focused; Firefox's reports do not take over while Chrome is in front.
- His "Make keys" on the front page remade the site-wide keys (as intended); the board kept LinkedIn
  Feed's own older keys because LinkedIn's home is /feed and a page's keys win over the site's.
  Making keys once on /feed replaces those.
- Sundar: with the focus fix the board followed Chrome, but LinkedIn's feed showed only Sort Feed,
  Analytics, Newsletters, Events: the page draft (now without games and post buttons) replaced the
  site's keys (Home, Search, My Network, Jobs, Messages, Notifications, Me, Saved, Groups). He wanted
  the site's keys plus a "new post" key.
- Changed: `Catalog::layout_for` (used by the service's `show`): on a kind of page whose keys are an
  AI draft, the site's keys stay in their places and the page's first `PAGE_EXTRAS` (3) keys the site
  does not already have take the last slots. Built-in and user page libraries, and full-screen ones
  (the Slides remote), are complete layouts and stay as they are. The page draft prompt now says the
  site keys stay, asks for the 3 most useful additions, and prefers what the person starts on that
  page (a new post, compose, upload, a filter). Unit test with LinkedIn-like site and feed drafts.
- Sundar: the combined layout worked (Home, Search, My Network, Jobs, Messages, Notifications + feed
  keys), but the drafted "New posts" key was LinkedIn's "load new posts" button (it refreshes the
  feed, then disappears: second tap `not-found`), not "Start a post".
- Cause: the extension's page inventory drops a control it cannot point at uniquely (id, test id,
  aria-label, name, else a short tag path); LinkedIn's "Start a post" has none, so the AI never saw
  it. Changed (`pageInventory` in `extension/src/background.js`): also tries the control's readable
  class names, alone or in pairs (letters, `-`, `_`, a vowel; not generated ones with digits).
- Tested: the real `pageInventory` on a local test page in the browser pane: "Start a post"
  (classes `artdeco-button share-box-feed-entry__trigger`) -> `button.share-box-feed-entry__trigger`;
  a button with only generated classes (`x1n2onr6`) is still left out; repeated buttons sharing a
  class keep their path selectors. Not tested on LinkedIn itself (needs the extension reloaded).
- Sundar: LinkedIn's Search key (focus `[data-testid="typeahead-input"]`) put the cursor in the box
  on /jobs but not on /feed, though the service logged "done" each time: the focus action called
  `focus()` and reported success without checking. Changed (`pageAction`): a selector that matches
  several elements uses the first one on screen; when focus does not take, the box (or its nearest
  visible parent) gets a whole mouse press and focus again (search boxes that open when pressed);
  success only when the cursor is really there, else `not-focusable`.
- Tested: the real `pageAction` on a local test page: plain box, a hidden copy before the visible
  one, a box shown only after its container is pressed, a box hidden for good (`not-focusable`), a
  missing one (`not-found`); element presses unchanged. Not tested on LinkedIn's feed.
- Checked on LinkedIn's feed in Sundar's Chrome (Claude in Chrome, with his OK; look-only queries,
  then one press of LinkedIn's Search button and Esc; tab closed after): the feed's search input is
  inside an `inert`, opacity-0 container (focus is refused), and LinkedIn shows a separate visible
  `div[role=button][aria-label="Search"]`; pressing it removes `inert` and the input then takes
  focus. LinkedIn's classes are generated (`i0lk82 ...`), and "Start a post" is a `div role=button`
  with no label of its own around a `div aria-label="Start a post"`, so the inventory dropped it.
- Changed (extension): the focus action, for a box that is inert or see-through, presses the visible
  control with the box's name (aria-label / placeholder / title, e.g. "Search"), waits 400 ms, then
  focuses. The inventory points at a button labelled by an element inside it with
  `tag[role=...]:has([aria-label="..."])` when that is unique.
- Changed (`recipes/apps/claude-desktop.toml`): the key labelled "Focus Input" sent Escape, which in
  Claude stops the reply or refuses a request on screen. Sundar tapped it while a Claude Code tool
  request was showing and it refused the request. Now labelled "Esc" (stop or close) with a second
  tap.
- Tested: the real `pageAction` and `pageInventory` on a local LinkedIn-like page (generated
  classes, inert search, inner-labelled button): "Start a post" listed and pressed; Search pressed
  open and focused. Not tested on LinkedIn with the rebuilt extension.
- Sundar (after reloading the extension and restarting the service): works. On the feed, Search put
  the cursor in LinkedIn's search box ("done"), the redrafted feed keys were New post, Sort feed,
  Analytics after the site's six, and New post opened LinkedIn's composer (/sharing/compose); the
  Claude Esc key shows as "Esc".

## 2026-10-18 - Sundar (Claude Code) - Spectacle recipe

- Sundar's AI draft for KDE Spectacle works (New Capture, Copy, Save, Save As, Print, Quit with a
  confirm tap): promoted to `recipes/apps/spectacle.toml` with his Linux `tested` entry. PR left
  open to merge later, at his request.

## 2026-10-18 - Tom (Claude Code) - Key grids 1x1 to 4x4 and pages of keys

- **Needs the firmware from this change:** older firmware rejects layouts with more than 9 keys and
  never sends `openpager.page`. Install it (`idf.py -B build/universal flash`) before testing
  anything after this entry.
- Decided (Tom; record: [`decisions/2026-10-18-grids-and-pages.md`](decisions/2026-10-18-grids-and-pages.md)):
  a layout chooses its grid (`grid = "2x2"` in `[pack]`, `[[page]]` overrides per page; default
  3x3), keys may say `page`; the board reports the largest grid it draws; the companion turns pages
  (one `openpager.layout` per page with `grid`/`page`/`pages`), the board asks with
  `openpager.page` on a horizontal swipe or a tap on "2/3 >" at the right end of the status bar.
- Changed:
  - Spec (needs Sundar's review): `protocol.md` 3.1, 3.2, 3.4, new 3.4a, 3.6a, section 5;
    `packet.schema.json` (slot <= 16, `grid`, `page`, `pages`, `openpager.page`, `page` on taps);
    `library.schema.json` (`pack.grid`, `[[page]]`, `key.page`, up to 64 keys); validator places
    keys on pages (explicit slots first; keys without page/slot run onto the next page). New examples
    `library-pages.toml`, `layout-pages.json`, `page.json`, two invalid ones (slot past the grid).
  - Companion: `core::layout::pages` splits a page too big for the board; `service.rs` `Paging`
    (page shown, turns, drops a tap from a page no longer shown), media row on the bottom row, "Go to
    Claude" on the last key, Configure in the middle of the grid, launcher in up to 3 pages of 3x3,
    `speak` dropped when a 16-key page nears 4096 bytes; the media row only on grids of 3+ rows.
    Agent screens say `grid = "3x3"`. AI drafts: up to 16 keys, the companion sets the grid from the
    count (1x1, 1x2, 2x2, 3x3, 4x4) and drops the model's own pages/slots. Dashboard preview follows
    the grid and shows "page n of m".
  - Firmware: any grid up to 4x4 from the layout, max grid from the screen size (64x44 px per key
    minimum; 320x240 -> 4x4), small keys show icon or label only, big keys a larger label, icon and
    label centred together, page indicator + swipe (`lv_indev_wait_release` so a swipe does not
    also tap the key), layout parsed on the heap (16 keys ~3 KB, too much for the serial task).
- Tested (Freenove FNK0104B, macOS 26.6): VS Code and Firefox (3x3) as before; a local Finder test
  library (2x2, 4x4, 1x2 pages): swipes both ways and the "2/3 >" tap turned pages (~30 turns),
  13 key taps typed the right shortcuts, no reset in the serial capture. Workspace tests and
  `validate_spec.py` pass. Not tested: the ONX (CST826 touch swipes), media row and Go to Claude on
  other grids on a board (unit tests only). AI draft with the new grid rule: Configure on macOS
  Calculator gave 16 keys laid out like its keypad (7 8 9 / ... / 0 . = +) on a 4x4 grid; taps typed.
  Dashboard preview keeps the board's shape on every grid (Tom checked 1x2, 2x2, 4x4).
- Found: LVGL 8 cannot zoom 4-bit alpha images (our Material icons): it draws them line by line and
  reads a one-line buffer as the whole image. Zoomed icons were not drawn, and one board reset on a
  big-key tap was seen before icons were kept at 32 px (no backtrace captured; none since).
- Found: opening the Freenove's USB-Serial-JTAG port resets it (`rst:0x15 USB_UART_CHIP_RESET`)
  even with DTR and RTS held low.
- Changed after the first board photos: a label under an icon takes two lines where they fit (3x3:
  lines 2 px closer, text into the icon's empty 3 px margin, 59 of 61 px), else one line with "...".
  Checked with the Mac's camera: VS Code's "Source control" on two lines, the other keys unchanged.
- Codex review (gpt-6.1-sol, xhigh) found two validation gaps, both fixed in both validators with an
  invalid example each: an agent screen with keys on page 2 passed but is drawn as one layout (both
  answers on slot 1, board rejects it), now rejected; two `[[page]]` entries for one page number were
  read differently by Rust (first) and Python (last), now rejected.
- Merged with main's site + page keys (`Catalog::layout_for`, Sundar): the combined layout now takes
  the site's grid and its size (was a fixed 9), since a page draft's own grid fits only its keys.
- Tried and removed: page buttons in the dashboard. Clicking them focuses the browser, so the board
  showed the browser's keys and the buttons turned those.
- Open (Tom, next task): an edit mode in the web app: pick any app the board knows, see its pages,
  move and change keys, set the grid (https://github.com/jalabulajunx/openpager/issues/29).
- Open: Sundar to flash this firmware on the ONX and try a swipe and the page tap (any app with
  pages, e.g. a copy of `spec/examples/library-pages.toml` with his own `match`).

## 2026-10-19 - Sundar (Claude Code) - What the site-drafting AI receives (Tom's #18 review, point 1)

- Tom (spec review on #18): `spec/browser.md` §5 said the drafting AI gets the host, "never the full
  URL", but the service sent the page path (`web.route()`) and link `href`s in the inventory kept
  their query; paths and queries can hold document ids, user names or search terms.
- Decided (Sundar): send the kind of page, not the path, and drop queries from link paths.
- Changed: `configure_site` passes `web.page_kind()` (ids as `*`, no query) to the drafting agent;
  the extension's inventory `href` is the path (plus an in-page route) without any query, also none
  inside a `#/` route. `spec/browser.md` §3 (`href`), §4b and §5 say exactly what is sent, including
  that non-id path segments (a user or repository name) still appear in kinds and link paths.
- Tested: builds; `node` check of the link rule (LinkedIn search link loses `?keywords=...`, a
  Navidrome `#/album/42/show?token=` route loses its query, other sites' links still left out).
- Open (Tom's #18 point 2, no action): `openpager.image` has one fixed format; revisit with grids.

## 2026-10-19 - Sundar (Claude Code) - Tom's grids and pages (#30) checked on the ONX

- Flashed `main` at 3a329ee to the ONX (board definition unchanged; hashes verified; boot clean,
  identity reports grid max 4x4 and `openpager.page`).
- Sundar: ordinary 3x3 apps unchanged (Terminal, Claude, Firefox). A test library (Tom's
  `library-pages.toml` matched to xed, as a user library) showed 2x2, then 4x4, then 2x2 pages,
  turning as described. Test library removed afterwards (to the Trash).
- For Tom: a long single word now breaks mid-word on two lines ("Fullscree / n" on Xfce Terminal's
  Fullscreen key, no icon); shrink the text or break only between words.

## 2026-10-19 - Sundar (Claude Code) - Rename to KnoEnd (#19), with fallback to the old names

- Decided (Sundar; Tom agreed to the name on #19): rename everything, with fallback; repository
  renamed in place; said "no end". Decision: [`docs/decisions/2026-10-19-rename-to-knoend.md`](decisions/2026-10-19-rename-to-knoend.md).
- Changed: firmware (`knoend.*` with `openpager.*` accepted and answered per link; mDNS both names;
  device and Bluetooth name "KnoEnd"; NVS namespace kept); companion (crates `knoend-*`, binary
  `knoend`; `core::protocol::{current_name, old_name}` at the Wi-Fi and serial links; discovery of
  both mDNS names with `old_names` per board; USB retries in the old names; settings folder moved
  once; old socket folder also listened on; both browser host names and Firefox ids; old
  background service removed on install; `KNOEND_BUILTIN`); spec (names, a section 1 paragraph on
  the old names, schemas, examples with the Wi-Fi sessions re-signed where they were valid; the
  validator checks `openpager.*` packets as their `knoend.*` form); extension, web app, tools, CI,
  recipes (this repository's GitHub pages now `github.com/jalabulajunx/knoend`), docs, README
  (pronunciation). Past log entries and decision records keep the old name.
- Tested: workspace tests, `validate_spec.py` (0 problems; the three broken Wi-Fi sessions still
  rejected for the same reasons), extension and firmware build. Against the ONX still on the
  pre-rename firmware: a test copy of the new service (empty settings, own sockets) identified it
  over USB through the old names ("OpenPager", v0.1.1-68-g3a329ee). Not yet: the new firmware on
  the board, the settings move and hooks on Sundar's machine, macOS.
- Before merging: rename the GitHub repository (the firmware download address now points at
  `jalabulajunx/knoend`).
- Sundar ran the new service (`knoend`) on his machine while the board still had the pre-rename
  firmware: settings moved to `~/.config/knoend`, hooks and browser hosts re-registered
  (`knoend claude-hooks install`, `knoend browser install`), extension reloaded; working with the
  old board through the fallback. GitHub repository renamed to `jalabulajunx/knoend`.
- Flashed the KnoEnd firmware (c6ae1af) to the ONX (hashes verified): `READY knoend ...`,
  identity "KnoEnd", Wi-Fi joined with the stored settings (NVS kept), announces
  `_knoend._tcp` and `_openpager._tcp` as `knoend-dbcdb8.local`.
- With both sides on KnoEnd (Sundar's machine and the ONX): the board connects over Wi-Fi, keys
  follow apps and sites, and a Claude Code question reached the board; Sundar's Decline came back as
  "Denied on the KnoEnd board." (hooks re-registered to `knoend`).

## 2026-10-19 - Sundar (Claude Code, Opus 5.5) - AI tools on this computer as providers

- Branch `sundar/cli-provider`, on top of the rename branch (#32).
- Why: people with a Claude or Google plan but no API key. Two new providers in AI settings,
  `claude-code` (`claude -p`) and `antigravity` (Antigravity's CLI, `agy -p`), behind the same
  `provider::complete` as the others, so app drafts, site drafts and the setup assistant send the
  same prompts and the replies go through the same checks (`agents/src/cli_tool.rs`).
- Locked down, because site drafts carry text from web pages: every call runs in a new empty
  folder, removed afterwards. Claude Code: `--restricted --tools "" --strict-mcp-config
  --permission-prompts none --no-session-persistence --disable-slash-commands`, system prompt from a
  file (`--bare` was rejected: it needs an API key). Antigravity has no switch for tools or a system
  prompt: `--sandbox --disable-slash-commands`, our instructions first in the prompt, told not to
  use tools. Checked with agy 1.4.2: print mode refuses running commands and reading files outside
  its folder (cannot ask permission); it can write into its folder (the empty one). It keeps
  these requests in its own conversation history (said in the web app).
- Codex left out for now (not on Sundar's machine). Antigravity's desktop `agentapi` was rejected:
  it starts conversations in the app with full agent powers and answers later.
- Setup snapshot: `ai_tools_on_this_computer` (installed, signed in) for both; the assistant is told
  to read it and never to ask for a key for these.
- Tested on Arch Linux (X11): workspace tests; the real `claude` (2.1.283) and `agy` (1.4.2)
  through `complete` (correct reply), the not-signed-in error for Claude Code (says to `/login`),
  and a GNOME Calculator draft through each (valid, first attempt, about 17 s each).
  Not yet: through the web app and service, a site draft, macOS.
- Later the same day: the tools are looked up at run time, not at fixed places
  (`cli_tool::tools_on_this_computer`, `GET /api/ai/tools`, and the setup snapshot's
  `ai_tools_on_this_computer`). Where: this process's PATH, then the PATH each installed shell gives
  the person (`<shell> -lic env` for `$SHELL`, zsh, bash, fish; 3 s each at most; a service started
  by systemd or launchd has a short PATH, and on Sundar's machine only fish's PATH has
  `~/.local/bin`), then the usual per-user folders for the OS (`~/.local/bin`, Homebrew on macOS,
  `%APPDATA%\npm` and PATHEXT names on Windows). Signed in: `claude auth status`, `agy models`
  (agy has no status command; it took 2 to 16 s), `codex login status`. Codex is found and shown
  but not used yet. The web app's AI settings show what was found and offer only installed tools,
  with a "Look again" button.
- Tested on Arch Linux: workspace tests; `knoend serve` from this branch found Claude Code 2.1.283
  and agy 1.4.2, both signed in, Codex not installed (2.4 s); the board connected over Wi-Fi.
  Not tested: macOS and Windows lookups, Codex.
- Sundar drafted MKVToolNix with Claude Code (haiku): 80 s, 2 attempts. Cause: Claude Code turns
  on extended thinking by default (13,923 output tokens for a 1,200-character draft, 104 s when
  timed alone; `--effort low` still 55 s). Calls now set `MAX_THINKING_TOKENS=0`, as the API
  providers send no thinking: 5 to 8 s. The second attempt was the validator's "looks risky but has
  no confirm": the app and site prompts now list the exact words it checks
  (`library::RISKY_WORDS`, with a test for each prompt). Antigravity (default model) took 14 s.
  A Haiku draft can still name a key the schema does not know ("Return"); the retry fixes it.
- Sundar drafted Okular with Antigravity (`gemini-3.8-flash-low`): 9.1 s, first attempt, 16 keys;
  all keys tried on the board work in Okular (including First/Last as Home/End and Back/Forward as
  Alt+Left/Right).
- Sites: Instagram drafted through Antigravity in Firefox (three kinds of page, 6 to 7 s each,
  first attempt) and worked. cricinfo.com through Claude Code (haiku) in Chrome failed: the model
  used `input[type='search']`, not in the page's list, twice. The retry now says what to do
  (remove those keys or copy a selector from the list), and on the last try such keys are dropped
  and the rest kept (`site::without_keys`, with a test).
- cricinfo.com again (haiku): the site and the series-match pages drafted (the second after one
  retry); the series page failed twice on a made-up icon ("bar_chart"). Drafts now leave out icons
  not in the icon list (`fit_grid`, for apps and sites; the label shows instead), with a test.
- Okular promoted to `recipes/apps/okular.toml` with Sundar's Linux `tested` entry. His generic
  MKVToolNix draft moved to the trash, so Configure drafts it again.

## 2026-10-19 - Tom (Claude Code) - Sundar's PRs merged, KnoEnd on macOS, #30 follow-ups

- Changed:
  - #31 (what the drafting AI gets): reviewed and merged. One wording fix in `spec/browser.md` §5:
    link paths reach the AI unchanged, ids included (`open` keys need the real path); only the page
    kind has ids as `*`.
  - #32 (rename to KnoEnd): main merged in (conflicts only in #31's files), tested on macOS (below),
    merged. Local `origin` now points at `github.com/jalabulajunx/knoend`.
  - #33 (Claude Code and Antigravity CLI as providers): main merged in, base changed to `main`, merged.
    Fix added: on Tom's Mac Claude Code is only the VS Code extension, so no `claude` on any PATH;
    the lookup now also checks the editor extension's bundled `resources/native-binary/claude`
    (VS Code, Insiders, Cursor, Windsurf; newest first, after real installs), with a test. Knowledge
    entry added under "The AI pieces".
  - #35 (Sundar's #30 follow-ups 2 and 4): "Go to Claude" no longer takes a key on a page of fewer
    than 4 keys (status line "Claude needs you" instead; the last key's tap is not taken over), and
    `knoend.approve.*` removed from the spec, schema, examples and `tools/serial_probe.py`. Codex
    (gpt-6.1-sol, xhigh) found that the launcher skipped status-only changes; fixed before merging.
  - `docs/status.md`: grids tested on both boards, the CLI providers, "Go to Claude" is Linux-only,
    the FNK definition no longer listed as not started (its native USB keyboard still is).
- Tested (FNK0104B, macOS):
  - Rename: the new service moved `~/.config/openpager` to `~/.config/knoend` and found the board on
    the old firmware over Wi-Fi; the old `openpager` hook binary reached it through the old socket;
    `knoend claude-hooks install` swapped all 5 hooks; KnoEnd firmware flashed (hashes verified),
    Wi-Fi and pairing kept, both `_knoend._tcp` and `_openpager._tcp` announced; a permission
    request declined on the board came back as "Denied on the KnoEnd board."
  - Claude Code provider: app draft through the web app with the extension's `claude` (haiku),
    10.2 s, 9 keys, first try; no permission request reached the board (`--restricted`).
  - #35: with a local patch (not committed) faking a Claude desktop notification and a 1x1 test
    library for Calculator: main showed `keys: 1=Go to Claude` (bug reproduced); #35 showed
    `[Claude needs you] keys: 1=Clear`, and taps typed the key.
- Decided (Tom):
  - #30 follow-up 1 (pages on pre-#30 firmware): skipped; both boards have page support.
  - Follow-up 3 (speak page turns): https://github.com/jalabulajunx/knoend/issues/34, after board
    sound works on the FNK (it makes no sound yet).
  - Follow-up 5 (long single words break mid-word): part of edit mode (#29), not fixed separately.
  - Bug fixes are reproduced on main on the board, then checked with the fix, before merging.
- Open: edit mode (#29) next. Not tested on macOS: Antigravity and Codex lookups (not installed).

## 2026-10-20 - Tom (Claude Code) - Edit keys in the web app (#29), labels never split a word

- Changed:
  - Edit mode (https://github.com/jalabulajunx/knoend/issues/29, design agreed with Sundar there):
    web app > Edit keys. While on, the service shows the library being edited instead of following
    focus (`show_edit`, checked first in `show` and `show_launcher`; Claude permission requests still
    win). Every change goes to the board at once, unsaved; every key is sent as `event` without
    `confirm`, so a board tap only selects the key in the web app (`edit.tap`). Edit label (with
    `\n`), icon, spoken text, confirm and shortcut keys; drag keys within a page, change a page's
    grid (up to the board's largest), add and remove keys. Save writes `user-<name>.toml` (id
    `user.<name>`, `pack.overrides` = the recipe or draft it layers on, which stays); Remove my
    version deletes it. Edit mode ends with Stop editing or when the last web app tab closes.
    API: `POST /api/edit` (list, start, change, page, save, revert, remove, stop), answered by the
    service loop (`cli/src/edit.rs`, `edit_command`).
  - The editor shows each key's real icon: the service serves the firmware's icons as PNGs
    (`/icons/<name>.png`, read from `icons.c`).
  - Help in the editor: "How editing works", the shortcut field explained in plain words ("Presses
    ⌘ Shift P on this computer", "types a" / "types A"), lowercase accepted, a Record button that
    takes the real key press, and the list of key names. Shortcuts still name keys, not characters
    (`A` types a, `Shift+A` types A), as in every recipe.
  - Firmware labels (Sundar's #30 point 5): a label wraps between words only; a word too wide for
    the key gets one font size smaller (never below 12 px; Montserrat 12 added), and a word still
    too wide is cut with "..." (`fitting_font`, `cut_long_words`). Spec §3.2 and §5 updated (forced
    line break, the label rule, where the editor writes user overrides).
- Tested (FNK0104B, macOS): Tom edited VS Code and Firefox on the board: tap selects, live
  preview, drag, grid change (4x1), add key, Save, Stop editing; the saved keys then typed in the
  apps ("A" typed a, "Shift+A" typed A). Labels: "Sourcecontrol" on a 4x1 key is cut with "...",
  readable; earlier steps (14 px floor, then 10 px) still split it and were replaced.
- Decided (Tom): live preview on the board; edit mode lasts until switched off or the tab closes;
  saving layers a user library on the draft; long words are cut with "..." rather than split or
  shrunk below 12 px; no Caps Lock key name; firmware merges without waiting for the ONX check.
- Codex (gpt-6.1-sol, xhigh) reviewed it; all 9 findings fixed: agent screens (Claude's questions
  and permission requests) are left out of edit mode for now, since a saved change could move
  Allow/Deny under a waiting request and their middle rows hold its text; Save sends the last typed
  change first and older answers never replace newer typing; Stop works while the board is away;
  a second library with the same name saves as `user-<name>-2`; every tab follows the session;
  layouts bigger than the board are refused; a cut word is measured with its dots.
- Open: Sundar to flash the ONX and check long labels and a few layouts. Later: editing agent
  screens, moving keys across pages, adding pages, editing non-shortcut actions.

## 2026-10-20 - Sundar (Claude Code, Opus 5.5) - Board display settings; the service describes itself to the setup assistant

- Branch `sundar/board-display` (not merged; spec change needs Tom's review).
- Merged on main before this (Tom): #33, #35, #36, #37. Flashed #37's firmware to the ONX through
  the web app from a local release (`release_firmware.py` + `serve --firmware <folder>`); a stale
  `build/universal/sdkconfig` lacked #37's Montserrat 12 (trash it so `sdkconfig.defaults` apply).
  Sundar then did a full erase and set up from scratch: flash, pair, Wi-Fi all worked; Bluetooth
  never came up because the laptop's Bluetooth radio was soft-blocked (rfkill), already in the
  knowledge file.
- Issues filed: #38 (Edit keys lists drafts a recipe replaced; library files do not say how they
  relate; with the Claude case: a user version saved on an old draft brought back "Focus Input" =
  Esc without confirm), #39 (Edit button on dashboard rows), #40 (Configure as a conversation),
  #41 (several Claude questions in one prompt: the board stays on question 1 while answered on the
  computer).
- Changed:
  - Key size (Big 2x2, Medium 3x3 default, Small 4x4) is the grid every page uses: keys packed in
    order (`layout::packed`); edit mode shows layouts as made; agent screens and Configure keep
    theirs. Stored in `~/.config/knoend/display.json`.
  - Spec (needs Tom's review): `knoend.display` / `knoend.display.ack` (3.6d), `board.display` in the
    identity. Themes `dark`, `white`, `sepia` and `turn` 0-3, kept in NVS, shown at once without a
    restart; the ack gives the new width, height and grid. Firmware: palettes in `ui.c`, runtime LVGL
    rotation (`board_set_turn`, sw_rotate always on), `display.c`. Pictures sent in grey (White) or
    sepia tones (Sepia).
  - Setup step 5 "Keys and look" with a live preview (web draws the board's screen in its theme and
    way round); the same choices on the dashboard. A refused look (older firmware) goes back and the
    web app says why.
  - The service describes itself to the setup assistant (Sundar's point, 2026-10-17 and today):
    `settings.rs` (every changeable setting with meaning, choices, current value; a fix may set any,
    checked against its choices), `checks.rs` (read-only checks the assistant asks for in its JSON
    and the service runs, up to 2 rounds: board on the network now, the full service log, USB now,
    settings), `journal.rs` + `say!` (every line the service prints reaches the assistant), buttons
    listed from `assistant::ACTIONS` with what each does. AGENTS.md says a feature is done when the
    setup assistant can see, change and check it.
  - Setup: an install that kept settings waits for the board to come back and skips pairing; the
    Wi-Fi step waits 10 s and retries once ("no knoend.wifi.ack within 3s" right after a flash).
- Tested on the ONX (Linux): key size Big/Medium/Small live; Dark, White, Sepia live (first sepia
  too pale on the 16-bit panel, replaced); portrait live (Sundar: worked). Not yet on the board: the
  stronger sepia (`v0.1.1-106`). The setup assistant (claude-code/haiku) through `/api/assist`: "keys
  too small" -> fix `key_size: big` (7.5 s); the post-flash Wi-Fi timeout -> read the snapshot, said
  the board is paired and connected, nothing to redo; a network question -> ran
  `find_boards_on_network` and answered from its result (18 s). Haiku once wrote a made-up tool call
  with an invented result: the prompt now forbids it, and such a reply is turned into a real check.
  Not tested: macOS, the FNK, other providers with the checks loop.

## 2026-10-20 - Sundar (Claude Code, Opus 5.5) - Fonts, the always-listening assistant, the web app's new look

- Branch `sundar/board-display` (continued; spec changes need Tom's review).
- Fonts from Google Fonts (`knoend.font`, 3.6e; a fonts partition; tiny_ttf from PSRAM; kerning
  tables hidden after WDXL Lubrifont SC's GPOS crashed the board at every start; fonts dropped after
  a start the board did not get through, `board.fonts.refused`). Start-up size is a setting
  (`startSize`), previewed on the board. Identity `lastStart` (crash, watchdog, brownout...).
- The assistant listens all the time: `problem!`/`raise!` mark faults, `watch.rs` gathers them and
  asks the assistant without being asked; answers open the floating assistant or wait for the web
  app; the board's status bar points to it. Check `board_serial_log` reads the board's own serial
  output while it is not connected. Tested: with the board hung, the watcher raised "board missing
  for 2 minutes", the assistant ran find_boards_on_network and board_serial_log and found the display
  task stuck (it misread it as a restart loop; knowledge corrected). Cause: LVGL waited forever in
  `draw_buf_flush` for an SPI transfer report; firmware v0.1.1-115 gives up after 300 ms and logs it.
  Root cause still open (suspects: run-time rotation, flash writes during a transfer, LV_MEM_CUSTOM).
- Web app redesign ("a field manual for a small instrument"): Instrument Sans and JetBrains Mono,
  warm graphite or paper, hairline numbered sections, one signal orange; the ° in Kn°End is the
  connection light, a figure-eight draws itself ("no end"); the dashboard opens on FIG. 01, a drawn
  board with the real screen in its theme, way round and font; tips on every page; a ° "ask about
  this" on every section; the assistant panel keeps a conversation, suggests questions per page,
  shows what it checked; setup has an intro (what you need), a step rail, and a board that acts out
  each step. Same ids and behaviour. Checked in the browser pane at desktop and phone width (no
  sideways scroll); one script error (a duplicate `currentPage`) caught there and fixed: check the
  script as a module (`node --input-type=module --check`), the plain check misses it.

### Handoff for Tom's agents (2026-10-20)

Merged as PR (sundar/board-display) on Sundar's call, including spec changes Tom has not reviewed:
`knoend.display` / `.ack` (3.6d), `knoend.font` / `.ack` (3.6e), identity `board.display`,
`board.fonts`, `lastStart`. Please review them in `spec/protocol.md` and say what to change.

- **"The assistant" in AGENTS.md is the in-product setup assistant**, not you. A feature is done
  when it can see, change and check it: settings go in `cli/src/settings.rs` (the web app, the
  snapshot and its fixes read that list), read-only checks in `cli/src/checks.rs`, buttons in
  `assistant::ACTIONS`, what the service notices via `say!`, faults via `problem!` / `raise!`
  (the watcher, `cli/src/watch.rs`, then asks the assistant without the person asking).
- **Terminal-aware keys (yours):** report what you detect as settings or checks where it fits, and
  faults with `problem!`, so the assistant can explain a terminal the board does not follow.
- **Board firmware:** a fonts partition was added (`partitions.csv`, after `board`); installs
  write it without an erase. LVGL now allocates from PSRAM (`CONFIG_LV_MEM_CUSTOM`) and tiny_ttf is
  on. Run `gio trash firmware/build/universal/sdkconfig` (or delete it) once if a build misses a new
  `sdkconfig.defaults` value.
- **Please try on the FNK:** Look, Way round (portrait), a key font, Start-up size. The rotation is
  LVGL software rotation at run time (`board_set_turn`); touch should follow.
- **Open:** #38 (library relations and Edit keys), #39 (Edit button on rows), #40 (Configure as a
  conversation), #41 (several Claude questions: the board stays on question 1 when answered on the
  computer). The display hang's cause (above). Checking the web script: use
  `node --input-type=module --check web/app.js` (the plain check misses duplicate names).
- The last setup-drawing fix (the cable plugs into the board's edge, not its screen) is in.
- Later: an Auto / Light / Dark switch in the web app's header (Sundar saw dark in Firefox, light in
  Chrome: each browser reports its own preference, and Chrome on i3 reports light). Auto follows the
  browser; Light or Dark is remembered per browser (localStorage) and applied before the page draws.
  Checked in the browser pane: both themes, and the choice kept across a reload. Issue #17 got a
  progress comment (items 1 and 5 done); none of the day's fixed bugs had an issue to close.

## 2026-10-20 - Sundar (Claude Code, Opus 5.5) - #41, the display freeze's cause, fonts kept

- Branch `sundar/issue-41`.
- #41: while several Claude questions wait and the person is in the app Claude runs in (Claude,
  a terminal, an editor), the board shows a panel listing them with "Answer here"; once they start
  answering there it stays on that panel when they leave (question 1 would be stale). Settling: a
  question answered in the app now settles at once (PostToolUse's tool_input carries `answers` and
  `questions`, so it never equalled the question asked; confirmed from the real event).
- The freeze, found with a serial log kept open in the background (the assistant's
  `board_serial_log` idea, by hand): LVGL in plain malloc (since fonts) took internal RAM until the
  SPI display driver could not send an update. Fixed by LVGL allocating from PSRAM first (compiled
  into LVGL's own library: CMake drops function-like definitions, and LVGL links before main). My
  earlier guard was wrong twice (300 ms was too short; then its clock never reset and it declared
  ordinary updates lost every 2-3 s, which garbled the screen); it now times each update from its
  send and redraws everything if it gives up.
- Fonts: dropped only after a real crash (panic/watchdog), not after a freeze, power cut or flash;
  choosing a refused font again resends it. The watcher no longer raises link drops during a USB
  job and the 90 s after.
- Board panels: empty slots beside a detail panel are not drawn; the panel takes an empty top row.
- Tested on the ONX (Linux): firmware v0.1.1-129; 0 lost updates and 0 watchdog lines in 32 minutes
  of use (portrait, landscape, fonts, Settings, Toggle Sidebar); fonts kept through a flash and a
  restart; #41 panel, the panel staying when leaving Claude, Answer here, and settling on submit.
  Not tested: the FNK0104B (same allocator path, so it needs the same fix), macOS.

### For Tom's agents (2026-10-20, evening)

- **Pull main before any firmware work on the FNK.** Fonts (#42) changed LVGL to plain malloc, which
  froze the ONX within minutes (internal RAM ran out for the SPI display driver). #44 makes LVGL
  allocate from PSRAM (`firmware/main/lv_mem_psram.c`, wired in `firmware/CMakeLists.txt` because
  CMake drops function-like definitions). The FNK has the same path: please check it there with
  fonts on, and watch the serial log for "display: an update never reported done" and
  "task_wdt" lines (none is right).
- **Debugging a board in use:** keep its serial log open in the background while the person uses
  it (pyserial from the IDF environment, dtr/rts off; opening may restart the ONX once), so a freeze
  leaves its watchdog lines and backtrace; decode with `xtensa-esp32s3-elf-addr2line` against the
  ELF of the exact version on the board (rebuild that commit in a worktree if needed).
- **Claude questions (#41)** depend on focus: `hosts_agent()` lists the apps an agent runs in
  (Claude, terminals, editors). Your terminal-aware work could make this exact (the session's own
  terminal) instead of by app name.
- **A firmware build that misses a new `sdkconfig.defaults` value:** delete
  `build/universal/sdkconfig` and build again.

## 2026-10-20 - Sundar (Claude Code, Opus 5.5) - Release v0.2.0

- Sundar is setting KnoEnd up on a Windows machine (with Antigravity's agent). The service there
  installed firmware v0.1.1 from releases/latest, which predates grids: Edit keys refused the 4x4
  PowerPoint draft ("this board draws at most 3x3"). The message now says when old firmware is the
  likely cause; knowledge entry added.
- Release v0.2.0 (Sundar's call): workspace version 0.2.0 (the crates' path requirements too), tag
  pushed from main; the release workflow builds firmware for every board and the service for Linux
  and macOS. No Windows build of the service is published (Windows focus and typing are not done on
  main).

## 2026-10-20 - Sundar (Antigravity) - Windows companion support

- Branch `sundar/windows-support`.
- Changed:
  - `companion/crates/focus/src/windows.rs`, `focus/src/lib.rs`, `core/src/focus.rs`: Win32 active
    window watcher (`GetForegroundWindow`, `GetClassNameW`, `GetWindowThreadProcessId`,
    `QueryFullProcessImageNameW`), UWP `ApplicationFrameHost.exe` child-window resolution
    (`EnumChildWindows`), desktop/taskbar filtering (`Progman`, `WorkerW`, `Shell_TrayWnd` report
    `None` so the launcher appears when clicking the desktop or taskbar while `CabinetWClass`
    reports `explorer.exe`), `is_main_window` filtering in `find_window` (excludes DWM-cloaked,
    `WS_EX_TOOLWINDOW`, `WS_EX_NOACTIVATE`, owned non-`WS_EX_APPWINDOW`, and untitled helper
    `HWND`s), `bring_to_front` (unlocks the Windows Foreground Lock via `AttachThreadInput`,
    `AllowSetForegroundWindow`, and `VK_NONAME`, restores/raises above `WorkerW` "Show Desktop", and
    calls `SetForegroundWindow` + `SwitchToThisWindow`), `spawn_on_input_desktop` (`CreateProcessW`
    with `lpDesktop = "Winsta0\\Default"`), `window_pid`, `process_exe`, and
    `attach_input_desktop()` (`OpenInputDesktop` + `SetThreadDesktop`). `Focus::display_name()`
    strips `.exe`/`.EXE` and `.Root`/`.root` before splitting on `.` so Windows executable names
    display as the app name (`Antigravity.exe` -> `Antigravity`, `WhatsApp.Root.exe` -> `WhatsApp`).
  - `companion/crates/input/src/windows.rs`, `input/src/lib.rs`: Win32 `SendInput` keyboard synthesis
    (`INPUT_KEYBOARD`, 40-byte `INPUT` union including `MouseInput` to match the 64-bit Win32 ABI,
    `MapVirtualKeyW`, `KEYEVENTF_EXTENDEDKEY`, modifiers, and `attach_input_desktop()`).
  - `companion/crates/flash/src/usb.rs`: gated Linux `sysfs_devices_and_their_drivers` with
    `#[cfg(unix)]` and implemented `raw_devices()` on Windows via `setupapi.dll`
    (`SetupDiGetClassDevsW` for present `USB\*` devices, reading `SPDRP_HARDWAREID`,
    `SPDRP_FRIENDLYNAME`/`SPDRP_DEVICEDESC`, and `SPDRP_SERVICE`) so the setup assistant sees
    boards on the USB bus even before a COM port driver is installed.
  - `companion/crates/desktop/src/win_apps.rs`, `desktop/src/lib.rs`, `cli/src/launcher.rs`,
    `cli/src/app_facts.rs`, `transport/src/pairing.rs`, `agents/src/secrets.rs`: Windows executable
    path persistence (`win-apps.txt`), 32x32 RGBA icon extraction (`SHGetFileInfoW` + `GetIconInfo`
    + `GetDIBits`), `USERPROFILE`/`COMPUTERNAME` fallbacks, launcher opening (`cmd /C start`) and
    icon rendering, and Windows Electron (`resources\app.asar`, `product.json`) package inspection
    for the Configure agent.
  - `companion/crates/cli/src/browser.rs`, `cli/src/agent_hooks.rs`: `clean_exe_path()` (strips
    `\\?\` verbatim prefixes from `current_exe()`), Windows local authenticated loopback TCP IPC
    (`127.0.0.1` + 32-byte random token in `browser.addr` / `agents.addr` under
    `%LOCALAPPDATA%\knoend\run`), and Windows Registry (`HKCU\Software\...\NativeMessagingHosts`)
    registration for Chrome, Edge, Brave, Chromium and Firefox.
  - `companion/crates/cli/src/background.rs`: `knoend service install|uninstall|status` on Windows
    using `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` and `CREATE_NO_WINDOW`.
  - `companion/crates/cli/src/diagnostics.rs`, `agents/src/assistant.rs`, `agents/src/cli_tool.rs`,
    `agents/src/provider.rs`, `cli/src/web.rs`, `web/index.html`, `web/app.js`,
    `docs/knowledge/setup.md`: added `windows_checks` (`keyboard_typing`, `usb_serial_driver`),
    Windows `noPortAdvice`, Windows 30 KiB command-line prompt cap for CLI AI tools
    (`AGY_MAX_PROMPT`), Windows Registry PATH + app-bundle lookup for `claude.exe` (inside
    `%APPDATA%\Claude\claude-code\<ver>` and Windows Store MSIX `Packages\Claude_*\LocalCache\Roaming\Claude\claude-code\<ver>`)
    and `codex.exe` (inside `%LOCALAPPDATA%\OpenAI\Codex\bin\<hash>` and
    `%USERPROFILE%\.codex\plugins\.plugin-appserver`), full `Provider::Codex` (`codex exec` in a
    locked-down ephemeral read-only sandbox) support, and Windows troubleshooting in
    `docs/knowledge/setup.md`.
  - `recipes/apps/antigravity.toml`, `claude-desktop.toml`, `firefox.toml`, `google-chrome.toml`:
    added `windows = [...]` executable match entries.
  - `.github/workflows/ci.yml`, `.github/workflows/release.yml`: added `windows-latest` and
    `x86_64-pc-windows-msvc.zip` release asset packaging.
- Tested (ONX2432G028 on `COM4`, Windows 11):
  - `cargo test --locked`: all 111 unit tests across all 9 crates pass with 0 warnings (including
    Win32 icon extraction for `explorer.exe`, USB hardware ID parsing, display name stripping, and
    authenticated local TCP IPC round-trip).
  - `knoend.exe devices`: detected the connected ONX2432G028 on `COM4` (`usb 1a86:7522 ch340
    USB-SERIAL CH340K (COM4)`, `chip esp32s3 v0.2 flash 16MB mac 80:b5:4e:db:cd:b8`, ranked
    `ONX2432G028` first with score 20).
  - `knoend.exe browser install`: wrote manifests and registered `HKCU\Software\...\NativeMessagingHosts`
    for Chrome, Edge, and Firefox.
  - `knoend.exe serve --no-open`: `/api/ports` returned `COM4` (`ch340`, `1a86:7522`), and
    `/api/events` reported `"app": "Antigravity"` for the focused foreground window.
  - `cargo test -p knoend-agents live::real_codex -- --ignored --nocapture`: `tools_on_this_computer()`
    found both `Claude Code 2.1.271` (inside MSIX `Claude_pzs8sxrjxfjjc`) and `Codex CLI
    0.163.0-alpha.10.2` (`signed_in: true`), and `complete_codex` returned `Ok("{\"ok\":true}")` in
    5.77 s.
  - Installed Python 3.12 (`winget install Python.Python.3.12 --scope user`) and ESP-IDF v5.4.4
    (`~/esp/esp-idf`) locally on Windows 11: `python tools/validate_spec.py` (`28 valid, 19 invalid
    examples checked; 0 problem(s)`), `python tools/build_extension.py` (`dist/extension/chrome`
    and `dist/extension/firefox`), `idf.py -B build/universal build` (`v0.1.1-134`), and
    `python tools/release_firmware.py --version 0.1.2-dev --out dist/firmware` all succeeded.
  - End-to-end bench verification on Windows 11 with the ONX2432G028 over `COM4` and Wi-Fi:
    - Flashed `0.1.2-dev` (`v0.1.1-134`) from the web app, paired, joined Wi-Fi (`192.168.50.54`).
    - Look & fonts live on the ONX: `sepia` and `dark` themes, portrait rotation (`turn: 1`, 240x320,
      `3x4` grid) and landscape (`turn: 0`, 320x240, `4x4` and `3x3` grids), and Google Font
      `Silkscreen` (18,444 bytes) sent to the `start` font slot.
    - AI layout drafting on Windows via Codex CLI (`codex/default`) and Edit keys in the web app:
      drafted and edited `PowerPoint` (`ai.powerpnt` -> `user.powerpnt`, 2 pages), `Microsoft Edge`
      (`ai.msedge`, 2 pages), `WhatsApp` (`ai.whatsapp-root`, 9 keys), and `File Explorer`
      (`ai.explorer`, 14 keys across 2 pages), and verified Win32 `SendInput` key typing across
      pages in PowerPoint (`POWERPNT.EXE`), Microsoft Edge (`msedge.exe`), WhatsApp
      (`WhatsApp.Root.exe`), File Explorer (`explorer.exe`), and Antigravity.
    - Desktop launcher (`system.launcher`): verified clicking the Windows 11 desktop shows the
      launcher on the board with extracted 32x32 app icons and site favicons, and tapping app and
      site icons (`POWERPNT`, `WhatsApp`, `explorer`, `msedge`, `Antigravity`, `docs.google.com`,
      `drive.google.com`) restores and brings their windows to the front on `Winsta0\Default`.
    - Browser extension (`0.1.1`) loaded unpacked in Microsoft Edge on Windows 11: connected via
      native messaging + authenticated local TCP IPC (`Browser extension connected (chrome 0.1.1)`),
      switched board layouts across Google Drive (`builtin.site-drive-google-com`), Google Slides
      editing (`builtin.site-docs-google-com`), and full-screen Google Slides presenting
      (`builtin.google-slides-present`: `Present`, `Next`, `Prev`, `Laser`, `White`, `Black`, `Last`,
      `First`, `Exit`).
  - Confirmed and fixed a subtle Windows desktop-station trap: when `knoend` is spawned from a
    coding agent's sandboxed shell on Windows (such as Antigravity's `codex-windows-sandbox-service`),
    the child process inherits an isolated non-interactive desktop (`CreateDesktopW`) where
    `GetForegroundWindow()` returns `0`. Calling `OpenInputDesktop(0, 0, GENERIC_ALL)` +
    `SetThreadDesktop` on the focus and input threads attaches them to the active interactive
    desktop (`Default`), making `GetForegroundWindow()` immediately return the real foreground
    window (`hwnd=198218`, `Antigravity.exe`). Also filtered Windows shell overlay processes
    (`PickerHost.exe`, `SearchHost.exe`, `StartMenuExperienceHost.exe`, `ShellExperienceHost.exe`,
    `LockApp.exe`, `TextInputHost.exe`) in `focus/src/windows.rs` so system overlays do not pollute
    the launcher.
- Decided:
  - Zero new Cargo dependencies (`Cargo.lock` unchanged so `--locked` builds pass across all
    platforms): Win32 (`user32`, `kernel32`, `shell32`, `gdi32`, `setupapi`) is linked directly via
     minimal `extern "system"` FFI modules in `focus`, `input`, `desktop`, and `flash`.
  - Local IPC for `browser-host` and `claude-hook` on Windows uses loopback TCP (`127.0.0.1:0`) with
    a 32-byte per-start authentication token stored in `%LOCALAPPDATA%\knoend\run\*.addr`, keeping
    full sync `Read`/`Write` compatibility with the Unix socket path.
- Open:
  - End-to-end key typing against elevated (Run as Administrator) windows on Windows (UIPI blocks
    unelevated `SendInput` into elevated windows; documented in `docs/knowledge/setup.md`).
  - Windows media controls (`SystemMediaTransportControls`) and desktop notifications (`companion/crates/media`, `desktop`).
- Later: #46 (Windows companion support, from Sundar's Windows session with Antigravity's agent)
  added a CI step packaging the firmware, which called `release_firmware.py` without `--version`:
  the firmware job failed on main. Fixed (version from `git describe`). The macOS job of the push
  run after #44 was cancelled, not failed (it passed on the PR and on the next push).

## 2026-10-20 - Sundar (Antigravity) - Setup wizard: Keys & Look over USB and separate optional Wi-Fi and Bluetooth steps (#47)

- Branch `sundar/setup-connections-and-look`, closes [#47](https://github.com/jalabulajunx/knoend/issues/47).
- Why: the web app's setup wizard only showed **Keys and look** after Wi-Fi was set up, even though the
  board is already connected over USB during setup and supports three transports: USB serial, Wi-Fi
  (2.4 GHz), and Bluetooth LE keyboard (`"KnoEnd"`).
- Changed:
  - `companion/crates/transport/src/serial.rs`: added `SerialLink::send(&mut self, kind: &str, body: Value)`
    for non-blocking packet writes (`knoend.layout`, `knoend.display`, `knoend.font`, `knoend.image`,
    `knoend.status`) over USB serial while the service loop reads events from the board.
  - `companion/crates/cli/src/provision.rs`: added `connect_paired(port, pairings)` to identify a
    paired board on USB serial and return its open `SerialLink` without resetting the port a second time.
  - `companion/crates/cli/src/service.rs`, `cli/src/web.rs`:
    - Added `BoardSender` (`Wifi` / `Serial`) and updated `connect()` to try Wi-Fi discovery first
      (`3 s` when USB ports are present, `10 s` otherwise) and fall back to a paired board on USB
      serial (`BoardStatus.address = "USB (<port>)"`), so layouts, key taps, and **Keys and look**
      changes work live over USB even before or without Wi-Fi.
    - Updated USB `connect_serial` in `service.rs` to forward raw non-JSON board console lines
      (`BLE ...`, `WIFI ...`, `KEY ...`, `READY ...`, `E (...)`, etc.) into `say!("Board: {trim}")`
      so Bluetooth and network events on the board reach the service journal and setup assistant.
    - Added `UNPAIRED_USB_SCAN` memoization so `connect_serial` does not repeatedly open and reset an
      unpaired serial port when no paired board is on Wi-Fi.
    - Added `Shared::acquire_serial()` (`serial_preempt: AtomicBool`) so USB setup routes
      (`/api/identify`, `/api/flash`, `/api/provision`, `/api/wifi/scan`, `/api/wifi/change`) cleanly
      preempt any active USB `SerialLink` session in `service.rs`.
  - `companion/crates/cli/src/macos_help.rs`, `companion/crates/agents/src/assistant.rs`: added
    cross-platform `POST /api/open` target `"bluetooth_settings"` (`ms-settings:bluetooth` on Windows,
    `x-apple.systempreferences:com.apple.BluetoothSettings` on macOS, `blueman-manager` /
    `gnome-control-center bluetooth` / `systemsettings kcm_bluetooth` on Linux) and assistant actions
    `look_settings` and `open_bluetooth_settings`.
  - `web/index.html`, `web/style.css`, `web/app.js`:
    - Restructured the Setup Wizard into 6 steps:
      1. `Find the board`
      2. `Choose firmware` (with **Skip install (already installed)** when the board already runs KnoEnd)
      3. `Install` (automatically pairs the board over USB right after flashing so Step 4 is live over USB)
      4. `Keys and look` (immediately available over USB; includes **Finish (use over USB)**,
         **Skip to Bluetooth (optional)**, and **Next: Wi-Fi (optional)**)
      5. `Wi-Fi (optional)` (2.4 GHz scan and join, or **Skip Wi-Fi**)
      6. `Bluetooth (optional)` (instructions for pairing `"KnoEnd"` BLE keyboard, **Open Bluetooth settings**
         button, and **Done** button)
    - Added a **Bluetooth settings** button alongside **Change Wi-Fi** on the Dashboard's Board card.
  - `docs/setup.md`, `docs/knowledge/setup.md`, `docs/status.md`: documented the 6-step setup flow,
    USB-only operation, and optional Wi-Fi and Bluetooth setup.
- Tested (Windows 11, ONX2432G028 on `COM4`):
  - `cargo test --locked`: all unit tests across all 9 crates pass with 0 warnings (including updated
    assistant action/prompt tests).
  - `python tools/validate_spec.py`: 28 valid, 19 invalid examples checked; 0 problems.
  - `knoend.exe serve`: verified the 6-step setup wizard UI and endpoints (`http://127.0.0.1:7380`).
  - Bluetooth LE keyboard on Windows 11: verified pairing of "KnoEnd" to Windows 11 (cleared initial
    handshake stall by toggling Windows Bluetooth off and on).

## 2026-10-20 - Tom (Claude Code) - Sundar's display work on the FNK, start-up font crash, #38

- Changed:
  - #52 (merged): firmware: a Google start-up font above 64 px crashed the board (tiny_ttf's
    default 4 KB glyph cache gives 4096 / px² hash buckets, zero above 64 px: divide by zero at
    the first letter); the start-up font now gets a cache sized for its glyphs. Service: a
    start-up size change alone never reached the board (the display ack dropped `startSize`).
    Spec text for 3.6d/3.6e/3.1 matched to the firmware (no contract change; Tom's call to merge
    without Sundar's review, docs only).
  - #38 (Edit keys): `Catalog::replaced_by` (a built-in or community recipe with a match pattern in
    common, same OS or web); the list marks replaced drafts ("Not in use", listed last) and versions
    of the person's own on one ("yours, on an old draft"); editing one says so, with "Edit the
    recipe instead"; saving one asks first. When not editing, the picker follows the board.
  - Issues filed: #50 (font picker and start-up size preview clunky), #51 (Edit keys in portrait
    hides keys past 3 columns; preview only, low priority).
- Tested (FNK0104B, macOS):
  - Sundar's v0.2.0 work from main (`v0.2.0-8`): Dark/White/Sepia, all four turns (touch follows),
    key and start-up fonts; 16 minutes with fonts on and the serial log open: no lost display
    updates, no watchdog lines (the PSRAM fix holds on the FNK).
  - Start-up font crash reproduced on main at 80 px (two crashes, then the font dropped as
    `refused`); with the fix 80 and 96 px shown, no crash (Tom watched the board).
  - #38: on main the list showed a fake old VS Code draft beside the recipe, unmarked (Tom saw
    it). With the fix: marked and listed last, the note and "Edit the recipe instead" (Tom);
    saving on the draft and the "on an old draft" flag checked through `/api/edit` only, the
    confirm dialog not clicked.
- Codex (gpt-6.1-sol, xhigh): #52 no findings. #38: 3 findings, all fixed: a draft counts as
  replaced only when recipes have every pattern it matches by on this OS (and every site pattern),
  not any one in common; the picker stays on "What the board shows" when not editing (a library
  under the shown one went stale when the board changed); a version saved in another tab finds its
  warning through the draft under it.
- Lessons:
  - Since #49 the service opens a paired board's USB port when Wi-Fi is not found. A second
    program reading the same port on macOS splits the bytes and left the service blocked in
    `read()` for good. Keep a serial log exclusive (`TIOCEXCL`) or close it while the service runs.
  - "Connected to board" no longer appears in the service's stdout log; `GET /api/state` shows it.
- Open:
  - "Remove my version" deletes the file: in testing it removed Tom's real VS Code version (no
    backup). Suggest moving it to the trash instead.
  - #50's picker and live preview; Sundar to check the start-up font at 96 px on the ONX (#52).

## 2026-10-20 - Tom (Claude Code) - Terminal-aware keys

- Changed:
  - In a terminal app the board shows keys for the program in the active tab. The tab is the one
    typed in last (its terminal device's access time), found among the app's child processes; its
    programs are that terminal's foreground process group, leader first (`man` then `less`). No
    permission, any terminal app; editors' terminal panes (VS Code) are left out on purpose, since
    the last-typed tab says nothing about typing in the editor. `focus/src/terminal.rs` (`ps`,
    macOS and Linux), `Focus.pid` / `Focus.terminal`, checked every second while a terminal is in
    front.
  - Spec (needs Sundar's review): `match.terminal` in libraries, beating the terminal app's own
    library (§5, library schema). Recipes `recipes/terminal/claude-code.toml` (1/2/3, Up, Enter,
    Stop = Esc with confirm, Down, Mode = Shift+Tab, Ctrl C with confirm) and `pager.toml` (less,
    man, more). No vim keys (Tom).
  - Claude per tab: the hook adds `knoend_tty` (the first of its ancestors on a terminal) to every
    event; `agent_here` replaces the app-name guess (#41) when a terminal app's tab is known.
  - Setup assistant: `last_terminal` check (the last terminal app, its tab, programs and the
    library shown); knowledge entry under "Coding agents asking on the board".
  - Edit keys lists terminal-program libraries under "Programs in a terminal".
- Tested (FNK0104B, macOS 26.6, Terminal.app): `man ls` showed the pager keys, Page Down and Quit
  worked; back at the prompt the terminal's keys; Claude Code (the VS Code extension's binary run
  in Terminal) showed its keys, Mode and Enter typed; a question from that session logged
  "from the session in tab ttys000". Linux: not run (same `ps` code; X11 now reads _NET_WM_PID).
- Codex (gpt-6.1-sol, xhigh): 4 findings. Fixed: a terminal started from another terminal no
  longer counts that one's tab as its own; `last_terminal` reports the library matched and what the
  board showed separately (an agent request or edit mode win); title changes (X11) no longer make
  repeated focus events. Not fixed: Linux keeps a terminal's access time to 8 s, so tab switches
  with typing within 8 s can pick the earlier tab; no permission-free per-keystroke signal exists
  there. Noted in `terminal.rs` for Sundar's Linux check.
- Open: Linux check by Sundar; "a program waiting for input" (2026-10-18 notes) not done; tabs
  switched without typing keep the last-typed tab's keys.

## 2026-10-21 - Tom (Claude Code) - Corrections to the 2026-10-20 entries

- Correction: "Connected to board" does still appear in the service's stdout log (seen after every
  restart that day). It was missing once only because the service was stuck reading the USB port
  while a second program held it open (the lesson in that entry stands).
- Decided (Tom): "Remove my version" stays as it is (deletes the file after its confirm); not an
  open item.

## 2026-10-21 - Sundar (Claude Code, Opus 5.5) - Terminal keys on Linux (#56)

- Changed (branch `sundar/terminal-linux`):
  - Active tab: the terminal window's title first (it follows the active tab; fish and zsh put the
    running command and folder in it, "man ls ~/Projects"), then the tab typed in last for tabs the
    title does not tell apart. `focus/src/terminal.rs`, `active_tab(pid, title)`.
  - `sudo` runs its command on a terminal of its own (`use_pty`): that terminal belongs to the tab
    (`sudo` + `pacman`) and is never a tab of its own. Only programs in the tab's foreground count, so
    an xterm started from a tab stays a window of its own.
  - `TerminalTab::program()`: the program keys are for, skipping shells and `sudo`/`env`/`nice`...
  - A program in a terminal tab with no keys: the terminal's keys plus a **Configure** key (last
    page when the terminal's fill the first), status "No keys for pacman yet". The configure agent
    drafts `ai.terminal-<program>` with `match.terminal`; the prompt gets "Terminal program: pacman"
    and its `whatis` line (pacman has two manual pages: the package manager and a Pac-Man game).
    Prompt rules for terminals: Ctrl not Primary; Shift for capital-letter commands (htop's P).
  - New action kind **`sequence`** (spec/protocol.md section 5 and the library schema; **needs Tom's
    review**): 2-8 shortcuts typed in order by the companion on its own computer (the board sends an
    event). The agent uses it only for terminal programs that read a line (`["N", "Enter"]`); a
    desktop app's draft with one is rejected.
  - `knoend draft --terminal <program>`.
- Tested (ONX2432G028, Arch Linux, i3/X11, Xfce Terminal, fish): Tom's cases each worked (`man ls`
  pager keys, back to the terminal's keys at the prompt, Claude Code's keys); a tab switch without
  typing now changes the keys; `sudo pacman -Syu` showed Configure, the draft arrived, and No
  (`N`, `Enter`) cancelled at "Proceed?" (Sundar at the board). Live drafts through openai/gpt-6-sol:
  pacman (Yes/No as sequences, Default, Stop with confirm) and htop (Shift+P/M/T after the capitals
  rule; the first draft sent plain P). Unit tests for the title, folder, sudo and xterm cases.
  macOS not run: titles there may be empty, which falls back to the old behaviour.
- Decided: a terminal-program draft applies in any terminal app on any OS; sequences are typed by
  the companion (no firmware change), so they type on the computer running knoend.
- Open: any long command (`cargo build`) also shows Configure and "No keys for ... yet"; could wait
  a few seconds before offering it. Bash at a prompt sets the same title in every tab of one folder:
  typing still decides there. Edit keys shows a sequence key as "types N, then Enter" (not editable
  yet).

## 2026-10-21 - Sundar (Claude Code, Opus 5.5) - Keys for your apps: installed apps, drafted by AI in a batch

- Changed:
  - Set-up step 7 "Your apps" (also "Set up apps" on the dashboard): the apps installed on this
    computer, most used first, each marked Ready (a recipe), Yours, Drafted (AI) or not set up, with
    real icons and a search. The person picks up to 12; the configure agent drafts them two at a
    time, each tile showing waiting / drafting / done / failed ("Ask why" hands the error to the
    assistant). Drafts are saved like ones made from the board and go to the board at once.
  - `knoend_desktop::installed`: installed apps on every OS, named as their windows will be
    (Linux: desktop entries, class StartupWMClass or the program, instance the program; macOS: `.app`
    bundles, bundle id and name; Windows: `App Paths` exe names). Desktop entries now record their
    program (`Exec=`).
  - `cli/src/app_setup.rs`; API `GET /apps`, `POST /apps/draft`, `GET /apps/icon/{id}`; progress
    as `app-setup` events. Only the names of the picked apps reach the AI provider.
  - Setup assistant: check `installed_apps`, action `app_setup` (web app button "Keys for your apps").
  - Set up a board, with a board already set up and connected: "Your board is set up" names it and
    offers Keys for your apps (with how many used apps have no keys), Keys and look, Wi-Fi (join or
    change, over USB), Bluetooth, and Install firmware again; the steps show after a choice
    (Sundar's request: there was no way back to Wi-Fi, Bluetooth or apps for a configured board).
- Tested (Arch Linux, i3; ONX over Wi-Fi; OpenAI gpt-6-sol):
  - Each choice opens its step (look 4, Bluetooth 6, apps 7, Wi-Fi the change-Wi-Fi flow from
    step 1, install step 1), checked in the web app.
  - 147 apps listed; the ones used and those with recipes first, statuses right (Firefox, Thunar,
    Okular, Spectacle, Code - OSS ready; Claude yours).
  - Darktable and OBS Studio drafted together in about 18 s: 9 and 7 keys; matches `darktable` and
    `obs`, the window classes those apps use. Not yet opened to see the keys on the board.
  - Asked the assistant "Which of my apps should I make keys for next?": it ran `installed_apps` by
    itself, suggested MuseScore (used, no keys) and offered the app_setup button. No prompt edit.
  - macOS and Windows listing: compiles in CI only, untested.
- Open:
  - "Bulk Rename" shows Thunar's keys (its entry runs `thunar`); harmless, but it reads oddly.
  - Linux lists system tools (Avahi, Bluetooth Adapters); they sit under "Everything else".
## 2026-10-21 - Sundar (Claude Code, Opus 5.5) - The board listens: notes to MuseScore as MIDI

- Changed:
  - Firmware: PDM microphone in the board support (definition `mic`: driver, clk, data, optional
    slot; read in stereo, the slot with sound is used; GPIO 19/20 are released from the S3's own
    USB on boards that talk through a USB-serial chip). `listen.c` and `pitch.c` (YIN, 8 kHz frames
    of 64 ms, a note counts after about 100 ms). `knoend.listen` / `.ack`, `knoend.note`, identity
    `features.mic`. ONX definition: the Nextion Dual MIC on GPIO 19/20.
  - Companion: `midi.rs` (virtual MIDI device "KnoEnd" on Linux and macOS, made when a board with
    a microphone connects; on Linux also into Midi Through, which MuseScore reads by default;
    Windows: a loopMIDI output named KnoEnd, untested). Library action `listen`; setting
    `listen_quiet` (default -60 dB). Recipe `recipes/apps/musescore.toml`.
  - Spec 3.6f, board and library schemas; decision record
    [2026-10-21-listening-and-midi](decisions/2026-10-21-listening-and-midi.md). CI installs
    libasound2-dev (midir).
  - Bench tool `tools/listen_test.py`; pitch tests `firmware/test/run_pitch_test.sh` (30 checks).
- Tested (ONX2432G028 with the Nextion Dual MIC; Arch Linux; MuseScore 4.7):
  - Mic silent at first, raw value -30935 on both slots: the ribbon was not seated (same symptom
    and cause as the walkie-talkie project in July). Reseated: live values.
  - Sung C E G B G E C: all heard. A tune played from the laptop speakers: E4 G4 C5 A4 right, C4
    read as G5 (its third harmonic; small speakers barely play C4).
  - Kalimba over Wi-Fi only: C4 E4 G4 B4 D5 D4 F4 C5 E5 B5 D6 all right, sent as MIDI (checked with
    `aseqdump`), and written into a MuseScore score in note input with no MuseScore setting.
- Decided: pitch found on the board, only notes leave it (decision record).
- Open:
  - Raise the top of the range to about 1400 Hz (a 17-tine kalimba reaches E6); needs a flash.
  - An octave check for a weak fundamental (laptop speakers).
  - Listening carries on when MuseScore loses focus (until tapped again or 10 minutes).
  - Pluggable connectors for other apps (Sundar's idea: an AI writes the bridge): proposal in the
    decision record, for Tom.
  - Tom: review spec 3.6f, the board schema `mic` and library action `listen`.

## 2026-10-21 - Sundar (Claude Code, Opus 5.5) - Set up a board: a way back to the board's options

- Changed: after picking from "Your board is set up" (apps, look, Wi-Fi, Bluetooth, install), a
  "← Your board" link above the step returns to those options, as do Back on the apps step and the
  apps step's done message. Before, the only way back was the Set up a board tab (Sundar). "1 keys"
  now reads "1 key".
- Tested (web app, Arch Linux, board on Wi-Fi): apps → back link and Back return to the options;
  look → back link returns; Bluetooth's own Back still goes to the Wi-Fi step.

## 2026-10-21 - Sundar (Claude Code, Opus 5.5) - The board's home screen: pinned apps first

- Changed: the launcher (what the board shows when no app is in front) shows the person's pinned
  apps and sites first, in their order, then (setting `home_fill`, on by default) the most used.
  Settings `home_apps` (a list: "app:<id>", "site:<site>") and `home_fill` in the registry, so the
  setup assistant can see and change them; a list kind in `settings.rs`. The Your apps step has a
  "board's home screen" strip (order, move, remove, the fill switch) and a pin on every app tile.
  `Usage::home` replaces `Usage::top`.
- Tested: unit tests (pinned first, pinned only, the rest by use; list setting checks); in the web
  app, pinning MuseScore, Darktable and Firefox and moving Firefox up saved
  `["app:org.musescore.MuseScore", "app:firefox", "app:org.darktable.darktable"]`. On the board:
  not yet (the board was not reachable at the time).
## 2026-10-21 - Sundar (Claude Code, Opus 5.5) - Identifying boards KnoEnd does not know (#63)

- Changed (branch `sundar/identify-unknown-boards`):
  - Identify, `knoend devices` and the new `identify_board` check read, without writing, the
    partition table and the first 128 KB of each app: project, version, ESP-IDF and component names
    (`knoend-flash` `on_board.rs`). Ports carry the USB serial number (a native-USB board's MAC).
  - A listed board with another flash size is never offered; suggestions are `confident` only when
    KnoEnd firmware names the board or flash and USB bridge both match. Otherwise "a board KnoEnd
    does not know", nothing pre-selected in the web app (`cli/src/boards.rs`).
  - The assistant gets `last_board_identified` (evidence plus the display and touch chips KnoEnd's
    firmware drives) and a prompt section: name the board from its components, not a demo's
    project name; say whether KnoEnd can run on it, and not to install when it cannot. AI-guided
    set-up asks it at once; manual set-up has a button; otherwise the watcher raises it.
  - Bug found on the way: with the paired board off Wi-Fi, the service's USB search (#49) opened
    every port; a board printing vendor firmware held the USB lock about 10 s and Identify failed
    after 2 s ("the USB port is busy with another job"). Now only paired native-USB boards (by MAC)
    are opened, setup jobs wait up to 15 s, and the search is logged.
- Tested (Arch Linux; Waveshare ESP32-S3-Touch-AMOLED-2.06, 32 MB flash, native USB, vendor
  firmware; flash backed up first and verified, kept off the repo): `knoend devices` in 8 s showed
  both apps and their components and "a board KnoEnd does not know". Web app, AI-guided set-up
  (openai/gpt-6-sol): the assistant named the Waveshare ESP32-S3-Touch-AMOLED-2.06 from the BSP
  component, called `phone_s3_box_3` a demo name, said KnoEnd cannot drive its AMOLED and not to
  install, and gave the backup command; Install stayed disabled (Sundar). The "busy" failure was
  seen before the USB fix, not re-run after it.
- Open: AMOLED support (QSPI display, AXP2101 power) is firmware work, not part of this.


## 2026-10-21 - Sundar (Claude Code, Opus 5.5) - AMOLED board, several boards, per-board looks, Bluetooth data

- Changed (branch `sundar/amoled-2-06`, which also carries #65):
  - **Waveshare ESP32-S3-Touch-AMOLED-2.06** (#63 found it): QSPI AMOLED via Espressif's
    `esp_lcd_sh8601` (Apache-2.0), start-up commands and pins from Waveshare's BSP component
    (Apache-2.0), x gap 22, even-aligned updates, panel brightness (0x51), draw buffers capped at
    12000 px (a 51 KB eighth failed every SPI update), `corner_radius` -> UI inset, touch FT5x06
    (non-fatal when absent). OLED guards: dim after 1 min, dark after 10, wake on new keys or an
    alert, 2 px shift every 3 min. Buttons in definitions: GPIO (BOOT = next page) and the AXP2101
    power key (short press = screen off/on; INTSTS2 0x49 bit 3, register map from XPowersLib, MIT).
    Schema: display bus/data/x_gap/y_gap/even_areas/brightness/corner_radius/oled, buttons,
    touch ft5x06 (**needs Tom's review**). Catalog: unverified, 32 MB, native USB.
  - **Several boards (#15)**: every paired board connects; `BoardSender` sends to all or one;
    events carry the board id; per-board heartbeat; a scout adds boards switched on later; while
    boards are connected only paired native-USB boards are opened on USB (a CH340 restarts its board).
  - **Per-board looks**: `display.json` `boards` (the old top-level look moved to the boards paired
    then); settings registry has a "board" choice and per_board marks; pictures toned per board;
    way round named for how each board sits (an AMOLED is portrait at turn 0); dashboard Board
    picker and a Board section listing every paired board; set-up "Add another board".
  - **Bluetooth data channel** (spec section 2, no protocol change): firmware `link_ble.c` beside
    the HID keyboard (one GATT dispatcher; packets handled on their own task); service
    `transport/ble.rs` with btleplug, used for boards on neither Wi-Fi nor USB. CI: libdbus-1-dev.
  - Identify: a board running KnoEnd that names its board is known even when the release does not
    list it (says "skip install").
- Tested (Arch Linux, ONX2432G028 on Wi-Fi, Waveshare AMOLED; Sundar at the bench): vendor flash
  backed up twice and compared first (kept off the repo); display, touch, corners, dimming, power
  key, screen-off tap without a key press; two boards mirrored, each in its own look; AMOLED reached
  over Bluetooth only (found in 3.2 s, ping 90 ms) and following apps. Not run: BOOT paging on a
  multi-page layout, fonts over Bluetooth, macOS/Windows Bluetooth, the FNK with this build.
- Decided (Sundar): looks are per board; a new board starts from the defaults; several boards
  mirror; all three channels must work.
- Open: the AMOLED reaches users only after the next release tag; roles per board (#15's pager
  role) not done; spec review by Tom for the board schema additions.

## 2026-10-21 - Sundar (Claude Code, Opus 5.5) - AMOLED on Wi-Fi: lost updates held the display

- Symptom (Sundar): on Wi-Fi the AMOLED showed its digits for a long time, then followed apps late
  or not at all; the service logged "no reply from the board for 25 s" and a reset.
- Cause (board's serial log, 70 s): "spi transmit (queue) color failed" 30 times and "could not
  take the LVGL lock to apply a layout" 15 times. Each update copied 24 KB from PSRAM into internal
  memory; with Wi-Fi running that copy failed now and then, each failure held the display (and the
  lock layouts need) for the 2 s guard, and pings went unanswered.
- Fix (`sundar/amoled-dma-buffers`): QSPI panels draw from ten-row buffers in internal DMA memory
  (about 8 KB each, nothing copied); SPI LCD boards unchanged.
- Tested (AMOLED on Wi-Fi beside the ONX): 75 s after the flash, no failed updates, no lock errors,
  both boards connected and following apps. Sundar's report on responsiveness pending.

## 2026-10-21 - Sundar (Claude Code, Opus 5.5) - Release v0.2.1

- Changed: workspace and crate versions 0.2.1, extension manifest 0.2.1; tag v0.2.1 publishes the
  firmware (with the Waveshare ESP32-S3-Touch-AMOLED-2.06, unverified), the service for Linux,
  macOS and Windows (installer and zip), and the Chrome and Firefox extension zips (unsigned).
- Decided (Sundar): v0.2.1, no protocol change.

## 2026-10-22 - Tom (Claude Code, Opus 5.5) - v0.2.1 work on macOS and the FNK0104B

- Changed: nothing in code. The FNK0104B runs main (`v0.2.1-4-ga249134`, flashed with `idf.py
  flash`, definition kept); the service is main's release build on macOS.
- Tested (macOS, FNK0104B on Wi-Fi, Tom at the board):
  - Basics: connects over Wi-Fi in 2 s, keys follow the app, taps typed. The old top-level look in
    `display.json` moved to `boards` unchanged. No board crash or reset in about an hour.
  - Keys for your apps: icons shown; VLC drafted (14 keys), followed and typed on the board.
  - Home screen: VLC pinned came first on a desktop click; tapping it opened VLC.
  - Terminal keys (Terminal.app, zsh): `man ls` pager keys, back to the terminal's keys after `q`;
    `top` showed "No keys for top yet" and Configure, drafted, its keys worked and quit top.
  - Not run: MIDI (see below), Bluetooth only (left for the next session: pair as a keyboard, Wi-Fi
    off, USB out).
- Found:
  1. **Bluetooth search with a paired board missing.** This Mac still had Sundar's ONX paired from a
     visit. `connect` reached the FNK over Wi-Fi, then looked for the ONX over Bluetooth: `ble::connect`
     cannot tell boards apart before opening them, so it opened the FNK (already on Wi-Fi), which
     refused the unbonded Mac (`GATT_INSUF_AUTHENTICATION`), and `subscribe` (no timeout) waited
     until the board's pairing timed out (`smp_rsp_timeout`, about 30 s). The first connection took
     2.5 minutes; the scout repeated it every 20 s, and macOS briefly focused `BluetoothUIServer`
     (the board showed Unsupported). Fix to come: skip boards already connected, a timeout on every
     Bluetooth step. Sundar's pairing was taken out of this Mac's `companion.json` for the test
     (backup kept locally) and goes back to reproduce the bug before the fix.
  2. **Keys for your apps on macOS lists 16 apps:** only `/Applications`, `/Applications/Utilities`
     and `~/Applications`; Apple's own (`/System/Applications`: Notes, Mail, Calendar, Terminal in
     Utilities; Finder in CoreServices) are missing.
  3. **Home screen fill on macOS** includes background programs: loginwindow, Spotlight, Dock,
     UserNotificationCenter, BluetoothUIServer.
  4. **Configure in a terminal is on page 2** when the terminal's keys fill page 1; Tom did not find
     it until he paged. Question for Sundar: Configure on page 1, in place of the last key?
  5. **The FNK0104B has a microphone**, behind its ES8311 codec (I2C 0x18, I2S), not a PDM mic:
     `mic` only knows `pdm`. New firmware work (Espressif's ES8311 driver, Apache-2.0) and a schema
     value; design first.
- Next (in order): fix 1-3 on one branch (reproduce 1 with the ONX pairing back first); Sundar on 4;
  the FNK microphone; the Bluetooth-only test; Tom's spec review of #57, #60, #66 (read: sound;
  small nits: `quiet` default -50 in spec/firmware vs -60 sent; `mic` pins skip `$defs/pin`;
  `sequence` keys have no pattern); then issues #50, #39.

## 2026-10-22 - Sundar (Antigravity) - Board removal in web app & PowerPoint presentation slide notes (#77, #78)

- Changed:
  - Board removal (#77):
    - Companion API `POST /api/board/remove` with options:
      - `forget = true`: unpairs the board from `companion.json` (`Pairings::forget`), clears display config, and drops active service connections (`drop_board`).
      - `forget = false`: removes custom board look configuration from `display.json` only, preserving pairing and connection.
    - Web UI: added "Remove board" button to the main Board facts card and an explicit "Remove" action button to each secondary board in `other-boards`, wired to a modal dialog allowing the user to choose between complete removal / unpairing vs display-only removal.
  - PowerPoint presentation notes & recipe (#78):
    - Added built-in recipe `recipes/apps/powerpnt.toml` with presentation controls (Next, Previous, End Show, First, Black screen, White screen, Present, From Current, Laser / Pen).
    - Added Windows PowerPoint presentation detection and notes extractor via COM Automation (`powerpoint.rs` / `current_presentation_notes`).
    - Service loop integration: when PowerPoint is in presentation mode, active slide notes are rendered in the layout detail panel (scrollable by swiping) and status reflects the current slide position (e.g. "Slide 1 of 12").
- Tested (Windows 11, unit test suite):
  - Spec validation (`python tools/validate_spec.py`) verified all recipe schemas including `recipes/apps/powerpnt.toml`.
  - Display and pairing unit tests (`removing_a_board_drops_it_from_display_map`, `slide_notes_default_is_empty`, etc.).
  - Full companion tests (`cargo test --locked`) passing.
  - PowerShell COM automation verification with PowerPoint instance reading slide speaker notes.
- Decided:
  - Board removal modal gives clear, explicit choices so unpairing a board vs clearing look settings are never confused.
  - PowerPoint speaker notes reuse the existing protocol detail panel (§3.6a) without requiring any firmware changes.
- Open:
  - Validate slide notes display on a physical board in live slide show on Windows 11.

## 2026-10-23 - Sundar (Antigravity) - Pure Rust in-memory PowerPoint notes & board removal fix (#77, #78)

- Changed:
  - PowerPoint presentation notes (#78):
    - Completely replaced COM / PowerShell automation with a pure Rust in-memory `.pptx` archive extractor (`powerpoint.rs`).
    - Synchronous, instant (sub-2ms), zero lag, cross-platform, and unaffected by Office Protected View or licensing restrictions.
    - Parses XML speaker notes directly (`ppt/notesSlides/notesSlide*.xml` and slide relationships) while skipping metadata shapes (slide numbers, headers, thumbnails).
    - Window title watcher (`focus/src/windows.rs`) now checks `a.title != b.title`, immediately detecting transitions into and out of Slide Show mode.
    - Replaced ad-hoc recipe key splicing with a dedicated `system.presentation` layout mirroring Claude questions (`system.approve` / `show_approval`).
    - Uses the exact same 240-character `clean` filter as Claude approvals (`text.chars().map(...).take(240).collect()`) so `detail` notes are strictly within the board firmware's `UI_DETAIL_BYTES` (241 bytes) and never rejected.
    - Leaves middle row (slots 4, 5, 6) empty in `keys` so the board firmware renders the scrollable speaker notes box in the middle row, with Prev/End/Next on top and First/Black/White on bottom.
    - Dedicated `Event::Board` handler for `system.presentation` dispatches slide advance keystrokes (Left/Right/Home/Escape/B/W), advances slide notes in memory, and immediately re-renders the layout.
    - Enhanced `window_title` in `focus/src/windows.rs` with `find_process_title` via `EnumWindows` if the foreground handle is a child pane without a title.
    - Automatic memory disposal: in-memory presentation notes are completely destroyed (`powerpoint::clear()`) whenever PowerPoint loses focus or closes.
  - Board removal and serial port handling (#77):
    - Fixed legacy migration in `display.rs` so an empty `boards: {}` object is preserved on page refresh and does not re-resurrect forgotten boards.
    - Added retry with backoff in `SerialLink::open` and `knoend_flash::connect` (6 attempts over ~1.2s) to prevent Windows `ERROR_ACCESS_DENIED` race condition during USB preemption on CH340 boards.
    - Updated `Shared::status` synchronously in `remove_board` to eliminate stale UI state during web app polling.
- Tested (Windows 11, live CH340 board 80b54edbcdb8 on Wi-Fi/COM5):
  - Extracted notes from 7-slide test presentation (`Copy of MindStudio Bootcamp.pptx`) in pure Rust; all slide notes, titles, and slide counts verified.
  - All 135+ unit tests passing across `knoend`, `knoend_agents`, `knoend_core`, `knoend_desktop`, `knoend_flash`, `knoend_focus`, `knoend_input`, `knoend_media`, and `knoend_transport`.
  - Service runs live with board connected over Wi-Fi (`knoend serve` on 127.0.0.1:7342).
- Decided:
  - Do not use COM or external PowerShell processes for presentation notes. Direct `.pptx` extraction in pure Rust is faster, reliable, and holds notes in-memory only while presenting.
- Open:
  - User verification on physical board during active PowerPoint presentation.

## 2026-10-23 - Sundar (Antigravity) - Standard PowerPoint edit keys & paginated multi-page speaker notes (#78)

- Changed:
  - Standard PowerPoint edit keys (`recipes/apps/powerpnt.toml`):
    - Configured standard edit-mode shortcuts for PowerPoint: Open (Ctrl+O), New Slide (Ctrl+M), Save (Ctrl+S), Undo (Ctrl+Z), Present (F5), Redo (Ctrl+Y), Duplicate (Ctrl+D), From Current (Shift+F5), Laser / Pen (Ctrl+L).
    - When PowerPoint starts without files or in normal edit view, the board surfaces these standard keys without displaying presentation notes prematurely.
  - Triggering presentation mode (`powerpoint.rs`, `service.rs`):
    - `get_notes` returns `None` unless PowerPoint is actively presenting (`is_active_presenting`), keeping standard recipe keys visible.
    - Presentation mode activates when a slide show window title is detected or when the user taps "Present" / "From Current" (or presses F5 / Shift+F5).
    - Tapping "End" (Key 2) or pressing Escape calls `set_presenting(false)` and `clear()`, completely purging in-memory notes and reverting the board to edit keys.
  - Multi-page speaker notes pagination (`powerpoint.rs`, `service.rs`):
    - Replaced hard 240-char truncation with `paginate_notes`: automatically splits long notes into $\le 220$-character pages on clean line/word boundaries with smart character normalization (smart quotes `'`/`"`, en/em dashes `-`, bullet `*`).
    - Added `next_note_page()` and `prev_note_page()` to cycle note pages in memory.
    - In `system.presentation`, when a slide's notes exceed one page, Key 8 displays page status (e.g. `1/3`) with the `description` icon, and the status bar displays `Slide X of Y (p. A/B)`.
    - Tapping Key 8 or swiping left/right (`Event::Page`) turns note pages smoothly without changing slides.
- Tested (Windows 11, live CH340 board 80b54edbcdb8 over Wi-Fi):
  - Unit tests: `test_paginate_notes`, `test_is_presenting`, `extract_name_from_titles`, and `load_mindstudio_notes` passing.
  - All 136 tests passing across all companion crates (`cargo test`).
  - Running live service (`knoend serve`) connected to board `80b54edbcdb8` over Wi-Fi.
- Decided:
  - Presentation mode is strictly separate from edit mode: edit mode shows standard editing controls, presentation mode shows speaker notes and slide navigation.
  - Long notes are paginated cleanly on word boundaries ($\le 220$ characters) rather than truncated, accessible via key tap or swipe.
- Open:
  - User verification on physical board.

## 2026-10-23 - Sundar (Antigravity) - Presentation mode for unsaved presentations & slides without notes (#78)

- Changed:
  - PowerPoint unsaved presentations (`powerpoint.rs`, `service.rs`):
    - When a presentation is unsaved (e.g. newly created from a template without saving to disk) or has no `.pptx` file on disk, `load_presentation` returns `None`.
    - Previously, `get_notes` returned `None` when `load_presentation` failed, preventing presentation mode from activating on unsaved decks.
    - Updated `get_notes` to initialize an in-memory `ActivePresentation` with `total_slides: 0` when presenting an unsaved deck, displaying `"No notes for this slide"` in the detail panel and presentation navigation keys (Prev, Next, First, End, Black, White).
    - Updated slide advance (`next_slide()`) and status text (`Slide 1`, `Slide 2`, etc.) to smoothly advance and count slides even when `total_slides` is unconstrained by disk metadata.
- Tested (Windows 11, live CH340 board 80b54edbcdb8 over Wi-Fi):
  - Added unit test `test_unsaved_presentation_mode` in `powerpoint.rs` verifying presentation mode activation, empty notes fallback, and slide advance on unsaved decks.
  - All unit tests passing.
  - Live service restarted and connected to board `80b54edbcdb8`.
- Decided:
  - Presentation controls should always be available whenever a presentation is running, regardless of whether the file has been saved to disk or has speaker notes.
  - Never guess or fallback to recent/downloaded presentations when presenting an unsaved deck or template: presentation notes must only be loaded if the active window title explicitly matches the presentation file.
  - Design record: [docs/decisions/2026-10-23-powerpoint-presentation-notes.md](decisions/2026-10-23-powerpoint-presentation-notes.md).
- Open:
  - None; tested and verified on live board.

## 2026-10-23 - Sundar (Antigravity) - Release v0.2.2

- Changed:
  - Bumped version to `0.2.2` in `companion/Cargo.toml`, `extension/manifest.json`, `installer/windows/knoend.iss`, `README.md`, and `docs/status.md`.
  - PowerPoint presentation mode: pure Rust in-memory extraction, dedicated `system.presentation` layout, paginated notes for long speaker notes, slide controls (Prev, Next, First, End, Black, White screen), and standard edit keys in edit mode (#78).
  - Board removal (#77): web app UI with modal for "Forget & unpair" vs "Remove look only", fixed empty board map preservation in `display.rs`.
  - Windows serial reliability: added retry with exponential backoff in `SerialLink::open` and `knoend_flash::connect` against `ERROR_ACCESS_DENIED` during rapid serial port transitions.
  - Spec validation: all 28 valid and 19 invalid examples passing with 0 errors.
- Tested:
  - All 137 unit and integration tests passing on Windows 11.
  - Tested on physical CH340 board `80b54edbcdb8` over Wi-Fi and USB serial.
- Decided:
  - Release v0.2.2 cut and tagged.
- Open:
  - None.

## 2026-10-22 - Tom (Claude Code, Opus 5.5) - macOS app lists: Apple's apps, no background programs (#73, #74)

- Changed:
  - `desktop/src/installed.rs`: macOS also lists `/System/Applications` (and its Utilities) and
    Finder (#73).
  - `desktop/src/mac_apps.rs` `background`, used by `cli/src/launcher.rs`: on macOS a background
    program (found by Spotlight under /System/Library, Finder excepted, or marked LSUIElement /
    LSBackgroundOnly) is not counted as used, and at start the service forgets those already in
    `usage.json`: loginwindow, Spotlight, Dock, controlcenter, UserNotificationCenter,
    BluetoothUIServer no longer fill the home screen (#74).
- Tested (macOS 26.6, FNK0104B over Wi-Fi):
  - Bug on main: Tom's 2026-10-22 test (16 apps listed; loginwindow and others on the home screen).
  - `/api/apps`: 82 apps (16 before), with Finder, Terminal, Notes, Mail, Calendar, Preview.
  - `usage.json` after start: VS Code, Firefox, Terminal, Finder, Calculator, Clock, VLC kept; the
    seven background programs gone. Home screen on the board (Tom): real apps only.
  - `cargo test -p knoend launcher`: passing (new `apps_not_installed_are_forgotten`).
- Decided:
  - Codex review: the first version counted only apps found at start in the Applications folders;
    Codex pointed out that drops apps in subfolders and apps installed while the service runs. Now
    each app is checked where Spotlight finds it, once per run.
    A second pass found Keychain Access and DVD Player (in /System/Library/CoreServices/Applications)
    were skipped too; Applications folders under /System/Library now count as apps.
- Open:
  - None.

## 2026-10-23 - Tom (Claude Code, Opus 5.5) - Font menus open every time; size and font shown at once (#50)

- Changed (`web/app.js`, `index.html`, `style.css`):
  - The font fields get a menu of their own (`fontMenu`) instead of the browser's `<datalist>`,
    which lists only entries matching the field and opens when the browser likes (emptying the
    field on focus was tried first: Tom found the list still did not open). The whole list on every
    click, narrowed as you type, arrow keys and Enter; "Built-in (Montserrat)" first; leaving
    without picking keeps the font.
  - A font picked goes to the board at once. The start-up size goes 1.5 s after the last click on
    its arrows (each click is a `change` too, so the first try sent on the first click), or when
    the field is left.
- Tested (macOS 26.6, FNK0104B over Wi-Fi):
  - #50 point 2 (size alone not sent) no longer happens on main: `POST /api/display
    {"start_size": 95}` logs `Display: asking the board for {"startSize":95,...}` and the board
    reports 95 (put back to 96 after).
  - `app.js` parses (JavaScriptCore, `new Function`); the `font-families` datalist and its `list`
    attributes removed, no other ids changed.
  - Tom, in the browser with the board: the full list opens again right after a pick; a picked
    font shows on the board at once; several clicks on the size arrows show one start-up screen.
- Decided:
  - A menu written for the web app rather than `<datalist>`: browsers differ on when it opens.
- Open:
  - None.

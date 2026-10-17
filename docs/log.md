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

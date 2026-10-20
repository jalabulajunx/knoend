# What's on main

As of 2026-10-20 (evening). A one-page map for anyone (person or coding agent) picking the project up.
How to set it up: [`setup.md`](setup.md). Day-by-day detail, including what was tested and how:
[`log.md`](log.md). Working rules: [`../AGENTS.md`](../AGENTS.md). Picking up dynamic layouts or
terminal-aware keys: read the "Handoff for Tom's agents" section of the 2026-10-18 log entry first;
for the setup assistant, settings, fonts and the board's look, the 2026-10-20 handoff section.

## Works today (tested on an ONX2432G028 with Arch Linux, i3/X11, and with macOS 26.6 on Apple silicon)

| Piece | Where | Try it |
| --- | --- | --- |
| Universal firmware image + board definitions | `firmware/components/board_generic`, `firmware/boards/` | `idf.py -B build/universal build` |
| Web app: AI or manual setup, find, identify, install, pair, Wi-Fi from a scan, dashboard | `web/`, `companion/crates/cli/src/web.rs` | `knoend serve --firmware ../dist/firmware` |
| Identify and flash boards, manifest with SHA-256 per part | `companion/crates/flash`, `tools/release_firmware.py` | `knoend devices`, `knoend flash` |
| Setup assistant grounded in the knowledge base | `companion/crates/agents/src/assistant.rs`, `docs/knowledge/setup.md` | "Ask the assistant" in the web app |
| Key grid, status bar, confirm-tap for risky keys | `firmware/main/ui.c` | tap a key; console prints `KEY n ...` |
| Grids 1x1 to 4x4 per layout, pages turned by swipe or the "2/3 >" tap (Freenove and ONX tested) | `firmware/main/ui.c`, `core/src/layout.rs`, `cli/src/service.rs` | `spec/examples/library-pages.toml` |
| Bluetooth LE keyboard ("Just Works") | `firmware/main/ble_keyboard.c` | pair "KnoEnd"; keys type |
| Keys typed by the service when no Bluetooth host is connected | `companion/crates/input` | turn Bluetooth off; keys still type |
| Protocol v0 over USB serial and authenticated Wi-Fi WebSocket, mDNS | `firmware/main/`, `companion/crates/transport` | `knoend pair`, then `knoend run` |
| Layout follows the focused app (Linux X11) | `companion/crates/focus`, `cli/src/service.rs` | switch to VS Code |
| Configure agent: drafts a library for an unsupported app (any provider, or Claude Code / Antigravity CLI signed in on this computer, no API key) | `companion/crates/agents` | tap Configure, or `knoend draft` |
| Shared contracts and validator | `spec/`, `tools/validate_spec.py` | `python3 tools/validate_spec.py` |
| macOS: layout follows the front window, keys typed by the service, assistant snapshot shows USB driver and Accessibility state | `focus/src/macos.rs`, `input/src/macos.rs`, `cli/src/diagnostics.rs` | switch apps on a Mac; "Ask the assistant" |
| USB-Serial-JTAG serial line (native-USB boards): identify, install, pair | `firmware/main/`, `companion/crates/flash` | Freenove FNK0104B on macOS |
| Installing from the published release (v0.1.0; v0.1.1 adds the Mac fixes and onboarding work) through the web app | `companion/crates/cli/src/firmware.rs` | default firmware source |
| CI: spec, companion on Linux + macOS, firmware build | `.github/workflows/ci.yml` | every push and PR |

## Browser, desktop apps, coding agents (added 2026-10-18; Linux tested, macOS partly)

| Piece | Where | Try it |
| --- | --- | --- |
| Extension for Firefox and Chrome: active site, per-site buttons, page actions, page inventory, favicon | `extension/`, `spec/browser.md` | `python3 tools/build_extension.py`; see `extension/README.md` |
| Native messaging host + service socket | `companion/crates/cli/src/browser.rs` | `knoend browser install` |
| Site layouts (`match.web`: host, routes incl. `#/` hash routes, `@fullscreen`); built-in YouTube, Google Slides presenting | `recipes/sites/`, `core/src/focus.rs`, `core/src/catalog.rs` | YouTube; present a Slides deck |
| "Configure this site" (board or popup): AI drafts keys from the page's controls, site-wide then per kind of page | `agents/src/site.rs`, `cli/src/service.rs` | Configure on the board in any site |
| Now playing (MPRIS, Linux): media keys reach the player; media row in slots 7-9 while something plays | `companion/crates/media` | play anything |
| Material icons on keys (132 names, built into the firmware) | `spec/icons.txt`, `firmware/components/icons`, `tools/build_icons.py` | any built-in library |
| Claude Code asks through the board: permission requests (Allow / Always / Deny / Stop) with a scrollable explanation; withdrawn when answered on screen | `cli/src/agent_hooks.rs` | `knoend claude-hooks install`, new Claude Code session |
| Desktop notifications from followed apps (Linux D-Bus) | `companion/crates/desktop` | Claude desktop notifications |
| Launcher when nothing is focused: most used / recent apps and sites with their own colour icons; tap opens (Linux; macOS since 2026-10-18) | `cli/src/launcher.rs`, `desktop/src/apps.rs`, `mac_apps.rs`, `focus/src/macos.rs`, `knoend.image` | empty workspace; on a Mac click the desktop |
| Edit keys in the web app: the board shows the layout being edited, live; a board tap selects a key; label (forced line break), icon, spoken text, confirm, recorded shortcut; drag to move, grid per page, add/remove keys; Save as your own version on top of the recipe or draft (FNK tested; ONX not yet) | `cli/src/edit.rs`, `cli/src/service.rs` (`show_edit`), `web/app.js` | web app > Edit keys |
| Key labels never split a word: one font size smaller (12 px at least), then cut with "..." (FNK tested; ONX not yet) | `firmware/main/ui.c` (`fitting_font`, `cut_long_words`) | a long one-word label in Edit keys |
| Background service (systemd user unit / LaunchAgent) | `cli/src/background.rs` | `knoend service install` |
| Recipes: apps, sites and Claude Code's screens as data, each with who tested it on which OS | `recipes/`, `spec/protocol.md` section 5 | `python3 tools/validate_spec.py` |

## How the board looks, fonts, and the assistant that listens (added 2026-10-20; ONX + Linux tested)

| Piece | Where | Try it |
| --- | --- | --- |
| Key size (2x2, 3x3 default, 4x4): every page uses that grid, keys packed in order | `cli/src/display.rs`, `core/src/layout.rs` (`packed`) | dashboard > Look |
| Look (Dark, White, Sepia) and way round (landscape, portrait, upside down), kept on the board and changed live (`knoend.display`, spec 3.6d) | `firmware/main/display.c`, `ui.c`, `board_generic.c` (`board_set_turn`) | dashboard > Look |
| Fonts from Google Fonts for key labels and the start-up "Kn°End", kept in a fonts partition, start-up size (`knoend.font`, spec 3.6e) | `firmware/main/fonts.c`, `cli/src/fonts.rs` | dashboard > Key font / Start-up font |
| AI providers without an API key: Claude Code, Antigravity CLI found on this computer | `agents/src/cli_tool.rs` | AI settings |
| The service describes itself to the setup assistant: every setting, read-only checks it runs, everything the service prints | `cli/src/settings.rs`, `checks.rs`, `journal.rs` | ask "make the keys bigger" |
| The assistant listens all the time: faults (`problem!`) are put to it without being asked; its answer opens in the web app | `cli/src/watch.rs`, `consult.rs` | unplug the board for 2 minutes |
| Board reports why it last started (`lastStart`), drops a font only after a real crash, and recovers from a lost display update (LVGL memory from PSRAM; the freeze's cause, fixed 2026-10-20) | `firmware/main/protocol.c`, `fonts.c`, `lv_mem_psram.c`, `board_generic.c` | serial log |
| Several Claude questions (#41): a panel listing them while answered in the app, Answer here, cleared at once when answered | `cli/src/service.rs` (`show_answering`, `hosts_agent`), `agent_hooks.rs` (`settles`) | a multi-question prompt in Claude |
| Web app redesign: drawn board with its real screen (FIG. 01), tips, an "ask about this" ° on every section, an assistant panel with a conversation and suggestions, a setup rail with a board acting out each step | `web/` | `knoend serve` |

## Built, not yet tested

- GT911 touch, display start-up commands, active-high reset; the ESP32-S3-BOX-3 and Waveshare
  ESP32-S3-Touch-LCD-2 definitions (status "unverified").
- The 2026-10-20 additions on the FNK0104B and on macOS (display settings, fonts, the watcher's
  checks; `board_serial_log` assumes a USB-serial port).
- The display-freeze fix (LVGL memory from PSRAM, `firmware/main/lv_mem_psram.c`) on the
  FNK0104B: same code path as the ONX, where it is tested (0 lost updates in 32 minutes); run it
  there before turning fonts on.
- macOS: site icons, LaunchAgent. Compiled in CI only. "Go to Claude" never appears on macOS:
  it comes from desktop notifications, which are read only on Linux.
- The assistant's macOS buttons (open Accessibility, open Driver Extensions, install WCH's USB
  driver after Apple's notarization check), `cli/src/macos_help.rs`: the same steps were done by
  hand on 2026-10-17; the buttons themselves have not been pressed yet.

## Not started

| Piece | Where it goes |
| --- | --- |
| Freenove FNK0104B native USB keyboard | `firmware/boards/definitions/` |
| Board UI: unsupported-app card | `firmware/main/ui.c` |
| Windows focus and typing | `companion/crates/focus`, `input` |
| Terminal-aware keys (which CLI runs in the focused terminal) - Tom | `companion/crates/focus`, `cli/src/service.rs` |
| Codex / other agent CLIs' hooks (Claude Code done) | `cli/src/agent_hooks.rs` |
| Now playing on macOS (no MPRIS) | `companion/crates/media` |
| RGB-panel boards (4.3-7"), Hynitron CST3xx touch, quad-PSRAM image | see `boards/research.md` |

# What's on main

As of 2026-10-18. A one-page map for anyone (person or coding agent) picking the project up.
How to set it up: [`setup.md`](setup.md). Day-by-day detail, including what was tested and how:
[`log.md`](log.md). Working rules: [`../AGENTS.md`](../AGENTS.md). Picking up dynamic layouts or
terminal-aware keys: read the "Handoff for Tom's agents" section of the 2026-10-18 log entry first.

## Works today (tested on an ONX2432G028 with Arch Linux, i3/X11, and with macOS 26.6 on Apple silicon)

| Piece | Where | Try it |
| --- | --- | --- |
| Universal firmware image + board definitions | `firmware/components/board_generic`, `firmware/boards/` | `idf.py -B build/universal build` |
| Web app: AI or manual setup, find, identify, install, pair, Wi-Fi from a scan, dashboard | `web/`, `companion/crates/cli/src/web.rs` | `openpager serve --firmware ../dist/firmware` |
| Identify and flash boards, manifest with SHA-256 per part | `companion/crates/flash`, `tools/release_firmware.py` | `openpager devices`, `openpager flash` |
| Setup assistant grounded in the knowledge base | `companion/crates/agents/src/assistant.rs`, `docs/knowledge/setup.md` | "Ask the assistant" in the web app |
| Key grid, status bar, confirm-tap for risky keys | `firmware/main/ui.c` | tap a key; console prints `KEY n ...` |
| Bluetooth LE keyboard ("Just Works") | `firmware/main/ble_keyboard.c` | pair "OpenPager"; keys type |
| Keys typed by the service when no Bluetooth host is connected | `companion/crates/input` | turn Bluetooth off; keys still type |
| Protocol v0 over USB serial and authenticated Wi-Fi WebSocket, mDNS | `firmware/main/`, `companion/crates/transport` | `openpager pair`, then `openpager run` |
| Layout follows the focused app (Linux X11) | `companion/crates/focus`, `cli/src/service.rs` | switch to VS Code |
| Configure agent: drafts a library for an unsupported app (any provider) | `companion/crates/agents` | tap Configure, or `openpager draft` |
| Shared contracts and validator | `spec/`, `tools/validate_spec.py` | `python3 tools/validate_spec.py` |
| macOS: layout follows the front window, keys typed by the service, assistant snapshot shows USB driver and Accessibility state | `focus/src/macos.rs`, `input/src/macos.rs`, `cli/src/diagnostics.rs` | switch apps on a Mac; "Ask the assistant" |
| USB-Serial-JTAG serial line (native-USB boards): identify, install, pair | `firmware/main/`, `companion/crates/flash` | Freenove FNK0104B on macOS |
| Installing from the published release (v0.1.0; v0.1.1 adds the Mac fixes and onboarding work) through the web app | `companion/crates/cli/src/firmware.rs` | default firmware source |
| CI: spec, companion on Linux + macOS, firmware build | `.github/workflows/ci.yml` | every push and PR |

## Browser, desktop apps, coding agents (added 2026-10-18; Linux tested, macOS partly)

| Piece | Where | Try it |
| --- | --- | --- |
| Extension for Firefox and Chrome: active site, per-site buttons, page actions, page inventory, favicon | `extension/`, `spec/browser.md` | `python3 tools/build_extension.py`; see `extension/README.md` |
| Native messaging host + service socket | `companion/crates/cli/src/browser.rs` | `openpager browser install` |
| Site layouts (`match.web`: host, routes incl. `#/` hash routes, `@fullscreen`); built-in YouTube, Google Slides presenting | `recipes/sites/`, `core/src/focus.rs`, `core/src/catalog.rs` | YouTube; present a Slides deck |
| "Configure this site" (board or popup): AI drafts keys from the page's controls, site-wide then per kind of page | `agents/src/site.rs`, `cli/src/service.rs` | Configure on the board in any site |
| Now playing (MPRIS, Linux): media keys reach the player; media row in slots 7-9 while something plays | `companion/crates/media` | play anything |
| Material icons on keys (132 names, built into the firmware) | `spec/icons.txt`, `firmware/components/icons`, `tools/build_icons.py` | any built-in library |
| Claude Code asks through the board: permission requests (Allow / Always / Deny / Stop) with a scrollable explanation; withdrawn when answered on screen | `cli/src/agent_hooks.rs` | `openpager claude-hooks install`, new Claude Code session |
| Desktop notifications from followed apps (Linux D-Bus) | `companion/crates/desktop` | Claude desktop notifications |
| Launcher when nothing is focused: most used / recent apps and sites with their own colour icons; tap opens (Linux; macOS since 2026-10-18) | `cli/src/launcher.rs`, `desktop/src/apps.rs`, `mac_apps.rs`, `focus/src/macos.rs`, `openpager.image` | empty workspace; on a Mac click the desktop |
| Background service (systemd user unit / LaunchAgent) | `cli/src/background.rs` | `openpager service install` |
| Recipes: apps, sites and Claude Code's screens as data, each with who tested it on which OS | `recipes/`, `spec/protocol.md` section 5 | `python3 tools/validate_spec.py` |

## Built, not yet tested

- GT911 touch, display start-up commands, active-high reset; the ESP32-S3-BOX-3 and Waveshare
  ESP32-S3-Touch-LCD-2 definitions (status "unverified").
- macOS: site icons,
  Claude Code hooks, LaunchAgent. Compiled in CI only.
- The assistant's macOS buttons (open Accessibility, open Driver Extensions, install WCH's USB
  driver after Apple's notarization check), `cli/src/macos_help.rs`: the same steps were done by
  hand on 2026-10-17; the buttons themselves have not been pressed yet.

## Not started

| Piece | Where it goes |
| --- | --- |
| Freenove FNK0104B definition (and native USB keyboard) | `firmware/boards/definitions/` |
| Board UI: dynamic tiles, 6 per page with paging, unsupported-app card | `firmware/main/ui.c` |
| Windows focus and typing | `companion/crates/focus`, `input` |
| Terminal-aware keys (which CLI runs in the focused terminal) - Tom | `companion/crates/focus`, `cli/src/service.rs` |
| Codex / other agent CLIs' hooks (Claude Code done) | `cli/src/agent_hooks.rs` |
| Now playing on macOS (no MPRIS) | `companion/crates/media` |
| RGB-panel boards (4.3-7"), Hynitron CST3xx touch, quad-PSRAM image | see `boards/research.md` |

# What's on main

As of 2026-10-18. A one-page map for anyone (person or coding agent) picking the project up.
How to set it up: [`setup.md`](setup.md). Day-by-day detail, including what was tested and how:
[`log.md`](log.md). Working rules: [`../AGENTS.md`](../AGENTS.md).

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
| Installing v0.1.0 from the published release through the web app | `companion/crates/cli/src/firmware.rs` | default firmware source |
| CI: spec, companion on Linux + macOS, firmware build | `.github/workflows/ci.yml` | every push and PR |

## Built, not yet tested

- GT911 touch, display start-up commands, active-high reset; the ESP32-S3-BOX-3 and Waveshare
  ESP32-S3-Touch-LCD-2 definitions (status "unverified").
- The assistant's macOS buttons (open Accessibility, open Driver Extensions, install WCH's USB
  driver after Apple's notarization check), `cli/src/macos_help.rs`: the same steps were done by
  hand on 2026-10-17; the buttons themselves have not been pressed yet.

## Not started

| Piece | Where it goes |
| --- | --- |
| Freenove FNK0104B definition (and native USB keyboard) | `firmware/boards/definitions/` |
| Board UI: dynamic tiles, 6 per page with paging, unsupported-app card | `firmware/main/ui.c` |
| Windows focus and typing | `companion/crates/focus`, `input` |
| Chrome extension for per-website layouts | `extension/` |
| Agent-CLI hooks (Claude Code, Codex) and approvals on the board | `companion/crates/agents`, firmware |
| RGB-panel boards (4.3-7"), Hynitron CST3xx touch, quad-PSRAM image | see `boards/research.md` |

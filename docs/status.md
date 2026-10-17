# What's on main

As of 2026-10-17. A one-page map for anyone (person or coding agent) picking the project up.
The day-by-day detail, including what was tested and how, is in [`log.md`](log.md); working
rules are in [`../AGENTS.md`](../AGENTS.md).

## Works today (tested on an ONX2432G028 with Arch Linux, i3)

| Piece | Where | Try it |
| --- | --- | --- |
| Board profile ONX2432G028, written from the schematic | `firmware/components/board_onx2432g028` | `idf.py -B build/onx2432g028 -p /dev/ttyUSB0 flash` |
| 3x3 key grid, status bar, confirm-tap for risky keys | `firmware/main/ui.c` | tap a key; console prints `KEY n ...` |
| Bluetooth LE keyboard ("Just Works" pairing) | `firmware/main/ble_keyboard.c`, `components/esp_hid_gap` | pair "OpenPager"; keys type |
| Protocol v0 over USB serial | `firmware/main/protocol.c`, `link_serial.c` | `tools/serial_probe.py` |
| Wi-Fi: key and network over USB, mDNS, authenticated WebSocket | `firmware/main/link_wifi.c` | `openpager pair`, then `openpager run` |
| Shared contracts: packets, schemas, examples, validator | `spec/`, `tools/validate_spec.py` | `python3 tools/validate_spec.py` |
| Companion core: libraries, validation, matching, layouts | `companion/crates/core` | `cargo test` in `companion/` |
| Wi-Fi link, discovery, serial pairing (Rust) | `companion/crates/transport` | reproduces `spec/examples/wifi-session.json` |
| Focus watcher, Linux/X11 | `companion/crates/focus` | `cargo run --example print_focus` |
| `openpager` service: layout follows the focused app | `companion/crates/cli` | switch apps; the board follows |
| Configure agent: drafts a library for an unsupported app | `companion/crates/agents` | tap Configure; or `openpager draft --class ... --instance ...` |
| Built-in library: VS Code | `libraries/builtin/vscode.toml` | focus VS Code |
| Bench tools: serial probe, Python stand-in companion with attack tests | `tools/` | `tools/dummy_companion.py attacks` |

## Not started

| Piece | Where it goes |
| --- | --- |
| Freenove FNK0104B board profile (native USB keyboard) | `firmware/components/board_fnk0104b` (placeholder; build with `-DOPENPAGER_BOARD=fnk0104b`) |
| macOS and Windows focus backends | `companion/crates/focus` |
| Web app (onboarding, settings, review of AI drafts, flashing) and the local API it talks to | `web/`, `companion/crates/server` |
| Board identification and flashing from the service | `companion/crates/flash` |
| Chrome extension for per-website layouts | `extension/` |
| Board simulator | `companion/crates/sim` |
| Agent-CLI hooks (Claude Code, Codex) and approvals on the board | `companion/crates/agents`, firmware |

## Next on the ONX

- A full-screen "Unsupported app: <name>" card with a Configure button (today: a one-key layout).
- Keep the last layout across reboots.
- Show the Bluetooth pairing code on the board, then move back from "Just Works" to numeric
  comparison.

## Running it

```bash
# firmware (ESP-IDF v5.4.4)
. ~/esp/esp-idf/export.sh && cd firmware && idf.py -B build/onx2432g028 -p /dev/ttyUSB0 flash

# companion (Rust)
cd companion && cargo build
target/debug/openpager pair          # board on USB: key + Wi-Fi settings
target/debug/openpager run           # follows the focused app over Wi-Fi
```

The configure agent reads `~/.config/openpager/agent.toml` (`provider = "gemini"`, `"openai"`,
`"openrouter"`, `"anthropic"`, `"ollama"` or `"openai-compatible"`); API keys come from environment
variables such as `GEMINI_API_KEY`, and the model from `agent.toml` or `<PROVIDER>_MODEL`.

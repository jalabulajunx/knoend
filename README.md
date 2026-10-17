# OpenPager

An open-source, cross-OS shortcut keypad and pager built on inexpensive ESP32-S3 touch boards.

OpenPager turns a 2.8" ESP32-S3 touchscreen board into a 3×3 keypad whose keys change with the app or
website you are using. It connects to your computer by wire, Bluetooth or home Wi-Fi. Over Wi-Fi it
becomes a pager: carry it around the house, see status from your computer on the board, and answer a
prompt (for example, approve a coding agent's command) from another room.

> **Status: early development.** The keypad, Wi-Fi pager link, app-following layouts, web setup and
> flashing work on Linux; much of the feature list is still planned. See
> [What works today](#what-works-today). **To set one up, follow [docs/setup.md](docs/setup.md).**

## Why

- **Shortcut keypads are closed and skip Linux.** Commercial keypads such as the Logitech MX Keypad
  run only through vendor software on Windows and macOS.
- **Every app needs setup.** A keypad is only useful where someone has already built a profile; an app
  or website nobody prepared gets blank keys.
- **Extensions are code.** Marketplace plugins run with your permissions. A shared layout should be
  safe to accept from a stranger.
- **The hardware is already cheap and common.** A board you may already own becomes the keypad.

## How it works

```
                      ┌──────────────────────── Computer (any OS) ───────────────────────┐
┌── Board ─────────┐  │                                                                   │
│ Touch keys 3×3   │──┼── Bluetooth LE keyboard (HID) ──────────► Sees a keyboard        │
│ Layouts (NVS/SD) │  │                                            (no install needed)    │
│ Firmware:        │◄─┼── Wi-Fi WebSocket / Bluetooth data / ──► Companion service (Rust)│
│  key → action    │  │   USB-C serial                             - active-app watcher   │
│ Status bar       │  │                                            - library matching     │
│ Speaker (beeps)  │  │                                            - agent hooks          │
└──────────────────┘  │                                            - AI library agent     │
        ▲             └───────────────────────────────────────────────────────────────────┘
        │ Wi-Fi: board's own web page (setup, settings) from any phone or laptop browser
```

- **The board works alone.** Paired over Bluetooth it is an ordinary keyboard on any computer, with
  nothing installed.
- **The companion adds the smarts.** A small local Rust service, used through a web app, watches
  which app or website is in focus, picks the matching layout and pushes it to the board, carries out
  actions, and feeds the status bar. It also identifies and flashes boards.
- **Layouts are data, not code.** A layout library is a TOML file checked by one validator, whether it
  is built in, contributed by the community, drafted by AI for an app nobody has configured, or your
  own override.

```toml
[pack]
name = "Amazon"
version = "1.2.0"
source = "community"   # builtin | community | ai | user

[match]
web = ["amazon.ca", "amazon.com", "amazon.*"]

[[key]]
label = "Orders"
speak = "Your orders"
action = { kind = "open", url = "/gp/your-account/order-history" }
```

### Planned features

- 3×3 (or 2×3) touch grid with a status bar underneath
- Three connections: USB-C, Bluetooth LE, home Wi-Fi (on-board web server at `openpager.local`)
- Per-app and per-website layouts on Linux, macOS and Windows (websites via a Chrome extension)
- Layout libraries: built-in, community (via pull request), AI-drafted on first use, and user overrides
- Approve or deny terminal coding agents (Claude Code, Codex and others) from the board through their
  hooks — the real command is shown, destructive ones need a second tap, and no answer means no
- Every key press spoken aloud, plus a 3D-printed grid overlay so keys can be found by touch
- Colour or e-ink-style theme
- First-run setup: pick from the apps installed on your machine, or describe your work to a setup agent
  running on a local or API model

### Safety rules

- Keys trigger only allowed actions: shortcuts, same-site pages, page elements, or commands you have
  named on your computer — never raw command lines.
- Buy, submit, send and delete need a second tap.
- Board and computer pair once with a shared key; every Wi-Fi message is authenticated.
- AI-drafted layouts are marked "unreviewed". Only the app ID, domain and page structure are sent to a
  model, and a local model can be used instead.
- Third-party plugins (code) will only ever be a separate, opt-in tier.

## Supported boards

All boards run one universal firmware image; a small JSON definition per board describes its display,
touch controller and pins, so adding a board is usually a definition, not new code.

| Board | Status | Display | Touch |
| --- | --- | --- | --- |
| ONX2432G028 | **supported** (tested) | 2.8" 320×240 ST7789 | CST826 |
| Espressif ESP32-S3-BOX-3 | unverified | 2.4" 320×240 ILI9342C | GT911 |
| Waveshare ESP32-S3-Touch-LCD-2 | unverified | 2" 240×320 ST7789T3 | CST816D |
| Freenove FNK0104B | coming | 2.8" 320×240 ILI9341 | FT6336 |

"Unverified" means written from the maker's documentation and not yet run on the board. Any
ESP32-S3 board with octal PSRAM, an ST7789 or ILI9341 SPI display and CST816, CST826, FT6336 or
GT911 touch can be added; see [docs/boards/research.md](docs/boards/research.md).

## What works today

Tested on an ONX2432G028 with Arch Linux (X11):

- Setting up an erased board from the web app: identify, install, pair, Wi-Fi from a scanned list
- Bluetooth LE keyboard (no software needed), and keys typed through the service when Bluetooth is off
- Authenticated Wi-Fi link found by mDNS; the board keeps working away from the computer
- Layout follows the focused app (built-in VS Code layout); "Unsupported app" with Configure, which
  asks an AI model (Gemini, OpenAI, Anthropic, OpenRouter or local Ollama) to draft a layout
- An AI setup assistant in the web app that knows the failures we have hit

Builds but untested: macOS (focus watcher, typing), the unverified boards, the release workflow.
Not built yet: Windows, per-website layouts (Chrome extension), coding-agent approvals, speech.

## Getting started

[docs/setup.md](docs/setup.md) covers both setting up a board and working on the code. In short: download `openpager` for Linux or macOS from the
[latest release](https://github.com/jalabulajunx/openpager/releases/latest), plug in the board, and run

```bash
./openpager serve     # opens the web app; firmware comes from the same release
```

Problems:
[docs/knowledge/setup.md](docs/knowledge/setup.md). What is on main: [docs/status.md](docs/status.md).

## Repository layout

```
spec/                 shared contracts: packet protocol, JSON schemas (incl. board definitions), examples
firmware/             ESP-IDF project (C): one universal image
  main/               grid UI, keys, protocol, Bluetooth / Wi-Fi / serial links
  components/         board_generic (drivers from the board definition), board_api, esp_hid_gap
  boards/             catalog.json and definitions/*.json, one per board
companion/            Rust workspace; the `openpager` binary (service + CLI)
  crates/             core, transport, focus, input, flash, agents, cli
web/                  web app served by the service (setup wizard, assistant, dashboard)
libraries/builtin/    built-in layout libraries (*.toml)
docs/                 setup guide, knowledge base, board research, log, decisions/
tools/                spec validator, firmware release packager, bench tools
```

## Prior art and credits

OpenPager is an integration of existing ideas, not a new invention. Projects that got here first:

- [Logitech MX Keypad](https://www.logitech.com/) — app-aware LCD keypad (Windows/macOS)
- [open-screen-deck](https://github.com/vcazan/open-screen-deck), [FreeTouchDeck](https://hackaday.io/project/175827-freetouchdeck),
  [Introvert-Programmer-Lab/opendeck](https://github.com/Introvert-Programmer-Lab/opendeck) — open ESP32 decks
- [duckyPad Pro](https://www.cnx-software.com/2025/09/23/duckypad-pro-20-key-esp32-s3-macropad-supports-up-to-3700-macros-using-duckyscript-language/) — Bluetooth macropad with active-window switching
- [StreamController](https://github.com/StreamController/StreamController), [OpenDeck](https://github.com/nekename/OpenDeck) — open Stream Deck software
- [ClaudePad](https://github.com/ailynux/ClaudePad), [orchestrator-pad](https://github.com/nickthelegend/orchestrator-pad) — keypads for AI coding agents
- AI-generated Stream Deck profiles (Deck Smith, Raycast generator, OpenDeck Factory), Elgato Smart Profiles
- [KDE Connect](https://github.com/KDE/kdeconnect-kde) — packet, identity and pairing concepts
  (concepts only; no code used)

### Pre-existing work and tools

- The Bluetooth keyboard code builds on Espressif's `esp_hid_device` example (Unlicense / CC0).
- A bench prototype (USB serial + Bluetooth keyboard) was built before this repository started. It is
  not included; its board drivers were the board maker's example code, so the board profiles here are
  written from the schematics and Espressif's open-source components.
- AI tools: built with help from Claude Code (planning, research and code).

## Trademarks

Logitech, MX Keypad, Elgato and Stream Deck are trademarks of their owners. OpenPager is not affiliated
with or endorsed by them; product names are used only to describe compatibility and comparison.

## License

[MIT](LICENSE).

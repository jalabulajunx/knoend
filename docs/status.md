# What's on main

As of 2026-10-21. A one-page map for anyone (person or coding agent) picking the project up.
How to set it up: [`setup.md`](setup.md). Day-by-day detail, including what was tested and how:
[`log.md`](log.md). Working rules: [`../AGENTS.md`](../AGENTS.md). Picking up dynamic layouts or
terminal-aware keys: read the "Handoff for Tom's agents" section of the 2026-10-18 log entry first;
for the setup assistant, settings, fonts and the board's look, the 2026-10-20 handoff section.

## Works today (tested on an ONX2432G028 with Arch Linux, i3/X11, macOS 26.6 on Apple silicon, and Windows 11)

| Piece | Where | Try it |
| --- | --- | --- |
| Universal firmware image + board definitions | `firmware/components/board_generic`, `firmware/boards/` | `idf.py -B build/universal build` |
| Web app: AI or manual setup, find, identify, install, pair, Keys & look over USB, separate optional Wi-Fi and Bluetooth steps, dashboard | `web/`, `companion/crates/cli/src/web.rs` | `knoend serve --firmware ../dist/firmware` |
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
| Windows: layout follows the foreground window, keys typed via `SendInput`, USB bus scan (`setupapi`), app icons (`SHGetFileInfoW`), native messaging + agent hooks over authenticated local TCP, startup service (`HKCU\...\Run`) | `focus/src/windows.rs`, `input/src/windows.rs`, `desktop/src/win_apps.rs`, `cli/src/browser.rs`, `cli/src/agent_hooks.rs`, `cli/src/background.rs` | `knoend devices`, `knoend serve`, `knoend browser install` on Windows |
| USB-Serial-JTAG serial line (native-USB boards): identify, install, pair | `firmware/main/`, `companion/crates/flash` | Freenove FNK0104B on macOS |
| Release downloads: Windows installer (per user, unsigned; `installer/windows/knoend.iss`, built by Inno Setup in CI and the release), browser extension zips (unsigned), install notes (`installer/release-notes.md`) | `.github/workflows/release.yml`, `tools/build_extension.py` | CI artifact `windows-installer`; next tag |
| PowerPoint presentation notes & controls: in slide show mode, active slide notes appear in the detail panel (scrollable by swiping) with advance/prev/stop controls on Windows | `recipes/apps/powerpnt.toml`, `companion/crates/cli/src/powerpoint.rs`, `service.rs` | start a presentation in PowerPoint |
| Web app board removal: options to either forget/unpair completely or remove from display settings | `web/index.html`, `web/app.js`, `companion/crates/cli/src/web.rs`, `display.rs`, `pairing.rs` | "Remove board" on Board card in web app |
| Installing from the published release (v0.2.1, 2026-10-21: the AMOLED board, several boards mirrored with their own looks, Bluetooth as a full channel, the Windows installer; v0.2.0 before) through the web app | `companion/crates/cli/src/firmware.rs` | default firmware source |
| Keys for your apps: set-up step 7 (and Set up apps on the dashboard) lists installed apps, marks which have keys, and the AI drafts keys for up to 12 picked apps, two at a time (Linux tested; macOS and Windows listing untested) | `desktop/src/installed.rs`, `cli/src/app_setup.rs`, `web/` | Set up a board, Your apps |
| A board already set up: Set up a board offers apps, look, Wi-Fi, Bluetooth or installing again instead of starting over | `web/app.js` (`renderKnown`) | open Set up a board with a board connected |
| The board listens (boards with a mic: ONX + Nextion Dual MIC): a Listen key, notes found on the board (only notes leave it), sent as MIDI from the "KnoEnd" device; on Linux also into Midi Through, so MuseScore takes them unset (Linux tested; macOS/Windows MIDI untested). Spec 3.6f awaits Tom's review | `firmware/main/listen.c`, `pitch.c`, `cli/src/midi.rs`, `recipes/apps/musescore.toml` | open MuseScore, Note input, Listen, sing |
| CI: spec, companion on Linux + macOS + Windows, firmware build | `.github/workflows/ci.yml`, `.github/workflows/release.yml` | every push and PR |

## Browser, desktop apps, coding agents (added 2026-10-18; Linux + Windows tested, macOS partly)

| Piece | Where | Try it |
| --- | --- | --- |
| Extension for Firefox, Chrome and Edge: active site, per-site buttons, page actions, page inventory, favicon | `extension/`, `spec/browser.md` | `python3 tools/build_extension.py`; see `extension/README.md` |
| Native messaging host + service socket (Unix socket on Linux/macOS, authenticated local TCP + Registry on Windows) | `companion/crates/cli/src/browser.rs` | `knoend browser install` |
| Site layouts (`match.web`: host, routes incl. `#/` hash routes, `@fullscreen`); built-in YouTube, Google Slides presenting (Linux + Windows tested) | `recipes/sites/`, `core/src/focus.rs`, `core/src/catalog.rs` | YouTube; present a Slides deck |
| "Configure this site" (board or popup): AI drafts keys from the page's controls, site-wide then per kind of page | `agents/src/site.rs`, `cli/src/service.rs` | Configure on the board in any site |
| Now playing (MPRIS, Linux): media keys reach the player; media row in slots 7-9 while something plays | `companion/crates/media` | play anything |
| Material icons on keys (132 names, built into the firmware) | `spec/icons.txt`, `firmware/components/icons`, `tools/build_icons.py` | any built-in library |
| Claude Code asks through the board: permission requests (Allow / Always / Deny / Stop) with a scrollable explanation; withdrawn when answered on screen | `cli/src/agent_hooks.rs` | `knoend claude-hooks install`, new Claude Code session |
| Desktop notifications from followed apps (Linux D-Bus) | `companion/crates/desktop` | Claude desktop notifications |
| Launcher when nothing is focused: most used / recent apps and sites with their own colour icons; tap opens (Linux, macOS, Windows) | `cli/src/launcher.rs`, `desktop/src/apps.rs`, `mac_apps.rs`, `win_apps.rs`, `focus/src/macos.rs`, `focus/src/windows.rs`, `knoend.image` | empty workspace; click the desktop |
| Edit keys in the web app: the board shows the layout being edited, live; a board tap selects a key; label (forced line break), icon, spoken text, confirm, recorded shortcut; drag to move, grid per page, add/remove keys; Save as your own version on top of the recipe or draft (FNK and ONX tested) | `cli/src/edit.rs`, `cli/src/service.rs` (`show_edit`), `web/app.js` | web app > Edit keys |
| Key labels never split a word: one font size smaller (12 px at least), then cut with "..." (FNK and ONX tested) | `firmware/main/ui.c` (`fitting_font`, `cut_long_words`) | a long one-word label in Edit keys |
| Terminal-aware keys: in a terminal app, keys for the program in the active tab (the one typed in last; `match.terminal`): Claude Code, `less`/`man`; a Claude session's questions follow its own tab (macOS tested; Linux same code, untested) | `focus/src/terminal.rs`, `core/src/catalog.rs`, `recipes/terminal/`, `cli/src/agent_hooks.rs` (`knoend_tty`) | `man ls` or `claude` in Terminal |
| Background service (systemd user unit / LaunchAgent / Windows `HKCU\...\Run`) | `cli/src/background.rs` | `knoend service install` |
| Recipes: apps, sites and Claude Code's screens as data, each with who tested it on which OS | `recipes/`, `spec/protocol.md` section 5 | `python3 tools/validate_spec.py` |

## How the board looks, fonts, and the assistant that listens (added 2026-10-20; ONX + Linux + Windows tested)

| Piece | Where | Try it |
| --- | --- | --- |
| Key size (2x2, 3x3 default, 4x4): every page uses that grid, keys packed in order | `cli/src/display.rs`, `core/src/layout.rs` (`packed`) | dashboard > Look |
| Look (Dark, White, Sepia) and way round (landscape, portrait, upside down), kept on the board and changed live (`knoend.display`, spec 3.6d) | `firmware/main/display.c`, `ui.c`, `board_generic.c` (`board_set_turn`) | dashboard > Look |
| Fonts from Google Fonts for key labels and the start-up "Kn°End", kept in a fonts partition, start-up size (`knoend.font`, spec 3.6e) | `firmware/main/fonts.c`, `cli/src/fonts.rs` | dashboard > Key font / Start-up font |
| AI providers without an API key: Claude Code, Antigravity CLI, Codex CLI found on this computer | `agents/src/cli_tool.rs` | AI settings |
| The service describes itself to the setup assistant: every setting, read-only checks it runs, everything the service prints | `cli/src/settings.rs`, `checks.rs`, `journal.rs` | ask "make the keys bigger" |
| The assistant listens all the time: faults (`problem!`) are put to it without being asked; its answer opens in the web app | `cli/src/watch.rs`, `consult.rs` | unplug the board for 2 minutes |
| Board reports why it last started (`lastStart`), drops a font only after a real crash, and recovers from a lost display update (LVGL memory from PSRAM; the freeze's cause, fixed 2026-10-20) | `firmware/main/protocol.c`, `fonts.c`, `lv_mem_psram.c`, `board_generic.c` | serial log |
| Several Claude questions (#41): a panel listing them while answered in the app, Answer here, cleared at once when answered | `cli/src/service.rs` (`show_answering`, `hosts_agent`), `agent_hooks.rs` (`settles`) | a multi-question prompt in Claude |
| Web app redesign: drawn board with its real screen (FIG. 01), tips, an "ask about this" ° on every section, an assistant panel with a conversation and suggestions, a setup rail with a board acting out each step | `web/` | `knoend serve` |

## Several boards, Bluetooth, the AMOLED (added 2026-10-21; Linux + ONX + Waveshare AMOLED tested)

| What | Where | Try it |
| --- | --- | --- |
| Every paired board connects at once and they mirror each other; each keeps its own look (`display.json` `boards`); the Board section lists each | `cli/src/service.rs` (BoardSender, Scout), `display.rs`, `settings.rs` | switch on a second paired board |
| Bluetooth LE data channel beside the keyboard (spec section 2): a board on neither Wi-Fi nor USB is reached over Bluetooth | `firmware/main/link_ble.c`, `companion/crates/transport/src/ble.rs` | unplug a Bluetooth-paired board |
| Identify reads the firmware a board carries; "a board KnoEnd does not know"; check `identify_board` | `crates/flash/src/on_board.rs`, `cli/src/boards.rs` | Identify on a board with vendor firmware |
| QSPI AMOLED (SH8601/CO5300), panel brightness, rounded corners, OLED dimming and pixel shift, buttons (GPIO, AXP2101 power key) | `components/board_generic/board_generic.c`, `main/ui.c` | Waveshare ESP32-S3-Touch-AMOLED-2.06 |

## Built, not yet tested

- GT911 touch, display start-up commands, active-high reset; the ESP32-S3-BOX-3 and Waveshare
  ESP32-S3-Touch-LCD-2 definitions (status "unverified").
- Bluetooth data on macOS and Windows (built with btleplug; macOS needs the Bluetooth permission).
- Fonts over Bluetooth (up to about 480 KB; expect 10-20 s).
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
| Codex / other agent CLIs' hooks (Claude Code done) | `cli/src/agent_hooks.rs` |
| Now playing on macOS / Windows (no MPRIS) | `companion/crates/media` |
| RGB-panel boards (4.3-7"), Hynitron CST3xx touch, quad-PSRAM image | see `boards/research.md` |

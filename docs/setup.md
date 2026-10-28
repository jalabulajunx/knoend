# Setting up KnoEnd

From nothing to a board whose keys follow the app you are using. Part 1 is for anyone with a board;
part 2 is for people working on the code. When something goes wrong, see
[`knowledge/setup.md`](knowledge/setup.md), which lists every failure seen so far and its fix. The
web app's setup assistant reads the same file.

Tested on Arch Linux (X11, i3), macOS 26.6 (Apple silicon) and Windows 11 with an ONX2432G028. On a
Mac, use v0.1.1 or later: v0.1.0 cannot follow the focused app and fails with newer OpenAI models.

## What you need

- A supported ESP32-S3 touch board (see [Boards](#boards)) and a USB-C **data** cable. Many
  cables only charge.
- Optional: 2.4 GHz Wi-Fi if you want the board to work wirelessly away from the computer (the
  ESP32-S3 cannot join 5 GHz networks), and/or Bluetooth if you want the board to type keyboard
  shortcuts directly as the `"KnoEnd"` Bluetooth Low Energy keyboard. While plugged into the
  computer over USB, the board works over USB serial without requiring Wi-Fi or Bluetooth.
- Optional: an API key for an AI provider (Gemini, OpenAI, Anthropic or OpenRouter), a local model
  through Ollama, or Claude Code / Antigravity CLI / Codex CLI already signed in on this computer.
  The AI is used for the setup assistant and to draft layouts for apps and sites that have none.

## Part 1: set up a board

### 1. Get the `knoend` service

Download it from the [latest release](https://github.com/jalabulajunx/knoend/releases/latest):
`knoend-x86_64-unknown-linux-gnu.tar.gz` (Linux), `knoend-aarch64-apple-darwin.tar.gz` (Apple
silicon Mac), `knoend-x86_64-apple-darwin.tar.gz` (Intel Mac), or
`knoend-x86_64-pc-windows-msvc.zip` (Windows), and unpack it:

```bash
tar xzf knoend-*.tar.gz
./knoend --version
```

On a Mac the binary is not signed yet: the first time, right-click > Open, or run
`xattr -d com.apple.quarantine knoend`.

Or build it from source (stable Rust from [rustup](https://rustup.rs); on a Mac also the Xcode
Command Line Tools, `xcode-select --install`; on Linux also `pkg-config` and the libudev headers:
`libudev-dev` on Debian and Ubuntu, already in `systemd-libs` on Arch):

```bash
git clone https://github.com/jalabulajunx/knoend.git
cd knoend/companion
cargo build --release
```

**Mac USB driver.** Boards with a CH340K serial chip (the ONX2432G028, USB ID `1a86:7522`) need
WCH's driver on macOS; without it the wizard says "No board found". The setup assistant offers to
install it, or get `CH34xVCPDriver.pkg` from
[WCHSoftGroup/ch34xser_macos](https://github.com/WCHSoftGroup/ch34xser_macos). After installing,
switch it on in System Settings > General > Login Items & Extensions > Driver Extensions, then
unplug and replug the board.

**Linux serial access.** Your user must be in the group that owns the serial port: `uucp` on Arch,
`dialout` on Debian and Ubuntu. After adding yourself, log out and back in.

```bash
sudo usermod -aG uucp $USER
```

**Windows USB driver.** Windows Update usually installs the WCH CH340 (`CH341SER`) or Silicon Labs
CP210x serial driver automatically; if the board shows under "Other devices" in Device Manager with
no `COM` port, check Windows Update > Advanced options > Optional updates > Driver updates (or
install `CH341SER.EXE` from WCH). Native-USB boards (`303a:1001`) use Windows's built-in `usbser`
driver with no extra install.

### 2. Firmware (nothing to do)

The service downloads the firmware from the latest release, checks every part against the
manifest's SHA-256, and installs it. To install a firmware you built yourself instead (needs ESP-IDF
v5.4.4; see [Part 2](#esp-idf)), package it and pass the folder with `--firmware`:

```bash
. ~/esp/esp-idf/export.sh
cd firmware && idf.py -B build/universal build && cd ..
python3 tools/release_firmware.py --version 0.3.0-dev
```

This writes `dist/firmware/`, which holds `manifest.json`, the universal image (bootloader,
partition table, app) and one small definition file per board. Every board runs the same image;
only the definition differs.

### 3. Start the service and follow the web app

```bash
./knoend serve
```

(Add `--firmware ../dist/firmware` for a firmware you built yourself.)

The browser opens at `http://127.0.0.1:7342/`. (Use `--no-open` to stop it opening.) The wizard
then walks you through the setup:

1. **Choose how to set up.** *Guided by AI* is recommended if you'd rather not troubleshoot.
   Pick a provider, a model from the list, and enter your API key in the page.
   - Keys are stored in `~/.config/knoend/secrets.json`, readable only by you.
   - An environment variable such as `GEMINI_API_KEY` overrides the stored key.
   - *Manual* skips the AI.
2. **Find the board.** Plug it in. The service lists the USB serial ports and identifies the chip.
   - Identifying can restart the board; that is normal.
3. **Choose firmware.** The service suggests the most likely board. A board marked **Unverified**
   has a definition written from the maker's documentation that nobody has tested yet. If the board
   already runs KnoEnd, you can click **Skip install** to pair over USB and go straight to
   **Keys and look**.
4. **Install.** Every part is checked against the manifest's checksums before it is written, and the
   board is paired with this computer over USB right after installing.
   - Only erase if the board ran other firmware or misbehaves. Erasing removes its Wi-Fi settings
     and pairing.
5. **Keys and look.** Choose your key size (`2x2`, `3x3` or `4x4`), look (`Dark`, `White` or
   `Sepia`), orientation (`Landscape` or `Portrait`) and optional Google Fonts immediately over USB.
   You can finish here if you use the board over USB, or continue to the optional wireless steps.
6. **Wi-Fi (optional).** The board scans for 2.4 GHz networks; pick yours and type the password (or
   click **Skip Wi-Fi** to keep using USB).
   - The password goes only to the board over USB.
   - Once on Wi-Fi, you can unplug the board from the computer and power it from any USB charger on
     the same network; the service finds it by mDNS.
7. **Bluetooth (optional).** Pair the board as the **KnoEnd** Bluetooth Low Energy keyboard in your
   computer's Bluetooth settings (the **Open Bluetooth settings** button opens the right page on
   Windows, macOS and Linux). Even without Bluetooth, the companion types shortcuts for the board
   whenever it is connected over USB or Wi-Fi.
8. **Dashboard.** Shows the connection (`Wi-Fi` or `USB`), the focused app, and recent activity.

### Keep it running

`./knoend serve` stops when its terminal closes. To run it in the background from now on,
started at login and restarted if it stops:

```bash
./knoend service install
```

On Linux this is a systemd user service (`journalctl --user -u knoend -f` shows its log); on a
Mac, a LaunchAgent (log in `~/Library/Logs/knoend.log`; allow `knoend` itself under
Accessibility so it can type keys); on Windows, a startup entry in
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` running `knoend serve --no-open` in the
background with no console window. `./knoend service status` and `./knoend service uninstall`
check and remove it. Stop any `knoend serve` you started by hand first: two copies fight over the
board.

### 4. Use it

- **Switch apps.** The board's layout follows the focused window. VS Code has a built-in layout.
  Clicking the desktop or taskbar shows the **launcher** with your most-used apps and sites; tapping
  one brings its window to the front or opens it.
- **Unsupported apps.** An app with no layout shows *Unsupported app: <name>* with a **Configure**
  key. Tapping it asks the AI to draft a layout.
  - Drafts are saved in `~/.config/knoend/libraries/ai/` and marked unreviewed.
  - To keep your own edits, change `source` to `"user"` in the file (or edit and save in the web
    app's **Edit keys** panel).
- **How keys reach the computer:**
  - If the board is paired as a Bluetooth keyboard ("KnoEnd"), it types by itself, with nothing
    else running.
  - Otherwise it sends each key to the service, which types it for you.
- **On a Mac,** the service can only type for you once you allow it: System Settings > Privacy &
  Security > Accessibility, then switch on the app you started `knoend` from (Terminal, iTerm,
  or Visual Studio Code if you started it in VS Code's terminal), and restart `knoend`. The
  service log names the app; the setup assistant has a button that opens the page.

### Websites (browser extension)

The board can follow the website in your browser and press things on the page. Run
`./knoend browser install` once (and `python3 tools/build_extension.py` if building from source to
produce `dist/extension/chrome` and `dist/extension/firefox`), add the extension in Chrome, Edge or
Firefox (see `extension/README.md`), and turn board buttons on per site from the extension's popup.
YouTube, Google Drive and Google Slides have built-in layouts.

### Codex interruptions (local desktop and CLI)

This is a branch-preview feature, not yet included in a published release. Use a companion build
that includes the Codex integration. The board needs its existing data connection (USB, Wi-Fi or
supported Bluetooth data); hooks do not require reflashing. Bluetooth keyboard pairing alone is
not a companion data connection.

1. Start KnoEnd and check the dashboard reports the board connected. If using a custom runtime,
   start it with `knoend local --runtime <absolute-local-runtime> --recipes <absolute-recipe-folder>`.
   Keep runtime/config/logs outside a synced source tree. Windows hook executable and endpoint
   paths currently must contain no spaces or shell metacharacters.
2. Open **Dashboard > Codex interruptions**, leave **Enable Codex interruptions** on, and click
   **Prepare hooks**. Turn on **Interrupt for notifications** for completion alerts. Preparation
   adds six native handlers to the active user-level Codex `hooks.json`, preserves other hooks,
   makes a first backup, and leaves `config.toml` and its existing `notify` callback unchanged.
   The generated command includes the companion's exact private endpoint file, so a custom
   runtime does not need to match Codex's environment. Advanced CLI equivalent:
   `knoend codex-hooks install`, run in the companion's runtime environment.
3. In Codex, review/trust the exact KnoEnd hook definitions. This setup used the Codex app's
   hook controls; the CLI also exposes `/hooks`. Start a fresh local session afterwards. A
   prepared file is not proof of trust, and neither installation nor a synthetic screen proves
   a real Codex callback. Keep Codex's approval policy and managed restrictions as configured.
4. Test the **CLI** with a small prompt: `Reply only OK, without using tools or changing files.`
   A Codex app may bundle a CLI that is absent from the normal terminal's PATH. If `codex` is
   not recognized, use the installed executable directly or install a supported standalone CLI;
   changing folders does not fix a missing PATH entry. No global PATH change was made in this setup.
5. Check **Codex finished** appears on the board and tap **Dismiss** to restore normal keys.
   Then repeat the same prompt in a **new local Codex desktop chat**. Verify desktop delivery
   independently from the CLI. Both actual completion paths were physically confirmed here.

The recipes are `recipes/agents/codex-permission.toml` and
`recipes/agents/codex-completion.toml`. Use **Reload recipes** after editing them. A `Stop`
callback means the turn ended, not that the task succeeded. Completion alerts last five minutes
or until dismissed; a new prompt or interrupt clears that session's alerts. A normal
`SessionEnd` withdraws pending approvals but must not erase a completed-turn alert.

The **codex_agent** assistant check shows preparation and content-free callback counts. Actual
approval requests happen only when Codex's policy would ask: full-access sessions may produce
none. Supported board replies are **Allow once** and **Deny**; long/unreadable requests stay for
review in Codex, and ordinary question-card replies still require Codex's own UI. The synthetic
Deny check verified bridge response only, not a real policy-driven approval request. Cloud
orchestration is outside this local command-hook integration. No transcript scraping is used.

For uninstall, use **Remove hooks** or `knoend codex-hooks uninstall`; only KnoEnd's handlers are
removed. See [the detailed guide](codex-interruptions.md) for diagnostics and the tested scope.

### Where things are kept

Everything is in `~/.config/knoend/` (or `$XDG_CONFIG_HOME/knoend/`):

| File | What |
| --- | --- |
| `companion.json` | pairing key for each board (mode 600) |
| `secrets.json` | API keys entered in the web app (mode 600) |
| `agent.toml` | AI provider and model |
| `codex-agent.json` | local Codex interruption enablement |
| `libraries/` | your layouts; `libraries/ai/` holds AI drafts |

Codex hook definitions live separately in the active Codex user home (`$CODEX_HOME/hooks.json`,
or `~/.codex/hooks.json` by default), with a first `hooks.json.before-knoend-codex` backup.
These machine-specific files and private endpoint paths do not belong in the repository.

## Boards

| Board | Status | Notes |
| --- | --- | --- |
| ONX2432G028 | **supported** (tested) | USB-C goes to a CH340 serial chip, so it cannot act as a USB keyboard. Keys type over Bluetooth or through the service. |
| Espressif ESP32-S3-BOX-3 | unverified | GT911 touch. Early units with a TT21100 touch controller are not supported. |
| Waveshare ESP32-S3-Touch-LCD-2 | unverified | |
| Freenove FNK0104B | coming | Definition not written yet. |

Other ESP32-S3 boards with octal PSRAM, an ST7789 or ILI9341 SPI display, and CST816, CST826,
FT6336 or GT911 touch need only a JSON definition (`firmware/boards/definitions/`, schema
`spec/schemas/board.schema.json`). See [`boards/research.md`](boards/research.md) for popular
boards and what each would need.

## Part 2: working on the code

Read [`../AGENTS.md`](../AGENTS.md) first. It covers the shared log, branches, licences, and the
one-change-per-flash rule.

<a id="esp-idf"></a>
### Firmware (C, ESP-IDF v5.4.4)

Install ESP-IDF v5.4.4 by following
[Espressif's guide](https://docs.espressif.com/projects/esp-idf/en/v5.4.4/esp32s3/get-started/linux-macos-setup.html).
Clone it to `~/esp/esp-idf`, then run `./install.sh esp32s3`. Then:

```bash
. ~/esp/esp-idf/export.sh
cd firmware
idf.py -B build/universal build
idf.py -B build/universal -p /dev/ttyUSB0 flash monitor
```

`idf.py flash` writes the image but **not** a board definition. A board that has never had a
definition boots without a display and prints `BOARD no definition`. Either install once through
the web app, or write the padded definition from `dist/firmware/` to the `board` partition at
`0x310000`.

Opening the ONX's serial port restarts it. The serial log prints a line at each stage: `READY`,
`BOARD`, `WIFI ...`, `KEY n ...`.

### Companion (Rust)

```bash
cd companion
cargo build
cargo test
```

| Crate | What |
| --- | --- |
| `core` | protocol, libraries, validation, matching |
| `transport` | serial, Wi-Fi (mDNS + authenticated WebSocket), pairing |
| `focus` | focused app (X11 on Linux, the window server's front window on macOS, Win32 `GetForegroundWindow` on Windows) |
| `input` | typing keys (XTest on Linux, Core Graphics on macOS, Win32 `SendInput` on Windows) |
| `flash` | identify and flash boards (espflash), firmware manifest, USB bus scan |
| `agents` | AI providers (API + local CLI tools), setup assistant, layout drafting, key storage |
| `cli` | the `knoend` binary and the web API |

The web app (`web/`) is plain HTML, CSS and JS, built into the binary. Rebuild the binary after
editing it.

Other commands:

```bash
knoend devices --firmware ../dist/firmware
knoend flash --board onx2432g028 --port /dev/ttyUSB0 --firmware ../dist/firmware
knoend pair --port /dev/ttyUSB0
knoend run
knoend draft --class Code --instance code
```

- `devices`: list the ports, identify the chip, and suggest a board.
- `flash`: install from the command line.
- `pair`: pair a board and give it Wi-Fi, from the terminal.
- `run`: the service without the web app.
- `draft`: ask the AI for a layout; no board needed.

### Spec and checks

```bash
python3 tools/validate_spec.py
```

This checks `spec/examples/` and the board definitions against `spec/schemas/`. It needs Python
3.11+ and `jsonschema`. On Arch, install `python-jsonschema`, or use a virtualenv. On macOS
(Homebrew): `brew install python@3.13`, `python3.13 -m venv ~/.venvs/knoend`,
`~/.venvs/knoend/bin/pip install jsonschema`, then run the script with
`~/.venvs/knoend/bin/python`. Changes under
`spec/` need review from both maintainers.

CI (`.github/workflows/ci.yml`) runs the spec checks, the companion on Linux, macOS and Windows,
and the firmware build on every push and pull request.

### Releases

Pushing a `v*` tag runs `.github/workflows/release.yml`. It builds the firmware and the service for:

- Linux x86_64;
- macOS arm64;
- macOS x86_64;
- Windows x86_64.

It then publishes them as a GitHub release, which the service downloads by default. Confirm with
the maintainers before tagging.

### Known gaps

- Linux focus tracking needs X11. On Wayland, only XWayland apps are seen.
- macOS is tested on 26.6 with Apple silicon only; Intel Macs are untested.
- On Windows, User Interface Privilege Isolation (UIPI) blocks an unelevated `knoend` process from
  sending synthetic keystrokes (`SendInput`) into windows running as Administrator.

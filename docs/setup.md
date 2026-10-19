# Setting up KnoEnd

From nothing to a board whose keys follow the app you are using. Part 1 is for anyone with a board;
part 2 is for people working on the code. When something goes wrong, see
[`knowledge/setup.md`](knowledge/setup.md), which lists every failure seen so far and its fix. The
web app's setup assistant reads the same file.

Tested on Arch Linux (X11, i3) and on macOS 26.6 (Apple silicon) with an ONX2432G028. On a Mac,
use v0.1.1 or later: v0.1.0 cannot follow the focused app and fails with newer OpenAI models.
Windows is not supported yet.

## What you need

- A supported ESP32-S3 touch board (see [Boards](#boards)) and a USB-C **data** cable. Many
  cables only charge.
- 2.4 GHz Wi-Fi if you want the board to work away from the computer. The ESP32-S3 cannot join
  5 GHz networks.
- Optional: an API key for an AI provider (Gemini, OpenAI, Anthropic or OpenRouter), or a local
  model through Ollama. The AI is used for the setup assistant and to draft layouts for apps that
  have none.

## Part 1: set up a board

### 1. Get the `knoend` service

Download it from the [latest release](https://github.com/jalabulajunx/knoend/releases/latest):
`knoend-x86_64-unknown-linux-gnu.tar.gz` (Linux), `knoend-aarch64-apple-darwin.tar.gz` (Apple
silicon Mac) or `knoend-x86_64-apple-darwin.tar.gz` (Intel Mac), and unpack it:

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
   has a definition written from the maker's documentation that nobody has tested yet.
4. **Install.** Every part is checked against the manifest's checksums before it is written.
   - Only erase if the board ran other firmware or misbehaves. Erasing removes its Wi-Fi settings
     and pairing.
5. **Pair and Wi-Fi.** The board scans for networks; pick yours and type the password.
   - The password goes only to the board over USB.
   - The board and the computer exchange a pairing key. After that, every Wi-Fi message is
     authenticated.
6. **Dashboard.** Shows the connection, the focused app, and recent activity.

You can now unplug the board from the computer and power it from any USB charger on the same
network. The service finds it by mDNS.

### Keep it running

`./knoend serve` stops when its terminal closes. To run it in the background from now on,
started at login and restarted if it stops:

```bash
./knoend service install
```

On Linux this is a systemd user service (`journalctl --user -u knoend -f` shows its log); on a
Mac, a LaunchAgent (log in `~/Library/Logs/knoend.log`; allow `knoend` itself under
Accessibility so it can type keys). `./knoend service status` and `./knoend service uninstall`
check and remove it. Stop any `knoend serve` you started by hand first: two copies fight over the
board.

### 4. Use it

- **Switch apps.** The board's layout follows the focused window. VS Code has a built-in layout.
- **Unsupported apps.** An app with no layout shows *Unsupported app: <name>* with a **Configure**
  key. Tapping it asks the AI to draft a layout.
  - Drafts are saved in `~/.config/knoend/libraries/ai/` and marked unreviewed.
  - To keep your own edits, change `source` to `"user"` in the file.
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
`./knoend browser install` once, add the extension (see `extension/README.md`), and turn board
buttons on per site from the extension's popup. YouTube has a built-in layout.

### Where things are kept

Everything is in `~/.config/knoend/` (or `$XDG_CONFIG_HOME/knoend/`):

| File | What |
| --- | --- |
| `companion.json` | pairing key for each board (mode 600) |
| `secrets.json` | API keys entered in the web app (mode 600) |
| `agent.toml` | AI provider and model |
| `libraries/` | your layouts; `libraries/ai/` holds AI drafts |

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
| `focus` | focused app (X11 on Linux, the window server's front window on macOS) |
| `input` | typing keys (XTest on Linux, Core Graphics on macOS) |
| `flash` | identify and flash boards (espflash), firmware manifest |
| `agents` | AI providers, setup assistant, layout drafting, key storage |
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

CI (`.github/workflows/ci.yml`) runs the spec checks, the companion on Linux and macOS, and the
firmware build on every push and pull request.

### Releases

Pushing a `v*` tag runs `.github/workflows/release.yml`. It builds the firmware and the service for:

- Linux x86_64;
- macOS arm64;
- macOS x86_64.

It then publishes them as a GitHub release, which the service downloads by default. Confirm with
the maintainers before tagging.

### Known gaps

- Linux focus tracking needs X11. On Wayland, only XWayland apps are seen.
- macOS is tested on 26.6 with Apple silicon only; Intel Macs are untested.
- Windows has no focus or typing backend yet.
- The board UI always shows nine tiles. Dynamic tiles with paging are planned.

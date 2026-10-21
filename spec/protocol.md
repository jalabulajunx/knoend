# KnoEnd protocol, v0 (draft)

Status: **draft for review** (both owners review every change under `spec/`). `protocolVersion` is
`0` until the first tagged release; until then breaking changes are allowed but must be logged.

This document covers what travels between a **board** and a **companion** (the local `knoend`
service, or the board simulator). The service's own HTTP API for the web app is out of scope here.

Machine-checkable versions of everything below are in [`schemas/`](schemas/); every file in
[`examples/`](examples/) must validate (`python3 tools/validate_spec.py`).

## 1. Packets

Every message is one JSON object, UTF-8, the same shape on every transport (borrowed from KDE
Connect's packet format, concepts only):

```json
{ "id": 42, "type": "knoend.key", "body": { "key": 5, "layout": "builtin.vscode" } }
```

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | integer ≥ 0 | Sender's sequence number, +1 per packet, restarting at 0 on reboot or reconnect |
| `type` | string | `knoend.<name>`, one of the types in section 3 |
| `re` | integer, optional | `id` of the packet this one answers |
| `body` | object | Type-specific content; unknown fields must be ignored |

Rules:

- A receiver **ignores packet types it does not know** and fields it does not know, so either side
  can add features without breaking the other.
- Packets are at most **4096 bytes** of JSON. A layout that does not fit is an error, not split.
- Text shown on the board (`label`, `text`, `summary`) is plain UTF-8, no markup.

**Names from before the rename.** The project was called OpenPager until 2026-10-19, and boards and
companions from then (v0.1.x) name every type `openpager.<name>` (`openpager.layout`) and announce
`_openpager._tcp`. Both sides accept either prefix: a receiver treats `openpager.<name>` as
`knoend.<name>`, and answers each peer in the prefix that peer last used (a board) or that its
firmware understands (a companion: a board seen only as `_openpager._tcp`, or that answered only
`openpager.identity` over USB, gets `openpager.*`). Boards announce both `_knoend._tcp` and
`_openpager._tcp`. What a board stores (its NVS namespace `openpager`) keeps its old name, so a
firmware update keeps pairings and Wi-Fi. To be dropped once no v0.1.x boards or companions remain.

## 2. Transports and framing

| Transport | Framing | Notes |
| --- | --- | --- |
| USB-C serial | One packet per line (`\n`-terminated JSON), 115200 baud on the ONX2432G028 | Lines not starting with `{` are console/log output and are ignored |
| Wi-Fi | WebSocket `ws://<board>/ws` (found as `knoend-<id>.local`), one packet per text frame | Authenticated frames only, see section 4 |
| Bluetooth LE data | GATT service below; packets as `\n`-terminated JSON, split across writes/notifications | Runs beside the HID keyboard service on the same connection |

Bluetooth LE data service (proposed UUIDs, random base `f0e35c51-f991-4cdb-b330-97d0615e____`):

| Item | UUID | Properties |
| --- | --- | --- |
| Service | `f0e35c51-f991-4cdb-b330-97d0615e0001` | |
| To board (companion writes) | `f0e35c51-f991-4cdb-b330-97d0615e0002` | Write, Write Without Response; encrypted link required |
| From board (companion subscribes) | `f0e35c51-f991-4cdb-b330-97d0615e0003` | Notify; encrypted link required |

Whether macOS and Windows let a companion reach a custom service on a paired keyboard is untested
(plan gate, Oct 18). Where they do not, data uses Wi-Fi or serial.

## 3. Packet types

Direction: **B→C** board to companion, **C→B** companion to board.

| Type | Dir | Purpose |
| --- | --- | --- |
| `knoend.identity` | both | First packet on every connection: who I am and what I can do |
| `knoend.layout` | C→B | Replace the keys on the board |
| `knoend.layout.ack` | B→C | Layout accepted or rejected |
| `knoend.key` | B→C | A key was tapped and its action needs the companion |
| `knoend.page` | B→C | The person asked for the next or previous page of keys |
| `knoend.status` | C→B | Show a message on the status bar |
| `knoend.pair` | C→B | Serial only: hand the board a Wi-Fi key (section 4.1) |
| `knoend.pair.ack` | B→C | Key stored |
| `knoend.wifi` | C→B | Serial only: the home Wi-Fi name and password (section 4.1) |
| `knoend.wifi.ack` | B→C | Credentials stored; the board joins the network |
| `knoend.wifi.scan` | C→B | Serial only: list the Wi-Fi networks the board can hear (empty body) |
| `knoend.wifi.networks` | B→C | Up to 20 networks, strongest first: `ssid`, `rssi` (dBm), `channel`, `secured`. The ESP32-S3 hears 2.4 GHz only |
| `knoend.display` | C→B | How the board looks: its theme, which way it sits and the start-up name's size (section 3.6d) |
| `knoend.display.ack` | B→C | Stored and shown; the screen's size and largest grid as it now sits |
| `knoend.font` | C→B | A font for the key labels or the start-up name, in slices (section 3.6e) |
| `knoend.font.ack` | B→C | Font kept and in use, or why not |
| `knoend.ping` | both | Liveness check; answer with `knoend.ping` and `re` set |
| `knoend.error` | both | A packet could not be handled |

### 3.1 `knoend.identity`

Sent by both sides immediately after connecting; nothing else is sent before the peer's identity
arrives.

```json
{ "id": 0, "type": "knoend.identity", "body": {
    "deviceId": "a0b1c2d3e4f5", "deviceName": "KnoEnd", "deviceType": "board",
    "protocolVersion": 0, "firmware": "a1ea8a8", "lastStart": "power-on",
    "board": { "profile": "onx2432g028", "width": 320, "height": 240,
               "grid": { "cols": 4, "rows": 4 }, "statusBar": true,
               "display": { "theme": "dark", "turn": 0, "startSize": 64 },
               "fonts": { "capacity": 483328, "keys": null,
                          "start": { "name": "Edu AU VIC WA NT Dots", "crc": 2271390218 } } },
    "features": { "speaker": true, "battery": false, "nativeUsb": false,
                  "transports": ["ble-hid", "ble-data", "serial", "wifi"] },
    "incoming": ["knoend.layout", "knoend.status", "knoend.ping"],
    "outgoing": ["knoend.key", "knoend.page", "knoend.layout.ack", "knoend.ping"] } }
```

- `board.grid`: the largest grid the board draws legibly (cols and rows 1-4), from its screen size.
  The companion splits a page with a bigger grid onto pages of this size.
- `board.width`, `board.height` and `board.grid` are as the board sits now: turned to portrait, the
  ONX is 240x320 and its largest grid 3x4.
- `lastStart` (boards): why the board last started: `power-on`, `restart`, `crash`, `watchdog`,
  `brownout` (the power dipped), `wake` or `other`. The companion raises `crash`, `watchdog` and
  `brownout` as problems.
- `board.fonts`: the fonts the person chose that the board holds (section 3.6e): `capacity`, the
  largest font a slot takes in bytes, and per slot (`keys`, `start`) the font's `name` and CRC-32,
  or null for the built-in font; `refused`, when present, names the fonts the board dropped after a
  start it did not get through with them, comma-separated. Absent on firmware without fonts.
- `board.display`: the theme, turn and start-up size the board uses now (section 3.6d). Absent on firmware older
  than this message, which is always `dark`, turn 0.
- `deviceId`: stable per device (boards: base MAC, lowercase hex, no separators).
- `deviceType`: `board`, `companion` or `sim` (the simulator identifies as `sim` but otherwise
  behaves exactly like a board).
- `board` is present only when `deviceType` is `board` or `sim`.
- A companion that receives a higher `protocolVersion` than it supports shows "update needed" and
  sends nothing else.

### 3.2 `knoend.layout`

```json
{ "id": 7, "type": "knoend.layout", "body": {
    "layout": "builtin.vscode", "name": "VS Code", "source": "builtin", "reviewed": true,
    "grid": { "cols": 3, "rows": 3 }, "page": 1, "pages": 2,
    "keys": [
      { "key": 1, "label": "Palette", "speak": "Command palette",
        "send": { "hid": { "modifiers": 3, "usage": 19 } } },
      { "key": 2, "label": "Terminal", "send": { "event": true } },
      { "key": 9, "label": "Delete line", "confirm": true,
        "send": { "hid": { "modifiers": 3, "usage": 14 } } } ] } }
```

- `grid`: columns and rows of this layout (1-4 each, within the board's `grid`); 3x3 when absent.
- `key`: slot 1 to cols×rows (at most 16), numbered left to right, top to bottom. Slots not listed
  are blank.
- `label` (1-24 characters): `\n` starts a new line on the key. The board wraps it between words
  and never splits a word over two lines: a word too wide for the key gets the next smaller font
  (never below 12 px), and a word still too wide is cut and ends in "...". When the lines do not
  fit, the last one ends in "...".
- `page` / `pages`: this is page `page` of `pages` (both 1 when absent). With more than one page
  the board shows a page indicator ("2/3 >") at the right end of the status bar. The companion holds the pages and
  sends one at a time; see `knoend.page`.
- `send` says what a tap does, decided by the companion when it builds the packet:
  - `hid`: the board types this itself over the keyboard link (USB HID usage + modifier bits:
    1 Ctrl, 2 Shift, 4 Alt, 8 GUI/Cmd; right-hand modifiers are not used). Works with no companion.
    The companion has already resolved `Primary` to Ctrl or Cmd for the connected computer's OS.
  - `event`: the board sends `knoend.key` and the companion carries out the action (open a
    page, run a named command, press a page element via the extension).
- `confirm: true`: risky action (buy, submit, send, delete). The first tap shows "Tap again to
  confirm"; only a second tap on the same key within 3 s acts.
- `reviewed: false` (AI-drafted, unreviewed): the board marks the layout as unreviewed on screen.
- The board stores the last layout and keeps using it while the companion is away.

### 3.3 `knoend.layout.ack`

`{ "re": 7, "body": { "layout": "builtin.vscode", "ok": true } }`; on failure `ok: false` and
`error` (short reason, e.g. `"too many keys"`).

### 3.4 `knoend.key`

```json
{ "id": 18, "type": "knoend.key", "body": { "key": 2, "layout": "builtin.vscode", "page": 1, "seq": 31 } }
```

`page` is the page the key was on (1 when the layout has one page). The companion ignores a tap
whose page is not the one it last sent, so a tap that crosses a page change does nothing.

Sent for keys whose `send` is `event`, and also for `hid` keys tapped while no Bluetooth
keyboard host is connected: then the body carries `"hid": { "modifiers": 3, "usage": 19 }` and the
companion types that keystroke into the focused app itself (a board whose USB cannot be a keyboard,
like the ONX2432G028, still works over Wi-Fi or USB serial). `seq` counts every tap since boot, so the companion can
detect lost packets. One packet per tap (sent on release), however long the key is held.

### 3.4a `knoend.page`

```json
{ "id": 19, "type": "knoend.page", "body": { "layout": "builtin.vscode", "to": "next" } }
```

Sent when the person swipes left (`next`) or right (`prev`) across the keys, or taps the page indicator
(`next`). Only while the layout has more than one page. The companion sends the new page as an
`knoend.layout`; pages wrap around. A swipe never also taps the key it started on.

### 3.5 `knoend.status`

```json
{ "id": 9, "type": "knoend.status", "body": { "text": "Build passed", "level": "info", "ttl": 30 } }
```

`level`: `info`, `attention` (highlighted) or `alert` (highlighted + beep). `ttl` seconds; omitted
means until replaced. An empty `text` clears the bar.

### 3.6 (removed: `knoend.approve.request` / `knoend.approve.response`)

Removed on 2026-10-19; no board or companion ever used them. Since 2026-10-18 a coding agent's
permission request is an ordinary layout with `detail` (section 3.6a).

### 3.6a Layout extras: `icon`, `detail`

- A key's optional `icon` is a name from [`icons.txt`](icons.txt) (Material Icons, drawn white above a
  small label) or the name of a picture sent with `knoend.image` (`app:...`, `site:...`, drawn in
  colour). A board that does not know the name shows the label alone.
- A layout's optional `detail` (up to 240 characters, `\n` for new lines) is shown in place of the
  middle rows (every row but the top and bottom; their keys are hidden); the top and bottom rows
  get shorter and the text scrolls. Needs a grid of 3 rows or more; with fewer the board ignores
  it. Used for what a permission request is about.

### 3.6b `knoend.image` / `knoend.image.ack`

```json
{ "id": 30, "type": "knoend.image", "body": { "name": "app:firefox", "w": 32, "h": 32,
    "offset": 0, "total": 3072, "data": "<base64 of this slice>" } }
```

A picture for keys: `w`x`h` pixels (at most 64x64) of RGB565 with its two bytes swapped (as LVGL
draws with `LV_COLOR_16_SWAP`), then one byte of alpha: `total` = w*h*3 bytes. It is sent in slices
that each fit a packet (the companion uses 2048 bytes), in order, starting at `offset` 0. After the
last slice the board answers `knoend.image.ack` `{ "name": ..., "ok": true }` (or `ok: false` and
`error`). The board keeps about 48 pictures (least recently used replaced) until it restarts; the
companion sends a picture again on each new connection, before the layout that uses it.

### 3.6c What the companion shows when

- The focused app's or site's layout (section 5 and `browser.md`), with a media row while it plays.
- A coding agent's permission request (`system.approve`: Allow, Always, Deny, Stop, with `detail`)
  until it is answered on the board or in the agent's own window.
- Nothing focused (empty desktop): the launcher (`system.launcher`): up to 9 apps and sites the
  person used most and most recently, with their own icons; a tap opens one. Usage (seconds in front
  per app and site) is kept on the computer only.

### 3.6d `knoend.display` / `knoend.display.ack`

How the board looks, chosen by the person at setup and changeable from the web app. The board
keeps both values with its pairing and Wi-Fi settings, so it starts that way before any companion
connects.

```json
{ "id": 12, "type": "knoend.display", "body": { "theme": "sepia", "turn": 1, "startSize": 64 } }
{ "id": 4, "re": 12, "type": "knoend.display.ack", "body": { "theme": "sepia", "turn": 1,
    "startSize": 64, "width": 240, "height": 320, "grid": { "cols": 3, "rows": 4 } } }
```

- `theme`: `dark` (the usual: light text on dark keys, colour for pressed, confirm and the status
  levels), `white` or `sepia` (e-ink looks: dark ink on white, or on warm paper; high contrast,
  keys outlined; pressed keys are a lighter shade of the ink, a key waiting for confirmation and
  the `attention` and `alert` bars are filled with the ink, Material icons are in the ink colour).
  Pictures the companion sends (3.6b) are drawn as sent; for `white` the companion sends them in
  grey, for `sepia` in sepia tones.
- `turn`: quarter turns clockwise from the board's usual landscape (its definition's
  `display.rotation`): 0 landscape, 1 portrait, 2 landscape upside down, 3 portrait upside down.
  Touch follows the turn.
- `startSize`: pixels (24-96, 64 by default) of the name the board shows for a moment at start-up
  (section 3.6e). A change shows the name at the new size at once for 2 s, as a preview; so does a
  new `start` font.
- Any field may be left out to keep its current value, but not all three. A value out of range, or
  no field at all, is answered with `knoend.error` `bad-packet`; a board that cannot store the values
  answers `busy` and keeps its current look.
- The board stores the values and shows them at once, without restarting, so the person sees each
  choice as they make it. Its answer gives the values now in use (`theme`, `turn`, `startSize`) and
  the screen's `width`, `height` and largest `grid` as it now sits (they change with a turn); the companion uses them as it would the identity's, and sends
  the layout again in a grid that fits.
- The companion sends `knoend.display` after the identities when the person's choice differs from
  the identity's `board.display`, and when the person changes it.

### 3.6e `knoend.font` / `knoend.font.ack`

A font the person chose (from Google Fonts, in the web app), for one of two slots: `keys` (the key
labels) or `start` (the name the board shows for a moment at start-up). The board keeps it in flash
and uses it at once and after every restart, drawing it at whatever size it needs; a character the
font lacks is drawn in the built-in font.

```json
{ "id": 30, "type": "knoend.font", "body": { "slot": "start", "name": "Edu AU VIC WA NT Dots",
    "total": 84956, "offset": 0, "crc": 2271390218, "data": "<base64, at most 2048 bytes>" } }
{ "id": 9, "re": 71, "type": "knoend.font.ack", "body": { "slot": "start", "name": "Edu AU VIC WA NT Dots", "ok": true } }
{ "id": 31, "type": "knoend.font", "body": { "slot": "keys", "clear": true } }
```

- The font is TrueType (`glyf` outlines), at most `board.fonts.capacity` bytes. The companion asks
  Google Fonts for it trimmed to the characters the board shows (printable ASCII and a few marks).
- Slices go in order (`offset` 0 first); every slice repeats `slot`, `name`, `total` and `crc`, the
  CRC-32 (IEEE) of the whole font. The board answers once: after the last slice (`ok` true when
  the CRC matched and the font is in use), or at the first slice it cannot take (`ok` false and
  `error`). Until then the slot holds no font.
- `"clear": true` empties the slot: the built-in font again. The answer has no `name`.
- A board that crashes while starting with its stored fonts drops them at the next start and
  names them in `board.fonts.refused`; the companion does not send a refused font again by itself.
  The board hides a font's kerning tables (`GPOS`, `kern`) from its renderer: a font whose kerning
  points past its end crashed it.
- The companion sends a font after the identities when the person's choice differs from
  `board.fonts`, and when the person changes it.

### 3.7 `knoend.error`

`{ "re": 7, "body": { "code": "unsupported", "message": "..." } }`. Codes: `bad-packet`,
`unsupported`, `too-large`, `unauthorised`, `busy`.

## 4. Pairing and authentication (Wi-Fi)

Bluetooth uses the link's own pairing and encryption (currently "Just Works"; see the running log).
Serial is trusted: a cable means physical access. Wi-Fi needs its own trust, because anyone on the
network can reach the board. The trust is a 32-byte key `K` shared by one board and one companion,
handed over by cable.

### 4.1 Pair over USB (primary)

While the board is connected by USB, including right after the companion flashes it:

1. The companion generates `K` from the operating system's secure random source and stores it in
   the OS keyring under the board's `deviceId`.
2. It sends, **over serial only**:
   `{ "id": 3, "type": "knoend.pair", "body": { "companion": "laptop-3f9c2a", "companionName": "Workstation", "key": "<64 hex digits>" } }`
3. The board stores `K` for that companion (up to 4 companions; the oldest is replaced) and answers
   `knoend.pair.ack` with `{ "companion": "laptop-3f9c2a", "ok": true }`.

4. The companion can send the home Wi-Fi credentials the same way, also **serial only**:
   `{ "type": "knoend.wifi", "body": { "ssid": "Home", "password": "..." } }`, answered by
   `knoend.wifi.ack` `{ "ok": true }`. The board stores them and joins the network; neither side
   logs the password. An open network has an empty password.

Rules:
- The board **rejects `knoend.pair` and `knoend.wifi` on any transport other than serial** (`knoend.error`,
  code `unauthorised`).
- `K` is never part of the firmware image. Firmware is public and every board runs the same image,
  so a built-in key would be the same on every board.
- The web app asks before pairing ("Pair this board with this computer?"). Boards flashed from the
  hosted page without the service are paired the first time the service sees them over USB.
- Forgetting: the companion deletes its copy; "Forget all computers" on the board (and a factory
  reset) erases every stored key.

### 4.2 Discovery and session handshake

- The board announces `_knoend._tcp` (port 80) over mDNS with TXT `id=<deviceId>` and
  `pv=<protocolVersion>`, host name `knoend-<last 6 hex digits of deviceId>.local`; also
  `_openpager._tcp`, the name from before the rename (section 1).
  Nothing derived from `K` is broadcast (it could be replayed, and would let anyone track the board).
- The companion opens `ws://<board>/ws`. The first three text frames prove both sides hold `K`, using
  fresh 16-byte random nonces written as 32 hex digits:

| # | From | Message |
| --- | --- | --- |
| 1 | companion | `{ "hello": { "companion": "<companion id>", "nonce": "<Nc>" } }` |
| 2 | board | `{ "hello": { "board": "<deviceId>", "nonce": "<Nb>", "proof": HMAC(K, "board\n" + Nc + "\n" + Nb) } }` |
| 3 | companion | `{ "proof": HMAC(K, "companion\n" + Nb + "\n" + Nc) }` |

  HMAC is HMAC-SHA256, written as 64 lowercase hex digits. If the board holds no key for that
  companion, or a proof is wrong, the other side closes the connection. Each proof covers a nonce
  the other side just chose, so a recorded handshake cannot be replayed.

### 4.3 Authenticated frames

After the handshake every frame is:

`{ "ctr": 0, "mac": "<64 hex digits>", "packet": "<packet JSON as a string>" }`

- Session id `S` = `Nb` + `Nc` (64 hex digits, board's nonce first).
- `mac` = HMAC-SHA256(K, `S` + "\n" + `ctr` in decimal + "\n" + the exact `packet` text). No JSON
  canonicalisation is needed, and a frame from one session never verifies in another.
- `ctr` starts at 0 for each sender in each session and must increase; repeats are dropped.
- Frames that fail are dropped and the connection is closed.

The exact bytes are pinned by the test vector [`examples/wifi-session.json`](examples/wifi-session.json)
(all-zero test key; `tools/validate_spec.py` recomputes every proof and MAC).

### 4.4 Fallback: pairing code (no cable)

For a computer that never gets a cable: both screens show the same 6-digit code and the person
confirms on both. This needs a password-authenticated key exchange; the messages are not specified
yet (open item for v0.1).

## 5. Layout libraries (files)

A layout library is a **TOML file, data only**. One schema validates built-in, community,
AI-drafted and user-override libraries: [`schemas/library.schema.json`](schemas/library.schema.json)
(TOML is parsed to JSON first). Example: [`examples/library-amazon.toml`](examples/library-amazon.toml).

```toml
[pack]
id = "community.amazon"
name = "Amazon"
version = "1.2.0"
source = "community"          # builtin | community | ai | user
license = "MIT"

[match]
web = ["amazon.ca", "amazon.com"]

[[key]]
label = "Orders"
speak = "Your orders"
action = { kind = "open", url = "/gp/your-account/order-history" }
```

Actions (the only kinds allowed; anything else fails validation):

| `kind` | Fields | Carried out by |
| --- | --- | --- |
| `shortcut` | `keys`, e.g. `"Primary+Shift+P"` | Board, as `hid` (companion resolves `Primary` per OS) |
| `open` | `url`: a path on the matched site (must start with `/`) | Companion (via the extension) |
| `element` | `selector`: CSS selector on the matched page | Companion (via the extension) |
| `focus` | `selector`: a field on the matched page to type into | Companion (via the extension) |
| `media` | `command` (toggle, play, pause, next, previous, mute, forward, back), optional `selector` | Companion (media player, else the extension) |
| `command` | `name`: a command the user has defined on their computer | Companion; never a raw command line |
| `agent` | `reply`: allow, always, deny, stop, option, done, decline, go_to | Companion, answering a coding agent (only in `match.agent` libraries) |

Shortcut grammar: zero or more of `Primary`, `Ctrl`, `Shift`, `Alt`, `Meta` (Cmd/Super), each followed
by `+`, then one key: `A`–`Z`, `0`–`9`, `F1`–`F24`, `Enter`, `Escape`, `Tab`, `Space`, `Backspace`,
`Delete`, `Insert`, `Home`, `End`, `PageUp`, `PageDown`, `Up`, `Down`, `Left`, `Right`, and the
punctuation keys by US-layout position: `Backquote`, `Minus`, `Equal`, `BracketLeft`, `BracketRight`,
`Backslash`, `Semicolon`, `Quote`, `Comma`, `Period`, `Slash`.

`confirm = true` on a key marks it risky (second tap); validators also flag keys whose label or
action looks like buy/submit/send/delete without it.

**Grid and pages.** `grid = "2x2"` in `[pack]` (cols x rows, 1-4 each; default `"3x3"`) sets the
grid of every page; a `[[page]]` entry overrides it for one page (one entry per page number). A key's `page` (1-8, default 1) and
`slot` (1 to cols×rows of its page) are optional: a key with neither takes the next free slot,
running onto the next page when one is full; a key with only `page` takes the next free slot on
that page. At most 64 keys. The library's grids and pages are how it was made, and how the web
app's editor shows it. On the board, the companion shows every page in the person's **key size**
(2x2, 3x3 by default, or 4x4, within the board's `grid`): the keys in order (page, then slot),
blank slots left out, packed into as many pages of that grid as they need. So a 9-key 3x3 layout
fills one 4x4 page, and a 16-key 4x4 layout becomes two 3x3 pages. Agent screens keep their own
grid. The media row is the bottom row of the page shown
(only on grids of 3 rows or more); "Go to Claude" takes its last key, except on a page of fewer than
4 keys, where the status line says "Claude needs you" instead. AI drafts get their grid from
their number of keys, on one page: 1 key 1x1, 2 1x2, 3-4 2x2, 5-9 3x3, 10-16 4x4.

```toml
[pack]
grid = "2x2"                  # pages 1 and 3: four big keys

[[page]]
number = 2
grid = "4x4"                  # page 2: sixteen small keys (icon or label only)

[[key]]
page = 2
slot = 16
label = "Settings"
action = { kind = "shortcut", keys = "Primary+Comma" }
```

Precedence when several libraries match: user override → installed library → AI draft → default keys.
The web app's editor writes user overrides: `user-<name>.toml` in the person's library folder, id
`user.<name>`, with `overrides` naming the recipe or draft it layers on (which stays, underneath).

**Tested.** `[[pack.tested]]` entries record who saw a library work, on which OS and when
(`os` = linux | macos | windows, `by`, `date` YYYY-MM-DD, optional `note` such as the desktop and
browser). An OS without an entry is untested there, even when `match` lists it.

```toml
[[pack.tested]]
os = "linux"
by = "Sundar"
date = "2026-10-18"
note = "Arch Linux, i3 (X11), Firefox"
```

**Recipes.** Libraries shipped with KnoEnd live in the repository's `recipes/` folder and are
compiled into the companion: `apps/` (desktop apps, one file covering every OS through its `match`
entries and `Primary` shortcuts), `sites/` (`<host>.toml` for the whole site, `<host>/<page>.toml` for
a kind of page), and `agents/`. Each has `source = "builtin"` and an id starting `builtin.`;
`tools/validate_spec.py` checks every one. AI drafts stay on the computer that made them until a
person has used them and they are promoted into `recipes/` with a `tested` entry.

**Agent screens.** A library whose `match.agent` names an agent and a state
(`claude-code/permission`, `claude-code/question`, `claude-code/elsewhere`) sets what the board shows
for that state: which slot holds which answer (`agent` actions), labels, icons. The companion fills in
what changes per request: the explanation panel, the agent's options on `reply = "option"` keys (in
order; spare ones are hidden), `always` only when the agent offers a rule to remember, `done` only on
several-choice questions, and a second tap on allow/always for commands that look destructive. Agent
screens use `grid = "3x3"` (the explanation panel needs three rows) and have one page. A user
library with the same `match.agent` replaces the built-in screen. An `agent` key in a library without
`match.agent` is rejected (it would do nothing there).

## 6. Open questions

- Pairing-code key exchange for computers without a cable (section 4.4).
- Storing keys with NVS encryption on the board (today: plain NVS; reading them needs the board in hand).
- Icons: format and size on 106×66 px keys, and whether they travel in packets or live on microSD.
- What desktop `match` patterns are compared against on each OS (Linux: `.desktop` id or WM_CLASS;
  macOS: bundle id; Windows: executable name) and whether `*` wildcards are allowed.
- Whether the board speaks KDE Connect directly (roadmap).

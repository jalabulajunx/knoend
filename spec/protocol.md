# OpenPager protocol, v0 (draft)

Status: **draft for review** (both owners review every change under `spec/`). `protocolVersion` is
`0` until the first tagged release; until then breaking changes are allowed but must be logged.

This document covers what travels between a **board** and a **companion** (the local `openpager`
service, or the board simulator). The service's own HTTP API for the web app is out of scope here.

Machine-checkable versions of everything below are in [`schemas/`](schemas/); every file in
[`examples/`](examples/) must validate (`python3 tools/validate_spec.py`).

## 1. Packets

Every message is one JSON object, UTF-8, the same shape on every transport (borrowed from KDE
Connect's packet format, concepts only):

```json
{ "id": 42, "type": "openpager.key", "body": { "key": 5, "layout": "builtin.vscode" } }
```

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | integer ≥ 0 | Sender's sequence number, +1 per packet, restarting at 0 on reboot or reconnect |
| `type` | string | `openpager.<name>`, one of the types in section 3 |
| `re` | integer, optional | `id` of the packet this one answers |
| `body` | object | Type-specific content; unknown fields must be ignored |

Rules:

- A receiver **ignores packet types it does not know** and fields it does not know, so either side
  can add features without breaking the other.
- Packets are at most **4096 bytes** of JSON. A layout that does not fit is an error, not split.
- Text shown on the board (`label`, `text`, `summary`) is plain UTF-8, no markup.

## 2. Transports and framing

| Transport | Framing | Notes |
| --- | --- | --- |
| USB-C serial | One packet per line (`\n`-terminated JSON), 115200 baud on the ONX2432G028 | Lines not starting with `{` are console/log output and are ignored |
| Wi-Fi | WebSocket `ws://<board>/ws` (found as `openpager.local`), one packet per text frame | Authenticated frames only, see section 4 |
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
| `openpager.identity` | both | First packet on every connection: who I am and what I can do |
| `openpager.layout` | C→B | Replace the keys on the board |
| `openpager.layout.ack` | B→C | Layout accepted or rejected |
| `openpager.key` | B→C | A key was tapped and its action needs the companion |
| `openpager.status` | C→B | Show a message on the status bar |
| `openpager.approve.request` | C→B | A coding agent asks permission; show it and wait for Approve/Deny |
| `openpager.approve.response` | B→C | The person's answer |
| `openpager.pair` | C→B | Serial only: hand the board a Wi-Fi key (section 4.1) |
| `openpager.pair.ack` | B→C | Key stored |
| `openpager.wifi` | C→B | Serial only: the home Wi-Fi name and password (section 4.1) |
| `openpager.wifi.ack` | B→C | Credentials stored; the board joins the network |
| `openpager.wifi.scan` | C→B | Serial only: list the Wi-Fi networks the board can hear (empty body) |
| `openpager.wifi.networks` | B→C | Up to 20 networks, strongest first: `ssid`, `rssi` (dBm), `channel`, `secured`. The ESP32-S3 hears 2.4 GHz only |
| `openpager.ping` | both | Liveness check; answer with `openpager.ping` and `re` set |
| `openpager.error` | both | A packet could not be handled |

### 3.1 `openpager.identity`

Sent by both sides immediately after connecting; nothing else is sent before the peer's identity
arrives.

```json
{ "id": 0, "type": "openpager.identity", "body": {
    "deviceId": "a0b1c2d3e4f5", "deviceName": "OpenPager", "deviceType": "board",
    "protocolVersion": 0, "firmware": "a1ea8a8",
    "board": { "profile": "onx2432g028", "width": 320, "height": 240,
               "grid": { "cols": 3, "rows": 3 }, "statusBar": true },
    "features": { "speaker": true, "battery": false, "nativeUsb": false,
                  "transports": ["ble-hid", "ble-data", "serial", "wifi"] },
    "incoming": ["openpager.layout", "openpager.status", "openpager.approve.request", "openpager.ping"],
    "outgoing": ["openpager.key", "openpager.layout.ack", "openpager.approve.response", "openpager.ping"] } }
```

- `deviceId`: stable per device (boards: base MAC, lowercase hex, no separators).
- `deviceType`: `board`, `companion` or `sim` (the simulator identifies as `sim` but otherwise
  behaves exactly like a board).
- `board` is present only when `deviceType` is `board` or `sim`.
- A companion that receives a higher `protocolVersion` than it supports shows "update needed" and
  sends nothing else.

### 3.2 `openpager.layout`

```json
{ "id": 7, "type": "openpager.layout", "body": {
    "layout": "builtin.vscode", "name": "VS Code", "source": "builtin", "reviewed": true,
    "keys": [
      { "key": 1, "label": "Palette", "speak": "Command palette",
        "send": { "hid": { "modifiers": 3, "usage": 19 } } },
      { "key": 2, "label": "Terminal", "send": { "event": true } },
      { "key": 9, "label": "Delete line", "confirm": true,
        "send": { "hid": { "modifiers": 3, "usage": 14 } } } ] } }
```

- `key`: slot 1–9, numbered left to right, top to bottom. Slots not listed are blank.
- `send` says what a tap does, decided by the companion when it builds the packet:
  - `hid`: the board types this itself over the keyboard link (USB HID usage + modifier bits:
    1 Ctrl, 2 Shift, 4 Alt, 8 GUI/Cmd; right-hand modifiers are not used). Works with no companion.
    The companion has already resolved `Primary` to Ctrl or Cmd for the connected computer's OS.
  - `event`: the board sends `openpager.key` and the companion carries out the action (open a
    page, run a named command, press a page element via the extension).
- `confirm: true`: risky action (buy, submit, send, delete). The first tap shows "Tap again to
  confirm"; only a second tap on the same key within 3 s acts.
- `reviewed: false` (AI-drafted, unreviewed): the board marks the layout as unreviewed on screen.
- The board stores the last layout and keeps using it while the companion is away.

### 3.3 `openpager.layout.ack`

`{ "re": 7, "body": { "layout": "builtin.vscode", "ok": true } }`; on failure `ok: false` and
`error` (short reason, e.g. `"too many keys"`).

### 3.4 `openpager.key`

```json
{ "id": 18, "type": "openpager.key", "body": { "key": 2, "layout": "builtin.vscode", "seq": 31 } }
```

Sent for keys whose `send` is `event`, and also for `hid` keys tapped while no Bluetooth
keyboard host is connected: then the body carries `"hid": { "modifiers": 3, "usage": 19 }` and the
companion types that keystroke into the focused app itself (a board whose USB cannot be a keyboard,
like the ONX2432G028, still works over Wi-Fi or USB serial). `seq` counts every tap since boot, so the companion can
detect lost packets. One packet per tap (sent on release), however long the key is held.

### 3.5 `openpager.status`

```json
{ "id": 9, "type": "openpager.status", "body": { "text": "Build passed", "level": "info", "ttl": 30 } }
```

`level`: `info`, `attention` (highlighted) or `alert` (highlighted + beep). `ttl` seconds; omitted
means until replaced. An empty `text` clears the bar.

### 3.6 `openpager.approve.request` / `openpager.approve.response`

```json
{ "id": 12, "type": "openpager.approve.request", "body": {
    "request": "c1f3", "agent": "claude-code", "session": "openpager",
    "summary": "Run a shell command", "command": "rm -rf build/", "destructive": true,
    "timeout": 120 } }
```

```json
{ "id": 40, "re": 12, "type": "openpager.approve.response", "body": { "request": "c1f3", "decision": "deny" } }
```

- The board shows `summary` and the real `command` (truncated with an ellipsis if too long, never
  rewritten) with Approve and Deny keys. `destructive: true` makes Approve need a second tap.
- Several requests form a queue on the board, oldest first.
- **No answer means no:** after `timeout` seconds the board drops the request and sends nothing;
  the companion treats silence as "fall back to the agent's own prompt", never as approval.
- `decision`: `allow` or `deny`.

### 3.6a Layout extras: `icon`, `detail`

- A key's optional `icon` is a name from [`icons.txt`](icons.txt) (Material Icons, drawn white above a
  small label) or the name of a picture sent with `openpager.image` (`app:...`, `site:...`, drawn in
  colour). A board that does not know the name shows the label alone.
- A layout's optional `detail` (up to 240 characters, `\n` for new lines) is shown in place of the
  middle row (slots 4-6 are hidden); the top and bottom rows get shorter and the text scrolls.
  Used for what a permission request is about.

### 3.6b `openpager.image` / `openpager.image.ack`

```json
{ "id": 30, "type": "openpager.image", "body": { "name": "app:firefox", "w": 32, "h": 32,
    "offset": 0, "total": 3072, "data": "<base64 of this slice>" } }
```

A picture for keys: `w`x`h` pixels (at most 64x64) of RGB565 with its two bytes swapped (as LVGL
draws with `LV_COLOR_16_SWAP`), then one byte of alpha: `total` = w*h*3 bytes. It is sent in slices
that each fit a packet (the companion uses 2048 bytes), in order, starting at `offset` 0. After the
last slice the board answers `openpager.image.ack` `{ "name": ..., "ok": true }` (or `ok: false` and
`error`). The board keeps about 48 pictures (least recently used replaced) until it restarts; the
companion sends a picture again on each new connection, before the layout that uses it.

### 3.6c What the companion shows when

- The focused app's or site's layout (section 5 and `browser.md`), with a media row while it plays.
- A coding agent's permission request (`system.approve`: Allow, Always, Deny, Stop, with `detail`)
  until it is answered on the board or in the agent's own window.
- Nothing focused (empty desktop): the launcher (`system.launcher`): up to 9 apps and sites the
  person used most and most recently, with their own icons; a tap opens one. Usage (seconds in front
  per app and site) is kept on the computer only.

### 3.7 `openpager.error`

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
   `{ "id": 3, "type": "openpager.pair", "body": { "companion": "laptop-3f9c2a", "companionName": "Workstation", "key": "<64 hex digits>" } }`
3. The board stores `K` for that companion (up to 4 companions; the oldest is replaced) and answers
   `openpager.pair.ack` with `{ "companion": "laptop-3f9c2a", "ok": true }`.

4. The companion can send the home Wi-Fi credentials the same way, also **serial only**:
   `{ "type": "openpager.wifi", "body": { "ssid": "Home", "password": "..." } }`, answered by
   `openpager.wifi.ack` `{ "ok": true }`. The board stores them and joins the network; neither side
   logs the password. An open network has an empty password.

Rules:
- The board **rejects `openpager.pair` and `openpager.wifi` on any transport other than serial** (`openpager.error`,
  code `unauthorised`).
- `K` is never part of the firmware image. Firmware is public and every board runs the same image,
  so a built-in key would be the same on every board.
- The web app asks before pairing ("Pair this board with this computer?"). Boards flashed from the
  hosted page without the service are paired the first time the service sees them over USB.
- Forgetting: the companion deletes its copy; "Forget all computers" on the board (and a factory
  reset) erases every stored key.

### 4.2 Discovery and session handshake

- The board announces `_openpager._tcp` (port 80) over mDNS with TXT `id=<deviceId>` and
  `pv=<protocolVersion>`, host name `openpager-<last 6 hex digits of deviceId>.local`.
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
action looks like buy/submit/send/delete without it. At most 9 keys; `slot` (1–9) is optional,
otherwise keys fill slots in order.

Precedence when several libraries match: user override → installed library → AI draft → default keys.

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

**Recipes.** Libraries shipped with OpenPager live in the repository's `recipes/` folder and are
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
several-choice questions, and a second tap on allow/always for commands that look destructive. A user
library with the same `match.agent` replaces the built-in screen.

## 6. Open questions

- Pairing-code key exchange for computers without a cable (section 4.4).
- Storing keys with NVS encryption on the board (today: plain NVS; reading them needs the board in hand).
- Pages beyond 9 keys (`PluginDynamicFolder`-style folders).
- Icons: format and size on 106×66 px keys, and whether they travel in packets or live on microSD.
- What desktop `match` patterns are compared against on each OS (Linux: `.desktop` id or WM_CLASS;
  macOS: bundle id; Windows: executable name) and whether `*` wildcards are allowed.
- Whether the board speaks KDE Connect directly (roadmap).

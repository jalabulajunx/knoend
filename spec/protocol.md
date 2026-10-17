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

Sent only for keys whose `send` is `event`. `seq` counts every tap since boot, so the companion can
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

### 3.7 `openpager.error`

`{ "re": 7, "body": { "code": "unsupported", "message": "..." } }`. Codes: `bad-packet`,
`unsupported`, `too-large`, `unauthorised`, `busy`.

## 4. Pairing and authentication (Wi-Fi)

Bluetooth uses the link's own pairing and encryption (currently "Just Works"; see the running log).
Serial is trusted (physical access). Wi-Fi needs its own trust, because anyone on the network can
reach the board:

1. **Pairing (draft):** the board and the companion each show the same 6-digit code; the person
   confirms on both. The result is a 32-byte shared key `K`, stored on both sides.
2. **Every Wi-Fi frame is authenticated:**
   `{ "ctr": 105, "mac": "<64 hex digits>", "packet": "<packet JSON as a string>" }`
   where `ctr` is a per-sender counter that must increase (replays are dropped). `mac` is
   HMAC-SHA256 with key `K` over `ctr` in decimal, a newline, then the exact `packet` text (here
   `"105\n{\"id\":...}"`), so no JSON canonicalisation is needed.
3. Frames that fail the check are dropped and answered with `openpager.error` code `unauthorised`
   (unauthenticated, so the receiver must not trust it).

The key-exchange messages are not specified yet; open item for v0.1.

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
| `command` | `name`: a command the user has defined on their computer | Companion; never a raw command line |

Shortcut grammar: zero or more of `Primary`, `Ctrl`, `Shift`, `Alt`, `Meta` (Cmd/Super), each followed
by `+`, then one key: `A`–`Z`, `0`–`9`, `F1`–`F24`, `Enter`, `Escape`, `Tab`, `Space`, `Backspace`,
`Delete`, `Insert`, `Home`, `End`, `PageUp`, `PageDown`, `Up`, `Down`, `Left`, `Right`, and the
punctuation keys by US-layout position: `Backquote`, `Minus`, `Equal`, `BracketLeft`, `BracketRight`,
`Backslash`, `Semicolon`, `Quote`, `Comma`, `Period`, `Slash`.

`confirm = true` on a key marks it risky (second tap); validators also flag keys whose label or
action looks like buy/submit/send/delete without it. At most 9 keys; `slot` (1–9) is optional,
otherwise keys fill slots in order.

Precedence when several libraries match: user override → installed library → AI draft → default keys.

## 6. Open questions

- Pairing key exchange messages (section 4).
- Pages beyond 9 keys (`PluginDynamicFolder`-style folders).
- Icons: format and size on 106×66 px keys, and whether they travel in packets or live on microSD.
- What desktop `match` patterns are compared against on each OS (Linux: `.desktop` id or WM_CLASS;
  macOS: bundle id; Windows: executable name) and whether `*` wildcards are allowed.
- Whether the board speaks KDE Connect directly (roadmap).

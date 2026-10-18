# Browser link (v0 draft)

How the browser extension (Chrome, Chromium-based browsers, Firefox) and the `openpager` service
work together, so a board's keys can follow the website in the active tab and act on the page.
Packets elsewhere in `spec/` are unchanged; this is a separate link between two programs on the
same computer.

## 1. Pieces and path

```
extension (background) --native messaging--> openpager browser-host --local socket--> openpager serve
```

- **Native messaging** (as KDE's Plasma browser integration does): the browser starts the
  `openpager` binary itself and talks to it over stdin/stdout, each message a 32-bit
  native-endian length followed by that many bytes of UTF-8 JSON. Only extensions named in the
  host manifest may start it (`allowed_origins` in Chrome, `allowed_extensions` in Firefox). There
  is no network port and nothing for the person to pair.
- The host is a relay: it connects to the running service's **browser socket** and copies messages
  both ways, one JSON object per line. The socket is a Unix domain socket that only this user can
  open: `$XDG_RUNTIME_DIR/openpager/browser.sock` on Linux, `~/Library/Application
  Support/openpager/browser.sock` on macOS. If the service is not running, the host tells the
  extension (`{"type": "service", "running": false}`) and keeps retrying.
- Host manifest name: `io.github.jalabulajunx.openpager`. `openpager browser install` writes it for
  each installed browser; the browser runs the `openpager` binary with its own arguments (an
  extension origin for Chrome; a manifest path and extension id for Firefox), which is how the
  binary knows to act as the host.

## 2. Messages

Every message is a JSON object with a `type`. Extension to service:

| `type` | Body | Meaning |
| --- | --- | --- |
| `hello` | `browser` ("chrome", "firefox", ...), `version` | The extension started. |
| `tab` | `url`, `title`, `windowFocused` (bool), `granted` (bool: board buttons are on for this site) | The active tab of the focused window changed, navigated, or the window gained or lost focus. Sent only for `http`/`https` pages; other pages send `url: null`. |
| `inventory` | `request`, `url`, `items` (see 3) | Answer to `inventory`. |
| `configure` | (none) | The person pressed "Make keys for this site with AI" in the popup. |
| `result` | `request`, `ok`, `error?` | Answer to `act`. |

Service to extension:

| `type` | Body | Meaning |
| --- | --- | --- |
| `act` | `request`, `action` (see 4) | Carry out a key's action in the active tab. |
| `inventory` | `request` | Describe the active page's controls (only on a granted site). |
| `grant` | `request` | The board wants to act on the active site, which is not allowed yet: open the extension's "Allow" page next to it and answer (`result`) once the person chose, within two minutes. |
| `service` | `running` (bool) | Sent by the host itself when the service is (not) reachable. |
| `configure-status` | `ok`, `text` | How `configure` ended (keys made, or why not). |

`request` is an opaque string chosen by the service. The extension answers every `act` and
`inventory`, with `ok: false` and a short `error` if it could not (`"not-granted"`, `"not-found"`,
`"not-active"`, `"bad-action"`).

## 3. Page inventory: what the agent may build keys from

Collected only on a site the person granted (section 5), on request. It lists **controls, never page
text or field values**:

```json
{ "role": "button", "name": "Compose", "selector": "[data-testid=\"compose\"]",
  "shortcut": "c" }
```

- `role`: `button`, `link`, `textbox`, `searchbox`, `tab`, `checkbox`, `menuitem`, `media`.
- `name`: the accessible name (aria-label, label, alt, or the control's own short text), at most 60
  characters. Text a person typed into a field is never included.
- `selector`: a CSS selector that finds exactly this element, built from stable attributes in this
  order: `id`, `data-testid` / `data-test` / `data-qa`, `aria-label`, `name`, then a short path.
- `shortcut`: from `accesskey` or `aria-keyshortcuts`, when the page declares one.
- `href` (links): same-site path and query only.
- At most 300 items; hidden and disabled controls are left out.
- A link's or button's own short text (40 characters or less) is used as its name when it has no
  label, so names can include visible titles (a video or article title shown as a link). Field
  contents never are. This is why the inventory goes to the agent only when the person asks.

## 4. Actions on a page

A layout library key for a website uses one of these actions (`spec/protocol.md` section 5):

| `kind` | Fields | What the extension does |
| --- | --- | --- |
| `open` | `url` | Navigates the active tab to this path on the current site (a path only: a library cannot send the tab to another site). |
| `element` | `selector` | Clicks the element. |
| `focus` | `selector` | Focuses (and selects the text of) a field, ready to type. |
| `media` | `command`: `toggle`, `play`, `pause`, `next`, `previous`, `mute`, `forward`, `back` | Acts on the page's main `<video>`/`<audio>` element; `next`/`previous` click the player's own buttons when the library gives a `selector`. |

`shortcut` keys are typed by the board or the service as for any app, not by the extension.
On Linux, `media` keys go first to the browser's media player over MPRIS (as the keyboard's media
keys and KDE Connect do), which may start playback where a page script may not; the extension acts
only when no player is found or for `mute`.

## 4a. Which keys a page gets

`match.web` patterns name a host or parent domain (`youtube.com`), any ending (`amazon.*`), and
optionally a path prefix (`youtube.com/watch`). The longest matching pattern wins, so a site can
have different keys on different kinds of page. A site layout wins over the browser's own.

## 4b. Drafting keys for a site

"Make keys for this site with AI" (popup) or Configure (board, on a site with no keys) asks the
extension for the page inventory, then the drafting agent for up to 9 keys. The agent receives the
site's host, the page path without its query, and the inventory; its draft is pinned to the site
(`match.web`), saved as an unreviewed AI library, and rejected if any selector is not in the
inventory.
`element`, `focus` and `media` need the site to be granted; `open` needs only the `tabs` permission.

## 5. Permissions and privacy

- At install the extension holds `tabs` and `nativeMessaging` only: it can see which site is
  active, nothing on the page.
- Board buttons are turned on per site (extension popup; or, when a board key or Configure needs a
  site that is not allowed, a small extension page the extension opens next to it with Allow / Not
  now, since browsers only accept a permission request from a click in the browser). That asks the browser for
  the site's host permission (`https://example.com/*`); only then may the extension read the
  inventory or act there. Turning a site off removes the permission.
- The service sends the URL host to the AI drafting agent, never the full URL; the inventory goes to
  the agent only when the person asks for a layout for that site.

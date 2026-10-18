# Browser extension link: native messaging, per-site permission

2026-10-18 - Sundar (Claude Code)

## What

The browser extension reaches the `openpager` service through the browser's **native messaging**:
the browser starts the `openpager` binary as a host, which relays to the running service over a Unix
socket only this user can open (`spec/browser.md`). Board buttons are turned on **per site**: the
extension holds `tabs`, `nativeMessaging` and `scripting` at install and asks for one site's host
permission when the person turns buttons on there.

## Why

- The same approach KDE's Plasma browser integration uses; the browser itself checks that only our
  extension id may start the host. No port is opened and nothing has to be paired or copied.
- Per-site permission keeps the extension from reading pages the person never chose; the AI agent
  sees a page's controls only after that and only when asked.

## Alternatives rejected

- **A local WebSocket on 127.0.0.1 with a pairing token.** Simpler, but another open port and a
  token to protect; any local program could try it.
- **All sites at install.** One prompt, but the extension could read every page visited.

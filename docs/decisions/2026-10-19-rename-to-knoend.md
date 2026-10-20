# Rename: OpenPager becomes KnoEnd

2026-10-19 - Sundar (Claude Code); name agreed with Tom on #19

## What

The project, device and command are now **KnoEnd** (said "no end"; `knoend` in commands, crates and
network names; Kn°End only in the logo). Everything was renamed, with a fallback for what is already
out there:

- **Wire:** message types `knoend.<name>` and mDNS `_knoend._tcp`. Both sides accept `openpager.*`;
  the board answers each companion in the prefix it last used, and announces both mDNS names; the
  companion talks `openpager.*` to a board seen only as `_openpager._tcp` or that answers only
  `openpager.identity` over USB (spec/protocol.md section 1).
- **Kept on purpose:** the board's NVS namespace `openpager` (pairings and Wi-Fi survive a firmware
  update) and the Chrome extension's key (its id is unchanged).
- **Computer:** settings move once from `~/.config/openpager` to `~/.config/knoend`; the service
  also listens on the old socket folder, so Claude Code hooks and browser hosts installed before
  the rename (running the old program) still reach it until reinstalled; `knoend browser install`
  registers the new and the old host names, the new and old Firefox add-on ids; `knoend service
  install` removes an `openpager` background service; `KNOEND_BUILTIN` with `OPENPAGER_BUILTIN` as
  fallback.
- **Repository:** renamed in place to `jalabulajunx/knoend` (GitHub redirects the old URLs).

## Why

"OpenPager" named one feature; see #19 for the search, the checks and the trade-offs.

## Alternatives rejected

- **Rename only what people see** (keep `openpager.*` on the wire): less risk, but two names forever.
- **No fallback:** every board, extension and hook would have had to be updated at once.

## Open

- Drop the `openpager.*` / `_openpager._tcp` fallback once no v0.1.x boards or companions remain.
- Past log entries and decision records keep the old name (they are history).

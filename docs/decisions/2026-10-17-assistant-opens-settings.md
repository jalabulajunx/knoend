# The setup assistant may open settings pages and a verified driver installer

2026-10-17 - Tom (Claude Code)

## What

The setup assistant's fixed list of actions (`assistant::ACTIONS`) grows beyond the wizard's own
steps. On macOS it can offer buttons that:

- open System Settings > Privacy & Security > Accessibility;
- open System Settings > General > Login Items & Extensions (Driver Extensions);
- download WCH's USB-serial driver (`CH34xVCPDriver.pkg` from WCH's GitHub account) and open its
  installer, only if Gatekeeper accepts it as notarized and signed by WCH's Developer ID team
  (5JZGQTGU4W).

The service does this through `POST /api/open` with a target from a fixed list
(`companion/crates/cli/src/macos_help.rs`). The assistant's snapshot also gains macOS facts: USB
serial devices and whether each has a port, the WCH driver extension's state, whether openpager may
type keys, and which app macOS asks Accessibility for.

## Why

Setting up the ONX2432G028 and the Freenove FNK0104B on a Mac (2026-10-17) needed three things only
the person can approve: WCH's driver, its Driver Extension switch, and Accessibility for the app that
launched `openpager`. A coding agent found and prepared each one; the assistant could neither see
the cause (no port is not the same as no device) nor take the person there. The aim is that the
assistant alone can carry anyone through setup.

## Rules kept

- Nothing is approved, installed or erased without the person's click; the actions open a page or
  an installer, and macOS still asks for the password or the switch.
- Fixed targets only; the model chooses among buttons, never a URL, path or command.
- Anything downloaded is checked before it is opened.

## Alternatives rejected

- **Links in the assistant's text.** The model would write the URLs; a fixed list cannot be steered
  to another site, and settings pages need `open` on the Mac anyway.
- **Installing the driver silently** (`sudo installer`). Needs the password in the service; the
  person would not see what is installed.
- **Bundling WCH's driver.** Not ours to redistribute, and it would go stale.

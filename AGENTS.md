# Instructions for coding agents

Several people work on OpenPager, each with their own coding agent. This file and the running log
are how the agents share context. Read both before starting work.

Claude Code reads `CLAUDE.md` rather than this file: keep a local, untracked `CLAUDE.md` whose
first line is `@AGENTS.md`.

## Start and end of every session

1. **Read [`docs/status.md`](docs/status.md)** (what is on main) and the last few entries of
   [`docs/log.md`](docs/log.md) before changing anything.
2. **Append an entry to `docs/log.md`** before you finish: date, who (person + agent), what changed,
   what was tested and on which hardware or OS, decisions made, and open questions. Use the
   template at the top of the log.
3. Never rewrite or delete past entries. If an entry turns out wrong, add a new entry that corrects it.
4. A decision that changes the architecture or a shared contract also gets a short record in
   `docs/decisions/` (what, why, alternatives rejected), linked from the log entry.

## Working rules

- **Verify before claiming.** Check the content of outputs, not just exit codes. Say plainly what is
  untested. "Builds" is not "works on the board".
- **Hardware: one change per flash.** Build, flash, then wait for the person at the bench to report
  what the board does before the next change. Opening the ONX2432G028's serial port can reset it.
- **No one else's code without a licence that allows it.** Do not copy:
  - board-vendor example code (Nextion ONX2432G028 examples: no licence; Freenove examples:
    CC BY-NC-SA) - use Espressif's components and the schematics instead;
  - Logitech software, SDK assemblies, or the `Logitech/actions-sdk` examples (no licence).

  When you do use third-party code, keep its licence header and note the source and licence in the log.
- **This repository is public.** No secrets, Wi-Fi credentials, API keys, personal details, or private
  planning notes in commits, logs or comments.
- **Branches:** one per task, `sundar/...` or `tom/...`. Changes under `spec/` need review by both people.
- **AI tools are disclosed.** Name the agent in each log entry.

## Project layout and builds

See [README.md](README.md) for the layout, and [firmware/README.md](firmware/README.md) for
building and flashing. Firmware: C on ESP-IDF v5.4.4. Companion: Rust. Web app: `web/`.

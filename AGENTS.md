# Instructions for coding agents

Several people work on KnoEnd, each with their own coding agent. This file and the running log
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

## What we already know

Before hardware, flashing, Wi-Fi, Bluetooth or setup work, read
[`docs/knowledge/setup.md`](docs/knowledge/setup.md): board facts, every failure seen so far with
its cause and fix, and how to approach problems nobody has seen yet. The companion's setup assistant
uses the same file as its prompt, so **add what you learn there** (symptom, cause, fix, how it was
confirmed), not only in the log.

## The setup assistant should be able to do what you did

**The service describes itself.** "The assistant" here is the in-product setup assistant, the
onboarding AI in the web app that people ask for help, not you, the coding agent. It never reads
this file: it sees only its prompt (`agents/src/assistant.rs`), `docs/knowledge/setup.md` and the
snapshot the service sends it. So a feature you build is not done until that assistant can see it,
change it and check it: settings in `settings.rs`, buttons in `assistant::ACTIONS` (with what each
does), read-only checks in `checks.rs`, and what the service notices in its log via `say!`. Its
prompt tells it to read those lists each time instead of naming every setting, so problems and
settings nobody planned for still reach it.

The goal is that anyone can set up their computer and a board with the in-product setup assistant
alone, without a coding agent or a developer. So when you solve a setup problem by hand (a missing
driver, a permission, a wrong board, a setting), do not stop at the fix:

1. **Knowledge:** add symptom, cause, fix and how it was confirmed to
   [`docs/knowledge/setup.md`](docs/knowledge/setup.md). The assistant's prompt is built from it.
2. **Evidence:** make the deciding fact visible in the assistant's snapshot
   (`companion/crates/cli/src/diagnostics.rs`, `system_checks`), so the assistant reads it instead
   of asking the person to run commands. Facts only: never passwords, keys or window titles.
   Print what the service notices with `say!` (not `println!`): every such line reaches the
   assistant through the journal (`cli/src/journal.rs`). A line that reports a fault uses
   `problem!` (or `raise!` when the web app is sent the line anyway): the watcher
   (`cli/src/watch.rs`) then puts it to the setup assistant without the person asking. Evidence that is too slow or too big for
   every snapshot becomes a read-only check in `cli/src/checks.rs`, which the assistant runs when
   it needs it.
3. **Action:** every setting the person can change goes in `cli/src/settings.rs` (meaning,
   choices, current value): the web app, the assistant's snapshot and its one-click fixes all read
   that list, so the assistant can see and change a new setting with no prompt edit. If the fix
   needs the person (a password, an Allow switch, an installer), add an
   assistant action that takes them to the exact place (`assistant::ACTIONS`, the web app's
   `ACTIONS`, a fixed list in `cli/src/macos_help.rs` or alongside), or a checked field in the
   assistant's `fix` (`assistant::check_fix`) when the fix is a setting. Actions only open or prepare;
   nothing is approved, installed or erased without the person's click. Anything downloaded is
   checked before it is opened (checksum, or the OS's signature and notarization check).
4. **Prompt:** if the assistant has to behave differently (lead the steps, wait for the person,
   check again afterwards), say so in its system prompt (`companion/crates/agents/src/assistant.rs`)
   and add a test for the new text.
5. **Try it:** ask the assistant about the situation through the web app (or `POST /api/assist`)
   and check its answer uses the new evidence.

**Never make a rebuild the fix.** If the person would have to edit code or recompile to get past
something that varies outside our control (a provider's accepted parameters or address, a model
name, a driver), make it a run-time behaviour (retry, detect) or a setting the web app can change,
and say in the error which setting. A rebuild is only for our own bugs.

## Lessons that cost time

- **"Compiles in CI" is not "works".** The macOS focus watcher passed CI but never followed
  the focused app: `NSWorkspace.frontmostApplication` only updates in a program with a main run loop.
  Platform code needs a run on that platform before it is called working.
- **Read the companion's log first.** On 2026-10-17 every "nothing happens" (keys not typed, layout
  not changing, board not found) had its cause in the service log or in what macOS reports about USB
  devices, not on the board.
- **A board can be on the USB bus with no serial port** (no driver). Look at the USB devices, not only
  the port list (`system_profiler SPUSBHostDataType` on macOS, `lsusb`/`dmesg` on Linux).
- **Check that a setting reaches the hardware.** A definition value can be stored on the board and
  still be overwritten later in start-up (see the 2026-10-17 log entry on mirroring). Read back what
  the board holds before changing the value again.

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

# companion

Rust workspace for `knoend`: a local service plus the CLI.

- `crates/core` — protocol, library engine, validator
- `crates/transport` — BLE, serial, Wi-Fi
- `crates/focus` — active-app detection (`linux.rs`, `macos.rs`, `windows.rs`)
- `crates/agents` — AI layout drafting, onboarding agent, agent-CLI hooks
- `crates/sim` — board simulator (`knoend sim`), same protocol as the firmware
- `crates/flash` — board identification and flashing (`espflash`)
- `crates/server` — local HTTP/WebSocket API on 127.0.0.1; serves `web/`
- `crates/cli` — the `knoend` binary

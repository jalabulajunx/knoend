# spec

Shared contracts between firmware and companion, **v0 draft**. Both owners review every change.

- [`protocol.md`](protocol.md): packets, transports, packet types, Wi-Fi authentication, library format
- `schemas/`: `packet.schema.json`, `library.schema.json`, `wifi-message.schema.json` (JSON Schema 2020-12)
- `examples/`: sample packets and libraries; both sides use them as test fixtures.
  `examples/invalid/` holds inputs that must be rejected.

Check everything with:

```bash
python3 tools/validate_spec.py
```

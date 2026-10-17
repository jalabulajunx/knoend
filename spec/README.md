# spec

Shared contracts between firmware and companion. Both owners review every change.

- `protocol.md` — JSON packets `{id, type, body}`: `openpager.identity`, `.key`, `.layout`, `.status`, `.approve`
- `schemas/` — `library.schema.json`, `identity.schema.json`, packet schemas
- `examples/` — sample packets and libraries used by tests on both sides

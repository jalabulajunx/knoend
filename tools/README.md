# tools

- `validate_spec.py`: checks `spec/examples/` against `spec/schemas/` (valid ones must pass,
  `invalid/` ones must fail), plus packet size, duplicate slots, risky keys without confirm, and the
  Wi-Fi frame test vector. Needs Python 3.11+ and `jsonschema`.

- `release_firmware.py --version X`: packages a firmware build and the board definitions into
  `dist/firmware/` (manifest with SHA-256 per part); `knoend serve --firmware ../dist/firmware`
  installs from it.
- `serial_probe.py`, `dummy_companion.py`: bench tools (protocol probe; Python stand-in companion
  with attack tests).

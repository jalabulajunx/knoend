# tools

- `validate_spec.py`: checks `spec/examples/` against `spec/schemas/` (valid ones must pass,
  `invalid/` ones must fail), plus packet size, duplicate slots, risky keys without confirm, and the
  Wi-Fi frame test vector. Needs Python 3.11+ and `jsonschema`.

Planned: flashing scripts, host listener, test helpers.

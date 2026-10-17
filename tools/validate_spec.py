#!/usr/bin/env python3
"""Check spec/examples against spec/schemas.

Every file in spec/examples/ must pass; every file in spec/examples/invalid/ must fail. The
second half matters: a schema that accepts everything would otherwise pass silently.

Beyond the JSON Schemas it checks what schemas cannot express:
  - packets are at most 4096 bytes;
  - key slots are not used twice;
  - risky-looking keys (buy, pay, submit, send, delete ...) carry confirm = true;
  - the Wi-Fi frame example's MAC verifies with the all-zero test key.

Needs Python 3.11+ (tomllib) and the jsonschema package (Arch: python-jsonschema).
Usage: python3 tools/validate_spec.py
"""

import hashlib
import hmac
import json
import re
import sys
import tomllib
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent
SPEC = ROOT / "spec"
MAX_PACKET_BYTES = 4096
TEST_KEY = bytes(32)  # all-zero key: test vectors only
RISKY = re.compile(r"\b(buy|purchase|pay|checkout|order now|submit|send|delete|remove|erase)\b", re.I)


def load_schema(name):
    schema = json.loads((SPEC / "schemas" / name).read_text())
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema)


PACKET = load_schema("packet.schema.json")
LIBRARY = load_schema("library.schema.json")
WIFI_FRAME = load_schema("wifi-frame.schema.json")


def slots(keys, field):
    """Slots used by a list of keys; library keys without `slot` fill the next free one."""
    used, next_free = [], 1
    for key in keys:
        slot = key.get(field)
        if slot is None:
            while next_free in used:
                next_free += 1
            slot = next_free
        used.append(slot)
    return used


def risky_without_confirm(keys, describe):
    return [f"key '{k['label']}' looks risky but has no confirm"
            for k in keys if RISKY.search(describe(k)) and not k.get("confirm")]


def check_packet(text):
    errors = [e.message for e in PACKET.iter_errors(json.loads(text))]
    if len(text.encode()) > MAX_PACKET_BYTES:
        errors.append(f"packet is {len(text.encode())} bytes, limit {MAX_PACKET_BYTES}")
    if not errors:
        packet = json.loads(text)
        if packet["type"] == "openpager.layout":
            keys = packet["body"]["keys"]
            used = slots(keys, "key")
            if len(used) != len(set(used)):
                errors.append("a key slot is used twice")
            errors += risky_without_confirm(keys, lambda k: k["label"])
    return errors


def check_library(path):
    data = tomllib.loads(path.read_text())
    errors = [e.message for e in LIBRARY.iter_errors(data)]
    if not errors:
        used = slots(data["key"], "slot")
        if len(used) != len(set(used)) or max(used) > 9:
            errors.append("key slots collide or run past 9")
        errors += risky_without_confirm(
            data["key"], lambda k: " ".join([k["label"], k.get("speak", ""), json.dumps(k["action"])]))
    return errors


def check_wifi_frame(text):
    frame = json.loads(text)
    errors = [e.message for e in WIFI_FRAME.iter_errors(frame)]
    if not errors:
        expected = hmac.new(TEST_KEY, f"{frame['ctr']}\n{frame['packet']}".encode(),
                            hashlib.sha256).hexdigest()
        if not hmac.compare_digest(expected, frame["mac"]):
            errors.append("mac does not verify with the test key")
        errors += check_packet(frame["packet"])
    return errors


def check(path):
    if path.suffix == ".toml":
        return check_library(path)
    text = path.read_text().strip()
    if path.name.startswith("wifi-frame"):
        return check_wifi_frame(text)
    return check_packet(text)


def main():
    failures = 0
    valid = sorted(p for p in (SPEC / "examples").iterdir() if p.is_file())
    invalid = sorted((SPEC / "examples" / "invalid").iterdir())
    for path in valid:
        errors = check(path)
        status = "ok  " if not errors else "FAIL"
        failures += bool(errors)
        print(f"{status} {path.relative_to(ROOT)}")
        for error in errors:
            print(f"       {error}")
    for path in invalid:
        errors = check(path)
        status = "ok  " if errors else "FAIL"
        failures += not errors
        reason = errors[0] if errors else "accepted, but it should be rejected"
        print(f"{status} {path.relative_to(ROOT)}  (rejected: {reason})" if errors
              else f"{status} {path.relative_to(ROOT)}  ({reason})")
    print(f"\n{len(valid)} valid, {len(invalid)} invalid examples checked; {failures} problem(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())

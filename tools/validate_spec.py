#!/usr/bin/env python3
"""Check spec/examples against spec/schemas.

Every file in spec/examples/ must pass; every file in spec/examples/invalid/ must fail. The
second half matters: a schema that accepts everything would otherwise pass silently.

Beyond the JSON Schemas it checks what schemas cannot express:
  - packets are at most 4096 bytes;
  - key slots are not used twice;
  - risky-looking keys (buy, pay, submit, send, delete ...) carry confirm = true;
  - Wi-Fi session test vectors: both handshake proofs and every frame MAC are recomputed with the
    vector's key, and each sender's counter must increase.

Needs Python 3.11+ (tomllib) and the jsonschema package (Arch: python-jsonschema).
Usage: python3 tools/validate_spec.py            (spec examples)
       python3 tools/validate_spec.py --packets FILE  (every packet line in a transcript)
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
RISKY = re.compile(r"\b(buy|purchase|pay|checkout|order now|submit|send|delete|remove|erase)\b", re.I)


def load_schema(name):
    schema = json.loads((SPEC / "schemas" / name).read_text())
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema)


PACKET = load_schema("packet.schema.json")
LIBRARY = load_schema("library.schema.json")
WIFI = load_schema("wifi-message.schema.json")


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


MAX_PAGES = 8


def grid_size(text):
    cols, rows = text.split("x")
    return int(cols) * int(rows)


def placements(library):
    """(page, slot) of each library key, or None for a key that does not fit (spec/protocol.md
    section 5): explicit slots first; a key with only `page` takes that page's next free slot; a key
    with neither takes the next free slot, running onto the next page when one is full."""
    default = library["pack"].get("grid", "3x3")
    grids = {p["number"]: p["grid"] for p in library.get("page", [])}
    size = lambda page: grid_size(grids.get(page, default))
    keys = library["key"]
    used = {(k.get("page", 1), k["slot"]) for k in keys if "slot" in k}
    out, cursor = [], 1
    for key in keys:
        if "slot" in key:
            page, slot = key.get("page", 1), key["slot"]
            out.append((page, slot) if slot <= size(page) else None)
            continue
        page = key.get("page", cursor)
        while True:
            slot = next((s for s in range(1, size(page) + 1) if (page, s) not in used), None)
            if slot is not None or "page" in key or page >= MAX_PAGES:
                break
            page += 1
        if "page" not in key:
            cursor = page
        if slot is None:
            out.append(None)
        else:
            used.add((page, slot))
            out.append((page, slot))
    return out


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
            grid = packet["body"].get("grid", {"cols": 3, "rows": 3})
            if max(used, default=0) > grid["cols"] * grid["rows"]:
                errors.append("a key slot runs past the layout's grid")
            errors += risky_without_confirm(keys, lambda k: k["label"])
    return errors


def check_library(path):
    data = tomllib.loads(path.read_text())
    errors = [e.message for e in LIBRARY.iter_errors(data)]
    if not errors:
        placed = placements(data)
        if None in placed or len(placed) != len(set(placed)):
            errors.append("key slots collide or run past the grid")
        errors += risky_without_confirm(
            data["key"], lambda k: " ".join([k["label"], k.get("speak", ""), json.dumps(k["action"])]))
    return errors


def check_wifi_session(text):
    """Replays a recorded session: hello (companion), hello (board), proof (companion), frames."""
    session = json.loads(text)
    key = bytes.fromhex(session["key"])
    mac = lambda msg: hmac.new(key, msg.encode(), hashlib.sha256).hexdigest()
    messages = session["messages"]
    errors = []
    for n, item in enumerate(messages):
        errors += [f"message {n}: {e.message}" for e in WIFI.iter_errors(item["message"])]
    if errors:
        return errors
    if len(messages) < 3:
        return ["a session needs the three handshake messages"]

    nc = messages[0]["message"]["hello"]["nonce"]
    nb = messages[1]["message"]["hello"]["nonce"]
    if not hmac.compare_digest(messages[1]["message"]["hello"]["proof"], mac(f"board\n{nc}\n{nb}")):
        errors.append("board proof does not verify")
    if not hmac.compare_digest(messages[2]["message"]["proof"], mac(f"companion\n{nb}\n{nc}")):
        errors.append("companion proof does not verify")

    session_id = nb + nc
    last_ctr = {}
    for n, item in enumerate(messages[3:], start=3):
        frame, sender = item["message"], item["from"]
        if "packet" not in frame:
            errors.append(f"message {n}: expected a frame after the handshake")
            continue
        if not hmac.compare_digest(frame["mac"], mac(f"{session_id}\n{frame['ctr']}\n{frame['packet']}")):
            errors.append(f"message {n}: mac does not verify")
        if sender in last_ctr and frame["ctr"] <= last_ctr[sender]:
            errors.append(f"message {n}: {sender} counter {frame['ctr']} does not increase")
        last_ctr[sender] = frame["ctr"]
        errors += [f"message {n}: {e}" for e in check_packet(frame["packet"])]
    return errors


def check(path):
    if path.suffix == ".toml":
        return check_library(path)
    text = path.read_text().strip()
    if path.name.startswith("wifi-session"):
        return check_wifi_session(text)
    return check_packet(text)


def check_packet_file(path):
    """Validates every line starting with '{' in a captured transcript (e.g. from serial_probe.py)."""
    problems = 0
    for number, line in enumerate(Path(path).read_text().splitlines(), start=1):
        if not line.startswith("{"):
            continue
        try:
            errors = check_packet(line)
        except json.JSONDecodeError as error:
            errors = [f"not JSON: {error}"]
        if errors:
            problems += 1
            print(f"FAIL line {number}: {line[:100]}")
            for error in errors:
                print(f"       {error}")
    print(f"{path}: {problems} invalid packet(s)")
    return 1 if problems else 0


def check_board_definitions():
    """Every firmware/boards/definitions/*.json must pass board.schema.json and match its file name."""
    validator = load_schema("board.schema.json")
    problems = 0
    for path in sorted((ROOT / "firmware" / "boards" / "definitions").glob("*.json")):
        data = json.loads(path.read_text())
        errors = [e.message for e in validator.iter_errors(data)]
        if data.get("id") != path.stem:
            errors.append(f"id {data.get('id')!r} does not match the file name")
        problems += bool(errors)
        print(f"{'ok  ' if not errors else 'FAIL'} {path.relative_to(ROOT)}")
        for error in errors:
            print(f"       {error}")
    return problems


def check_recipes():
    """Every recipes/**/*.toml must be a valid library, shipped as builtin, with a unique id."""
    problems = 0
    seen = {}
    for path in sorted((ROOT / "recipes").rglob("*.toml")):
        errors = check_library(path)
        if not errors:
            pack = tomllib.loads(path.read_text())["pack"]
            if pack["source"] != "builtin" or not pack["id"].startswith("builtin."):
                errors.append("a recipe in the repository has source = \"builtin\" and a builtin. id")
            if pack["id"] in seen:
                errors.append(f"id {pack['id']} is also used by {seen[pack['id']]}")
            seen[pack["id"]] = path.relative_to(ROOT)
        problems += bool(errors)
        print(f"{'ok  ' if not errors else 'FAIL'} {path.relative_to(ROOT)}")
        for error in errors:
            print(f"       {error}")
    return problems


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--packets":
        return check_packet_file(sys.argv[2])
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
    failures += check_board_definitions()
    failures += check_recipes()
    print(f"\n{len(valid)} valid, {len(invalid)} invalid examples checked; {failures} problem(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())

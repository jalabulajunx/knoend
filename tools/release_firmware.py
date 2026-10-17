#!/usr/bin/env python3
"""Package the universal firmware image and the board definitions into a release folder.

Reads firmware/boards/catalog.json. For each image, copies the parts IDF lists in the build's
flasher_args.json (offset, size, SHA-256) and records where the "board" partition lives. For each
board with a definition (firmware/boards/definitions/<board>.json) that passes
spec/schemas/board.schema.json, adds the definition as its own file. Boards without a definition
or without a built image are listed with status "coming".

Usage: python3 tools/release_firmware.py --version 0.2.0 [--out dist/firmware]
Build first:  . ~/esp/esp-idf/export.sh && cd firmware && idf.py -B build/universal build
"""

import argparse
import csv
import hashlib
import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent
FIRMWARE = ROOT / "firmware"
BOARD_PARTITION_SUBTYPE = "0x40"


def board_partition_offset():
    """Offset of the 'board' partition, from firmware/partitions.csv."""
    lines = [l for l in (FIRMWARE / "partitions.csv").read_text().splitlines() if l.strip() and not l.startswith("#")]
    for row in csv.reader(lines):
        row = [c.strip() for c in row]
        if row[0] == "board" and row[2] == BOARD_PARTITION_SUBTYPE:
            return row[3]
    sys.exit("partitions.csv has no 'board' partition")


def record(data, name):
    return {"file": name, "sha256": hashlib.sha256(data).hexdigest(), "size": len(data)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--out", default=str(ROOT / "dist" / "firmware"))
    args = parser.parse_args()

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    catalog = json.loads((FIRMWARE / "boards" / "catalog.json").read_text())
    validator = Draft202012Validator(json.loads((ROOT / "spec" / "schemas" / "board.schema.json").read_text()))
    manifest = {"version": args.version, "images": {}, "boards": {}}

    for image_id, image in catalog["images"].items():
        build = FIRMWARE / "build" / image["build"]
        flasher_args = build / "flasher_args.json"
        if not flasher_args.exists():
            print(f"{image_id}: not built ({flasher_args} missing)")
            continue
        args_json = json.loads(flasher_args.read_text())
        parts = []
        for offset, rel in sorted(args_json["flash_files"].items(), key=lambda kv: int(kv[0], 16)):
            data = (build / rel).read_bytes()
            name = f"{image_id}-{Path(rel).name}"
            (out / name).write_bytes(data)
            parts.append({"offset": offset, **record(data, name)})
            print(f"{image_id}: {offset:>9} {name} {len(data)} bytes")
        manifest["images"][image_id] = {"chip": args_json["extra_esptool_args"]["chip"], "parts": parts,
                                        "definition_offset": board_partition_offset()}

    for board, entry in catalog["boards"].items():
        rec = {k: entry[k] for k in ("name", "image", "flash_size", "bridges", "note") if k in entry}
        definition_path = FIRMWARE / "boards" / "definitions" / f"{board}.json"
        ready = entry.get("status") == "supported" and entry["image"] in manifest["images"] and definition_path.exists()
        if ready:
            definition = json.loads(definition_path.read_text())
            errors = [e.message for e in validator.iter_errors(definition)]
            if errors:
                sys.exit(f"{definition_path}: {errors}")
            if definition["id"] != board:
                sys.exit(f"{definition_path}: id {definition['id']!r} is not {board!r}")
            data = json.dumps(definition, separators=(",", ":")).encode()
            name = f"board-{board}.json"
            (out / name).write_bytes(data)
            rec["definition"] = record(data, name)
            print(f"{board}: definition {len(data)} bytes")
        rec["status"] = "supported" if ready else "coming"
        manifest["boards"][board] = rec

    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"wrote {out / 'manifest.json'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

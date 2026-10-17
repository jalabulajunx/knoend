#!/usr/bin/env python3
"""Package firmware builds into a release folder with manifest.json.

For every board in firmware/boards/catalog.json whose build exists (firmware/build/<board>/), copy
the parts IDF lists in flasher_args.json, record offset, size and SHA-256, and write the manifest
that `openpager serve` reads. Boards without a build are listed with status "coming".

Usage: python3 tools/release_firmware.py --version 0.2.0 [--out dist/firmware]
Build first:  . ~/esp/esp-idf/export.sh && cd firmware && idf.py -B build/<board> build
"""

import argparse
import hashlib
import json
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIRMWARE = ROOT / "firmware"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--out", default=str(ROOT / "dist" / "firmware"))
    args = parser.parse_args()

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    catalog = json.loads((FIRMWARE / "boards" / "catalog.json").read_text())
    manifest = {"version": args.version, "images": {}, "boards": {}}

    for board, entry in catalog.items():
        build = FIRMWARE / "build" / board
        flasher_args = build / "flasher_args.json"
        record = {k: entry[k] for k in ("name", "flash_size", "bridges", "note") if k in entry}
        record["image"] = board
        if entry.get("status") != "supported" or not flasher_args.exists():
            record["status"] = "coming"
            manifest["boards"][board] = record
            print(f"{board}: no build, listed as coming")
            continue

        args_json = json.loads(flasher_args.read_text())
        parts = []
        for offset, rel in sorted(args_json["flash_files"].items(), key=lambda kv: int(kv[0], 16)):
            data = (build / rel).read_bytes()
            name = f"{board}-{Path(rel).name}"
            (out / name).write_bytes(data)
            parts.append({"offset": offset, "file": name, "sha256": hashlib.sha256(data).hexdigest(), "size": len(data)})
            print(f"{board}: {offset:>8} {name} {len(data)} bytes")
        manifest["images"][board] = {"chip": args_json["extra_esptool_args"]["chip"], "parts": parts}
        record["status"] = "supported"
        manifest["boards"][board] = record

    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"wrote {out / 'manifest.json'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

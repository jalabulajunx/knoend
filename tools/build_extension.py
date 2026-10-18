#!/usr/bin/env python3
"""Build the OpenPager browser extension for Chrome (and other Chromium browsers) and Firefox.

extension/manifest.json is shared; extension/chrome.json and extension/firefox.json add what
differs (background page, Chrome's public key that fixes the extension id, Firefox's add-on id).
Writes dist/extension/chrome/ and dist/extension/firefox/ plus a .zip of each.
"""

import json
import shutil
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "extension" / "src"
OUT = ROOT / "dist" / "extension"


def build(browser: str) -> Path:
    manifest = json.loads((ROOT / "extension" / "manifest.json").read_text())
    manifest.update(json.loads((ROOT / "extension" / f"{browser}.json").read_text()))
    target = OUT / browser
    if target.exists():
        shutil.rmtree(target)
    shutil.copytree(SRC, target)
    (target / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    archive = OUT / f"openpager-{browser}-{manifest['version']}.zip"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
        for path in sorted(target.rglob("*")):
            if path.is_file():
                z.write(path, path.relative_to(target))
    return target


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    for browser in ("chrome", "firefox"):
        print(f"{browser}: {build(browser).relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

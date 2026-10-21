#!/usr/bin/env python3
"""Build the KnoEnd browser extension for Chrome (and other Chromium browsers) and Firefox.

extension/manifest.json is shared; extension/chrome.json and extension/firefox.json add what
differs (background page, Chrome's public key that fixes the extension id, Firefox's add-on id).
Writes dist/extension/chrome/ and dist/extension/firefox/ plus a .zip of each.
`--version 0.2.1` sets the version (the release workflow passes the tag's).
"""

import json
import shutil
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "extension" / "src"
OUT = ROOT / "dist" / "extension"


def build(browser: str, version: str | None) -> Path:
    manifest = json.loads((ROOT / "extension" / "manifest.json").read_text())
    manifest.update(json.loads((ROOT / "extension" / f"{browser}.json").read_text()))
    if version:
        manifest["version"] = version
    target = OUT / browser
    if target.exists():
        shutil.rmtree(target)
    shutil.copytree(SRC, target)
    (target / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    archive = OUT / f"knoend-{browser}-{manifest['version']}.zip"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
        for path in sorted(target.rglob("*")):
            if path.is_file():
                z.write(path, path.relative_to(target))
    return target


def main() -> int:
    version = sys.argv[sys.argv.index("--version") + 1] if "--version" in sys.argv else None
    # Browsers take dotted numbers only: 0.2.1, not 0.2.1-3-gabc.
    if version and not all(part.isdigit() for part in version.split(".")):
        print(f"--version {version}: browsers take numbers and dots only, e.g. 0.2.1", file=sys.stderr)
        return 2
    OUT.mkdir(parents=True, exist_ok=True)
    for browser in ("chrome", "firefox"):
        print(f"{browser}: {build(browser, version).relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

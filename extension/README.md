# extension

The OpenPager browser extension for Chrome (and Chromium, Edge, Brave) and Firefox. It tells the
`openpager` service which website is active, so the board shows that site's keys, and carries out
those keys on the page: press a button, focus a field, control a video, open a page on the site.
The design and messages are in [`spec/browser.md`](../spec/browser.md).

- `src/`: the extension (background script, popup, icons), shared by both browsers.
- `manifest.json` + `chrome.json` / `firefox.json`: the manifest, with what differs per browser.
- Build: `python3 tools/build_extension.py` writes `dist/extension/chrome/` and
  `dist/extension/firefox/` (and a zip of each).

## Privacy

At install it can see which site is active and nothing on the page. In its popup you turn board
buttons on for one site at a time; the browser asks for that site's permission, and only then can the
extension act on the page or list its controls. It talks only to `openpager` on this computer, through
the browser's native messaging (no network port).

## Trying it

1. Register openpager with your browsers once: `openpager browser install` (writes a small file per
   browser that names the `openpager` binary; `openpager browser uninstall` removes them).
2. Add the extension:
   - **Chrome / Edge / Brave:** open `chrome://extensions`, switch on Developer mode, "Load
     unpacked", pick `dist/extension/chrome`. Its id is `echibclfkpicikodohehemgjefbpakpj` (fixed
     by the public key in `chrome.json`; the host only accepts that id).
   - **Firefox:** open `about:debugging#/runtime/this-firefox`, "Load Temporary Add-on", pick
     `dist/extension/firefox/manifest.json` (unsigned add-ons last until Firefox restarts).
3. Run `openpager serve`. The popup shows "connected".
4. Open a site that has a layout (YouTube is built in), open the popup, and turn board buttons on.

Tested: Chrome 152 (headless, loaded through DevTools' `Extensions.loadUnpacked`), against a
stand-in for the service: tab reports, page inventory, and media / focus / element / open actions.
Not yet tested: Firefox, a person's own browser profile, the popup's permission prompt.

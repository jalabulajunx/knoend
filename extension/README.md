# extension

The KnoEnd browser extension for Chrome (and Chromium, Edge, Brave) and Firefox. It tells the
`knoend` service which website is active, so the board shows that site's keys, and carries out
those keys on the page: press a button, focus a field, control a video, open a page on the site.
The design and messages are in [`spec/browser.md`](../spec/browser.md).

- `src/`: the extension (background script, popup, icons), shared by both browsers.
- `manifest.json` + `chrome.json` / `firefox.json`: the manifest, with what differs per browser.
- Build: `python3 tools/build_extension.py` writes `dist/extension/chrome/` and
  `dist/extension/firefox/` (and a zip of each).

## Privacy

At install it can see which site is active and nothing on the page. In its popup you turn board
buttons on for one site at a time; the browser asks for that site's permission, and only then can the
extension act on the page or list its controls. It talks only to `knoend` on this computer, through
the browser's native messaging (no network port).

## Trying it

1. Register knoend with your browsers once: `knoend browser install` (writes a small file per
   browser that names the `knoend` binary; `knoend browser uninstall` removes them).
2. Add the extension:
   - **Chrome / Edge / Brave:** open `chrome://extensions` (or `edge://extensions`), switch on
     Developer mode, "Load unpacked", pick `dist/extension/chrome`. Its id is
     `echibclfkpicikodohehemgjefbpakpj` (fixed by the public key in `chrome.json`; the host only
     accepts that id).
   - **Firefox:** open `about:debugging#/runtime/this-firefox`, "Load Temporary Add-on", pick
     `dist/extension/firefox/manifest.json`. A temporary add-on is removed when Firefox restarts.
     To keep it, have Mozilla sign it for self-distribution (free, automated, not listed publicly):
     upload `dist/extension/knoend-firefox-<version>.zip` at
     https://addons.mozilla.org/developers/addon/submit/distribution, choose "On your own", then
     open the signed `.xpi` it returns in Firefox. The add-on id is `knoend@jalabulajunx.github.io` (the id from before the rename, `openpager@jalabulajunx.github.io`, is still allowed),
     which is what `knoend browser install` allows.
3. Run `knoend serve`. The popup shows "connected".
4. Open a site, open the popup, and turn board buttons on. YouTube has keys built in (different
   ones on a video page); for any other site press "Make keys for this site with AI".

Tested, against a stand-in for the service and the real `knoend` host:

- Firefox 156 (headless, installed through WebDriver BiDi `webExtension.install`, host registered by
  `knoend browser install` in `~/.mozilla/native-messaging-hosts`): tab reports, inventory of a
  YouTube page (52 controls), media / focus / element / open actions; without the site granted,
  every page action and the inventory answer `not-granted` while `open` still works.
- Chrome 152 (headless, DevTools `Extensions.loadUnpacked`): the same actions; 35 controls.
- Microsoft Edge on Windows 11 with a real user profile and an ONX2432G028 board over Wi-Fi:
  `knoend browser install` (Windows Registry native messaging host + authenticated local TCP IPC),
  tab switching across Google Drive and Google Slides, AI site control inventory + drafting via
  Codex CLI, and end-to-end board keys controlling Google Slides editing and full-screen presenting.

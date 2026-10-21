The KnoEnd service for your computer, firmware for the board, and the browser extension.
The service installs the firmware for you: open its web app and follow the set-up.

## Install the service

- **Windows:** run `knoend-setup-<version>-x86_64.exe`. It installs for you only, needs no
  administrator rights, adds KnoEnd to the Start menu and, if you keep the boxes ticked, runs it in
  the background from sign-in and connects the browser extension. It is not code-signed yet:
  Windows SmartScreen says "unknown publisher"; choose **More info**, then **Run anyway**. The
  `.zip` is the same program without the installer.
- **macOS:** unpack `knoend-aarch64-apple-darwin.tar.gz` (Apple silicon) or
  `knoend-x86_64-apple-darwin.tar.gz` (Intel) and run `./knoend serve`; `./knoend service install`
  runs it from login. Not notarized yet: if macOS refuses to open it, run
  `xattr -d com.apple.quarantine knoend` once.
- **Linux:** unpack `knoend-x86_64-unknown-linux-gnu.tar.gz` and run `./knoend serve`;
  `./knoend service install` runs it from login (a systemd user service).

`knoend serve` opens the web app at http://127.0.0.1:7342/, where you install the firmware on your
board, choose its look and set up the AI.

## The browser extension (optional)

The board follows the website you are on. Not in the browser stores yet:

- **Chrome, Edge, Brave:** unzip `knoend-chrome-<version>.zip`, open `chrome://extensions`, turn on
  **Developer mode**, choose **Load unpacked** and pick the unzipped folder.
- **Firefox:** open `about:debugging`, **This Firefox**, **Load Temporary Add-on**, and pick
  `knoend-firefox-<version>.zip` (it stays until Firefox restarts, until the add-on is signed).

Then run `knoend browser install` (the Windows installer already did).

## Firmware

`manifest.json`, the universal ESP32-S3 image and one definition per supported board. The service
reads them from the latest release; there is nothing to download by hand.

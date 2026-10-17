# firmware

ESP-IDF v5.4.4 project (C).

- `main/` — core: grid UI, keys, protocol, BLE / Wi-Fi / serial transports, storage, web server
- `components/board_api/` — `board.h`: display, touch, audio, storage, battery, USB
- `components/board_onx2432g028/`, `components/board_fnk0104b/` — one profile per board
- `boards/` — `sdkconfig.defaults.<board>`

Hardware rule: one change per flash, tested on the board before the next.

## Build and flash

```bash
. ~/esp/esp-idf/export.sh
idf.py -B build/onx2432g028 build                               # ONX2432G028 (default)
idf.py -B build/fnk0104b -DOPENPAGER_BOARD=fnk0104b build       # FNK0104B (profile not written yet)
idf.py -B build/onx2432g028 -p /dev/ttyUSB0 flash
```

Each tap prints `KEY <1-9> seq=<n> t=<ms>` on the console (115200 baud on the ONX).

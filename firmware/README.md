# firmware

ESP-IDF v5.4.4 project (C).

- `main/` — core: grid UI, keys, protocol, BLE / Wi-Fi / serial transports, storage, web server
- `components/board_api/` — `board.h`: display, touch, audio, storage, battery, USB
- `components/board_onx2432g028/`, `components/board_fnk0104b/` — one profile per board
- `boards/` — `sdkconfig.defaults.<board>`

Hardware rule: one change per flash, tested on the board before the next.

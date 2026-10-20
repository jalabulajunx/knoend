#!/usr/bin/env python3
"""A stand-in for the companion service, for bench-testing the board's Wi-Fi link.

  pair     Over USB serial: hand the board a fresh pairing key (knoend.pair) and, if you
           type them, your Wi-Fi name and password (knoend.wifi). Run it yourself: it prompts
           for the password, which is sent to the board only and never stored or printed here.
  listen   Find the board over mDNS, connect, run the handshake from spec/protocol.md section 4,
           push a layout and a status message, then print every packet the board sends.
  attacks  Try three things the board must refuse: an unknown companion, a replayed handshake,
           and a tampered frame. Each must end with the board closing the connection.

The pairing key is kept in ~/.config/knoend/dummy-companion.json (mode 600). The real
companion will use the OS keyring instead.

`pair` needs pyserial; `listen` and `attacks` need websockets and zeroconf. On the bench these
live in different Pythons, so:
  ~/.espressif/python_env/idf5.4_py3.14_env/bin/python tools/dummy_companion.py pair
  python3 tools/dummy_companion.py listen
"""

import argparse
import asyncio
import getpass
import hashlib
import hmac
import json
import os
import secrets
import sys
import time
from pathlib import Path

CONFIG = Path.home() / ".config" / "knoend" / "dummy-companion.json"
EXAMPLES = Path(__file__).resolve().parent.parent / "spec" / "examples"


# ---- local state ----

def load_config():
    if CONFIG.exists():
        return json.loads(CONFIG.read_text())
    return {"companion": f"dummy-{secrets.token_hex(3)}", "companionName": "Dummy companion", "boards": {}}


def save_config(config):
    CONFIG.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(CONFIG, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        json.dump(config, f, indent=2)
    os.chmod(CONFIG, 0o600)


def mac(key, *parts):
    return hmac.new(key, "\n".join(parts).encode(), hashlib.sha256).hexdigest()


# ---- pair (serial) ----

def pair(args):
    import serial  # pyserial: in the ESP-IDF Python on the bench

    config = load_config()
    port = serial.Serial(args.port, 115200, timeout=0.1)
    buffer = b""
    next_id = [0]

    def lines(seconds):
        nonlocal buffer
        end = time.time() + seconds
        while time.time() < end:
            buffer += port.read(4096)
            while b"\n" in buffer:
                raw, buffer = buffer.split(b"\n", 1)
                yield raw.decode(errors="replace").rstrip("\r")

    def request(packet_type, body, reply_type, seconds=3.0):
        packet_id = next_id[0]
        next_id[0] += 1
        port.write(json.dumps({"id": packet_id, "type": packet_type, "body": body}).encode() + b"\n")
        for line in lines(seconds):
            if line.startswith("{"):
                packet = json.loads(line)
                if packet.get("type") in (reply_type, "knoend.error") and packet.get("re") == packet_id:
                    return packet
        return None

    for _ in lines(1.5):  # drain boot/console output
        pass

    identity = request("knoend.identity", {
        "deviceId": config["companion"], "deviceName": config["companionName"], "deviceType": "companion",
        "protocolVersion": 0, "incoming": ["knoend.key"], "outgoing": ["knoend.layout"]},
        "knoend.identity")
    if identity is None:
        sys.exit("No identity from the board. Is it on this port, running KnoEnd firmware?")
    board = identity["body"]["deviceId"]
    print(f"Board {board} ({identity['body']['board']['profile']}, firmware {identity['body'].get('firmware')})")

    key = secrets.token_bytes(32)
    ack = request("knoend.pair", {"companion": config["companion"], "companionName": config["companionName"],
                                     "key": key.hex()}, "knoend.pair.ack")
    if ack is None or not ack["body"].get("ok"):
        sys.exit(f"Pairing failed: {ack}")
    config["boards"][board] = key.hex()
    save_config(config)
    print(f"Paired: key stored for board {board} in {CONFIG} (mode 600)")

    ssid = input("Wi-Fi network name (Enter to skip): ").strip()
    if not ssid:
        return
    password = getpass.getpass(f"Password for {ssid!r} (not shown, not stored here): ")
    ack = request("knoend.wifi", {"ssid": ssid, "password": password}, "knoend.wifi.ack")
    password = None
    if ack is None or not ack["body"].get("ok"):
        sys.exit(f"Wi-Fi settings rejected: {ack}")
    print("Board accepted the Wi-Fi settings; watching it join (30 s):")
    for line in lines(30):
        if line.startswith("WIFI"):
            print(f"  {line}")
            if line.startswith("WIFI services up"):
                print("Ready. Now run: python3 tools/dummy_companion.py listen")
                return
    print("The board did not report its services within 30 s; check the reason codes above.")


# ---- Wi-Fi (discovery, handshake, frames) ----

def discover(config, seconds):
    """Browses _knoend._tcp; resolves names afterwards (zeroconf forbids blocking calls in
    its own callback thread)."""
    from zeroconf import ServiceBrowser, ServiceListener, Zeroconf

    service = "_knoend._tcp.local."
    names = set()

    class Listener(ServiceListener):
        def add_service(self, zc, type_, name):
            names.add(name)

        def update_service(self, zc, type_, name):
            names.add(name)

        def remove_service(self, zc, type_, name):
            pass

    zc = Zeroconf()
    ServiceBrowser(zc, service, Listener())
    found = {}
    end = time.time() + seconds
    while time.time() < end and not any(board in found for board in config["boards"]):
        time.sleep(0.5)
        for name in list(names):
            info = zc.get_service_info(service, name, timeout=3000)
            if info is None or not info.parsed_addresses():
                continue
            txt = {k.decode(): (v or b"").decode() for k, v in info.properties.items()}
            found[txt.get("id", "")] = (info.parsed_addresses()[0], info.port, txt, name)
    zc.close()
    for board, entry in found.items():
        print(f"Found {entry[3]} at {entry[0]}:{entry[1]} txt={entry[2]}"
              f"{'  (paired)' if board in config['boards'] else '  (not paired with this companion)'}")
    for board in config["boards"]:
        if board in found:
            return board, found[board][0], found[board][1]
    return None


class Session:
    """One authenticated connection: the handshake, then {ctr, mac, packet} frames."""

    def __init__(self, ws, key, companion):
        self.ws, self.key, self.companion = ws, key, companion
        self.session_id = None
        self.ctr_out, self.last_ctr_in, self.next_id = 0, -1, 0

    async def handshake(self, expected_board):
        nc = secrets.token_hex(16)
        await self.ws.send(json.dumps({"hello": {"companion": self.companion, "nonce": nc}}))
        hello = json.loads(await asyncio.wait_for(self.ws.recv(), 5))["hello"]
        nb = hello["nonce"]
        if hello["board"] != expected_board:
            raise RuntimeError(f"board id {hello['board']} is not {expected_board}")
        if not hmac.compare_digest(hello["proof"], mac(self.key, "board", nc, nb)):
            raise RuntimeError("board proof does not verify: wrong key, or not our board")
        proof = mac(self.key, "companion", nb, nc)
        await self.ws.send(json.dumps({"proof": proof}))
        self.session_id = nb + nc
        return {"nc": nc, "nb": nb, "proof": proof}

    async def send(self, packet_type, body):
        packet = json.dumps({"id": self.next_id, "type": packet_type, "body": body}, separators=(",", ":"))
        self.next_id += 1
        frame = {"ctr": self.ctr_out, "mac": mac(self.key, self.session_id, str(self.ctr_out), packet),
                 "packet": packet}
        self.ctr_out += 1
        await self.ws.send(json.dumps(frame))

    async def receive(self, timeout):
        frame = json.loads(await asyncio.wait_for(self.ws.recv(), timeout))
        if not hmac.compare_digest(frame["mac"], mac(self.key, self.session_id, str(frame["ctr"]), frame["packet"])):
            raise RuntimeError("frame from the board does not verify")
        if frame["ctr"] <= self.last_ctr_in:
            raise RuntimeError(f"board counter went backwards: {frame['ctr']}")
        self.last_ctr_in = frame["ctr"]
        return frame["packet"]


def packet_checker():
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from validate_spec import check_packet
    return check_packet


async def listen(args):
    import websockets

    config = load_config()
    if not config["boards"]:
        sys.exit("No paired board. Run the pair step over USB first.")
    target = await asyncio.to_thread(discover, config, args.discover)  # zeroconf blocks; keep it off the loop
    if target is None:
        sys.exit("No paired board found on the network (mDNS _knoend._tcp).")
    board, address, port = target
    check_packet = packet_checker()

    async with websockets.connect(f"ws://{address}:{port}/ws", max_size=None) as ws:
        session = Session(ws, bytes.fromhex(config["boards"][board]), config["companion"])
        await session.handshake(board)
        print("Handshake ok: both sides proved the key")

        identity = json.loads(await session.receive(5))
        print(f"Board identity over Wi-Fi: {identity['body']['deviceId']} firmware {identity['body'].get('firmware')}")

        layout = json.loads((EXAMPLES / "layout-amazon.json").read_text())["body"]
        await session.send("knoend.layout", layout)
        await session.send("knoend.status", {"text": "Dummy companion over Wi-Fi: tap a key", "level": "attention"})
        await session.send("knoend.ping", {})

        problems = 0
        end = time.time() + args.seconds
        print(f"Listening {args.seconds} s; tap keys on the board:")
        while time.time() < end:
            try:
                text = await session.receive(max(0.1, end - time.time()))
            except asyncio.TimeoutError:
                break
            errors = check_packet(text)
            problems += bool(errors)
            print(f"  {text}{'  INVALID: ' + '; '.join(errors) if errors else ''}")
        print(f"Done. {problems} packet(s) failed the schema.")


async def attacks(args):
    import websockets

    config = load_config()
    target = await asyncio.to_thread(discover, config, args.discover)  # zeroconf blocks; keep it off the loop
    if target is None:
        sys.exit("No paired board found on the network.")
    board, address, port = target
    url = f"ws://{address}:{port}/ws"
    key = bytes.fromhex(config["boards"][board])
    results = []

    async def closed_by_board(ws, seconds=5):
        try:
            while True:
                await asyncio.wait_for(ws.recv(), seconds)
        except websockets.ConnectionClosed:
            return True
        except asyncio.TimeoutError:
            return False

    # 1. A companion the board was never paired with.
    async with websockets.connect(url) as ws:
        await ws.send(json.dumps({"hello": {"companion": "stranger-0001", "nonce": secrets.token_hex(16)}}))
        results.append(("unknown companion is refused", await closed_by_board(ws)))

    # 2. Record a real handshake, then replay its proof on a new connection.
    async with websockets.connect(url) as ws:
        session = Session(ws, key, config["companion"])
        recorded = await session.handshake(board)
    async with websockets.connect(url) as ws:
        await ws.send(json.dumps({"hello": {"companion": config["companion"], "nonce": recorded["nc"]}}))
        await ws.recv()  # the board answers with a fresh nonce...
        await ws.send(json.dumps({"proof": recorded["proof"]}))  # ...so the old proof must fail
        results.append(("replayed handshake is refused", await closed_by_board(ws)))

    # 3. A valid session, then a frame whose packet was changed after signing.
    async with websockets.connect(url) as ws:
        session = Session(ws, key, config["companion"])
        await session.handshake(board)
        await session.receive(5)  # identity
        packet = json.dumps({"id": 0, "type": "knoend.ping", "body": {}}, separators=(",", ":"))
        good_mac = mac(key, session.session_id, "0", packet)
        tampered = packet.replace("ping", "pong")
        await ws.send(json.dumps({"ctr": 0, "mac": good_mac, "packet": tampered}))
        results.append(("tampered frame is refused", await closed_by_board(ws)))

    for name, ok in results:
        print(f"{'ok  ' if ok else 'FAIL'} {name}")
    return 0 if all(ok for _, ok in results) else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("pair")
    p.add_argument("--port", default="/dev/ttyUSB0")
    p = sub.add_parser("listen")
    p.add_argument("--seconds", type=int, default=120)
    p.add_argument("--discover", type=float, default=10)
    p = sub.add_parser("attacks")
    p.add_argument("--discover", type=float, default=10)
    args = parser.parse_args()

    if args.command == "pair":
        pair(args)
        return 0
    if args.command == "listen":
        asyncio.run(listen(args))
        return 0
    return asyncio.run(attacks(args))


if __name__ == "__main__":
    sys.exit(main())

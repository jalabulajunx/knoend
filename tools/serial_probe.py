#!/usr/bin/env python3
"""Bench test for the board's serial protocol (spec/protocol.md), over USB-C.

1. Sends a scripted sequence (identity, ping, layouts good and bad, status, pairing good and bad,
   malformed packets, an unknown type, an oversized line) and checks each reply: the right type,
   `re` pointing at the request, `ok` true or false as expected.
2. Leaves a bench-test layout on the board and listens, printing every packet, while a person taps.
3. Writes the whole transcript to a file; check it against the schemas afterwards with
   `python3 tools/validate_spec.py --packets <transcript>`.

Needs pyserial (the ESP-IDF Python has it):
  ~/.espressif/python_env/idf5.4_py3.14_env/bin/python tools/serial_probe.py \
      --port /dev/ttyUSB0 --listen 120 --transcript probe.log
"""

import argparse
import json
import secrets
import sys
import time
from pathlib import Path

import serial

EXAMPLES = Path(__file__).resolve().parent.parent / "spec" / "examples"

BENCH_LAYOUT = {
    "layout": "test.bench", "name": "Bench test", "source": "user", "reviewed": False,
    "keys": [
        {"key": 1, "label": "A", "send": {"hid": {"modifiers": 0, "usage": 0x04}}},
        {"key": 2, "label": "Hello", "send": {"event": True}},
        {"key": 3, "label": "Long label wraps onto 2", "send": {"event": True}},
        {"key": 5, "label": "Delete", "confirm": True, "send": {"event": True}},
        {"key": 7, "label": "B", "send": {"hid": {"modifiers": 0, "usage": 0x05}}},
        {"key": 9, "label": "Shift C", "send": {"hid": {"modifiers": 2, "usage": 0x06}}},
    ],
}


class Board:
    def __init__(self, port, transcript):
        self.serial = serial.Serial(port, 115200, timeout=0.1)
        self.transcript = open(transcript, "w")
        self.next_id = 100
        self.buffer = b""

    def log(self, direction, line):
        self.transcript.write(f"{line}\n" if direction == "in" else f"# sent: {line}\n")
        self.transcript.flush()

    def send_raw(self, line):
        self.log("out", line[:200])
        self.serial.write(line.encode() + b"\n")

    def send(self, packet_type, body):
        packet_id = self.next_id
        self.next_id += 1
        self.send_raw(json.dumps({"id": packet_id, "type": packet_type, "body": body}))
        return packet_id

    def lines(self, seconds):
        """Yields lines received within `seconds`."""
        end = time.time() + seconds
        while time.time() < end:
            self.buffer += self.serial.read(4096)
            while b"\n" in self.buffer:
                raw, self.buffer = self.buffer.split(b"\n", 1)
                line = raw.decode(errors="replace").rstrip("\r")
                self.log("in", line)
                yield line

    def packets(self, seconds):
        for line in self.lines(seconds):
            if line.startswith("{"):
                try:
                    yield json.loads(line)
                except json.JSONDecodeError:
                    yield {"type": "(unparseable)", "raw": line}

    def expect(self, reply_type, re, seconds=2.0):
        for packet in self.packets(seconds):
            if packet.get("type") == reply_type and (re is None or packet.get("re") == re):
                return packet
        return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", default="/dev/ttyUSB0")
    parser.add_argument("--listen", type=int, default=120, help="seconds to listen for taps")
    parser.add_argument("--transcript", default="serial-probe.log")
    args = parser.parse_args()

    board = Board(args.port, args.transcript)
    results = []

    def check(name, ok, detail=""):
        results.append(ok)
        print(f"{'ok  ' if ok else 'FAIL'} {name}{'  - ' + detail if detail else ''}")

    for _ in board.packets(2.0):  # drain whatever the board printed on opening the port
        pass

    example = lambda name: json.loads((EXAMPLES / name).read_text())

    rid = board.send("openpager.identity", example("identity-companion.json")["body"])
    reply = board.expect("openpager.identity", rid)
    check("identity answered with the board's identity", reply is not None
          and reply["body"].get("deviceType") == "board",
          json.dumps(reply["body"])[:160] if reply else "no reply")

    rid = board.send("openpager.ping", {})
    check("ping answered", board.expect("openpager.ping", rid) is not None)

    for name in ("layout-vscode.json", "layout-amazon.json"):
        rid = board.send("openpager.layout", example(name)["body"])
        reply = board.expect("openpager.layout.ack", rid)
        check(f"{name} accepted", reply is not None and reply["body"].get("ok") is True,
              json.dumps(reply["body"]) if reply else "no reply")

    bad_layouts = {
        "slot used twice": {"layout": "test.bad", "name": "Bad", "keys": [
            {"key": 1, "label": "One", "send": {"event": True}},
            {"key": 1, "label": "Again", "send": {"event": True}}]},
        "ten keys": {"layout": "test.bad", "name": "Bad", "keys": [
            {"key": (i % 9) + 1, "label": str(i), "send": {"event": True}} for i in range(10)]},
        "unknown send": {"layout": "test.bad", "name": "Bad", "keys": [
            {"key": 1, "label": "Run", "send": {"shell": "rm -rf ~"}}]},
        "label over 24 characters": {"layout": "test.bad", "name": "Bad", "keys": [
            {"key": 1, "label": "x" * 25, "send": {"event": True}}]},
    }
    for name, body in bad_layouts.items():
        rid = board.send("openpager.layout", body)
        reply = board.expect("openpager.layout.ack", rid)
        check(f"layout rejected: {name}", reply is not None and reply["body"].get("ok") is False,
              reply["body"].get("error", "") if reply else "no reply")

    rid = board.send("openpager.pair", {"companion": "serial-probe", "companionName": "Probe",
                                        "key": secrets.token_hex(32)})
    reply = board.expect("openpager.pair.ack", rid)
    check("pairing key accepted over serial", reply is not None and reply["body"].get("ok") is True,
          json.dumps(reply["body"]) if reply else "no reply")

    rid = board.send("openpager.pair", {"companion": "serial-probe", "key": "00ff"})
    reply = board.expect("openpager.pair.ack", rid)
    check("pairing rejected: short key", reply is not None and reply["body"].get("ok") is False,
          reply["body"].get("error", "") if reply else "no reply")

    board.send_raw("{not json")
    reply = board.expect("openpager.error", None)
    check("malformed JSON gets bad-packet", reply is not None and reply["body"].get("code") == "bad-packet")

    board.send_raw(json.dumps({"id": 150, "type": "openpager.ping"}))
    reply = board.expect("openpager.error", 150)
    check("packet without body gets bad-packet", reply is not None and reply["body"].get("code") == "bad-packet")

    board.send("openpager.approve.request", example("approve-request.json")["body"])
    unexpected = [p for p in board.packets(1.5)]
    check("unknown/unsupported type is ignored", not unexpected,
          f"got {unexpected}" if unexpected else "")

    board.send_raw('{"id": 160, "type": "openpager.ping", "body": {"pad": "' + "x" * 5000 + '"}}')
    reply = board.expect("openpager.error", None, seconds=3.0)
    check("oversized line gets too-large", reply is not None and reply["body"].get("code") == "too-large")

    rid = board.send("openpager.ping", {})
    check("still answering after the bad input", board.expect("openpager.ping", rid) is not None)

    rid = board.send("openpager.layout", BENCH_LAYOUT)
    reply = board.expect("openpager.layout.ack", rid)
    check("bench layout accepted", reply is not None and reply["body"].get("ok") is True)
    board.send("openpager.status", {"text": "Serial probe: tap the keys", "level": "attention"})

    print(f"\n{sum(results)}/{len(results)} automatic checks passed. Listening {args.listen} s for taps:")
    for line in board.lines(args.listen):
        if line.startswith("{") or line.startswith("KEY") or line.startswith("BLE"):
            print(f"  {line}")
    print(f"Transcript: {args.transcript}")
    return 0 if all(results) else 1


if __name__ == "__main__":
    sys.exit(main())

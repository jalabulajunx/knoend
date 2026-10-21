#!/usr/bin/env python3
"""Bench check for listening (spec/protocol.md 3.6f): over USB serial, asks the board to listen and
prints the notes it reports, with the board's own log lines. No companion needed.
Opening the port restarts some boards (the ONX2432G028): the script waits for it to start.
Usage: python3 tools/listen_test.py [--port /dev/ttyUSB0] [--seconds 30] [--quiet -50]
Needs pyserial (in ESP-IDF's Python: . ~/esp/esp-idf/export.sh)."""
import argparse
import json
import time

import serial

NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]


def name(midi):
    return f"{NAMES[midi % 12]}{midi // 12 - 1}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", default="/dev/ttyUSB0")
    ap.add_argument("--seconds", type=int, default=30)
    ap.add_argument("--quiet", type=int, default=-50)
    args = ap.parse_args()
    link = serial.Serial()
    link.port, link.baudrate, link.timeout = args.port, 115200, 0.5
    link.dtr = link.rts = False
    link.open()
    started = time.time()
    seen_identity = False
    while time.time() - started < 8 and not seen_identity:
        line = link.readline().decode(errors="replace").strip()
        if '"identity"' in line:
            seen_identity = True
            body = json.loads(line[line.index("{"):])["body"]
            print("board:", body["board"]["profile"], body["firmware"], "mic:", body["features"].get("mic"))
    if not seen_identity:
        link.write(b'{"id":1,"type":"knoend.identity","body":{"deviceName":"listen_test"}}\n')
    link.write(json.dumps({"id": 2, "type": "knoend.listen",
                           "body": {"on": True, "seconds": args.seconds, "quiet": args.quiet}}).encode() + b"\n")
    print(f"listening for {args.seconds} s: sing or play notes")
    notes = []
    until = time.time() + args.seconds + 3
    while time.time() < until:
        line = link.readline().decode(errors="replace").strip()
        if not line:
            continue
        if line.startswith("{"):
            packet = json.loads(line)
            kind, body = packet.get("type", ""), packet.get("body", {})
            if kind.endswith(".note"):
                if body.get("on"):
                    notes.append(name(body["midi"]))
                    print(f"  NOTE ON  {name(body['midi']):4} midi {body['midi']} {body.get('hz')} Hz velocity {body.get('velocity')}")
                else:
                    print(f"  note off {name(body['midi'])}")
            elif kind.endswith(".listen.ack") or kind.endswith(".error"):
                print(" ", kind, body)
                if kind.endswith(".listen.ack") and body.get("on") is False:
                    break
        elif "listen" in line or "microphone" in line or "Guru" in line or "panic" in line:
            print("  board:", line)
    print("notes heard:", " ".join(notes) or "none")


if __name__ == "__main__":
    main()

# Listening for notes, and MIDI to music apps

- **What:** a board with a microphone (board definition `mic`, identity `features.mic`) listens
  while asked (`knoend.listen`) and reports notes (`knoend.note`), spec/protocol.md 3.6f. A library
  key `action.kind = "listen"` starts and stops it. The companion is a MIDI device called KnoEnd and
  sends each note on; on Linux it also sends into Midi Through.
- **Why:** a musician plays or sings and the note lands in the score (MuseScore in note input), from
  any room the board's Wi-Fi reaches. MIDI is what every notation program and DAW already reads, with
  exact pitch and octave, which keystrokes are not.
- **Pitch on the board, not on the computer:** only notes leave the board, never sound. That keeps
  Wi-Fi traffic tiny and answers "is it recording me?" plainly. YIN, written for this firmware and
  tested on the computer (`firmware/test/run_pitch_test.sh`); at 8 kHz it costs about 20% of one core
  while there is sound.
- **Alternatives rejected:** streaming audio to the computer (bandwidth, privacy, latency over
  Wi-Fi); typing note letters into MuseScore (no octave, MuseScore-only); a choice of microphone slot
  in the definition (the board now picks the slot with sound in it).
- **Not decided yet:** pluggable connectors for apps beyond keystrokes and MIDI (a separate program
  per app speaking a small fixed message format, approved by the person, possibly MCP). Proposed in
  the 2026-10-21 log entry; needs Tom.
- Spec changes (3.6f, board schema `mic`, library action `listen`) need Tom's review.

//! Notes the board hears (it listens through its microphone: spec/protocol.md 3.6f) go out as
//! MIDI, so any music app takes them as if from a keyboard: MuseScore and Dorico in note input,
//! a DAW recording. On Linux and macOS the service is itself a MIDI device called "KnoEnd" (a
//! virtual port). On Linux the notes also go into "Midi Through", which MuseScore and most Linux
//! music apps listen to unless told otherwise, so nothing needs setting there. Windows has no virtual ports without a driver: there the notes go to an output
//! whose name contains "KnoEnd" or "loopMIDI" (made with the free loopMIDI), else they are not sent
//! and the service says why.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use midir::{MidiOutput, MidiOutputConnection};

const NAME: &str = "KnoEnd";

static OUT: Mutex<Option<MidiOutputConnection>> = Mutex::new(None);
/// Direct connections to music apps' own MIDI inputs (Linux, macOS), so they need no setting.
static APPS: Mutex<Vec<(String, MidiOutputConnection)>> = Mutex::new(Vec::new());
static LISTENING: AtomicBool = AtomicBool::new(false);

/// Whether the board says it is listening now (its `knoend.listen.ack`).
pub fn listening() -> bool {
    LISTENING.load(Ordering::Relaxed)
}

pub fn set_listening(on: bool) {
    LISTENING.store(on, Ordering::Relaxed);
}

/// Opens the MIDI output if it is not open. Err says why notes cannot be sent.
pub fn open() -> Result<(), String> {
    let mut out = OUT.lock().map_err(|_| "MIDI lock")?;
    if out.is_some() {
        return Ok(());
    }
    let midi = MidiOutput::new(NAME).map_err(|e| format!("MIDI is not available: {e}"))?;
    #[cfg(unix)]
    {
        use midir::os::unix::VirtualOutput;
        let connection = midi.create_virtual(NAME).map_err(|e| format!("could not make the KnoEnd MIDI device: {e}"))?;
        say!("MIDI: the KnoEnd MIDI device is ready; choose \"KnoEnd\" as the MIDI input in the music app");
        *out = Some(connection);
        drop(out);
        #[cfg(target_os = "linux")]
        connect_app("midi through");
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let ports = midi.ports();
        let names: Vec<String> = ports.iter().map(|p| midi.port_name(p).unwrap_or_default()).collect();
        let at = names.iter().position(|n| n.contains(NAME)).or_else(|| names.iter().position(|n| n.to_lowercase().contains("loopmidi")))
            .ok_or_else(|| format!("no MIDI output for the notes: make a loopMIDI port called \"KnoEnd\" (outputs now: {})",
                                   if names.is_empty() { "none".to_string() } else { names.join(", ") }))?;
        let name = names[at].clone();
        let connection = midi.connect(&ports[at], NAME).map_err(|e| format!("could not open the MIDI output {name}: {e}"))?;
        say!("MIDI: notes go to {name}");
        *out = Some(connection);
        Ok(())
    }
}

/// Connects the notes straight to the MIDI inputs of the app called `app` ("MuseScore": any input
/// whose name contains it, ignoring case), so the person does not have to choose KnoEnd in the app.
/// Returns the inputs now connected; none is not an error (the app may not have one open, or
/// another app is in front): the KnoEnd device still works when chosen in the app.
pub fn connect_app(app: &str) -> Vec<String> {
    let wanted = app.to_lowercase();
    let Ok(mut apps) = APPS.lock() else { return Vec::new() };
    // Kept: Midi Through (Linux) and the inputs of this app.
    apps.retain(|(name, _)| name.to_lowercase().contains(&wanted) || name.to_lowercase().contains("midi through"));
    let Ok(midi) = MidiOutput::new(NAME) else { return Vec::new() };
    let ports = midi.ports();
    for port in &ports {
        let name = midi.port_name(port).unwrap_or_default();
        if wanted.is_empty() || !name.to_lowercase().contains(&wanted) || apps.iter().any(|(n, _)| n == &name) {
            continue;
        }
        let Ok(midi) = MidiOutput::new(NAME) else { continue };
        match midi.connect(port, NAME) {
            Ok(connection) => {
                say!("MIDI: notes go straight to {name}");
                apps.push((name, connection));
            }
            Err(e) => say!("MIDI: could not connect to {name}: {e}"),
        }
    }
    apps.iter().map(|(n, _)| n.clone()).collect()
}

/// Sends a note starting (`velocity` 1-127) or stopping, on channel 1.
pub fn note(midi: u8, on: bool, velocity: u8) -> Result<(), String> {
    open()?;
    let mut out = OUT.lock().map_err(|_| "MIDI lock")?;
    let connection = out.as_mut().ok_or("MIDI is not open")?;
    let message = if on { [0x90, midi & 0x7f, velocity.clamp(1, 127)] } else { [0x80, midi & 0x7f, 0] };
    connection.send(&message).map_err(|e| format!("MIDI: {e}"))?;
    if let Ok(mut apps) = APPS.lock() {
        // An app that quit: its connection fails; it is dropped and found again at the next Listen.
        apps.retain_mut(|(_, c)| c.send(&message).is_ok());
    }
    Ok(())
}

/// The word an app's MIDI input is named by, from its window's app name: its leading letters
/// ("MuseScore4" -> "musescore"), or None when that is too short to pick out the app.
pub fn app_word(app: &str) -> Option<String> {
    let word: String = app.chars().take_while(|c| c.is_alphabetic()).collect::<String>().to_lowercase();
    (word.chars().count() >= 4).then_some(word)
}

/// The name of a MIDI note ("C4" for 60).
pub fn name(midi: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[usize::from(midi % 12)], i16::from(midi / 12) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_named_as_musicians_write_them() {
        assert_eq!(name(60), "C4");
        assert_eq!(name(69), "A4");
        assert_eq!(name(61), "C#4");
        assert_eq!(name(21), "A0");
    }

    #[test]
    fn an_app_is_found_by_the_leading_word_of_its_name() {
        assert_eq!(app_word("MuseScore4").as_deref(), Some("musescore"));
        assert_eq!(app_word("Dorico").as_deref(), Some("dorico"));
        assert_eq!(app_word("x1"), None, "too short to pick out an app");
    }
}

//! Platforms without keystroke injection yet.

pub struct Typist;

impl Typist {
    pub fn connect() -> Result<Typist, String> {
        Err("typing keystrokes for the board is not supported on this OS yet".into())
    }
    pub fn can_type(&self, _usage: u8) -> bool {
        false
    }
    pub fn tap(&self, _modifiers: u8, _usage: u8) -> Result<(), String> {
        Err("not supported on this OS".into())
    }
}

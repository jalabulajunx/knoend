//! Lists which protocol keys this keyboard layout can type (sends no keystrokes):
//! `cargo run -p openpager-input --example check_layout`
fn main() {
    let typist = openpager_input::Typist::connect().expect("typist");
    let usages: Vec<u8> = (0x04..=0x38).chain(0x3A..=0x45).chain(0x49..=0x52).chain(0x68..=0x73).collect();
    let missing: Vec<String> = usages.iter().filter(|&&u| openpager_input::key_for_usage(u).is_some() && !typist.can_type(u))
        .map(|u| format!("{:?}", openpager_input::key_for_usage(*u).unwrap())).collect();
    let total = usages.iter().filter(|&&u| openpager_input::key_for_usage(u).is_some()).count();
    println!("{} of {} keys typeable; missing: {:?}", total - missing.len(), total, missing);
}

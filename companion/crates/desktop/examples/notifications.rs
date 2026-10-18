//! Prints desktop notifications as apps post them: `cargo run -p openpager-desktop --example notifications`
fn main() {
    let result = openpager_desktop::watch_notifications(|n| {
        println!("{} [{}]: {} | {}", n.app, n.desktop_entry, n.summary, n.body);
        true
    });
    if let Err(e) = result {
        eprintln!("{e}");
    }
}

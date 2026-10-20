//! Prints desktop notifications as apps post them: `cargo run -p knoend-desktop --example notifications`
fn main() {
    let result = knoend_desktop::watch_notifications(|n| {
        println!("{} [{}]: {} | {}", n.app, n.desktop_entry, n.summary, n.body);
        true
    });
    if let Err(e) = result {
        eprintln!("{e}");
    }
}

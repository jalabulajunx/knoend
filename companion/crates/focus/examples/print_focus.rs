//! Prints the focused app now, then each change, for 20 seconds: `cargo run --example print_focus`.
fn main() {
    let watcher = openpager_focus::Watcher::connect().expect("focus watcher");
    let mut last = watcher.current();
    println!("now: {last:?}");
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < 20 {
        last = watcher.wait_for_change(last.as_ref());
        println!("changed: {last:?}");
    }
}

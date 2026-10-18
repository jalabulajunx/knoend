//! Prints every media player and what it is playing: `cargo run -p openpager-media --example now_playing`
fn main() {
    match openpager_media::Players::connect() {
        Ok(players) => {
            for p in players.list() {
                println!("{} [{}] {:?} by {:?} ({}) seek={} next={} prev={}", p.player, p.status, p.title, p.artist, p.url,
                         p.can_seek, p.can_go_next, p.can_go_previous);
            }
        }
        Err(e) => eprintln!("{e}"),
    }
}

//! Sends one command to one player: `cargo run -p knoend-media --example command -- <bus> playpause|seek10`
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let players = knoend_media::Players::connect().expect("D-Bus");
    let command = match args[2].as_str() {
        "playpause" => knoend_media::Command::PlayPause,
        "seek10" => knoend_media::Command::Seek(10),
        other => panic!("{other}"),
    };
    println!("{:?}", players.command(&args[1], command));
}

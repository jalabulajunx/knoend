//! MPRIS over the session D-Bus (blocking calls; the service runs them off its async threads).

use std::collections::HashMap;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

use crate::{Command, NowPlaying};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";

pub struct Players {
    connection: Connection,
}

fn text(value: Option<&OwnedValue>) -> String {
    match value.map(|v| &**v) {
        Some(Value::Str(s)) => s.to_string(),
        Some(Value::Array(items)) => items.iter().filter_map(|i| match i {
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        }).collect::<Vec<_>>().join(", "),
        _ => String::new(),
    }
}

impl Players {
    pub fn connect() -> Result<Players, String> {
        Connection::session().map(|connection| Players { connection }).map_err(|e| format!("D-Bus session: {e}"))
    }

    /// Every MPRIS player (not playerctld, which only mirrors another one).
    pub fn list(&self) -> Vec<NowPlaying> {
        let names: Vec<String> = Proxy::new(&self.connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus")
            .and_then(|p| p.call("ListNames", &()))
            .unwrap_or_default();
        names.into_iter()
            .filter(|n| n.starts_with(PREFIX) && !n.ends_with(".playerctld"))
            .filter_map(|bus| self.read(&bus))
            .collect()
    }

    fn read(&self, bus: &str) -> Option<NowPlaying> {
        let player = Proxy::new(&self.connection, bus, PATH, "org.mpris.MediaPlayer2.Player").ok()?;
        let root = Proxy::new(&self.connection, bus, PATH, "org.mpris.MediaPlayer2").ok()?;
        let metadata: HashMap<String, OwnedValue> = player.get_property("Metadata").unwrap_or_default();
        let flag = |name: &str| player.get_property::<bool>(name).unwrap_or(false);
        Some(NowPlaying {
            bus: bus.to_string(),
            player: bus[PREFIX.len()..].split('.').next().unwrap_or("").to_string(),
            identity: root.get_property::<String>("Identity").unwrap_or_default(),
            desktop_entry: root.get_property::<String>("DesktopEntry").unwrap_or_default(),
            status: player.get_property::<String>("PlaybackStatus").unwrap_or_default(),
            title: text(metadata.get("xesam:title")),
            artist: text(metadata.get("xesam:artist")),
            url: text(metadata.get("xesam:url")),
            can_seek: flag("CanSeek"),
            can_go_next: flag("CanGoNext"),
            can_go_previous: flag("CanGoPrevious"),
        })
    }

    pub fn command(&self, bus: &str, command: Command) -> Result<(), String> {
        let player = Proxy::new(&self.connection, bus, PATH, "org.mpris.MediaPlayer2.Player").map_err(|e| e.to_string())?;
        let result: zbus::Result<()> = match command {
            Command::PlayPause => player.call("PlayPause", &()),
            Command::Play => player.call("Play", &()),
            Command::Pause => player.call("Pause", &()),
            Command::Next => player.call("Next", &()),
            Command::Previous => player.call("Previous", &()),
            Command::Seek(seconds) => player.call("Seek", &(seconds * 1_000_000)),
        };
        result.map_err(|e| e.to_string())
    }
}

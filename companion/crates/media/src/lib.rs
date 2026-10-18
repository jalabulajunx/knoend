//! What is playing, and controlling it: Linux's MPRIS media players over D-Bus (what KDE Connect
//! uses). Browsers publish each playing tab there (title, artist, play state), as do Spotify, VLC
//! and most players. Commands arrive like the keyboard's media keys, so a browser lets them start
//! playback, which a script on the page may not do.

use serde::Serialize;

/// One media player and what it is playing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NowPlaying {
    /// D-Bus name, e.g. "org.mpris.MediaPlayer2.firefox.instance_1_39078".
    pub bus: String,
    /// The player's short name ("firefox", "spotify", "chromium").
    pub player: String,
    /// The player's own name for itself ("Mozilla firefox") and its desktop entry ("firefox").
    pub identity: String,
    pub desktop_entry: String,
    /// "Playing", "Paused" or "Stopped".
    pub status: String,
    pub title: String,
    pub artist: String,
    /// The page or file, when the player gives one.
    pub url: String,
    pub can_seek: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
}

impl NowPlaying {
    pub fn playing(&self) -> bool {
        self.status == "Playing"
    }

    /// Whether this player belongs to the focused app (`app`/`instance` as the focus watcher
    /// reports them, e.g. "firefox"/"Navigator", "Google-chrome"/"google-chrome", "Spotify").
    pub fn belongs_to(&self, app: &str, instance: &str) -> bool {
        let focus: Vec<String> = [app, instance].iter().map(|s| s.to_ascii_lowercase()).filter(|s| !s.is_empty()).collect();
        let mine: Vec<String> = [&self.player, &self.identity, &self.desktop_entry].iter()
            .map(|s| s.to_ascii_lowercase()).filter(|s| !s.is_empty()).collect();
        let chromium = |s: &str| ["chrome", "chromium", "edge", "brave"].iter().any(|c| s.contains(c));
        focus.iter().any(|f| mine.iter().any(|m| m == f || f.ends_with(&format!(".{m}")) || (chromium(f) && chromium(m))))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
    /// Seek by this many seconds (negative: back).
    Seek(i64),
}

/// The player to show and control for the focused app: its own, playing ones first.
pub fn for_focus<'a>(players: &'a [NowPlaying], app: &str, instance: &str) -> Option<&'a NowPlaying> {
    players.iter().filter(|p| p.belongs_to(app, instance)).max_by_key(|p| (p.playing(), p.status == "Paused"))
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Players;

#[cfg(not(target_os = "linux"))]
pub struct Players;

#[cfg(not(target_os = "linux"))]
impl Players {
    pub fn connect() -> Result<Players, String> {
        Err("media players are read through MPRIS, which only Linux has".into())
    }
    pub fn list(&self) -> Vec<NowPlaying> {
        Vec::new()
    }
    pub fn command(&self, _bus: &str, _command: Command) -> Result<(), String> {
        Err("not supported on this OS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(player: &str, identity: &str, entry: &str, status: &str) -> NowPlaying {
        NowPlaying { bus: format!("org.mpris.MediaPlayer2.{player}"), player: player.into(), identity: identity.into(),
                     desktop_entry: entry.into(), status: status.into(), title: String::new(), artist: String::new(),
                     url: String::new(), can_seek: true, can_go_next: false, can_go_previous: false }
    }

    #[test]
    fn players_are_matched_to_the_focused_app() {
        let firefox = player("firefox", "Mozilla firefox", "firefox", "Paused");
        assert!(firefox.belongs_to("firefox", "Navigator"));
        assert!(firefox.belongs_to("org.mozilla.firefox", "Firefox"));
        assert!(!firefox.belongs_to("Code", "code"));
        let chrome = player("chromium", "Chrome", "google-chrome", "Playing");
        assert!(chrome.belongs_to("Google-chrome", "google-chrome"));
        let spotify = player("spotify", "Spotify", "spotify", "Playing");
        assert!(spotify.belongs_to("Spotify", "spotify"));
    }

    #[test]
    fn the_playing_player_wins() {
        let players = vec![player("firefox", "Mozilla firefox", "firefox", "Paused"),
                           NowPlaying { bus: "b2".into(), ..player("firefox", "Mozilla firefox", "firefox", "Playing") }];
        assert_eq!(for_focus(&players, "firefox", "Navigator").unwrap().bus, "b2");
        assert!(for_focus(&players, "Code", "code").is_none());
    }
}

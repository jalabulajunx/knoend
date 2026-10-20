//! Edit mode (#29): the web app changes a library while the board shows it, page by page, instead
//! of following focus. Every change reaches the board at once, unsaved; a tap on the board selects
//! a key in the web app. Saving writes a user library (`user.<name>`) that layers on the recipe or
//! AI draft it came from (`pack.overrides`); that one stays, underneath.

use std::path::{Path, PathBuf};

use knoend_core::library::{Library, Source};
use knoend_core::protocol::Grid;
use serde_json::{json, Value};

/// What the web app asks the service loop to do.
pub enum Command {
    /// Every library there is, for the picker.
    List,
    /// Start editing (or switch to) the library with this id; None: the one on the board.
    Start(Option<String>),
    /// The library as edited now (checked, then shown on the board).
    Change(Value),
    /// Show this page (0-based) of the library being edited.
    Page(usize),
    Save,
    /// Back to the last saved version.
    Revert,
    /// Delete the person's own version; edit the one underneath.
    Remove,
    Stop,
}

pub struct Edit {
    /// The library as edited so far, unsaved changes included.
    pub library: Library,
    /// As last saved, or as it was when editing started.
    pub saved: Library,
    /// The person's own file for it; None until a recipe or draft is first saved.
    pub path: Option<PathBuf>,
}

impl Edit {
    pub fn start(library: &Library, path: &Path) -> Edit {
        let mut library = library.clone();
        pin(&mut library);
        let own = library.pack.source == Source::User && path.is_file();
        Edit { saved: library.clone(), library, path: own.then(|| path.to_path_buf()) }
    }

    pub fn dirty(&self) -> bool {
        serde_json::to_value(&self.library).ok() != serde_json::to_value(&self.saved).ok()
    }

    /// For the web app: the library, its pages (those with keys, as the board shows them), the page
    /// shown and the largest grid the board draws.
    pub fn state(&self, page: usize, max: Grid) -> Value {
        let pages: Vec<Value> = page_numbers(&self.library).into_iter().map(|n| {
            let grid = self.library.grid(n);
            json!({"number": n, "grid": format!("{}x{}", grid.cols, grid.rows)})
        }).collect();
        json!({"library": self.library, "pages": pages, "page": page, "max": max, "dirty": self.dirty(),
               "own": self.path.is_some(), "overrides": self.library.pack.overrides})
    }

    /// Writes the person's own version into `dir` (`user-<name>.toml`, id `user.<name>`), or over
    /// their existing file. Returns the file and the library as saved.
    pub fn save(&mut self, dir: &Path) -> Result<(PathBuf, Library), String> {
        let mut library = self.library.clone();
        let path = match &self.path {
            Some(path) => path.clone(),
            None => {
                let name = own_name(&library.pack.id).to_string();
                if library.pack.source != Source::User {
                    library.pack.overrides = Some(library.pack.id.clone());
                }
                library.pack.id = format!("user.{name}");
                library.pack.source = Source::User;
                library.pack.tested.clear();
                dir.join(format!("user-{name}.toml"))
            }
        };
        let toml = toml::to_string(&library).map_err(|e| e.to_string())?;
        let text = format!("# Edited in the KnoEnd web app{}.\n\n{toml}",
                           library.pack.overrides.as_deref().map(|o| format!(" (layered on {o})")).unwrap_or_default());
        // Read back exactly as the service will load it.
        Library::parse(&text).map_err(|e| format!("not saved: {e}"))?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        self.library = library.clone();
        self.saved = library.clone();
        self.path = Some(path.clone());
        Ok((path, library))
    }
}

/// Gives every key its page and slot, so moving one key leaves the others where they are.
pub fn pin(library: &mut Library) {
    let placed = library.placed();
    for (key, (page, slot)) in library.keys.iter_mut().zip(placed) {
        key.page = Some(page);
        key.slot = Some(slot);
    }
}

/// The library's page numbers that have keys, in order: the board's pages 1, 2, ...
pub fn page_numbers(library: &Library) -> Vec<u8> {
    let mut numbers: Vec<u8> = library.placed().into_iter().map(|(page, _)| page).collect();
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

/// The id's name without its source ("ai.web.youtube-com" -> "web.youtube-com").
fn own_name(id: &str) -> &str {
    id.split_once('.').map(|(_, rest)| rest).unwrap_or(id)
}

/// An edited library from the web app, checked by the same rules as a file. Edit mode keeps every
/// key on a page the board draws whole, so the board's pages are the library's.
pub fn check(json: Value, max: Grid) -> Result<Library, String> {
    let library = Library::from_json(json).map_err(|e| e.to_string())?;
    for n in page_numbers(&library) {
        let grid = library.grid(n);
        if !grid.fits(max) {
            return Err(format!("page {n} is {}x{}; this board draws at most {}x{}", grid.cols, grid.rows, max.cols, max.rows));
        }
    }
    Ok(library)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DRAFT: &str = r#"
[pack]
id = "ai.calculator"
name = "Calculator"
version = "0.1.0"
source = "ai"
grid = "2x2"

[match]
macos = ["com.apple.calculator"]

[[key]]
label = "Clear"
action = { kind = "shortcut", keys = "Escape" }

[[key]]
label = "Equals"
action = { kind = "shortcut", keys = "Enter" }
"#;

    #[test]
    fn keys_are_pinned_where_they_land() {
        let edit = Edit::start(&Library::parse(DRAFT).unwrap(), Path::new("/nonexistent"));
        let spots: Vec<_> = edit.library.keys.iter().map(|k| (k.page, k.slot)).collect();
        assert_eq!(spots, vec![(Some(1), Some(1)), (Some(1), Some(2))]);
        assert!(!edit.dirty() && edit.path.is_none(), "a draft has no file of the person's own yet");
        let state = edit.state(0, Grid::LARGEST);
        assert_eq!(state["pages"], json!([{"number": 1, "grid": "2x2"}]));
    }

    #[test]
    fn saving_a_draft_makes_a_user_library_on_top() {
        let dir = std::env::temp_dir().join(format!("knoend-edit-{}", std::process::id()));
        let mut edit = Edit::start(&Library::parse(DRAFT).unwrap(), Path::new("/nonexistent"));
        edit.library.keys[0].label = "AC\nall".into();
        edit.library.keys.swap(0, 1);
        assert!(edit.dirty());
        let (path, saved) = edit.save(&dir).unwrap();
        assert_eq!(path, dir.join("user-calculator.toml"));
        assert_eq!((saved.pack.id.as_str(), saved.pack.source.clone(), saved.pack.overrides.as_deref()),
                   ("user.calculator", Source::User, Some("ai.calculator")));
        let back = Library::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.keys[1].label, "AC\nall", "a forced line break is kept");
        assert_eq!((back.keys[1].page, back.keys[1].slot), (Some(1), Some(1)));
        assert!(!edit.dirty());
        // Saving again writes the same file.
        edit.library.keys[0].label = "=".into();
        assert_eq!(edit.save(&dir).unwrap().0, path);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pages_too_big_for_the_board_and_collisions_are_refused() {
        let mut library = Library::parse(DRAFT).unwrap();
        pin(&mut library);
        library.pack.grid = Some("4x4".into());
        assert!(check(serde_json::to_value(&library).unwrap(), Grid { cols: 3, rows: 3 }).unwrap_err().contains("at most 3x3"));
        library.pack.grid = Some("2x2".into());
        library.keys[1].slot = Some(1);
        assert!(check(serde_json::to_value(&library).unwrap(), Grid::LARGEST).is_err());
    }
}

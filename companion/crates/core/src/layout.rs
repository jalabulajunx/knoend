//! A library becomes layout packets for one OS and one board: shortcuts the board can type itself
//! become `hid`; everything else becomes `event`, carried out by the companion. Each page is its own
//! packet; a page whose grid is bigger than the board's is split onto pages the board can draw.

use crate::focus::Os;
use crate::keys::parse_shortcut;
use crate::library::{Action, Library, Source};
use crate::protocol::{Grid, LayoutBody, LayoutKey, Send};

/// Pages one layout may have on the board (a 4x4 library page split for a 1x1 board makes 16).
pub const MAX_BOARD_PAGES: usize = 64;

/// One page as the board shows it: its grid, and which library key (index) sits in which slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub grid: Grid,
    pub keys: Vec<(usize, u8)>,
}

/// The library's pages on a board that draws at most `max`: pages without keys are left out; a
/// page too big for the board becomes several, keys in slot order.
pub fn pages(library: &Library, max: Grid) -> Vec<Page> {
    let placed = library.placed();
    let mut numbers: Vec<u8> = placed.iter().map(|(page, _)| *page).collect();
    numbers.sort_unstable();
    numbers.dedup();
    let mut out = Vec::new();
    for number in numbers {
        let mut keys: Vec<(usize, u8)> = placed.iter().enumerate().filter(|(_, (page, _))| *page == number).map(|(i, (_, slot))| (i, *slot)).collect();
        keys.sort_by_key(|(_, slot)| *slot);
        let grid = library.grid(number);
        if grid.fits(max) {
            out.push(Page { grid, keys });
        } else {
            let small = grid.within(max);
            for chunk in keys.chunks(usize::from(small.size())) {
                out.push(Page { grid: small, keys: chunk.iter().zip(1..).map(|((i, _), slot)| (*i, slot)).collect() });
            }
        }
    }
    out.truncate(MAX_BOARD_PAGES);
    out
}

/// Page `index` (0-based) of `pages` as a layout packet body.
pub fn build(library: &Library, os: Os, pages: &[Page], index: usize) -> Result<LayoutBody, String> {
    let page = pages.get(index).ok_or("no such page")?;
    let mut keys = Vec::new();
    for &(i, slot) in &page.keys {
        let key = &library.keys[i];
        let send = match &key.action {
            Action::Shortcut { keys } => {
                let chord = parse_shortcut(keys, os)?;
                Send::Hid { modifiers: chord.modifiers, usage: chord.usage }
            }
            _ => Send::Event(true),
        };
        keys.push(LayoutKey { key: slot, label: key.label.clone(), speak: key.speak.clone(), icon: key.icon.clone(), confirm: key.confirm, send });
    }
    let source = match library.pack.source {
        Source::Builtin => "builtin",
        Source::Community => "community",
        Source::Ai => "ai",
        Source::User => "user",
    };
    let paged = pages.len() > 1;
    Ok(LayoutBody {
        layout: library.pack.id.clone(),
        name: library.pack.name.clone(),
        source: source.to_string(),
        reviewed: library.pack.source != Source::Ai,
        grid: Some(page.grid),
        page: paged.then(|| index as u8 + 1),
        pages: paged.then(|| pages.len() as u8),
        keys,
        ..LayoutBody::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn example(name: &str) -> String {
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../spec/examples").join(name)).unwrap()
    }

    /// The spec's layout examples were written by hand; building them from the matching
    /// library examples must give exactly the same packet body.
    #[test]
    fn libraries_build_the_spec_layout_examples() {
        for (name, layout) in [("library-vscode.toml", "layout-vscode.json"), ("library-amazon.toml", "layout-amazon.json")] {
            let library = Library::parse(&example(name)).unwrap();
            let built = build(&library, Os::Linux, &pages(&library, Grid::LARGEST), 0).unwrap();
            let packet: serde_json::Value = serde_json::from_str(&example(layout)).unwrap();
            let expected: LayoutBody = serde_json::from_value(packet["body"].clone()).unwrap();
            assert_eq!(built, expected, "{name} -> {layout}");
        }
    }

    #[test]
    fn pages_split_for_a_smaller_board() {
        let library = Library::parse(&example("library-pages.toml")).unwrap();
        // On a 4x4 board: 2x2, the 4x4 page 2, 2x2.
        let big = pages(&library, Grid::LARGEST);
        assert_eq!(big.iter().map(|p| p.grid).collect::<Vec<_>>(),
                   vec![Grid { cols: 2, rows: 2 }, Grid::LARGEST, Grid { cols: 2, rows: 2 }]);
        assert_eq!(big[1].keys, vec![(4, 1), (5, 16)]);
        // On a 2x2 board page 2 is cut to 2x2; its two keys fit one page, renumbered 1 and 2.
        let small = pages(&library, Grid { cols: 2, rows: 2 });
        assert_eq!(small.len(), 3);
        assert_eq!(small[1], Page { grid: Grid { cols: 2, rows: 2 }, keys: vec![(4, 1), (5, 2)] });
        // A 1x1 board: one key per page.
        assert_eq!(pages(&library, Grid { cols: 1, rows: 1 }).len(), 7);
        let body = build(&library, Os::Linux, &small, 1).unwrap();
        assert_eq!((body.page, body.pages, body.keys.len()), (Some(2), Some(3), 2));
    }

    #[test]
    fn one_page_layouts_carry_no_page_numbers() {
        let library = Library::parse(&example("library-vscode.toml")).unwrap();
        let body = build(&library, Os::Linux, &pages(&library, Grid::LARGEST), 0).unwrap();
        assert_eq!((body.grid, body.page, body.pages), (Some(Grid::DEFAULT), None, None));
    }
}

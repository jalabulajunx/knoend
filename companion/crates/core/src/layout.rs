//! A library becomes a layout packet for one OS: shortcuts the board can type itself become
//! `hid`; everything else becomes `event`, carried out by the companion.

use crate::focus::Os;
use crate::keys::parse_shortcut;
use crate::library::{Action, Library, Source};
use crate::protocol::{LayoutBody, LayoutKey, Send};

pub fn build(library: &Library, os: Os) -> Result<LayoutBody, String> {
    let mut keys = Vec::new();
    for (key, slot) in library.keys.iter().zip(library.slots()) {
        let send = match &key.action {
            Action::Shortcut { keys } => {
                let chord = parse_shortcut(keys, os)?;
                Send::Hid { modifiers: chord.modifiers, usage: chord.usage }
            }
            _ => Send::Event(true),
        };
        keys.push(LayoutKey { key: slot, label: key.label.clone(), speak: key.speak.clone(), icon: key.icon.clone(), confirm: key.confirm, send });
    }
    keys.sort_by_key(|k| k.key);
    let source = match library.pack.source {
        Source::Builtin => "builtin",
        Source::Community => "community",
        Source::Ai => "ai",
        Source::User => "user",
    };
    Ok(LayoutBody {
        detail: None,
        layout: library.pack.id.clone(),
        name: library.pack.name.clone(),
        source: source.to_string(),
        reviewed: library.pack.source != Source::Ai,
        keys,
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
        for (library, layout) in [("library-vscode.toml", "layout-vscode.json"), ("library-amazon.toml", "layout-amazon.json")] {
            let built = build(&Library::parse(&example(library)).unwrap(), Os::Linux).unwrap();
            let packet: serde_json::Value = serde_json::from_str(&example(layout)).unwrap();
            let expected: LayoutBody = serde_json::from_value(packet["body"].clone()).unwrap();
            assert_eq!(built, expected, "{library} -> {layout}");
        }
    }
}

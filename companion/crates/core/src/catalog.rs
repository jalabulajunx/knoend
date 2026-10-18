//! All installed libraries, and which one applies to the focused app.
//!
//! Precedence (spec section 5): user override, then installed (built-in or community), then an AI
//! draft. Libraries that fail validation are skipped and reported, never half-loaded.

use std::fs;
use std::path::{Path, PathBuf};

use crate::focus::{Focus, Os};
use crate::library::{Library, Source};

#[derive(Default)]
pub struct Catalog {
    libraries: Vec<(PathBuf, Library)>,
}

/// A file that was skipped, and why.
pub struct Rejected {
    pub path: PathBuf,
    pub reason: String,
}

impl Catalog {
    /// Loads every `*.toml` under the given directories (missing directories are fine).
    pub fn load(dirs: &[PathBuf]) -> (Catalog, Vec<Rejected>) {
        let mut catalog = Catalog::default();
        let mut rejected = Vec::new();
        for dir in dirs {
            let Ok(entries) = fs::read_dir(dir) else { continue };
            let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
            paths.sort();
            for path in paths {
                match fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| Library::parse(&t).map_err(|e| e.to_string())) {
                    Ok(library) => catalog.libraries.push((path, library)),
                    Err(reason) => rejected.push(Rejected { path, reason }),
                }
            }
        }
        (catalog, rejected)
    }

    pub fn add(&mut self, path: PathBuf, library: Library) {
        self.libraries.retain(|(p, l)| p != &path && l.pack.id != library.pack.id);
        self.libraries.push((path, library));
    }

    pub fn len(&self) -> usize {
        self.libraries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.libraries.is_empty()
    }

    /// The library for what is focused, by precedence: a library for the browser tab's site
    /// first, then one for the app. `None` means "unsupported".
    pub fn find(&self, focus: &Focus) -> Option<&Library> {
        let rank = |source: &Source| match source {
            Source::User => 0,
            Source::Builtin | Source::Community => 1,
            Source::Ai => 2,
        };
        if let Some(web) = &focus.web {
            // The most specific pattern wins ("youtube.com/watch" over "youtube.com"), then source.
            let site = self.libraries.iter().map(|(_, l)| l)
                .filter_map(|l| l.matches.web.iter().filter(|p| web.matches_pattern(p)).map(|p| p.len()).max().map(|n| (l, n)))
                .min_by_key(|(l, n)| (std::cmp::Reverse(*n), rank(&l.pack.source)))
                .map(|(l, _)| l);
            if site.is_some() {
                return site;
            }
        }
        self.libraries.iter().map(|(_, l)| l)
            .filter(|l| patterns(l, focus.os).iter().any(|p| focus.matches_pattern(p)))
            .min_by_key(|l| rank(&l.pack.source))
    }

    /// The library that covers a whole site (a `match.web` pattern without a route), if any.
    pub fn site_wide(&self, web: &crate::focus::WebPage) -> Option<&Library> {
        self.libraries.iter().map(|(_, l)| l)
            .find(|l| l.matches.web.iter().any(|p| !p.contains('/') && web.matches_pattern(p)))
    }

    /// The library for a coding agent's screen ("claude-code/permission"): the person's own first,
    /// then built-in, then AI drafts.
    pub fn agent(&self, name: &str) -> Option<&Library> {
        let rank = |source: &Source| match source {
            Source::User => 0,
            Source::Builtin | Source::Community => 1,
            Source::Ai => 2,
        };
        self.libraries.iter().map(|(_, l)| l)
            .filter(|l| l.matches.agent.iter().any(|a| a.eq_ignore_ascii_case(name)))
            .min_by_key(|l| rank(&l.pack.source))
    }

    /// A library by its id.
    pub fn get(&self, id: &str) -> Option<&Library> {
        self.libraries.iter().map(|(_, l)| l).find(|l| l.pack.id == id)
    }

    pub fn path_of(&self, id: &str) -> Option<&Path> {
        self.libraries.iter().find(|(_, l)| l.pack.id == id).map(|(p, _)| p.as_path())
    }
}

fn patterns(library: &Library, os: Os) -> &[String] {
    match os {
        Os::Linux => &library.matches.linux,
        Os::Macos => &library.matches.macos,
        Os::Windows => &library.matches.windows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focus(app: &str, instance: &str) -> Focus {
        Focus { os: Os::Linux, app: app.into(), instance: instance.into(), title: String::new(), web: None }
    }

    #[test]
    fn finds_by_class_or_instance_and_reports_unsupported() {
        let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../spec/examples");
        let (catalog, rejected) = Catalog::load(&[examples.clone(), examples.join("invalid")]);
        assert_eq!(catalog.len(), 2, "the two valid example libraries");
        assert_eq!(rejected.len(), 3, "the three invalid example libraries");
        assert_eq!(catalog.find(&focus("Code", "code")).map(|l| l.pack.id.as_str()), Some("builtin.vscode"));
        assert_eq!(catalog.find(&focus("code-oss", "")).map(|l| l.pack.id.as_str()), Some("builtin.vscode"));
        assert!(catalog.find(&focus("com.anthropic.Claude", "com.anthropic.claude")).is_none());
    }

    #[test]
    fn user_override_beats_builtin_and_ai_comes_last() {
        let vscode = |id: &str, source: &str| Library::parse(&format!(
            "[pack]\nid = \"{id}\"\nname = \"X\"\nversion = \"1.0.0\"\nsource = \"{source}\"\n[match]\nlinux = [\"code\"]\n[[key]]\nlabel = \"Save\"\naction = {{ kind = \"shortcut\", keys = \"Primary+S\" }}\n"
        )).unwrap();
        let mut catalog = Catalog::default();
        catalog.add("ai.toml".into(), vscode("ai.vscode", "ai"));
        assert_eq!(catalog.find(&focus("code", "")).unwrap().pack.id, "ai.vscode");
        catalog.add("builtin.toml".into(), vscode("builtin.vscode", "builtin"));
        assert_eq!(catalog.find(&focus("code", "")).unwrap().pack.id, "builtin.vscode");
        catalog.add("user.toml".into(), vscode("user.vscode", "user"));
        assert_eq!(catalog.find(&focus("code", "")).unwrap().pack.id, "user.vscode");
    }
}

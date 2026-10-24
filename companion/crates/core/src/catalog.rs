//! All installed libraries, and which one applies to the focused app.
//!
//! Precedence (spec section 5): user override, then installed (built-in or community), then an AI
//! draft. Libraries that fail validation are skipped and reported, never half-loaded.

use std::fs;
use std::path::{Path, PathBuf};

use crate::focus::{Focus, Os};
use crate::library::{Library, LibraryKey, Source};

/// How many keys of its own a kind of page adds to its site's keys.
pub const PAGE_EXTRAS: usize = 3;

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

    /// `root` and every folder below it, for a tree of libraries such as `recipes/` (`apps/`,
    /// `sites/<host>/`, `agents/`). `load` reads only the folders it is given.
    pub fn folders(root: &Path) -> Vec<PathBuf> {
        let mut out = vec![root.to_path_buf()];
        let mut i = 0;
        while i < out.len() {
            if let Ok(entries) = fs::read_dir(&out[i]) {
                let mut subs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
                subs.sort();
                out.extend(subs);
            }
            i += 1;
        }
        out
    }

    pub fn add(&mut self, path: PathBuf, library: Library) {
        self.libraries.retain(|(p, l)| p != &path && l.pack.id != library.pack.id);
        self.libraries.push((path, library));
    }

    /// Every library and the file it came from (built-in recipes have no file of their own here).
    pub fn all(&self) -> impl Iterator<Item = (&PathBuf, &Library)> {
        self.libraries.iter().map(|(p, l)| (p, l))
    }

    pub fn by_id(&self, id: &str) -> Option<(&PathBuf, &Library)> {
        self.all().find(|(_, l)| l.pack.id == id)
    }

    /// The recipe that replaced an AI draft on `os`: built-in or community libraries have every
    /// site and app pattern the draft matches by there, so the draft is never shown. The recipe
    /// with its first pattern; None for anything but a draft, and for a draft still in use.
    pub fn replaced_by(&self, draft: &Library, os: Os) -> Option<&Library> {
        if draft.pack.source != Source::Ai {
            return None;
        }
        // Every pattern the draft matches by here (its sites, its apps on `os`), each with a
        // recipe that has it too: otherwise `find` still picks the draft somewhere.
        let wanted: Vec<(&String, bool)> = draft.matches.web.iter().map(|p| (p, true))
            .chain(patterns(draft, os).iter().map(|p| (p, false))).collect();
        let covering = |&(pattern, web): &(&String, bool)| self.all().map(|(_, l)| l)
            .filter(|l| matches!(l.pack.source, Source::Builtin | Source::Community))
            .find(|l| if web { l.matches.web.as_slice() } else { patterns(l, os) }.iter().any(|q| q.eq_ignore_ascii_case(pattern)));
        let first = covering(wanted.first()?)?;
        wanted.iter().all(|w| covering(w).is_some()).then_some(first)
    }

    /// True when a library of the person's own or a recipe shares an app or terminal pattern with
    /// AI draft `draft` on `os`: `find` picks that one there, so changing the draft shows nothing.
    pub fn shadowed(&self, draft: &Library, os: Os) -> bool {
        let mine: Vec<&String> = patterns(draft, os).iter().chain(&draft.matches.terminal).collect();
        self.all().map(|(_, l)| l).filter(|l| l.pack.source != Source::Ai)
            .any(|l| patterns(l, os).iter().chain(&l.matches.terminal).any(|q| mine.iter().any(|p| p.eq_ignore_ascii_case(q))))
    }

    pub fn remove(&mut self, id: &str) {
        self.libraries.retain(|(_, l)| l.pack.id != id);
    }

    pub fn len(&self) -> usize {
        self.libraries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.libraries.is_empty()
    }

    /// The library for what is focused, by precedence: a library for the program in a terminal's
    /// active tab (its foreground leader first: "man" before the "less" it runs), or for the browser
    /// tab's site, then one for the app. `None` means "unsupported".
    pub fn find(&self, focus: &Focus) -> Option<&Library> {
        let rank = |source: &Source| match source {
            Source::User => 0,
            Source::Builtin | Source::Community => 1,
            Source::Ai => 2,
        };
        for program in focus.terminal.iter().flat_map(|t| &t.programs) {
            let found = self.libraries.iter().map(|(_, l)| l)
                .filter(|l| l.matches.terminal.iter().any(|p| p.eq_ignore_ascii_case(program)))
                .min_by_key(|l| rank(&l.pack.source));
            if found.is_some() {
                return found;
            }
        }
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

    /// What the board shows for `focus`: the best library, except that on a kind of page with AI-drafted
    /// keys of its own the site's keys stay. The site's keys come first, in the same places on every
    /// page of the site, and the page's keys take the last slots (at most `PAGE_EXTRAS`, skipping any
    /// the site already has). Built-in and user page libraries are complete layouts and are kept as
    /// they are, and so is anything for full screen (a slide remote).
    pub fn layout_for(&self, focus: &Focus) -> Option<Library> {
        let found = self.find(focus)?;
        let page_draft = found.pack.source == Source::Ai
            && found.matches.web.iter().any(|p| p.contains('/') && !p.contains("@fullscreen"));
        let site = focus.web.as_ref().filter(|_| page_draft).and_then(|web| self.site_wide(web)).filter(|s| s.pack.id != found.pack.id);
        let Some(site) = site else { return Some(found.clone()) };
        let same = |a: &LibraryKey, b: &LibraryKey| a.label.eq_ignore_ascii_case(&b.label) || a.action == b.action;
        let extras: Vec<LibraryKey> = found.keys.iter().filter(|k| !site.keys.iter().any(|s| same(s, k))).take(PAGE_EXTRAS).cloned().collect();
        // One page in the site's grid (the page draft's own grid was chosen for its keys alone).
        let grid = site.grid(1);
        let mut keys: Vec<LibraryKey> = site.keys.iter().take(usize::from(grid.size()).saturating_sub(extras.len())).cloned().collect();
        keys.extend(extras);
        for key in &mut keys {
            key.slot = None;
            key.page = None;
        }
        let mut combined = found.clone();
        combined.keys = keys;
        combined.pack.grid = Some(format!("{}x{}", grid.cols, grid.rows));
        combined.pages.clear();
        Some(combined)
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
        Focus { os: Os::Linux, app: app.into(), instance: instance.into(), title: String::new(), web: None, pid: None, terminal: None }
    }

    #[test]
    fn finds_by_class_or_instance_and_reports_unsupported() {
        let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../spec/examples");
        let (catalog, rejected) = Catalog::load(&[examples.clone(), examples.join("invalid")]);
        assert_eq!(catalog.len(), 3, "the three valid example libraries");
        assert_eq!(rejected.len(), 7, "the seven invalid example libraries");
        assert_eq!(catalog.find(&focus("Code", "code")).map(|l| l.pack.id.as_str()), Some("builtin.vscode"));
        assert_eq!(catalog.find(&focus("code-oss", "")).map(|l| l.pack.id.as_str()), Some("builtin.vscode"));
        assert!(catalog.find(&focus("com.anthropic.Claude", "com.anthropic.claude")).is_none());
    }

    fn site_library(id: &str, source: &str, web: &str, labels: &[&str]) -> Library {
        let keys: String = labels.iter().map(|l| format!("[[key]]\nlabel = \"{l}\"\naction = {{ kind = \"open\", url = \"/{}\" }}\n", l.to_lowercase().replace(' ', "-"))).collect();
        Library::parse(&format!("[pack]\nid = \"{source}.{id}\"\nname = \"{id}\"\nversion = \"0.1.0\"\nsource = \"{source}\"\n[match]\nweb = [\"{web}\"]\n{keys}")).unwrap()
    }

    #[test]
    fn a_drafted_page_keeps_the_site_keys_and_adds_its_own() {
        let mut catalog = Catalog::default();
        let site = ["Home", "Search", "Network", "Jobs", "Messages", "Alerts", "Me", "Saved", "Groups"];
        catalog.add("site.toml".into(), site_library("site-linkedin-com", "ai", "linkedin.com", &site));
        catalog.add("feed.toml".into(), site_library("site-linkedin-com-feed", "ai", "linkedin.com/feed", &["New Post", "Saved", "Sort Feed", "Analytics", "Events"]));
        let mut f = focus("google-chrome", "google-chrome");
        f.web = Some(crate::focus::WebPage { host: "www.linkedin.com".into(), url: "https://www.linkedin.com/feed/".into(), title: String::new(),
                                             granted: true, fullscreen: false, favicon: String::new() });
        let shown = catalog.layout_for(&f).unwrap();
        let labels: Vec<&str> = shown.keys.iter().map(|k| k.label.as_str()).collect();
        assert_eq!(labels, ["Home", "Search", "Network", "Jobs", "Messages", "Alerts", "New Post", "Sort Feed", "Analytics"],
                   "site keys in their places, then the page's first three it does not already have");
        assert_eq!(shown.pack.id, "ai.site-linkedin-com-feed");
        // A built-in page layout is complete as it is.
        catalog.add("feed2.toml".into(), site_library("site-linkedin-com-feed2", "builtin", "linkedin.com/feed", &["Only"]));
        assert_eq!(catalog.layout_for(&f).unwrap().keys.len(), 1);
    }

    #[test]
    fn a_drafted_page_on_a_site_with_its_own_grid_stays_on_one_page() {
        let mut catalog = Catalog::default();
        let labels: Vec<String> = (1..=14).map(|i| format!("Site {i}")).collect();
        let mut site = site_library("site-example-com", "ai", "example.com", &labels.iter().map(String::as_str).collect::<Vec<_>>());
        site.pack.grid = Some("4x4".into());
        // The page draft's own grid was chosen for its three keys alone.
        let mut page = site_library("site-example-com-docs", "ai", "example.com/docs", &["Search docs", "Next", "Previous"]);
        page.pack.grid = Some("2x2".into());
        catalog.add("site.toml".into(), site);
        catalog.add("docs.toml".into(), page);
        let mut f = focus("firefox", "firefox");
        f.web = Some(crate::focus::WebPage { host: "example.com".into(), url: "https://example.com/docs/".into(), title: String::new(),
                                             granted: true, fullscreen: false, favicon: String::new() });
        let shown = catalog.layout_for(&f).unwrap();
        assert_eq!(shown.grid(1), crate::protocol::Grid::LARGEST, "the site's grid");
        assert_eq!(shown.keys.len(), 16, "13 site keys and the page's 3");
        assert!(shown.placed().iter().all(|(page, _)| *page == 1), "all on one page");
    }

    #[test]
    fn a_recipes_tree_loads_from_its_subfolders() {
        let recipes = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../recipes");
        let (catalog, rejected) = Catalog::load(&Catalog::folders(&recipes));
        assert!(rejected.is_empty(), "every recipe loads");
        assert!(catalog.agent("claude-code/permission").is_some(), "agents/");
        assert!(catalog.get("builtin.vscode").is_some(), "apps/");
        let pages = catalog.libraries.iter().filter(|(p, _)| p.parent().and_then(|d| d.parent()).is_some_and(|d| d.ends_with("sites"))).count();
        assert!(pages > 0, "sites/<host>/ pages");
    }

    #[test]
    fn a_program_in_the_terminal_tab_beats_the_terminal_app() {
        let lib = |id: &str, matches: &str| Library::parse(&format!(
            "[pack]\nid = \"builtin.{id}\"\nname = \"X\"\nversion = \"1.0.0\"\nsource = \"builtin\"\n[match]\n{matches}\n[[key]]\nlabel = \"Q\"\naction = {{ kind = \"shortcut\", keys = \"Q\" }}\n"
        )).unwrap();
        let mut catalog = Catalog::default();
        catalog.add("terminal.toml".into(), lib("terminal", "linux = [\"xfce4-terminal\"]"));
        catalog.add("pager.toml".into(), lib("pager", "terminal = [\"less\", \"man\"]"));
        catalog.add("claude.toml".into(), lib("claude", "terminal = [\"claude\"]"));
        let mut f = focus("Xfce4-terminal", "xfce4-terminal");
        let in_tab = |f: &mut Focus, programs: &[&str]| {
            f.terminal = Some(crate::focus::TerminalTab { tty: "pts/1".into(), programs: programs.iter().map(|p| p.to_string()).collect() });
        };
        assert_eq!(catalog.find(&f).unwrap().pack.id, "builtin.terminal", "no tab known");
        in_tab(&mut f, &["bash"]);
        assert_eq!(catalog.find(&f).unwrap().pack.id, "builtin.terminal", "a shell at its prompt");
        in_tab(&mut f, &["git", "less"]);
        assert_eq!(catalog.find(&f).unwrap().pack.id, "builtin.pager", "git log's pager");
        in_tab(&mut f, &["Claude"]);
        assert_eq!(catalog.find(&f).unwrap().pack.id, "builtin.claude");
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

    #[test]
    fn a_draft_under_the_persons_own_library_is_shadowed() {
        let lib = |id: &str, source: &str, matches: &str| Library::parse(&format!(
            "[pack]\nid = \"{id}\"\nname = \"X\"\nversion = \"1.0.0\"\nsource = \"{source}\"\n[match]\n{matches}\n[[key]]\nlabel = \"Save\"\naction = {{ kind = \"shortcut\", keys = \"Primary+S\" }}\n"
        )).unwrap();
        let mut catalog = Catalog::default();
        let calculator = lib("ai.calculator", "ai", "macos = [\"Calculator\", \"com.apple.calculator\"]");
        let htop = lib("ai.terminal-htop", "ai", "terminal = [\"htop\"]");
        catalog.add("ai/calculator.toml".into(), calculator.clone());
        catalog.add("ai/htop.toml".into(), htop.clone());
        assert!(!catalog.shadowed(&calculator, Os::Macos), "another draft does not shadow it");
        catalog.add("user-calculator.toml".into(), lib("user.calculator", "user", "macos = [\"calculator\"]"));
        assert!(catalog.shadowed(&calculator, Os::Macos), "the person's own keys, any case");
        assert!(!catalog.shadowed(&calculator, Os::Linux), "per OS");
        catalog.add("htop.toml".into(), lib("community.htop", "community", "terminal = [\"htop\"]"));
        assert!(catalog.shadowed(&htop, Os::Linux), "a recipe for the terminal program");
    }

    #[test]
    fn a_draft_is_replaced_only_when_recipes_cover_all_it_matches_on_this_os() {
        let lib = |id: &str, source: &str, matches: &str| Library::parse(&format!(
            "[pack]\nid = \"{id}\"\nname = \"X\"\nversion = \"1.0.0\"\nsource = \"{source}\"\n[match]\n{matches}\n[[key]]\nlabel = \"Save\"\naction = {{ kind = \"shortcut\", keys = \"Primary+S\" }}\n"
        )).unwrap();
        let mut catalog = Catalog::default();
        catalog.add("ai-claude.toml".into(), lib("ai.claude", "ai", "linux = [\"com.anthropic.Claude\"]"));
        catalog.add("ai-firefox.toml".into(), lib("ai.firefox", "ai", "linux = [\"firefox\"]\nmacos = [\"org.mozilla.firefox\"]"));
        catalog.add("ai-site.toml".into(), lib("ai.site", "ai", "web = [\"linkedin.com\", \"linkedin.com/feed\"]"));
        let replaced = |catalog: &Catalog, id: &str, os: Os| catalog.replaced_by(catalog.by_id(id).unwrap().1, os).map(|l| l.pack.id.clone());
        assert_eq!(replaced(&catalog, "ai.claude", Os::Linux), None, "no recipe yet");
        catalog.add("claude.toml".into(), lib("builtin.claude", "builtin", "linux = [\"com.anthropic.claude\"]"));
        catalog.add("firefox.toml".into(), lib("builtin.firefox", "builtin", "linux = [\"firefox\"]"));
        catalog.add("site.toml".into(), lib("builtin.site", "builtin", "web = [\"linkedin.com\"]"));
        assert_eq!(replaced(&catalog, "ai.claude", Os::Linux).as_deref(), Some("builtin.claude"), "same class, any case");
        assert_eq!(replaced(&catalog, "ai.firefox", Os::Linux).as_deref(), Some("builtin.firefox"));
        assert_eq!(replaced(&catalog, "ai.firefox", Os::Macos), None, "the recipe is for Linux; on macOS the draft is in use");
        assert_eq!(replaced(&catalog, "ai.site", Os::Linux), None, "its feed pattern has no recipe");
        catalog.add("feed.toml".into(), lib("builtin.feed", "builtin", "web = [\"linkedin.com/feed\"]"));
        assert_eq!(replaced(&catalog, "ai.site", Os::Linux).as_deref(), Some("builtin.site"));
        assert_eq!(replaced(&catalog, "builtin.claude", Os::Linux), None, "only drafts are replaced");
    }
}

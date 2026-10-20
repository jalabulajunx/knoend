//! Embeds every recipe (recipes/**/*.toml) in the binary, so a released `knoend` works
//! without the source tree.
use std::fmt::Write;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    for entry in std::fs::read_dir(dir).expect("recipes folder").flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|x| x == "toml") {
            println!("cargo:rerun-if-changed={}", path.display());
            out.push(path);
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../recipes").canonicalize().expect("recipes/");
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();
    let mut code = String::from("pub const BUILTIN: &[(&str, &str)] = &[\n");
    for path in paths {
        let name = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        writeln!(code, "    ({name:?}, include_str!({:?})),", path.display().to_string()).unwrap();
    }
    code.push_str("];\n");
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("builtin.rs"), code).unwrap();
}

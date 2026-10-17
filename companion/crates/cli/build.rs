//! Embeds every built-in library (libraries/builtin/*.toml) in the binary, so a released
//! `openpager` works without the source tree.
use std::fmt::Write;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../libraries/builtin");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut paths: Vec<_> = std::fs::read_dir(&dir).expect("libraries/builtin").flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
    paths.sort();
    let mut code = String::from("pub const BUILTIN: &[(&str, &str)] = &[\n");
    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        writeln!(code, "    ({name:?}, include_str!({:?})),", path.canonicalize().unwrap().display().to_string()).unwrap();
    }
    code.push_str("];\n");
    std::fs::write(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("builtin.rs"), code).unwrap();
}

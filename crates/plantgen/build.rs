//! Compiles the built-in library in: every
//! `library/<family>/<genus>/<id>/spec.json` becomes one entry of
//! `library::LIBRARY`, sorted by path, so adding a species is adding its
//! folder. No index is kept: the tree is the index. `library`'s tests hold
//! the tree to its rules.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// The folders directly inside `path`, sorted by name.
fn folders(path: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|entry| entry.is_dir())
        .collect();
    found.sort();
    found
}

fn name(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| panic!("{} has no UTF-8 name", path.display()))
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("set by Cargo"));
    let root = manifest.join("library");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut entries = String::new();
    let mut count = 0_usize;
    for family in folders(&root) {
        for genus in folders(&family) {
            for species in folders(&genus) {
                let spec = species.join("spec.json");
                assert!(spec.is_file(), "{} has no spec.json", species.display());
                writeln!(
                    entries,
                    "    Species {{ id: {:?}, family: {:?}, genus: {:?}, source: include_str!({:?}) }},",
                    name(&species),
                    name(&family),
                    name(&genus),
                    spec.display().to_string(),
                )
                .expect("writing to a String");
                count += 1;
            }
        }
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("set by Cargo")).join("library.rs");
    fs::write(
        &out,
        format!(
            "/// Every species of the built-in library ({count}), sorted by family, genus and id.\n\
             pub const LIBRARY: &[Species] = &[\n{entries}];\n"
        ),
    )
    .unwrap_or_else(|error| panic!("cannot write {}: {error}", out.display()));
}

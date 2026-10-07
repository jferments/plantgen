//! The built-in library: the species compiled into `PlantGen`, one folder
//! each, `library/<family>/<genus>/<id>/spec.json`. The family is that of
//! the species' accepted name in the World Checklist of Vascular Plants
//! (WCVP v16); the genus folder is the genus the id is written with. The
//! build script walks the tree and compiles every spec in, so the tree is
//! the index: adding a species is adding its folder.

/// One species of the built-in library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Species {
    /// The species' id, its folder's name: genus and epithet in lower
    /// case, joined by hyphens.
    pub id: &'static str,
    /// Its family's folder: the family's name in lower case.
    pub family: &'static str,
    /// Its genus's folder: the genus the id is written with, in lower case.
    pub genus: &'static str,
    /// Its spec, as JSON.
    pub source: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/library.rs"));

/// The species with id `id`, if the library has it.
#[must_use]
pub fn species(id: &str) -> Option<&'static Species> {
    LIBRARY.iter().find(|species| species.id == id)
}

/// The spec of the species `id`, as JSON, for use in constants: a
/// catalogue built from it does not compile if the library lacks a species
/// it names.
///
/// # Panics
///
/// Panics, at compile time when used in a constant, if the library has no
/// species `id`.
#[must_use]
pub const fn source(id: &str) -> &'static str {
    let mut index = 0;
    while index < LIBRARY.len() {
        if same(LIBRARY[index].id, id) {
            return LIBRARY[index].source;
        }
        index += 1;
    }
    panic!("the built-in library has no species with this id")
}

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut index = 0;
    while index < a.len() {
        if a[index] != b[index] {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{LIBRARY, source, species};

    fn lower_name(name: &str) -> bool {
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
            && !name.starts_with('-')
            && !name.ends_with('-')
    }

    fn entries(path: &Path) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        found.sort();
        found
    }

    /// The tree's rules: a folder per family, per genus and per species, in
    /// lower case; the species folder holds its `spec.json`, whose `id` is
    /// the folder's name and starts with the genus folder's; families end
    /// in "aceae"; no id twice, and nothing anywhere else.
    #[test]
    fn the_library_keeps_the_tree_rules() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("library");
        let mut ids = BTreeSet::new();
        let mut walked = 0;
        for family in entries(&root) {
            let family_name = family.file_name().unwrap().to_str().unwrap();
            assert!(
                family.is_dir(),
                "{} is not a family folder",
                family.display()
            );
            assert!(
                lower_name(family_name) && family_name.ends_with("aceae"),
                "{family_name}"
            );
            for genus in entries(&family) {
                let genus_name = genus.file_name().unwrap().to_str().unwrap();
                assert!(genus.is_dir(), "{} is not a genus folder", genus.display());
                assert!(
                    lower_name(genus_name) && !genus_name.contains('-'),
                    "{genus_name}"
                );
                for folder in entries(&genus) {
                    let id = folder.file_name().unwrap().to_str().unwrap();
                    assert!(
                        folder.is_dir(),
                        "{} is not a species folder",
                        folder.display()
                    );
                    assert!(lower_name(id), "{id}");
                    assert!(
                        id.starts_with(&format!("{genus_name}-")),
                        "{id} under {genus_name}"
                    );
                    let files: Vec<String> = entries(&folder)
                        .iter()
                        .map(|file| file.file_name().unwrap().to_str().unwrap().to_owned())
                        .collect();
                    assert_eq!(files, ["spec.json"], "{id}");
                    let spec: serde_json::Value = serde_json::from_str(
                        &fs::read_to_string(folder.join("spec.json")).unwrap(),
                    )
                    .unwrap();
                    assert_eq!(spec["id"], id, "{id}");
                    assert!(ids.insert(id.to_owned()), "{id} twice");
                    walked += 1;
                }
            }
        }
        assert_eq!(
            walked,
            LIBRARY.len(),
            "the build script compiles every folder in"
        );
    }

    #[test]
    fn the_library_is_sorted_and_found_by_id() {
        let keys: Vec<(&str, &str, &str)> = LIBRARY
            .iter()
            .map(|species| (species.family, species.genus, species.id))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
        for entry in LIBRARY {
            assert_eq!(species(entry.id), Some(entry));
            assert_eq!(source(entry.id), entry.source);
        }
        assert_eq!(species("acer-unknown"), None);
    }
}

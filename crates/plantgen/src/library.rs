//! The species and programs a build reads.
//!
//! The built-in library is the species compiled into `PlantGen`, one folder
//! each, `library/<family>/<genus>/<id>/spec.json`, and the programs in
//! `programs/`. The family is that of the species' accepted name in the
//! World Checklist of Vascular Plants (WCVP v16); the genus folder is the
//! genus the id is written with. The build script walks the tree and
//! compiles every spec in, so the tree is the index: adding a species is
//! adding its folder.
//!
//! A [`Library`] is the built-in one ([`Library::builtin`]), or a folder in
//! the same layout read on top of it ([`Library::from_dir`]): its species
//! and programs replace built-in ones of the same id or name, and add to
//! them. A package's key hashes a spec's and a program's text, never where
//! they came from, so a file holding a built-in species' text builds that
//! species' built-in package.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::conditions::Conditions;
use crate::niche::Niche;
use crate::shed::Shed;
use crate::spec::{PROGRAMS, PlantSpec, SpecError};

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
    /// Its typical site, a conditions document, as JSON.
    pub conditions: Option<&'static str>,
    /// What falls from it ([`crate::shed`]), as JSON.
    pub shed: Option<&'static str>,
    /// Where it grows ([`crate::niche`]), as JSON.
    pub niche: Option<&'static str>,
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

/// The niche of the species `id` (`niche.json`), as JSON, for use in
/// constants, as [`source`] is: a list of species built from it does not
/// compile if one lacks a niche.
///
/// # Panics
///
/// Panics, at compile time when used in a constant, if the library has no
/// species `id` or it has no niche.
#[must_use]
pub const fn niche_source(id: &str) -> &'static str {
    let mut index = 0;
    while index < LIBRARY.len() {
        if same(LIBRARY[index].id, id) {
            match LIBRARY[index].niche {
                Some(niche) => return niche,
                None => panic!("this built-in species has no niche"),
            }
        }
        index += 1;
    }
    panic!("the built-in library has no species with this id")
}

/// Species and programs: the built-in ones, or a folder's on top of them.
#[derive(Debug, Clone)]
pub struct Library {
    root: Option<PathBuf>,
    /// Every species, sorted by family, genus and id.
    species: Vec<Entry>,
    /// Each species' place in `species`, by id.
    index: BTreeMap<String, usize>,
    programs: BTreeMap<String, Cow<'static, str>>,
}

/// One species of a [`Library`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub family: String,
    pub genus: String,
    /// The spec's file, for a species read from a folder; `None` for a
    /// built-in one.
    pub path: Option<PathBuf>,
    source: Cow<'static, str>,
    conditions: Option<Cow<'static, str>>,
    shed: Option<Cow<'static, str>>,
    niche: Option<Cow<'static, str>>,
}

impl Entry {
    /// Its spec, as JSON.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Its typical site (`conditions.json`), as JSON, if it has one.
    #[must_use]
    pub fn conditions_source(&self) -> Option<&str> {
        self.conditions.as_deref()
    }

    /// Its typical site: the conditions a viewer shows and `plantc grow`
    /// uses unless told otherwise. Today it names a preset only.
    ///
    /// # Errors
    ///
    /// Fails if its `conditions.json` is invalid.
    pub fn conditions(&self) -> Result<Option<Conditions>, String> {
        self.conditions
            .as_deref()
            .map(Conditions::from_json)
            .transpose()
            .map_err(|error| format!("{}'s conditions.json: {error}", self.id))
    }

    /// What falls from it (`shed.json`), if it has the section.
    ///
    /// # Errors
    ///
    /// Fails if its `shed.json` is invalid or names another species.
    pub fn shed(&self) -> Result<Option<Shed>, String> {
        let Some(text) = self.shed.as_deref() else {
            return Ok(None);
        };
        let shed =
            Shed::from_json(text).map_err(|error| format!("{}'s shed.json: {error}", self.id))?;
        if shed.id != self.id {
            return Err(format!(
                "{}'s shed.json names `{}`; it must name its folder's species",
                self.id, shed.id
            ));
        }
        Ok(Some(shed))
    }

    /// Where it grows (`niche.json`), if it has the section.
    ///
    /// # Errors
    ///
    /// Fails if its `niche.json` is invalid or names another species.
    pub fn niche(&self) -> Result<Option<Niche>, String> {
        let Some(text) = self.niche.as_deref() else {
            return Ok(None);
        };
        let niche =
            Niche::from_json(text).map_err(|error| format!("{}'s niche.json: {error}", self.id))?;
        if niche.species != self.id {
            return Err(format!(
                "{}'s niche.json names `{}`; it must name its folder's species",
                self.id, niche.species
            ));
        }
        Ok(Some(niche))
    }

    /// Check every section beside the spec.
    fn check_sections(&self) -> Result<(), String> {
        self.conditions()?;
        self.shed()?;
        self.niche()?;
        Ok(())
    }
}

/// A library folder that cannot be read, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryError(pub String);

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LibraryError {}

impl Library {
    /// The built-in library: the species and programs compiled in.
    #[must_use]
    pub fn builtin() -> &'static Self {
        static BUILTIN: OnceLock<Library> = OnceLock::new();
        BUILTIN.get_or_init(|| {
            let species: Vec<Entry> = LIBRARY
                .iter()
                .map(|species| Entry {
                    id: species.id.to_string(),
                    family: species.family.to_string(),
                    genus: species.genus.to_string(),
                    path: None,
                    source: Cow::Borrowed(species.source),
                    conditions: species.conditions.map(Cow::Borrowed),
                    shed: species.shed.map(Cow::Borrowed),
                    niche: species.niche.map(Cow::Borrowed),
                })
                .collect();
            Self {
                root: None,
                index: index(&species),
                species,
                programs: PROGRAMS
                    .iter()
                    .map(|(name, source)| ((*name).to_string(), Cow::Borrowed(*source)))
                    .collect(),
            }
        })
    }

    /// The built-in library with the folder `root` read on top of it: a
    /// species tree in `root/library/<family>/<genus>/<id>/spec.json` and
    /// programs in `root/programs/<name>.lsys`, either of which may be
    /// missing but not both. A species or program replaces a built-in one
    /// of the same id or name. Every spec and the sections beside it
    /// (`conditions.json`, `shed.json`, `niche.json`) are read and checked
    /// now; other files beside them (sections a later revision reads) are
    /// left alone.
    ///
    /// # Errors
    ///
    /// Fails on a folder or file that cannot be read, a folder named
    /// against the tree's rules, a species folder without its
    /// `spec.json`, a spec whose `id` is not its folder's, an invalid
    /// section or one naming another species, an id twice, or a program
    /// file whose name is not a program name.
    pub fn from_dir(root: &Path) -> Result<Self, LibraryError> {
        let tree = root.join("library");
        let programs = root.join("programs");
        if !tree.is_dir() && !programs.is_dir() {
            return Err(LibraryError(format!(
                "{} holds neither a library/ nor a programs/ folder",
                root.display()
            )));
        }
        let mut library = Self::builtin().clone();
        library.root = Some(root.to_path_buf());
        if tree.is_dir() {
            let mut read: BTreeMap<String, Entry> = BTreeMap::new();
            for family in folders(&tree)? {
                let family_name = folder_name(&family, "a family", is_taxon_name)?;
                for genus in folders(&family)? {
                    let genus_name = folder_name(&genus, "a genus", is_taxon_name)?;
                    for folder in folders(&genus)? {
                        let id = folder_name(&folder, "a species", is_id)?;
                        if !id.starts_with(&format!("{genus_name}-")) {
                            return Err(LibraryError(format!(
                                "{} is filed under the genus {genus_name}; its id must start with `{genus_name}-`",
                                folder.display()
                            )));
                        }
                        let path = folder.join("spec.json");
                        let source = fs::read_to_string(&path).map_err(|error| {
                            LibraryError(format!("cannot read {}: {error}", path.display()))
                        })?;
                        let named = serde_json::from_str::<serde_json::Value>(&source)
                            .ok()
                            .and_then(|spec| spec.get("id")?.as_str().map(str::to_string));
                        if named.as_deref() != Some(id.as_str()) {
                            return Err(LibraryError(format!(
                                "{} must be the spec of `{id}`, its folder's name; it names {}",
                                path.display(),
                                named.map_or_else(
                                    || "no id".to_string(),
                                    |named| format!("`{named}`")
                                )
                            )));
                        }
                        if read.contains_key(&id) {
                            return Err(LibraryError(format!(
                                "{} is the second folder for `{id}`",
                                folder.display()
                            )));
                        }
                        let entry = Entry {
                            id: id.clone(),
                            family: family_name.clone(),
                            genus: genus_name.clone(),
                            path: Some(path),
                            source: Cow::Owned(source),
                            conditions: section(&folder, "conditions.json")?,
                            shed: section(&folder, "shed.json")?,
                            niche: section(&folder, "niche.json")?,
                        };
                        entry.check_sections().map_err(|error| {
                            LibraryError(format!("{}: {error}", folder.display()))
                        })?;
                        read.insert(id, entry);
                    }
                }
            }
            library
                .species
                .retain(|entry| !read.contains_key(&entry.id));
            library.species.extend(read.into_values());
            library
                .species
                .sort_by(|a, b| (&a.family, &a.genus, &a.id).cmp(&(&b.family, &b.genus, &b.id)));
            library.index = index(&library.species);
        }
        if programs.is_dir() {
            for path in files(&programs)? {
                if path.extension().is_none_or(|extension| extension != "lsys") {
                    continue;
                }
                let name = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|stem| is_id(stem))
                    .ok_or_else(|| {
                        LibraryError(format!(
                            "{} is not named as a program: lowercase letters, digits and hyphens",
                            path.display()
                        ))
                    })?;
                let source = fs::read_to_string(&path).map_err(|error| {
                    LibraryError(format!("cannot read {}: {error}", path.display()))
                })?;
                library
                    .programs
                    .insert(name.to_string(), Cow::Owned(source));
            }
        }
        Ok(library)
    }

    /// The folder the library was read from; `None` for the built-in one.
    #[must_use]
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Every species, sorted by family, genus and id.
    #[must_use]
    pub fn species(&self) -> &[Entry] {
        &self.species
    }

    /// The species `id`, if the library has it.
    #[must_use]
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.index.get(id).map(|&place| &self.species[place])
    }

    /// The species `id`, parsed and checked against this library.
    ///
    /// # Errors
    ///
    /// Fails if the library has no such species or its spec is invalid.
    pub fn spec(&self, id: &str) -> Result<PlantSpec, SpecError> {
        let entry = self.entry(id).ok_or_else(|| {
            let known: Vec<&str> = self.species.iter().map(|entry| entry.id.as_str()).collect();
            SpecError(match &self.root {
                None => format!(
                    "no built-in species `{id}`; built-in species: {}",
                    known.join(", ")
                ),
                Some(root) => format!(
                    "no species `{id}` in {} or the built-in library; species: {}",
                    root.display(),
                    known.join(", ")
                ),
            })
        })?;
        PlantSpec::from_json_in(entry.source(), self).map_err(|error| match &entry.path {
            Some(path) => SpecError(format!("{}: {error}", path.display())),
            None => error,
        })
    }

    /// The program `name`'s source, if the library has it.
    #[must_use]
    pub fn program(&self, name: &str) -> Option<&str> {
        self.programs.get(name).map(AsRef::as_ref)
    }

    /// Every program's name, sorted.
    pub fn programs(&self) -> impl Iterator<Item = &str> {
        self.programs.keys().map(String::as_str)
    }
}

/// The section `name` beside a spec in `folder`, if the folder has it.
fn section(folder: &Path, name: &str) -> Result<Option<Cow<'static, str>>, LibraryError> {
    let path = folder.join(name);
    if !path.is_file() {
        return Ok(None);
    }
    fs::read_to_string(&path)
        .map(|text| Some(Cow::Owned(text)))
        .map_err(|error| LibraryError(format!("cannot read {}: {error}", path.display())))
}

fn index(species: &[Entry]) -> BTreeMap<String, usize> {
    species
        .iter()
        .enumerate()
        .map(|(place, entry)| (entry.id.clone(), place))
        .collect()
}

/// A species id or program name: 1 to 64 lowercase letters, digits and
/// hyphens.
fn is_id(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// A family's or genus's folder: its name in lowercase letters.
fn is_taxon_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_lowercase())
}

fn folder_name(path: &Path, what: &str, rule: fn(&str) -> bool) -> Result<String, LibraryError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| rule(name))
        .map(str::to_string)
        .ok_or_else(|| {
            LibraryError(format!(
                "{} is not named as {what} folder: lowercase letters{}",
                path.display(),
                if what == "a species" {
                    ", digits and hyphens"
                } else {
                    ""
                }
            ))
        })
}

fn entries(path: &Path) -> Result<Vec<PathBuf>, LibraryError> {
    let mut found = fs::read_dir(path)
        .and_then(|entries| {
            entries
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|error| LibraryError(format!("cannot read {}: {error}", path.display())))?;
    found.sort();
    Ok(found)
}

/// The folders directly inside `path`, sorted; files there are left alone.
fn folders(path: &Path) -> Result<Vec<PathBuf>, LibraryError> {
    Ok(entries(path)?
        .into_iter()
        .filter(|entry| entry.is_dir())
        .collect())
}

/// The files directly inside `path`, sorted.
fn files(path: &Path) -> Result<Vec<PathBuf>, LibraryError> {
    Ok(entries(path)?
        .into_iter()
        .filter(|entry| entry.is_file())
        .collect())
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

    use super::{LIBRARY, Library, niche_source, source, species};
    use crate::spec::{PROGRAMS, PlantSpec};

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
    /// the folder's name and starts with the genus folder's, and its
    /// typical site, `conditions.json`, and where those sections are
    /// written, where it grows, `niche.json`, and what falls from it,
    /// `shed.json`; families end in "aceae"; no id twice, and nothing
    /// anywhere else.
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
                    // Its record's sections: the spec and its typical site,
                    // and where it grows and what falls from it where those
                    // sections are written.
                    let optional = ["niche.json", "shed.json"];
                    let expected: Vec<&str> = ["conditions.json", "spec.json"]
                        .into_iter()
                        .chain(
                            optional
                                .into_iter()
                                .filter(|name| files.iter().any(|file| file == name)),
                        )
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect();
                    assert_eq!(files, expected, "{id}");
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

    /// Every built-in species' typical site is valid and names a preset
    /// its variants grow in, so `plantc grow` grows as before.
    #[test]
    fn every_typical_site_names_one_of_its_environments() {
        for entry in Library::builtin().species() {
            let conditions = entry.conditions().unwrap().unwrap();
            let spec = Library::builtin().spec(&entry.id).unwrap();
            assert_eq!(
                conditions.preset,
                Some(spec.variants.environments[0]),
                "{}",
                entry.id
            );
        }
    }

    /// Every built-in section is valid and names its folder's species.
    #[test]
    fn every_built_in_section_is_valid() {
        for entry in Library::builtin().species() {
            entry.check_sections().unwrap();
        }
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

    /// A folder of its own under the system's temporary folder, removed
    /// when dropped.
    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("plantgen-library-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, relative: &str, text: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_built_in_library_is_the_tree_and_the_programs() {
        let library = Library::builtin();
        assert_eq!(library.root(), None);
        assert_eq!(library.species().len(), LIBRARY.len());
        for (entry, species) in library.species().iter().zip(LIBRARY) {
            assert_eq!(
                (
                    entry.id.as_str(),
                    entry.family.as_str(),
                    entry.genus.as_str()
                ),
                (species.id, species.family, species.genus)
            );
            assert_eq!(entry.source(), species.source);
            assert_eq!(entry.path, None);
        }
        assert_eq!(library.programs().count(), PROGRAMS.len());
        for (name, program) in PROGRAMS {
            assert_eq!(library.program(name), Some(program));
        }
        assert!(
            library
                .spec("acer-unknown")
                .unwrap_err()
                .0
                .starts_with("no built-in species `acer-unknown`")
        );
    }

    #[test]
    fn a_folder_replaces_and_adds_species_and_programs() {
        let folder = Folder::new("overlay");
        let douglas_fir = source("pseudotsuga-menziesii")
            .replace("\"Douglas-fir\"", "\"Douglas fir, from a file\"");
        assert_ne!(douglas_fir, source("pseudotsuga-menziesii"));
        folder.write(
            "library/pinaceae/pseudotsuga/pseudotsuga-menziesii/spec.json",
            &douglas_fir,
        );
        // A species the built-ins lack, grown by a program they lack, with
        // its niche, and a section beside its spec that this revision does
        // not read.
        let new_species = source("thuja-plicata")
            .replace("\"thuja-plicata\"", "\"zelkova-test\"")
            .replace("\"program\": \"conifer\"", "\"program\": \"conifer-test\"");
        folder.write(
            "library/ulmaceae/zelkova/zelkova-test/spec.json",
            &new_species,
        );
        folder.write(
            "library/ulmaceae/zelkova/zelkova-test/niche.json",
            &niche_source("thuja-plicata").replace("\"thuja-plicata\"", "\"zelkova-test\""),
        );
        folder.write("library/ulmaceae/zelkova/zelkova-test/wood.json", "{}");
        folder.write("library/aliases.json", "{}");
        let conifer = source_of_program("conifer");
        folder.write("programs/conifer-test.lsys", conifer);
        folder.write("programs/README.md", "not a program");

        let library = Library::from_dir(&folder.0).unwrap();
        assert_eq!(library.root(), Some(folder.0.as_path()));
        assert_eq!(library.species().len(), LIBRARY.len() + 1);
        let entry = library.entry("pseudotsuga-menziesii").unwrap();
        assert_eq!(entry.source(), douglas_fir);
        assert!(
            entry
                .path
                .as_ref()
                .unwrap()
                .ends_with("pseudotsuga-menziesii/spec.json")
        );
        assert_eq!(
            library
                .spec("pseudotsuga-menziesii")
                .unwrap()
                .taxon
                .common_name,
            "Douglas fir, from a file"
        );
        let keys: Vec<(&str, &str, &str)> = library
            .species()
            .iter()
            .map(|entry| {
                (
                    entry.family.as_str(),
                    entry.genus.as_str(),
                    entry.id.as_str(),
                )
            })
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
        assert_eq!(library.program("conifer-test"), Some(conifer));
        assert_eq!(library.programs().count(), PROGRAMS.len() + 1);
        let spec = library.spec("zelkova-test").unwrap();
        spec.program_in(&library).unwrap();
        let niche = library.entry("zelkova-test").unwrap().niche().unwrap();
        assert_eq!(niche.unwrap().species, "zelkova-test");
        // The built-in library knows neither.
        assert!(PlantSpec::builtin("zelkova-test").is_err());
        assert!(
            spec.validate()
                .unwrap_err()
                .0
                .contains("unknown program `conifer-test`; built-in programs:")
        );
    }

    #[test]
    fn folders_against_the_rules_are_refused() {
        let refused = |files: &[(&str, &str)], expected: &str| {
            let folder = Folder::new("refused");
            for (relative, text) in files {
                folder.write(relative, text);
            }
            let message = Library::from_dir(&folder.0).unwrap_err().0;
            assert!(message.contains(expected), "{message}");
        };
        let douglas_fir = source("pseudotsuga-menziesii");
        refused(
            &[("notes.txt", "")],
            "holds neither a library/ nor a programs/ folder",
        );
        refused(
            &[(
                "library/pinaceae/abies/pseudotsuga-menziesii/spec.json",
                douglas_fir,
            )],
            "its id must start with `abies-`",
        );
        refused(
            &[(
                "library/Pinaceae/pseudotsuga/pseudotsuga-menziesii/spec.json",
                douglas_fir,
            )],
            "is not named as a family folder",
        );
        refused(
            &[(
                "library/pinaceae/pseudotsuga/pseudotsuga-menziesii/niche.json",
                "{}",
            )],
            "cannot read",
        );
        refused(
            &[(
                "library/pinaceae/pseudotsuga/pseudotsuga-glauca/spec.json",
                douglas_fir,
            )],
            "must be the spec of `pseudotsuga-glauca`, its folder's name; it names `pseudotsuga-menziesii`",
        );
        refused(
            &[
                (
                    "library/pinaceae/pseudotsuga/pseudotsuga-menziesii/spec.json",
                    douglas_fir,
                ),
                (
                    "library/rosaceae/pseudotsuga/pseudotsuga-menziesii/spec.json",
                    douglas_fir,
                ),
            ],
            "is the second folder for `pseudotsuga-menziesii`",
        );
        refused(
            &[("programs/Conifer.lsys", "")],
            "is not named as a program",
        );
        // Its sections are checked, and name its species.
        let fir_at =
            |file: &str| format!("library/pinaceae/pseudotsuga/pseudotsuga-menziesii/{file}");
        let spec_path = fir_at("spec.json");
        let niche_path = fir_at("niche.json");
        refused(
            &[(&spec_path, douglas_fir), (&niche_path, "{}")],
            "pseudotsuga-menziesii's niche.json: invalid niche",
        );
        let cedar = niche_source("thuja-plicata");
        refused(
            &[(&spec_path, douglas_fir), (&niche_path, cedar)],
            "niche.json names `thuja-plicata`; it must name its folder's species",
        );
    }

    /// A package's key hashes the spec's and the program's text, never
    /// where they came from: files holding the built-in text build the
    /// built-in package, a guest's host included, and a changed program
    /// another.
    #[test]
    fn files_with_the_built_in_text_build_the_built_in_package() {
        use crate::package::{DEFAULT_DAY, Inputs};
        use crate::quality::STANDARD;
        let folder = Folder::new("keys");
        for id in ["hedera-helix", "acer-macrophyllum"] {
            let entry = Library::builtin().entry(id).unwrap();
            folder.write(
                &format!("library/{}/{}/{id}/spec.json", entry.family, entry.genus),
                entry.source(),
            );
        }
        for (name, program) in PROGRAMS {
            folder.write(&format!("programs/{name}.lsys"), program);
        }
        let library = Library::from_dir(&folder.0).unwrap();
        for id in ["hedera-helix", "acer-macrophyllum"] {
            let built_in =
                Inputs::on_day(&PlantSpec::builtin(id).unwrap(), &STANDARD, DEFAULT_DAY).unwrap();
            let from_files =
                Inputs::on_day_in(&library.spec(id).unwrap(), &library, &STANDARD, DEFAULT_DAY)
                    .unwrap();
            assert_eq!(from_files.key, built_in.key, "{id}");
        }
        let program = library.program("climber").unwrap();
        folder.write("programs/climber.lsys", &format!("{program}\n# changed\n"));
        let changed = Library::from_dir(&folder.0).unwrap();
        let spec = changed.spec("hedera-helix").unwrap();
        let built_in = Inputs::on_day(
            &PlantSpec::builtin("hedera-helix").unwrap(),
            &STANDARD,
            DEFAULT_DAY,
        )
        .unwrap();
        assert_ne!(
            Inputs::on_day_in(&spec, &changed, &STANDARD, DEFAULT_DAY)
                .unwrap()
                .key,
            built_in.key
        );
    }

    fn source_of_program(name: &str) -> &'static str {
        PROGRAMS
            .iter()
            .find(|(program, _)| *program == name)
            .map(|(_, source)| *source)
            .unwrap()
    }
}

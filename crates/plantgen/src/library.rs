//! The species and programs a build reads.
//!
//! The built-in library is the species compiled into `PlantGen`, one folder
//! each, `library/<family>/<genus>/<id>/spec.json`, and the programs in
//! `programs/`. The family is that of the species' accepted name in the
//! World Checklist of Vascular Plants (WCVP v16); the genus folder is the
//! genus the id is written with. The build script walks the tree and
//! compiles every spec in, so the tree is the index: adding a species is
//! adding its folder. A taxon below a species (a subspecies, variety,
//! form or cultivar) has its folder in its species', named by its species'
//! id, its rank and its epithet
//! (`pinus/pinus-contorta/pinus-contorta-var-contorta/`); a species folder
//! holding such folders holds nothing else yet. Ids follow WCVP's
//! accepted names ([`Library::check_taxa`]): when a taxon is renamed its
//! folder moves, and `library/aliases.json` maps its old id to the new one
//! ([`Library::current`]), so a host holding the old id still finds it.
//!
//! A [`Library`] is the built-in one ([`Library::builtin`]), or a folder in
//! the same layout read on top of it ([`Library::from_dir`]): its species
//! and programs replace built-in ones of the same id or name, and add to
//! them. A package's key hashes a spec's and a program's text, never where
//! they came from, so a file holding a built-in species' text builds that
//! species' built-in package.
//!
//! Beside the families, `library/sources/<id>.json` holds the sources that
//! evidence notes cite by id ([`crate::evidence::Source`]), one file each,
//! so every value from one source can be listed ([`Library::citations`])
//! and removed together.
//!
//! Taxa above species may have specs of their own, *rank files*
//! ([`crate::inherit`], growth plan G2): `<family>/family.json`,
//! `<family>/<genus>/genus.json`, and `_ranks/<rank>/<name>.json` for the
//! orders, clades and other ranks, each naming its parent. A species'
//! spec is then its own `spec.json` merged onto the rank files above it,
//! the nearer winning ([`Entry::source`]); [`Library::inherited`] says
//! which file set each value. A folder's rank files replace built-in ones
//! at the same path and add to them, and built-in species inherit them
//! too.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;

use crate::conditions::Conditions;
use crate::evidence::{FieldEvidence, Source};
use crate::inherit::{self, Inherited, Part, Programs, RankFile};
use crate::niche::Niche;
use crate::shed::Shed;
use crate::spec::{GrowthForm, PROGRAMS, PlantSpec, SpecError};
use crate::traits::Vocabulary;

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
    /// For a taxon below a species, that species' folder, which holds its
    /// own; `None` for a species.
    pub species: Option<&'static str>,
    /// Its spec, as JSON: its own `spec.json`, merged onto the rank files
    /// above it where there are any ([`crate::inherit`]).
    pub source: &'static str,
    /// Its own `spec.json`, as written.
    pub own: &'static str,
    /// Its typical site, a conditions document, as JSON.
    pub conditions: Option<&'static str>,
    /// What falls from it ([`crate::shed`]), as JSON.
    pub shed: Option<&'static str>,
    /// Where it grows ([`crate::niche`]), as JSON.
    pub niche: Option<&'static str>,
}

include!(concat!(env!("OUT_DIR"), "/library.rs"));

/// The species with id `id`, or with the id `id` was renamed to, if the
/// library has it.
#[must_use]
pub fn species(id: &str) -> Option<&'static Species> {
    let id = followed(id);
    LIBRARY.iter().find(|species| species.id == id)
}

/// `id`, or where it is the old id of a renamed taxon
/// (`library/aliases.json`), its current id.
const fn followed(id: &str) -> &str {
    let mut index = 0;
    while index < ALIASES.len() {
        if same(ALIASES[index].0, id) {
            return ALIASES[index].1;
        }
        index += 1;
    }
    id
}

/// The spec of the species `id`, as JSON, for use in constants: a
/// catalogue built from it does not compile if the library lacks a species
/// it names. An old id finds the species it was renamed to.
///
/// # Panics
///
/// Panics, at compile time when used in a constant, if the library has no
/// species `id`.
#[must_use]
pub const fn source(id: &str) -> &'static str {
    let id = followed(id);
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
    let id = followed(id);
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
    /// Each program that extends another, joined with the programs it
    /// builds on (see [`Library::program`]).
    chains: BTreeMap<String, String>,
    /// The sources evidence notes cite, by id.
    sources: BTreeMap<String, Source>,
    /// The specs of taxa above species, by their path in the tree
    /// ([`crate::inherit`]).
    ranks: BTreeMap<String, RankFile>,
    /// The old ids of renamed taxa (`library/aliases.json`), by old id.
    aliases: BTreeMap<String, Alias>,
    /// Each program's parameters, its inherited ones included, which
    /// rules are checked against.
    params: Programs,
}

/// One value that cites a source: the file holding its evidence note, by
/// its path in the tree (a species' `spec.json`, `niche.json` or
/// `shed.json`, or a rank file), and the note's path within it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    pub file: String,
    pub path: String,
}

/// The id a renamed taxon now has, for its old id
/// (`library/aliases.json`), with the evidence note saying why it moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias {
    pub id: String,
    pub evidence: FieldEvidence,
}

/// `library/aliases.json`: each old id with the id it stands for, and an
/// evidence note on each (`aliases.<old id>`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AliasesFile {
    schema: u32,
    aliases: BTreeMap<String, String>,
    #[serde(default)]
    evidence: BTreeMap<String, FieldEvidence>,
}

/// The aliases of the text of an `aliases.json`, each with its note.
fn parse_aliases(text: &str) -> Result<BTreeMap<String, Alias>, String> {
    let file: AliasesFile = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if file.schema != 1 {
        return Err(format!("schema {} is not 1", file.schema));
    }
    let mut notes = file.evidence;
    let aliases = file
        .aliases
        .into_iter()
        .map(|(old, id)| {
            let evidence = notes
                .remove(&format!("aliases.{old}"))
                .ok_or_else(|| format!("the alias `{old}` has no evidence note"))?;
            evidence
                .check()
                .map_err(|error| format!("the evidence note on `aliases.{old}`: {error}"))?;
            Ok((old, Alias { id, evidence }))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    match notes.keys().next() {
        Some(path) => Err(format!("the evidence note on `{path}` is on no alias")),
        None => Ok(aliases),
    }
}

/// One species of a [`Library`], or a taxon below a species.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub family: String,
    pub genus: String,
    /// For a taxon below a species, that species' folder, which holds its
    /// own; `None` for a species.
    pub species: Option<String>,
    /// The spec's file, for a species read from a folder; `None` for a
    /// built-in one.
    pub path: Option<PathBuf>,
    /// Its own `spec.json`, as written.
    own: Cow<'static, str>,
    /// Its spec merged onto the rank files above it, where there are any.
    merged: Option<Cow<'static, str>>,
    conditions: Option<Cow<'static, str>>,
    shed: Option<Cow<'static, str>>,
    niche: Option<Cow<'static, str>>,
}

impl Entry {
    /// Its spec, as JSON: its own `spec.json`, merged onto the rank files
    /// above it where there are any (growth plan G2), else that file's
    /// text as written.
    #[must_use]
    pub fn source(&self) -> &str {
        self.merged.as_deref().unwrap_or(&self.own)
    }

    /// Its own `spec.json`, as written.
    #[must_use]
    pub fn own_source(&self) -> &str {
        &self.own
    }

    /// Its own `spec.json`'s path in the library tree, which names it
    /// where values and notes say where they came from.
    #[must_use]
    pub fn file(&self) -> String {
        format!("{}/spec.json", self.folder())
    }

    /// Its folder's path in the library tree.
    #[must_use]
    pub fn folder(&self) -> String {
        match &self.species {
            None => format!("{}/{}/{}", self.family, self.genus, self.id),
            Some(species) => format!("{}/{}/{species}/{}", self.family, self.genus, self.id),
        }
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

    /// The evidence notes of its own spec, niche and shed, each with the
    /// file holding it, by its path in the tree, and its path.
    fn notes(&self) -> Result<Vec<Note>, String> {
        let folder = self.folder();
        let own: Value = serde_json::from_str(&self.own)
            .map_err(|error| format!("{}'s spec.json: {error}", self.id))?;
        let evidence: BTreeMap<String, FieldEvidence> = own
            .get("evidence")
            .map(|evidence| serde_json::from_value(evidence.clone()))
            .transpose()
            .map_err(|error| format!("{}'s spec.json: evidence: {error}", self.id))?
            .unwrap_or_default();
        let mut notes: Vec<Note> = evidence
            .into_iter()
            .map(|(path, note)| (format!("{folder}/spec.json"), path, note))
            .collect();
        if let Some(niche) = self.niche()? {
            notes.extend(
                niche
                    .evidence
                    .into_iter()
                    .map(|(path, note)| (format!("{folder}/niche.json"), path, note)),
            );
        }
        if let Some(shed) = self.shed()? {
            notes.extend(
                shed.evidence
                    .into_iter()
                    .map(|(path, note)| (format!("{folder}/shed.json"), path, note)),
            );
        }
        Ok(notes)
    }

    /// Check every section beside the spec.
    fn check_sections(&self) -> Result<(), String> {
        self.conditions()?;
        self.shed()?;
        self.niche()?;
        Ok(())
    }
}

/// An evidence note, with the file holding it (by its path in the tree)
/// and its path there.
type Note = (String, String, FieldEvidence);

/// A rank file's evidence notes, each with the file and its path there
/// (a form's under `forms.<growth form>.`).
fn rank_notes(rank: &RankFile) -> Result<Vec<Note>, String> {
    let mut notes = Vec::new();
    for (at, part) in rank.parts() {
        for (path, note) in &part.notes {
            let path = if at.is_empty() {
                path.clone()
            } else {
                format!("{at}.{path}")
            };
            let note = serde_json::from_value(note.clone()).map_err(|error| {
                format!("{}: the evidence note on `{path}`: {error}", rank.file)
            })?;
            notes.push((rank.file.clone(), path, note));
        }
    }
    Ok(notes)
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
    ///
    /// # Panics
    ///
    /// Panics on a compiled-in source or rank file that is invalid, which
    /// the library's tests rule out.
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
                    species: species.species.map(str::to_string),
                    path: None,
                    own: Cow::Borrowed(species.own),
                    // The build script merged it where rank files stand
                    // above it.
                    merged: (species.source != species.own)
                        .then_some(Cow::Borrowed(species.source)),
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
                chains: BTreeMap::new(),
                sources: SOURCES
                    .iter()
                    .map(|(id, text)| {
                        let source = Source::from_json(text)
                            .unwrap_or_else(|error| panic!("library/sources/{id}.json: {error}"));
                        assert_eq!(
                            source.id, *id,
                            "library/sources/{id}.json must name its own file's id"
                        );
                        ((*id).to_string(), source)
                    })
                    .collect(),
                ranks: RANKS
                    .iter()
                    .map(|(file, text)| {
                        let rank = RankFile::parse(file, text)
                            .unwrap_or_else(|error| panic!("library/{file}: {error}"));
                        ((*file).to_string(), rank)
                    })
                    .collect(),
                aliases: ALIASES_FILE
                    .map(|text| {
                        parse_aliases(text)
                            .unwrap_or_else(|error| panic!("library/aliases.json: {error}"))
                    })
                    .unwrap_or_default(),
                params: inherit::program_params(PROGRAMS.iter().copied()),
            }
            .with_chains()
        })
    }

    /// The built-in library with the folder `root` read on top of it: a
    /// species tree in `root/library/<family>/<genus>/<id>/spec.json` (a
    /// taxon below a species in `<genus>/<species>/<id>/`) and
    /// programs in `root/programs/<name>.lsys`, either of which may be
    /// missing but not both. A species or program replaces a built-in one
    /// of the same id or name, a rank file (`library/_ranks/…`,
    /// `family.json`, `genus.json`) the built-in one at the same path, and
    /// an alias in `library/aliases.json` the built-in one of the same old
    /// id.
    /// Every spec, merged onto the rank files above it, the sections
    /// beside it (`conditions.json`, `shed.json`, `niche.json`) and every
    /// rank file are read and checked now; other files beside them
    /// (sections a later revision reads) are left alone.
    ///
    /// # Errors
    ///
    /// Fails on a folder or file that cannot be read, a folder named
    /// against the tree's rules, a species folder without its
    /// `spec.json`, a spec whose `id` is not its folder's, an invalid
    /// spec, section, rank file or alias, a section naming another
    /// species, an id twice, an old id still in use, or a program file
    /// whose name is not a program name.
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
        if tree.join("sources").is_dir() {
            for path in files(&tree.join("sources"))? {
                if path.extension().is_none_or(|extension| extension != "json") {
                    continue;
                }
                let id = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|stem| crate::evidence::is_source_id(stem))
                    .ok_or_else(|| {
                        LibraryError(format!(
                            "{} is not named as a source: lowercase letters, digits and hyphens",
                            path.display()
                        ))
                    })?;
                let text = fs::read_to_string(&path).map_err(|error| {
                    LibraryError(format!("cannot read {}: {error}", path.display()))
                })?;
                let source = Source::from_json(&text)
                    .map_err(|error| LibraryError(format!("{}: {error}", path.display())))?;
                if source.id != id {
                    return Err(LibraryError(format!(
                        "{} must be the source `{id}`, its file's name; it names `{}`",
                        path.display(),
                        source.id
                    )));
                }
                library.sources.insert(id.to_string(), source);
            }
        }
        let rank_files = if tree.is_dir() {
            rank_files_in(&tree)?
        } else {
            Vec::new()
        };
        let ranks_read = !rank_files.is_empty();
        for (file, path) in rank_files {
            let text = fs::read_to_string(&path).map_err(|error| {
                LibraryError(format!("cannot read {}: {error}", path.display()))
            })?;
            let rank = RankFile::parse(&file, &text)
                .map_err(|error| LibraryError(format!("{}: {error}", path.display())))?;
            library.ranks.insert(file, rank);
        }
        if tree.is_dir() {
            let mut read: BTreeMap<String, Entry> = BTreeMap::new();
            for family in folders(&tree)? {
                if family
                    .file_name()
                    .is_some_and(|name| name == "sources" || name == inherit::RANKS)
                {
                    continue;
                }
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
                        if let Some(rank) = rank_in(&id) {
                            return Err(LibraryError(format!(
                                "{} names a taxon below a species (`-{rank}-`); its folder goes in its species' folder",
                                folder.display()
                            )));
                        }
                        let below = folders(&folder)?;
                        if below.is_empty() {
                            let place = (family_name.as_str(), genus_name.as_str(), None);
                            read_taxon(&mut read, place, &folder, id)?;
                            continue;
                        }
                        if let Some(file) = files(&folder)?.into_iter().find(|file| {
                            file.extension()
                                .is_some_and(|extension| extension == "json")
                        }) {
                            return Err(LibraryError(format!(
                                "{} holds the taxa below its species, so it holds no file of its own, as {} is: how they take the species' sections is not settled",
                                folder.display(),
                                file.display()
                            )));
                        }
                        for taxon in below {
                            let taxon_id = folder_name(&taxon, "a species", is_id)?;
                            if !is_below(&id, &taxon_id) {
                                return Err(LibraryError(format!(
                                    "{} is filed under the species {id}; its id must be `{id}-` and a rank (`subsp`, `var`, `f`, `cv`), a hyphen and an epithet",
                                    taxon.display()
                                )));
                            }
                            if let Some(inside) = folders(&taxon)?.first() {
                                return Err(LibraryError(format!(
                                    "{} is a folder in a taxon below a species; such a taxon holds no folders",
                                    inside.display()
                                )));
                            }
                            let place =
                                (family_name.as_str(), genus_name.as_str(), Some(id.as_str()));
                            read_taxon(&mut read, place, &taxon, taxon_id)?;
                        }
                    }
                }
            }
            if tree.join("aliases.json").is_file() {
                let path = tree.join("aliases.json");
                let text = fs::read_to_string(&path).map_err(|error| {
                    LibraryError(format!("cannot read {}: {error}", path.display()))
                })?;
                let aliases = parse_aliases(&text)
                    .map_err(|error| LibraryError(format!("{}: {error}", path.display())))?;
                library.aliases.extend(aliases);
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
        let programs_read = programs.is_dir();
        if programs_read {
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
        library.params = inherit::program_params(
            library
                .programs
                .iter()
                .map(|(name, source)| (name.as_str(), source.as_ref())),
        );
        let at_tree = |error: String| LibraryError(format!("{}/{error}", tree.display()));
        library.check_ranks().map_err(at_tree)?;
        library.check_traits().map_err(at_tree)?;
        // New rank files may stand above built-in species too, and new
        // programs change which rules apply.
        library
            .merge_specs(ranks_read || programs_read)
            .map_err(at_tree)?;
        for entry in &library.species {
            serde_json::from_str::<PlantSpec>(entry.source())
                .map_err(|error| at_tree(format!("{}: invalid spec: {error}", entry.file())))?;
        }
        library.check_citations().map_err(LibraryError)?;
        library.check_evidence().map_err(LibraryError)?;
        library.check_taxa().map_err(LibraryError)?;
        library.check_aliases().map_err(LibraryError)?;
        Ok(library.with_chains())
    }

    /// Merge each species' spec onto the rank files above it: every
    /// species' when `all`, else those read from a folder.
    fn merge_specs(&mut self, all: bool) -> Result<(), String> {
        for entry in &mut self.species {
            if !all && entry.path.is_none() {
                continue;
            }
            let above = inherit::above(&self.ranks, &entry.family, &entry.genus)?;
            entry.merged =
                inherit::effective_text(&entry.file(), &entry.own, &above, &self.params)?
                    .map(Cow::Owned);
        }
        Ok(())
    }

    /// Check every rank file against the library: the chain above it
    /// (each parent there, ranks in order, no loop), its forms (growth
    /// forms), the programs it names, and its values, which must fit a
    /// spec: laid over a built-in species' spec, without its nulls, each
    /// part parses as one.
    ///
    /// # Errors
    ///
    /// Names the first rank file that fails, and why.
    pub fn check_ranks(&self) -> Result<(), String> {
        if self.ranks.is_empty() {
            return Ok(());
        }
        let probes: Vec<Value> = Self::builtin()
            .species
            .iter()
            .filter_map(|entry| serde_json::from_str(entry.source()).ok())
            .collect();
        for rank in self.ranks.values() {
            inherit::chain(&self.ranks, &rank.file)?;
            for (at, part) in rank.parts() {
                let label = if at.is_empty() {
                    rank.file.clone()
                } else {
                    format!("{}, {at}", rank.file)
                };
                if let Some(form) = at.strip_prefix("forms.")
                    && serde_json::from_value::<GrowthForm>(Value::String(form.into())).is_err()
                {
                    return Err(format!("{label}: `{form}` is not a growth form"));
                }
                if let Some(program) = part
                    .values
                    .get("generator")
                    .and_then(|generator| generator.get("program"))
                    .and_then(Value::as_str)
                    && self.program(program).is_none()
                {
                    return Err(format!("{label}: unknown program `{program}`"));
                }
                fits(part, &probes)
                    .map_err(|error| format!("{label}: its values do not fit a spec: {error}"))?;
                Vocabulary::builtin()
                    .check_part(part, &self.params)
                    .map_err(|error| format!("{label}: {error}"))?;
                own_notes(&part.values, &part.traits, &part.rules, &part.notes)
                    .map_err(|error| format!("{label}: {error}"))?;
            }
        }
        Ok(())
    }

    /// Check every species' own spec as [`Library::check_ranks`] does a
    /// rank file's part: its traits and rules against the vocabulary and
    /// the programs, and a note of its own, citing a source, on each value,
    /// trait and rule it sets. Then the traits it inherits with its own:
    /// none of them a character that another of them rules out, such as
    /// bark on a herbaceous stem.
    ///
    /// # Errors
    ///
    /// Names the first species that fails, and why.
    pub fn check_traits(&self) -> Result<(), String> {
        let vocabulary = Vocabulary::builtin();
        for entry in &self.species {
            let own: Value = serde_json::from_str(&entry.own)
                .map_err(|error| format!("{}: {error}", entry.file()))?;
            let part = Part::of_spec(&own).map_err(|error| format!("{}: {error}", entry.file()))?;
            vocabulary
                .check_part(&part, &self.params)
                .map_err(|error| format!("{}: {error}", entry.file()))?;
            own_notes(&part.values, &part.traits, &part.rules, &part.notes)
                .map_err(|error| format!("{}: {error}", entry.file()))?;
        }
        for entry in &self.species {
            let traits = self.traits_of(&entry.id).map_err(|error| error.0)?;
            if let Some(key) = vocabulary.coverage(&traits).ruled_out.first() {
                let why = vocabulary.ruled_out_by(key, &traits).unwrap_or_default();
                return Err(format!(
                    "{}: it states `{key}` with what it inherits, but {why}",
                    entry.file()
                ));
            }
        }
        Ok(())
    }

    /// The traits the species `id` states, its own and those it inherits,
    /// each with its value as stated.
    ///
    /// # Errors
    ///
    /// As [`Library::inherited`].
    pub fn traits_of(&self, id: &str) -> Result<serde_json::Map<String, Value>, SpecError> {
        Ok(self.inherited(id)?.trait_values())
    }

    /// Each program's parameters, its inherited ones included.
    #[must_use]
    pub fn program_params(&self) -> &Programs {
        &self.params
    }

    /// The source `id`, if the library holds it.
    #[must_use]
    pub fn source(&self, id: &str) -> Option<&Source> {
        self.sources.get(id)
    }

    /// Every source, sorted by id.
    pub fn sources(&self) -> impl Iterator<Item = &Source> {
        self.sources.values()
    }

    /// Every value that cites the source `id`: in each species' own spec,
    /// niche and shed, then in each rank file, the paths of the evidence
    /// notes naming it. A source need not be in the library to be looked
    /// for, so this also finds what still cites a source being removed.
    ///
    /// # Errors
    ///
    /// Fails on a section or note that cannot be parsed.
    pub fn citations(&self, id: &str) -> Result<Vec<Citation>, String> {
        Ok(self
            .notes()?
            .into_iter()
            .filter(|(_, _, note)| note.source.as_deref() == Some(id))
            .map(|(file, path, _)| Citation { file, path })
            .collect())
    }

    /// Every evidence note of every species and rank file names a source
    /// the library holds, or none.
    ///
    /// # Errors
    ///
    /// Names the first note citing an unknown source.
    pub fn check_citations(&self) -> Result<(), String> {
        for (file, path, note) in self.notes()? {
            note.check()
                .map_err(|error| format!("{file}: the evidence note on `{path}`: {error}"))?;
            if let Some(id) = note
                .source
                .as_deref()
                .filter(|id| !self.sources.contains_key(*id))
            {
                return Err(format!(
                    "{file}: the evidence note on `{path}` cites `{id}`, which is not in the library's sources/"
                ));
            }
        }
        Ok(())
    }

    /// Every species' spec, as it inherits it, carries per-value evidence
    /// ([`PlantSpec::check_evidence`]), and every note of its niche and
    /// shed cites a source (their own checks hold them to a note on every
    /// value; [`Library::check_citations`] to sources the library holds).
    ///
    /// # Errors
    ///
    /// Names the first species whose spec fails, or the first niche or
    /// shed note that cites no source.
    pub fn check_evidence(&self) -> Result<(), String> {
        for entry in &self.species {
            let document: Value = serde_json::from_str(entry.source())
                .map_err(|error| format!("{}: {error}", entry.file()))?;
            PlantSpec::check_evidence(&document, self)
                .map_err(|error| format!("{}: {error}", entry.file()))?;
        }
        for (file, path, note) in self.notes()? {
            if note.source.is_none() {
                return Err(format!(
                    "{file}: the evidence note on `{path}` cites no source"
                ));
            }
        }
        Ok(())
    }

    /// Every species' taxon agrees with its folders: its id is its
    /// `scientific_name` written as an id (ids follow WCVP's accepted
    /// names), and where it names them, its `family` is the family
    /// folder's name (WCVP's spelling, in lower case), its `genus` the
    /// genus folder's, and the first word of its `scientific_name` is that
    /// genus.
    ///
    /// # Errors
    ///
    /// Names the first species whose taxon disagrees, and how.
    pub fn check_taxa(&self) -> Result<(), String> {
        for entry in &self.species {
            let spec: PlantSpec = serde_json::from_str(entry.source())
                .map_err(|error| format!("{}: {error}", entry.file()))?;
            let taxon = &spec.taxon;
            let written = id_of(&taxon.scientific_name);
            if written != entry.id {
                return Err(format!(
                    "{}: its id is `{}`, but its scientific name `{}` written as an id is `{written}`; ids follow WCVP's accepted names, so a renamed taxon's folder takes the new id and `aliases.json` keeps the old one",
                    entry.file(),
                    entry.id,
                    taxon.scientific_name
                ));
            }
            let disagree = |what: &str, named: &str, folder: &str| {
                Err(format!(
                    "{}: taxon.{what} is `{named}`, but its folder is `{folder}`",
                    entry.file()
                ))
            };
            if let Some(family) = &taxon.family
                && family.to_lowercase() != entry.family
            {
                return disagree("family", family, &entry.family);
            }
            if let Some(genus) = &taxon.genus {
                if genus.to_lowercase() != entry.genus {
                    return disagree("genus", genus, &entry.genus);
                }
                if taxon.scientific_name.split_whitespace().next() != Some(genus.as_str()) {
                    return Err(format!(
                        "{}: taxon.scientific_name `{}` is not written in its genus `{genus}`",
                        entry.file(),
                        taxon.scientific_name
                    ));
                }
            }
        }
        Ok(())
    }

    /// Every evidence note in the library: each species' own (its spec,
    /// niche and shed), then each rank file's, then each alias'.
    fn notes(&self) -> Result<Vec<Note>, String> {
        let mut notes = Vec::new();
        for entry in &self.species {
            notes.extend(entry.notes()?);
        }
        for rank in self.ranks.values() {
            notes.extend(rank_notes(rank)?);
        }
        notes.extend(self.aliases.iter().map(|(old, alias)| {
            (
                "aliases.json".to_string(),
                format!("aliases.{old}"),
                alias.evidence.clone(),
            )
        }));
        Ok(notes)
    }

    /// Every rank file, sorted by its path in the tree.
    pub fn ranks(&self) -> impl Iterator<Item = &RankFile> {
        self.ranks.values()
    }

    /// The rank file at `file`, its path in the tree
    /// (`sapindaceae/family.json`), if the library has it.
    #[must_use]
    pub fn rank(&self, file: &str) -> Option<&RankFile> {
        self.ranks.get(file)
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

    /// The species `id`, if the library has it, or the one it was
    /// renamed to ([`Library::current`]).
    #[must_use]
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.current(id)
            .and_then(|id| self.index.get(id))
            .map(|&place| &self.species[place])
    }

    /// The current id of the taxon `id`: `id` itself if the library holds
    /// it, else the id it was renamed to (`library/aliases.json`), if the
    /// library holds that.
    #[must_use]
    pub fn current<'a>(&'a self, id: &'a str) -> Option<&'a str> {
        if self.index.contains_key(id) {
            return Some(id);
        }
        self.aliases
            .get(id)
            .map(|alias| alias.id.as_str())
            .filter(|id| self.index.contains_key(*id))
    }

    /// Every old id of a renamed taxon, with the id it stands for, sorted
    /// by old id.
    pub fn aliases(&self) -> impl Iterator<Item = (&str, &Alias)> {
        self.aliases
            .iter()
            .map(|(old, alias)| (old.as_str(), alias))
    }

    /// Every alias is the old id of a taxon the library holds: an id that
    /// no taxon of the library now has, standing for one that a taxon has
    /// (so no alias stands for another).
    ///
    /// # Errors
    ///
    /// Names the first alias against these rules.
    pub fn check_aliases(&self) -> Result<(), String> {
        for (old, alias) in &self.aliases {
            if !is_id(old) {
                return Err(format!(
                    "aliases.json: `{old}` is not an id: 1 to 64 lowercase letters, digits and hyphens"
                ));
            }
            if let Some(&place) = self.index.get(old) {
                return Err(format!(
                    "aliases.json: `{old}` is the old id of `{}`, but {} still has it",
                    alias.id,
                    self.species[place].file()
                ));
            }
            if !self.index.contains_key(&alias.id) {
                return Err(format!(
                    "aliases.json: `{old}` stands for `{}`, which no taxon of the library has{}",
                    alias.id,
                    if self.aliases.contains_key(&alias.id) {
                        " (it is an alias itself: point the old id at the current one)"
                    } else {
                        ""
                    }
                ));
            }
        }
        Ok(())
    }

    /// The species `id`, parsed and checked against this library.
    ///
    /// # Errors
    ///
    /// Fails if the library has no such species or its spec is invalid.
    pub fn spec(&self, id: &str) -> Result<PlantSpec, SpecError> {
        let entry = self.known(id)?;
        PlantSpec::from_json_in(entry.source(), self).map_err(|error| match &entry.path {
            Some(path) => SpecError(format!("{}: {error}", path.display())),
            None => error,
        })
    }

    /// The species `id`'s spec as it inherits it: the files it is merged
    /// from, nearest first, and the file that set each value and each
    /// evidence note (`plantc check` prints them).
    ///
    /// # Errors
    ///
    /// Fails if the library has no such species or its own spec is not a
    /// JSON object.
    pub fn inherited(&self, id: &str) -> Result<Inherited, SpecError> {
        let entry = self.known(id)?;
        let above = inherit::above(&self.ranks, &entry.family, &entry.genus).map_err(SpecError)?;
        let own: Value = serde_json::from_str(&entry.own)
            .map_err(|error| SpecError(format!("{}: {error}", entry.file())))?;
        inherit::inherit(&entry.file(), &own, &above, &self.params).map_err(SpecError)
    }

    /// The effective spec of a spec file's `text` (`file` names it in
    /// messages): when its `id` is a species of the library, its text
    /// merged onto the rank files that species stands on, as the library
    /// merges the species' own `spec.json`; else its own traits turned
    /// into values by its own rules. `None` when that leaves the text as
    /// it is.
    ///
    /// # Errors
    ///
    /// As [`inherit::effective_text`].
    pub fn effective_text(&self, file: &str, text: &str) -> Result<Option<String>, String> {
        let id = serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|spec| spec.get("id")?.as_str().map(str::to_string));
        let above = match id.as_deref().and_then(|id| self.entry(id)) {
            Some(entry) => inherit::above(&self.ranks, &entry.family, &entry.genus)?,
            None => Vec::new(),
        };
        inherit::effective_text(file, text, &above, &self.params)
    }

    /// The species `id`, or an error listing the species there are.
    fn known(&self, id: &str) -> Result<&Entry, SpecError> {
        self.entry(id).ok_or_else(|| {
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
        })
    }

    /// The program `name`'s source, if the library has it.
    #[must_use]
    pub fn program(&self, name: &str) -> Option<&str> {
        self.chains
            .get(name)
            .map(String::as_str)
            .or_else(|| self.programs.get(name).map(AsRef::as_ref))
    }

    /// The text of program `name` alone, without the programs it extends.
    #[must_use]
    pub fn program_text(&self, name: &str) -> Option<&str> {
        self.programs.get(name).map(AsRef::as_ref)
    }

    /// `source` joined with the programs it extends from this library, as
    /// [`Library::program`] gives a built-in program: the text a program
    /// compiles from. A program that extends none, or one this library
    /// lacks, comes back as it is, and compiling it names the problem.
    #[must_use]
    pub fn chain_of<'a>(&self, source: &'a str) -> Cow<'a, str> {
        let mut text = Cow::Borrowed(source);
        let mut parent = crate::lsys::parser::extends_of(source);
        let mut depth = 1;
        while let Some(name) = parent {
            let Some(next) = self.programs.get(&name) else {
                break;
            };
            depth += 1;
            if depth > crate::lsys::chain::MAX_DEPTH {
                break;
            }
            let joined = text.to_mut();
            joined.push('\n');
            joined.push_str(next);
            parent = crate::lsys::parser::extends_of(next);
        }
        text
    }

    fn with_chains(mut self) -> Self {
        let chains = self
            .programs
            .iter()
            .filter(|(_, source)| crate::lsys::parser::extends_of(source).is_some())
            .map(|(name, source)| (name.clone(), self.chain_of(source).into_owned()))
            .collect();
        self.chains = chains;
        self
    }

    /// Every program's name, sorted.
    pub fn programs(&self) -> impl Iterator<Item = &str> {
        self.programs.keys().map(String::as_str)
    }
}

/// The rank files in the library tree `tree`, by their path in it:
/// `_ranks/<rank>/<name>.json`, `<family>/family.json` and
/// `<family>/<genus>/genus.json`.
fn rank_files_in(tree: &Path) -> Result<Vec<(String, PathBuf)>, LibraryError> {
    let mut found = Vec::new();
    let ranks = tree.join(inherit::RANKS);
    if ranks.is_dir() {
        for rank in folders(&ranks)? {
            let rank_name = folder_name(&rank, "a rank", is_id)?;
            for path in files(&rank)? {
                if path.extension().is_none_or(|extension| extension != "json") {
                    continue;
                }
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| LibraryError(format!("{} has no UTF-8 name", path.display())))?
                    .to_string();
                found.push((format!("{}/{rank_name}/{file_name}", inherit::RANKS), path));
            }
        }
    }
    for family in folders(tree)? {
        if family
            .file_name()
            .is_some_and(|name| name == "sources" || name == inherit::RANKS)
        {
            continue;
        }
        let family_name = folder_name(&family, "a family", is_taxon_name)?;
        if family.join("family.json").is_file() {
            found.push((
                format!("{family_name}/family.json"),
                family.join("family.json"),
            ));
        }
        for genus in folders(&family)? {
            let genus_name = folder_name(&genus, "a genus", is_taxon_name)?;
            if genus.join("genus.json").is_file() {
                found.push((
                    format!("{family_name}/{genus_name}/genus.json"),
                    genus.join("genus.json"),
                ));
            }
        }
    }
    Ok(found)
}

/// Every value, trait and rule a file sets (one part of a rank file, or a
/// species' own spec) has a note of that file's own, on its path or a
/// subtree holding it, citing a source: a value it inherits keeps the
/// note of the file that set it, so each file notes what it sets. The
/// schema, id, tier and provenance need none, as in a spec
/// ([`crate::spec::UNNOTED`]).
fn own_notes(
    values: &serde_json::Map<String, Value>,
    traits: &serde_json::Map<String, Value>,
    rules: &serde_json::Map<String, Value>,
    notes: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    let mut document = values.clone();
    for (key, part) in [("traits", traits), ("rules", rules)] {
        if !part.is_empty() {
            document.insert(key.into(), Value::Object(part.clone()));
        }
    }
    let notes: BTreeMap<String, FieldEvidence> = notes
        .iter()
        .map(|(path, note)| {
            serde_json::from_value(note.clone())
                .map(|note| (path.clone(), note))
                .map_err(|error| format!("the evidence note on `{path}`: {error}"))
        })
        .collect::<Result<_, String>>()?;
    if let Some(path) = notes
        .iter()
        .find_map(|(path, note)| note.source.is_none().then_some(path))
    {
        return Err(format!("the evidence note on `{path}` cites no source"));
    }
    crate::evidence::check_coverage(&Value::Object(document), &notes, crate::spec::UNNOTED)
}

/// Whether a rank file's part fits a spec: laid over one of `probes`
/// (complete specs), without its nulls, it parses as one. Any probe will
/// do, so a part that sets some of a section only the species below it
/// complete fits wherever a probe has that section.
fn fits(part: &Part, probes: &[Value]) -> Result<(), String> {
    let values = inherit::without_nulls(&part.values);
    let mut first = None;
    for probe in probes {
        let Value::Object(mut spec) = probe.clone() else {
            continue;
        };
        inherit::patch(&mut spec, &values);
        if !part.notes.is_empty() {
            let mut evidence = match spec.remove("evidence") {
                Some(Value::Object(evidence)) => evidence,
                _ => serde_json::Map::new(),
            };
            evidence.extend(part.notes.clone());
            spec.insert("evidence".into(), Value::Object(evidence));
        }
        match serde_json::from_value::<PlantSpec>(Value::Object(spec)) {
            Ok(_) => return Ok(()),
            Err(error) => {
                first.get_or_insert_with(|| error.to_string());
            }
        }
    }
    Err(first.unwrap_or_else(|| "no built-in spec to try it on".into()))
}

/// The section `name` beside a spec in `folder`, if the folder has it.
/// Where a taxon's folder sits: its family's and genus's folders, and for
/// a taxon below a species, the species'.
type Place<'a> = (&'a str, &'a str, Option<&'a str>);

/// Read the folder of the taxon `id`, at `place` in the tree, into `read`.
fn read_taxon(
    read: &mut BTreeMap<String, Entry>,
    (family, genus, species): Place<'_>,
    folder: &Path,
    id: String,
) -> Result<(), LibraryError> {
    let path = folder.join("spec.json");
    let source = fs::read_to_string(&path)
        .map_err(|error| LibraryError(format!("cannot read {}: {error}", path.display())))?;
    let named = serde_json::from_str::<serde_json::Value>(&source)
        .ok()
        .and_then(|spec| spec.get("id")?.as_str().map(str::to_string));
    if named.as_deref() != Some(id.as_str()) {
        return Err(LibraryError(format!(
            "{} must be the spec of `{id}`, its folder's name; it names {}",
            path.display(),
            named.map_or_else(|| "no id".to_string(), |named| format!("`{named}`"))
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
        family: family.to_string(),
        genus: genus.to_string(),
        species: species.map(str::to_string),
        path: Some(path),
        own: Cow::Owned(source),
        merged: None,
        conditions: section(folder, "conditions.json")?,
        shed: section(folder, "shed.json")?,
        niche: section(folder, "niche.json")?,
    };
    entry
        .check_sections()
        .map_err(|error| LibraryError(format!("{}: {error}", folder.display())))?;
    read.insert(id, entry);
    Ok(())
}

/// The ranks below species an id names (`subsp`, `var`, `f`, `cv`).
const RANKS_BELOW_SPECIES: [&str; 4] = ["subsp", "var", "f", "cv"];

/// The rank below species the id `id` names, if it names one: a word of it
/// that is one of [`RANKS_BELOW_SPECIES`].
fn rank_in(id: &str) -> Option<&'static str> {
    id.split('-')
        .find_map(|word| RANKS_BELOW_SPECIES.into_iter().find(|rank| *rank == word))
}

/// Whether `id` names a taxon below the species `species`: the species'
/// id, a rank below species and an epithet, joined by hyphens.
fn is_below(species: &str, id: &str) -> bool {
    id.strip_prefix(species)
        .and_then(|rest| rest.strip_prefix('-'))
        .and_then(|rest| rest.split_once('-'))
        .is_some_and(|(rank, epithet)| {
            RANKS_BELOW_SPECIES.contains(&rank) && !epithet.is_empty() && rank_in(epithet).is_none()
        })
}

/// A scientific name written as an id: its words in lower case, without
/// the dot of a rank's abbreviation or any sign but a hyphen, joined by
/// hyphens, and a cultivar's quoted name after `cv`
/// (`Pinus contorta var. contorta`: `pinus-contorta-var-contorta`;
/// `Acer palmatum 'Bloodgood'`: `acer-palmatum-cv-bloodgood`).
fn id_of(name: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut quoted = false;
    for word in name.split_whitespace() {
        if !quoted && word.starts_with(['\'', '\u{2018}']) {
            quoted = true;
            words.push("cv".to_string());
        }
        let word: String = word
            .chars()
            .filter(|letter| letter.is_ascii_alphanumeric() || *letter == '-')
            .collect();
        if !word.is_empty() {
            words.push(word.to_ascii_lowercase());
        }
    }
    words.join("-")
}

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
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{
        ALIASES, Citation, LIBRARY, Library, RANKS, RankFile, SOURCES, Vocabulary, niche_source,
        source, species,
    };
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

    /// A species' or lower taxon's folder holds its record's sections: its
    /// spec and its typical site, and where it grows and what falls from
    /// it where those sections are written; its spec names its folder.
    fn record(folder: &Path, ids: &mut BTreeSet<String>) {
        let id = folder.file_name().unwrap().to_str().unwrap();
        assert!(lower_name(id), "{id}");
        let files: Vec<String> = entries(folder)
            .iter()
            .map(|file| file.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
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
        let spec: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(folder.join("spec.json")).unwrap()).unwrap();
        assert_eq!(spec["id"], id, "{id}");
        assert!(ids.insert(id.to_owned()), "{id} twice");
    }

    /// The tree's rules: a folder per family, per genus and per species, in
    /// lower case; the species folder holds its `spec.json`, whose `id` is
    /// the folder's name and starts with the genus folder's, and its
    /// typical site, `conditions.json`, and where those sections are
    /// written, where it grows, `niche.json`, and what falls from it,
    /// `shed.json`; or instead, a folder laid out as a species' for each
    /// taxon below it, whose id extends the species' with a rank and an
    /// epithet; families end in "aceae"; no id twice, and nothing anywhere
    /// else but the `sources/` and `_ranks/` folders and `aliases.json`
    /// beside the families and the rank files (`family.json`,
    /// `genus.json`) in a family's and a genus's folder.
    #[test]
    fn the_library_keeps_the_tree_rules() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("library");
        let mut ids = BTreeSet::new();
        let mut walked = 0;
        let mut ranks = 0;
        for family in entries(&root) {
            let family_name = family.file_name().unwrap().to_str().unwrap();
            // The sources values cite sit beside the families, and the
            // old ids of renamed taxa.
            if family_name == "sources" || family_name == "aliases.json" {
                continue;
            }
            // So do the ranks kept flat, a folder per rank.
            if family_name == crate::inherit::RANKS {
                for rank in entries(&family) {
                    assert!(rank.is_dir(), "{} is not a rank folder", rank.display());
                    for file in entries(&rank) {
                        assert!(
                            file.is_file()
                                && file
                                    .extension()
                                    .is_some_and(|extension| extension == "json"),
                            "{} is not a rank file",
                            file.display()
                        );
                        ranks += 1;
                    }
                }
                continue;
            }
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
                if genus_name == "family.json" {
                    ranks += 1;
                    continue;
                }
                assert!(genus.is_dir(), "{} is not a genus folder", genus.display());
                assert!(
                    lower_name(genus_name) && !genus_name.contains('-'),
                    "{genus_name}"
                );
                for folder in entries(&genus) {
                    let id = folder.file_name().unwrap().to_str().unwrap();
                    if id == "genus.json" {
                        ranks += 1;
                        continue;
                    }
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
                    assert_eq!(super::rank_in(id), None, "{id} is filed as a species");
                    let below: Vec<PathBuf> = entries(&folder)
                        .into_iter()
                        .filter(|entry| entry.is_dir())
                        .collect();
                    if below.is_empty() {
                        record(&folder, &mut ids);
                        walked += 1;
                        continue;
                    }
                    // A species folder holding the taxa below it holds
                    // nothing else.
                    assert_eq!(entries(&folder), below, "{id}");
                    for taxon in below {
                        let taxon_id = taxon.file_name().unwrap().to_str().unwrap();
                        assert!(super::is_below(id, taxon_id), "{taxon_id} under {id}");
                        record(&taxon, &mut ids);
                        walked += 1;
                    }
                }
            }
        }
        assert_eq!(
            walked,
            LIBRARY.len(),
            "the build script compiles every folder in"
        );
        assert_eq!(ranks, RANKS.len(), "and every rank file");
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
        // Every built-in source is valid (`Library::builtin` panics on one
        // that is not) and every note cites one it holds.
        let library = Library::builtin();
        assert_eq!(library.sources().count(), SOURCES.len());
        library.check_citations().unwrap();
        // Every value of every species' spec has its note, citing a source.
        library.check_evidence().unwrap();
        // Every built-in species names its WCVP family, its genus and its
        // accepted taxon's plant_name_id, and they agree with its folders.
        library.check_taxa().unwrap();
        for entry in library.species() {
            let taxon = library.spec(&entry.id).unwrap().taxon;
            assert!(
                taxon.family.is_some() && taxon.genus.is_some() && taxon.plant_name_id.is_some(),
                "{}",
                entry.id
            );
        }
        // And every rank file, with the chain above it, and every trait
        // and rule.
        assert_eq!(library.ranks().count(), RANKS.len());
        library.check_ranks().unwrap();
        library.check_traits().unwrap();
    }

    /// A spec file of a library species' id stands on that species' rank
    /// files, so a copy of its own spec is its spec; a spec of no species
    /// of the library stands on none.
    #[test]
    fn a_spec_file_stands_on_its_species_rank_files() {
        let library = Library::builtin();
        let entry = library.entry("vachellia-tortilis").unwrap();
        let copy = library.effective_text("copy.json", entry.own_source());
        assert_eq!(copy.unwrap().as_deref(), Some(entry.source()));
        let other = entry
            .own_source()
            .replace("\"vachellia-tortilis\"", "\"vachellia-test\"");
        assert_eq!(library.effective_text("other.json", &other), Ok(None));
    }

    /// The parameters rules are checked against and read, from the
    /// programs' text, are those the compiled programs have, with their
    /// defaults.
    #[test]
    fn program_parameters_are_the_compiled_programs() {
        let library = Library::builtin();
        assert_eq!(library.program_params().len(), PROGRAMS.len());
        for (name, params) in library.program_params() {
            let program = crate::lsys::Program::compile(library.program(name).unwrap()).unwrap();
            let defaults = program.resolve_params(&BTreeMap::new()).unwrap();
            let compiled: BTreeMap<String, Option<f64>> = program
                .params
                .iter()
                .zip(defaults)
                .map(|(param, default)| (param.name.clone(), Some(default)))
                .collect();
            assert_eq!(params, &compiled, "{name}");
        }
    }

    /// The paths of a JSON value's leaves (an array is one) and their
    /// values, leaving out its `evidence`, `traits` and `rules`, which stay
    /// out of the spec.
    fn leaves(value: &serde_json::Value, at: &str, found: &mut Vec<(String, serde_json::Value)>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, inside) in map {
                    if at.is_empty() && ["evidence", "traits", "rules"].contains(&key.as_str()) {
                        continue;
                    }
                    let path = if at.is_empty() {
                        key.clone()
                    } else {
                        format!("{at}.{key}")
                    };
                    leaves(inside, &path, found);
                }
            }
            _ => found.push((at.to_string(), value.clone())),
        }
    }

    /// A species' spec keeps every value and note of its own file, the
    /// nearest in its chain, but for its traits and rules and their notes,
    /// and is what the library merges at run time; `source` gives it, as
    /// `Entry::source` does.
    #[test]
    fn every_species_keeps_its_own_values() {
        let library = Library::builtin();
        for entry in library.species() {
            let own: serde_json::Value = serde_json::from_str(entry.own_source()).unwrap();
            let merged: serde_json::Value = serde_json::from_str(entry.source()).unwrap();
            let mut found = Vec::new();
            leaves(&own, "", &mut found);
            let merged_map = merged.as_object().unwrap();
            for (path, value) in found {
                assert_eq!(
                    crate::inherit::find(merged_map, &path),
                    Some(&value),
                    "{} {path}",
                    entry.id
                );
            }
            if let Some(notes) = own.get("evidence").and_then(|notes| notes.as_object()) {
                for (path, note) in notes {
                    let (head, _) = path.split_once('.').unwrap_or((path, ""));
                    if head == "traits" || head == "rules" {
                        assert!(merged["evidence"].get(path).is_none(), "{}", entry.id);
                        continue;
                    }
                    assert_eq!(
                        merged["evidence"].get(path),
                        Some(note),
                        "{} {path}",
                        entry.id
                    );
                }
            }
            assert_eq!(
                library.inherited(&entry.id).unwrap().spec,
                merged,
                "{}",
                entry.id
            );
            assert_eq!(source(&entry.id), entry.source());
        }
    }

    #[test]
    fn a_taxon_must_agree_with_its_folders() {
        let species = "library/pinaceae/pseudotsuga/pseudotsuga-menziesii/spec.json";
        for (from, to, wanted) in [
            (
                "\"family\": \"Pinaceae\"",
                "\"family\": \"Cupressaceae\"",
                "taxon.family is `Cupressaceae`",
            ),
            (
                "\"genus\": \"Pseudotsuga\"",
                "\"genus\": \"Abies\"",
                "taxon.genus is `Abies`",
            ),
            (
                "\"genus\": \"Pseudotsuga\"",
                "\"genus\": \"pseudotsuga\"",
                "is not written in its genus",
            ),
            // Its id is not its name: a renamed taxon keeps neither.
            (
                "\"scientific_name\": \"Pseudotsuga menziesii\"",
                "\"scientific_name\": \"Pseudotsuga taxifolia\"",
                "written as an id is `pseudotsuga-taxifolia`",
            ),
        ] {
            let folder = Folder::new("taxa");
            let text = source("pseudotsuga-menziesii");
            assert!(text.contains(from), "{from}");
            folder.write(species, &text.replace(from, to));
            let error = Library::from_dir(&folder.0).unwrap_err().0;
            assert!(error.contains(wanted), "{wanted}: {error}");
        }
    }

    /// Old ids find the taxa renamed from them, in constants and in the
    /// library; every alias stands for a taxon the library holds, with a
    /// note citing its source; a taxon below a species sits in its
    /// species' folder.
    #[test]
    fn old_ids_find_renamed_taxa() {
        let library = Library::builtin();
        library.check_aliases().unwrap();
        let aliases: Vec<(&str, &str)> = library
            .aliases()
            .map(|(old, alias)| (old, alias.id.as_str()))
            .collect();
        assert!(!aliases.is_empty());
        assert_eq!(aliases, ALIASES);
        for &(old, current) in ALIASES {
            assert_eq!(library.current(old), Some(current));
            assert_eq!(library.entry(old).unwrap().id, current);
            assert_eq!(library.spec(old).unwrap().id, current);
            assert_eq!(species(old).unwrap().id, current);
            assert_eq!(source(old), source(current));
            assert_eq!(
                super::id_of(&library.spec(current).unwrap().taxon.scientific_name),
                current
            );
        }
        assert_eq!(
            niche_source("prosopis-velutina"),
            niche_source("neltuma-velutina")
        );
        assert_eq!(library.current("no-such-plant"), None);
        assert!(library.citations("wcvp-v16").unwrap().contains(&Citation {
            file: "aliases.json".to_string(),
            path: "aliases.prosopis-velutina".to_string(),
        }));
        let shore_pine = library.entry("pinus-contorta-var-contorta").unwrap();
        assert_eq!(shore_pine.species.as_deref(), Some("pinus-contorta"));
        assert_eq!(
            shore_pine.file(),
            "pinaceae/pinus/pinus-contorta/pinus-contorta-var-contorta/spec.json"
        );
        assert_eq!(
            super::id_of("Acer palmatum 'Bloodgood'"),
            "acer-palmatum-cv-bloodgood"
        );
    }

    #[test]
    fn aliases_and_lower_taxa_against_the_rules_are_refused() {
        let refused = |files: &[(&str, &str)], expected: &str| {
            let folder = Folder::new("aliases");
            for (relative, text) in files {
                folder.write(relative, text);
            }
            let message = Library::from_dir(&folder.0).unwrap_err().0;
            assert!(message.contains(expected), "{expected}: {message}");
        };
        let aliases = |old: &str, current: &str| {
            format!(
                r#"{{"schema": 1, "aliases": {{"{old}": "{current}"}}, "evidence": {{"aliases.{old}": {{"evidence": "SourceInferred", "source": "wcvp-v16", "note": "A test."}}}}}}"#
            )
        };
        refused(
            &[(
                "library/aliases.json",
                &aliases("pseudotsuga-menziesii", "thuja-plicata"),
            )],
            "`pseudotsuga-menziesii` is the old id of `thuja-plicata`, but pinaceae/pseudotsuga/pseudotsuga-menziesii/spec.json still has it",
        );
        refused(
            &[("library/aliases.json", &aliases("pinus-test", "pinus-none"))],
            "`pinus-test` stands for `pinus-none`, which no taxon of the library has",
        );
        refused(
            &[(
                "library/aliases.json",
                &aliases("prosopis-test", "prosopis-velutina"),
            )],
            "(it is an alias itself",
        );
        refused(
            &[(
                "library/aliases.json",
                r#"{"schema": 1, "aliases": {"pinus-test": "pinus-contorta-var-contorta"}}"#,
            )],
            "the alias `pinus-test` has no evidence note",
        );
        // A species folder under an old id.
        let mesquite = source("neltuma-velutina")
            .replace("\"neltuma-velutina\"", "\"prosopis-velutina\"")
            .replace("\"Neltuma velutina\"", "\"Prosopis velutina\"")
            .replace("\"genus\": \"Neltuma\"", "\"genus\": \"Prosopis\"");
        refused(
            &[(
                "library/fabaceae/prosopis/prosopis-velutina/spec.json",
                &mesquite,
            )],
            "`prosopis-velutina` is the old id of `neltuma-velutina`, but fabaceae/prosopis/prosopis-velutina/spec.json still has it",
        );
        // Taxa below a species: in its folder, named from its id, and
        // alone there.
        let shore_pine = source("pinus-contorta-var-contorta");
        refused(
            &[(
                "library/pinaceae/pinus/pinus-contorta-var-contorta/spec.json",
                shore_pine,
            )],
            "names a taxon below a species (`-var-`)",
        );
        refused(
            &[(
                "library/pinaceae/pinus/pinus-contorta/pinus-contorta-contorta/spec.json",
                shore_pine,
            )],
            "is filed under the species pinus-contorta; its id must be",
        );
        refused(
            &[
                (
                    "library/pinaceae/pinus/pinus-contorta/pinus-contorta-var-contorta/spec.json",
                    shore_pine,
                ),
                ("library/pinaceae/pinus/pinus-contorta/niche.json", "{}"),
            ],
            "holds the taxa below its species, so it holds no file of its own",
        );
    }

    const SOURCE: &str = r#"{"id": "flora-test", "title": "A test flora",
        "authors": "Test authors", "year": 2026, "licence": "CC0-1.0",
        "tier": 1, "kind": "text"}"#;

    #[test]
    fn values_cite_sources_by_id_and_are_found_by_them() {
        let folder = Folder::new("sources");
        folder.write("library/sources/flora-test.json", SOURCE);
        // A spec, its niche and its shed each citing the source.
        let cite = |text: &str, path: &str| {
            let mut value: serde_json::Value = serde_json::from_str(text).unwrap();
            value["evidence"][path]["source"] = "flora-test".into();
            serde_json::to_string_pretty(&value).unwrap()
        };
        let species = "library/pinaceae/pseudotsuga/pseudotsuga-menziesii";
        let entry = Library::builtin().entry("pseudotsuga-menziesii").unwrap();
        folder.write(
            &format!("{species}/spec.json"),
            &cite(source("pseudotsuga-menziesii"), "allometry"),
        );
        folder.write(
            &format!("{species}/niche.json"),
            &cite(niche_source("pseudotsuga-menziesii"), "moisture"),
        );
        let shed = entry.shed.as_deref().expect("the Douglas-fir sheds");
        let shed_note = serde_json::from_str::<serde_json::Value>(shed).unwrap()["evidence"]
            .as_object()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .clone();
        folder.write(&format!("{species}/shed.json"), &cite(shed, &shed_note));
        // And a rank file above it, in one form's part.
        folder.write(
            "library/pinaceae/family.json",
            r#"{"schema": 1, "rank": "family", "name": "Pinaceae",
                "forms": {"excurrent_tree": {"generator": {"params": {"nod_years": 2.0}},
                    "evidence": {"generator.params.nod_years": {"evidence": "Authored",
                        "source": "flora-test", "note": "Leaders nod two years."}}}}}"#,
        );
        let library = Library::from_dir(&folder.0).unwrap();
        assert_eq!(library.source("flora-test").unwrap().title, "A test flora");
        assert!(library.source("no-such-source").is_none());
        library.spec("pseudotsuga-menziesii").unwrap();
        let found = library.citations("flora-test").unwrap();
        let at = |file: &str, path: &str| Citation {
            file: format!("pinaceae/pseudotsuga/pseudotsuga-menziesii/{file}"),
            path: path.into(),
        };
        assert_eq!(
            found,
            [
                at("spec.json", "allometry"),
                at("niche.json", "moisture"),
                at("shed.json", &shed_note),
                Citation {
                    file: "pinaceae/family.json".into(),
                    path: "forms.excurrent_tree.generator.params.nod_years".into(),
                },
            ]
        );
        assert!(library.citations("no-such-source").unwrap().is_empty());
        assert!(
            Library::builtin()
                .citations("flora-test")
                .unwrap()
                .is_empty()
        );

        // A niche note must cite a source.
        let uncited = {
            let mut niche: serde_json::Value =
                serde_json::from_str(niche_source("pseudotsuga-menziesii")).unwrap();
            niche["evidence"]["moisture"]
                .as_object_mut()
                .unwrap()
                .remove("source");
            serde_json::to_string_pretty(&niche).unwrap()
        };
        let kept = fs::read_to_string(folder.0.join(format!("{species}/niche.json"))).unwrap();
        folder.write(&format!("{species}/niche.json"), &uncited);
        let error = Library::from_dir(&folder.0).unwrap_err().0;
        assert!(
            error.contains("niche.json: the evidence note on `moisture` cites no source"),
            "{error}"
        );
        folder.write(&format!("{species}/niche.json"), &kept);

        // Without the source, the citing notes are refused.
        fs::remove_file(folder.0.join("library/sources/flora-test.json")).unwrap();
        let error = Library::from_dir(&folder.0).unwrap_err().0;
        assert!(error.contains("cites `flora-test`"), "{error}");
        // A rank file's too.
        for file in ["spec.json", "niche.json", "shed.json"] {
            fs::remove_file(folder.0.join(species).join(file)).unwrap();
        }
        fs::remove_dir(folder.0.join(species)).unwrap();
        let error = Library::from_dir(&folder.0).unwrap_err().0;
        assert!(
            error.contains(
                "pinaceae/family.json: the evidence note on `forms.excurrent_tree.generator.params.nod_years` cites `flora-test`"
            ),
            "{error}"
        );
    }

    #[test]
    fn sources_against_the_rules_are_refused() {
        for (file, text, wanted) in [
            ("Flora.json", SOURCE.to_string(), "not named as a source"),
            (
                "other-flora.json",
                SOURCE.to_string(),
                "must be the source `other-flora`",
            ),
            (
                "flora-test.json",
                SOURCE.replace("\"tier\": 1", "\"tier\": 9"),
                "tier 9",
            ),
            (
                "flora-test.json",
                SOURCE.replace("\"kind\"", "\"sort\""),
                "invalid source",
            ),
        ] {
            let folder = Folder::new("bad-sources");
            folder.write(&format!("library/sources/{file}"), &text);
            let error = Library::from_dir(&folder.0).unwrap_err().0;
            assert!(error.contains(wanted), "{file}: {error}");
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
            // A program is its own text, joined with the programs it
            // extends when it extends one (growth plan G1).
            assert_eq!(library.program_text(name), Some(program));
            assert_eq!(
                library.program(name),
                Some(library.chain_of(program).as_ref())
            );
            if crate::lsys::parser::extends_of(program).is_none() {
                assert_eq!(library.program(name), Some(program));
            }
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
    fn a_program_that_extends_a_built_in_compiles_with_it() {
        let folder = Folder::new("extends");
        folder.write(
            "programs/leafier.lsys",
            "lsystem leafier 1 extends broadleaf;\nparam leaf_size = 0.5;\n",
        );
        let library = Library::from_dir(&folder.0).unwrap();
        assert_eq!(
            library.program_text("leafier"),
            Some("lsystem leafier 1 extends broadleaf;\nparam leaf_size = 0.5;\n")
        );
        let program = crate::lsys::Program::compile(library.program("leafier").unwrap()).unwrap();
        assert_eq!((program.name.as_str(), program.revision), ("leafier", 1));
        let broadleaf =
            crate::lsys::Program::compile(library.program("broadleaf").unwrap()).unwrap();
        assert_eq!(program.params.len(), broadleaf.params.len());
        // A program that extends none is its own text, so its packages
        // keep their keys.
        assert_eq!(
            library.program("broadleaf"),
            library.program_text("broadleaf")
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
            .replace("\"program\": \"conifer\"", "\"program\": \"conifer-test\"")
            .replace("\"Thuja plicata\"", "\"Zelkova test\"")
            .replace("\"family\": \"Cupressaceae\"", "\"family\": \"Ulmaceae\"")
            .replace("\"genus\": \"Thuja\"", "\"genus\": \"Zelkova\"");
        folder.write(
            "library/ulmaceae/zelkova/zelkova-test/spec.json",
            &new_species,
        );
        folder.write(
            "library/ulmaceae/zelkova/zelkova-test/niche.json",
            &niche_source("thuja-plicata").replace("\"thuja-plicata\"", "\"zelkova-test\""),
        );
        folder.write("library/ulmaceae/zelkova/zelkova-test/wood.json", "{}");
        // And an old id for it.
        folder.write(
            "library/aliases.json",
            r#"{"schema": 1, "aliases": {"zelkova-old": "zelkova-test"},
                "evidence": {"aliases.zelkova-old": {"evidence": "Synthetic",
                "source": "plantgen-authors", "note": "A test rename."}}}"#,
        );
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
        assert_eq!(library.current("zelkova-old"), Some("zelkova-test"));
        assert_eq!(library.aliases().count(), super::ALIASES.len() + 1);
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

    /// A spec split over its family's and its genus's files and an
    /// overlay of its own is the whole spec again, and builds the package
    /// the whole spec builds: the key hashes the merged spec. Each value
    /// and note says which file it came from.
    #[test]
    fn a_spec_split_over_rank_files_builds_the_same_package() {
        use crate::package::{DEFAULT_DAY, Inputs};
        use crate::quality::STANDARD;
        use serde_json::{Value, json};
        let id = "pseudotsuga-menziesii";
        let whole: Value = serde_json::from_str(source(id)).unwrap();
        let mut own = whole.as_object().unwrap().clone();
        let mut evidence = own["evidence"].as_object().unwrap().clone();
        // Each value moves with its notes: a file notes what it sets.
        let mut notes_of = |key: &str| -> serde_json::Map<String, Value> {
            let paths: Vec<String> = evidence
                .keys()
                .filter(|path| *path == key || path.starts_with(&format!("{key}.")))
                .cloned()
                .collect();
            paths
                .into_iter()
                .map(|path| {
                    let note = evidence.remove(&path).unwrap();
                    (path, note)
                })
                .collect()
        };
        let mut family = json!({"schema": 1, "rank": "family", "name": "Pinaceae"});
        family["appearance"] = own.remove("appearance").unwrap();
        family["evidence"] = Value::Object(notes_of("appearance"));
        let mut genus = json!({"schema": 1, "rank": "genus", "name": "Pseudotsuga"});
        genus["growth"] = own.remove("growth").unwrap();
        genus["variants"] = own.remove("variants").unwrap();
        let mut genus_notes = notes_of("growth");
        genus_notes.extend(notes_of("variants"));
        genus["evidence"] = Value::Object(genus_notes);
        own.insert("evidence".into(), Value::Object(evidence));
        let folder = Folder::new("split");
        folder.write("library/pinaceae/family.json", &family.to_string());
        folder.write(
            "library/pinaceae/pseudotsuga/genus.json",
            &genus.to_string(),
        );
        let file = format!("pinaceae/pseudotsuga/{id}/spec.json");
        folder.write(
            &format!("library/{file}"),
            &serde_json::to_string_pretty(&own).unwrap(),
        );
        let library = Library::from_dir(&folder.0).unwrap();
        let spec = library.spec(id).unwrap();
        assert_eq!(spec, PlantSpec::builtin(id).unwrap());
        let built_in =
            Inputs::on_day(&PlantSpec::builtin(id).unwrap(), &STANDARD, DEFAULT_DAY).unwrap();
        let from_ranks = Inputs::on_day_in(&spec, &library, &STANDARD, DEFAULT_DAY).unwrap();
        assert_eq!(from_ranks.key, built_in.key);
        let entry = library.entry(id).unwrap();
        assert_ne!(entry.own_source(), entry.source());
        let inherited = library.inherited(id).unwrap();
        assert_eq!(
            inherited.chain,
            [
                file.as_str(),
                "pinaceae/pseudotsuga/genus.json",
                "pinaceae/family.json"
            ]
        );
        assert_eq!(
            inherited.origins["growth.step"],
            "pinaceae/pseudotsuga/genus.json"
        );
        assert_eq!(
            inherited.origins["appearance.foliage"],
            "pinaceae/family.json"
        );
        assert_eq!(inherited.origins["taxon.common_name"], file);
        assert_eq!(inherited.notes["appearance"], "pinaceae/family.json");
        assert_eq!(inherited.notes["allometry"], file);
    }

    /// A folder's rank files stand above the built-in species of their
    /// taxa too, and only those.
    #[test]
    fn rank_files_in_a_folder_reach_built_in_species() {
        let folder = Folder::new("ranks");
        folder.write(
            "library/_ranks/order/pinales.json",
            r#"{"schema": 1, "rank": "order", "name": "Pinales",
                "generator": {"params": {"order_test": 2.0}},
                "evidence": {"generator.params.order_test":
                    {"evidence": "Authored", "source": "plantgen-authors", "note": "The order's."}}}"#,
        );
        folder.write(
            "library/pinaceae/family.json",
            r#"{"schema": 1, "rank": "family", "name": "Pinaceae", "parent": "order/pinales",
                "forms": {"excurrent_tree": {"generator": {"params": {"family_test": 1.0}},
                    "evidence": {"generator.params.family_test": {"evidence": "Authored",
                        "source": "plantgen-authors", "note": "The family's."}}}}}"#,
        );
        let library = Library::from_dir(&folder.0).unwrap();
        // They replace built-in files at the same paths and add the rest.
        let added = ["_ranks/order/pinales.json", "pinaceae/family.json"]
            .iter()
            .filter(|file| !RANKS.iter().any(|(path, _)| path == *file))
            .count();
        assert_eq!(library.ranks().count(), RANKS.len() + added);
        assert_eq!(
            library
                .rank("pinaceae/family.json")
                .unwrap()
                .parent
                .as_deref(),
            Some("_ranks/order/pinales.json")
        );
        let inherited = library.inherited("pseudotsuga-menziesii").unwrap();
        assert_eq!(
            inherited.origins["generator.params.order_test"],
            "_ranks/order/pinales.json"
        );
        assert_eq!(
            inherited.origins["generator.params.family_test"],
            "pinaceae/family.json, forms.excurrent_tree"
        );
        // The order's note stays: nothing nearer sets that param.
        assert_eq!(
            inherited.notes["generator.params.order_test"],
            "_ranks/order/pinales.json"
        );
        let merged: serde_json::Value =
            serde_json::from_str(library.entry("pseudotsuga-menziesii").unwrap().source()).unwrap();
        assert_eq!(merged["generator"]["params"]["order_test"], 2.0);
        // A maple is no conifer.
        assert_eq!(
            library.entry("acer-macrophyllum").unwrap().source(),
            source("acer-macrophyllum")
        );
    }

    #[test]
    fn rank_files_against_the_library_are_refused() {
        let refused = |files: &[(&str, &str)], expected: &str| {
            let folder = Folder::new("refused-ranks");
            for (relative, text) in files {
                folder.write(relative, text);
            }
            let message = Library::from_dir(&folder.0).unwrap_err().0;
            assert!(message.contains(expected), "{message}");
        };
        let family = |extra: &str| {
            format!(r#"{{"schema": 1, "rank": "family", "name": "Pinaceae"{extra}}}"#)
        };
        let at = "library/pinaceae/family.json";
        refused(&[(at, "{}")], "pinaceae/family.json: `schema` must be 1");
        refused(
            &[(at, &family(r#", "generator": {"program": "no-such"}"#))],
            "pinaceae/family.json: unknown program `no-such`",
        );
        refused(
            &[(at, &family(r#", "forms": {"tree": {}}"#))],
            "pinaceae/family.json, forms.tree: `tree` is not a growth form",
        );
        refused(
            &[(at, &family(r#", "generator": {"paramz": {}}"#))],
            "pinaceae/family.json: its values do not fit a spec: unknown field `paramz`",
        );
        refused(
            &[(at, &family(r#", "forms": {"shrub": {"tier": 3}}"#))],
            "pinaceae/family.json, forms.shrub: its values do not fit a spec",
        );
        refused(
            &[(at, &family(r#", "parent": "order/no-such-order""#))],
            "pinaceae/family.json: its parent _ranks/order/no-such-order.json is not in the library",
        );
        refused(
            &[("library/_ranks/Order/pinales.json", "{}")],
            "is not named as a rank folder",
        );
        // A part may leave a section to the species below it, and delete
        // what is set above it.
        let folder = Folder::new("partial-ranks");
        folder.write(
            at,
            &family(
                r#", "appearance": {"foliage": [0.1, 0.3, 0.1]}, "allometry": null,
                "evidence": {
                    "appearance.foliage": {"evidence": "Authored", "source": "plantgen-authors", "note": "A test."}}"#,
            ),
        );
        Library::from_dir(&folder.0).unwrap();
    }

    /// A family's traits and the rules homed in it reach the species
    /// below it, built-in ones too, unless a species sets the value by
    /// hand; `plantc spec` reads the outcome from `Library::inherited`.
    #[test]
    fn traits_and_rules_in_a_folder_reach_species() {
        let folder = Folder::new("traits");
        folder.write(
            "library/sapindaceae/family.json",
            r#"{"schema": 1, "rank": "family", "name": "Sapindaceae",
                "evidence": {
                    "traits": {"evidence": "Authored", "source": "plantgen-authors", "note": "Test traits."},
                    "rules": {"evidence": "Authored", "source": "plantgen-authors", "note": "Test rules."}},
                "traits": {"leaf_length_m": 0.2, "leaf_arrangement": "opposite"},
                "rules": {
                    "generator.params.space_density": {
                        "rule": "clamp(8 * pow(0.15 / leaf_length_m, 0.75), 5, 60)",
                        "why": "Corner's rules"},
                    "generator.params.alternate": {
                        "map": {"leaf_arrangement": {"alternate": 1, "opposite": 0}}}}}"#,
        );
        // A maple of the folder's own that leaves both to the family.
        let mut own: serde_json::Value = serde_json::from_str(source("acer-circinatum")).unwrap();
        own["id"] = "acer-test".into();
        own["taxon"]["scientific_name"] = "Acer test".into();
        let params = own["generator"]["params"].as_object_mut().unwrap();
        params.remove("space_density");
        params.remove("alternate");
        folder.write(
            "library/sapindaceae/acer/acer-test/spec.json",
            &serde_json::to_string_pretty(&own).unwrap(),
        );
        let library = Library::from_dir(&folder.0).unwrap();
        let spec = library.spec("acer-test").unwrap();
        // 8 × 0.75^0.75, to the 6 significant digits a rule keeps.
        let expected = (8.0 * crate::math::pow(0.75, 0.75)).clamp(5.0, 60.0);
        let expected: f64 = format!("{expected:.5e}").parse().unwrap();
        assert!((spec.generator.params["space_density"] - expected).abs() < 1e-12);
        assert!(spec.generator.params["alternate"].abs() < 1e-12);
        let inherited = library.inherited("acer-test").unwrap();
        assert_eq!(
            inherited.rules["generator.params.space_density"],
            ("sapindaceae/family.json".to_string(), None)
        );
        // The built-in vine maple sets its shoot spacing by hand.
        let vine = library.inherited("acer-circinatum").unwrap();
        assert_eq!(
            vine.rules["generator.params.space_density"].1.as_deref(),
            Some("set by hand in sapindaceae/acer/acer-circinatum/spec.json")
        );
        // A maple outside the folder's family keeps its spec.
        assert_eq!(
            library.entry("thuja-plicata").unwrap().source(),
            source("thuja-plicata")
        );
    }

    #[test]
    fn traits_and_rules_against_the_vocabulary_are_refused() {
        let refused = |text: &str, expected: &str| {
            let folder = Folder::new("refused-traits");
            folder.write("library/sapindaceae/family.json", text);
            let message = Library::from_dir(&folder.0).unwrap_err().0;
            assert!(message.contains(expected), "{message}");
        };
        let family = |extra: &str| {
            format!(r#"{{"schema": 1, "rank": "family", "name": "Sapindaceae", {extra}}}"#)
        };
        refused(
            &family(r#""traits": {"leaf_size": 1}"#),
            "sapindaceae/family.json: `leaf_size` is not a trait in traits.json",
        );
        refused(
            &family(r#""traits": {"leaf_arrangement": "spiral"}"#),
            "`leaf_arrangement` is one of alternate",
        );
        refused(
            &family(r#""rules": {"generator.params.alternate": {"rule": "leaf_form * 2"}}"#),
            "it reads `leaf_form`, which is neither a number, count or bool trait",
        );
        refused(
            &family(r#""traits": {"ray_florets": {"min": 8, "max": 5}}"#),
            "`ray_florets`: its `max` 5 is below its `min` 8",
        );
        refused(
            &family(r#""rules": {"generator.params.alternate": {"rule": "leaf_sise * 2"}}"#),
            "it reads `leaf_sise`, which is neither",
        );
        refused(
            &family(r#""rules": {"generator.params.alternate": {"map": {"leaflets": {"1": 1}}}}"#),
            "a map reads an enum or bool trait; `leaflets` is a count",
        );
        refused(
            &family(r#""rules": {"generator.params.space_densty": {"rule": "leaflets"}}"#),
            "no program has a parameter `space_densty`",
        );
        // A species' own traits are checked too.
        let folder = Folder::new("refused-own-traits");
        let mut own: serde_json::Value = serde_json::from_str(source("acer-circinatum")).unwrap();
        own["traits"] = serde_json::json!({"clonal": "yes"});
        folder.write(
            "library/sapindaceae/acer/acer-circinatum/spec.json",
            &own.to_string(),
        );
        let message = Library::from_dir(&folder.0).unwrap_err().0;
        assert!(
            message.contains("acer-circinatum/spec.json: `clonal` is a bool"),
            "{message}"
        );
        // And what it states with what it inherits: bark needs wood.
        let folder = Folder::new("refused-ruled-out");
        folder.write(
            "library/sapindaceae/family.json",
            &family(
                r#""traits": {"woodiness": "herbaceous"}, "evidence": {"traits": {"evidence": "Authored", "source": "plantgen-authors", "note": "Test."}}"#,
            ),
        );
        let mut own: serde_json::Value = serde_json::from_str(source("acer-circinatum")).unwrap();
        own["traits"] = serde_json::json!({"bark_texture": "smooth"});
        own["evidence"]["traits"] = serde_json::json!({"evidence": "Authored",
            "source": "plantgen-authors", "note": "Test."});
        folder.write(
            "library/sapindaceae/acer/acer-circinatum/spec.json",
            &own.to_string(),
        );
        let message = Library::from_dir(&folder.0).unwrap_err().0;
        assert!(
            message.contains(
                "acer-circinatum/spec.json: it states `bark_texture` with what it inherits, but `bark_texture` is a character of the bark, which exists only where `woodiness` is woody or semi; here `woodiness` is herbaceous"
            ),
            "{message}"
        );
    }

    /// No trait is named as a parameter of a built-in program, so a
    /// formula's names say what they read.
    #[test]
    fn no_trait_is_named_as_a_parameter() {
        let vocabulary = Vocabulary::builtin();
        for (program, params) in Library::builtin().program_params() {
            for param in params.keys() {
                assert!(
                    !vocabulary.traits.contains_key(param),
                    "`{param}` is a trait and a parameter of `{program}`"
                );
            }
        }
    }

    /// Stating characters changes no species' spec, and so no package,
    /// while no rule reads them: every built-in spec is what it would be
    /// with no trait stated anywhere.
    #[test]
    fn characters_change_no_spec_until_a_rule_reads_them() {
        let library = Library::builtin();
        let mut stated = 0;
        let stripped: BTreeMap<String, RankFile> = library
            .ranks
            .iter()
            .map(|(file, rank)| {
                let mut rank = rank.clone();
                stated += rank.shared.traits.len();
                rank.shared.traits.clear();
                for part in rank.forms.values_mut() {
                    stated += part.traits.len();
                    part.traits.clear();
                }
                (file.clone(), rank)
            })
            .collect();
        for entry in library.species() {
            let mut own: serde_json::Value = serde_json::from_str(entry.own_source()).unwrap();
            if let Some(traits) = own.as_object_mut().unwrap().remove("traits") {
                stated += traits.as_object().unwrap().len();
            }
            let above = crate::inherit::above(&stripped, &entry.family, &entry.genus).unwrap();
            let bare =
                crate::inherit::inherit(&entry.file(), &own, &above, library.program_params())
                    .unwrap();
            // The notes on traits left in `own` stay out of the spec too.
            assert_eq!(
                bare.spec,
                library.inherited(&entry.id).unwrap().spec,
                "{}",
                entry.id
            );
        }
        // The library states characters, so the test tests something.
        assert!(stated > 20, "{stated}");
    }

    fn source_of_program(name: &str) -> &'static str {
        PROGRAMS
            .iter()
            .find(|(program, _)| *program == name)
            .map(|(_, source)| *source)
            .unwrap()
    }
}

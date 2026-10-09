//! Compiles the built-in library in: every
//! `library/<family>/<genus>/<id>/spec.json`, with the `conditions.json`,
//! `shed.json` and `niche.json` beside it, becomes one entry of `library::LIBRARY`,
//! sorted by path, so
//! adding a species is adding its folder. A species folder holding
//! folders instead holds the taxa below the species (subspecies,
//! varieties, forms, cultivars), each laid out as a species is:
//! `library/<family>/<genus>/<species>/<id>/spec.json`. Every
//! `library/sources/<id>.json` becomes one entry of `library::SOURCES`,
//! sorted by id, and `library/aliases.json`, the old ids of renamed taxa,
//! becomes `library::ALIASES`. No index is kept: the tree is the index.
//! `library`'s tests hold the tree to its rules.
//!
//! The rank files (growth plan G2: `library/_ranks/<rank>/<name>.json`,
//! `<family>/family.json`, `<family>/<genus>/genus.json`) become
//! `library::RANKS`, by path, and each species' entry carries its
//! effective spec, its own `spec.json` merged onto the rank files above
//! it and its traits resolved by their rules, by [`inherit`], the same
//! code the library runs, against the parameters of the programs in
//! `programs/`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
#[path = "src/formula.rs"]
mod formula;
#[allow(dead_code)]
#[path = "src/inherit.rs"]
mod inherit;

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

/// The JSON files directly inside `path`, sorted by name.
fn json_files(path: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "json")
        })
        .collect();
    found.sort();
    found
}

fn name(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| panic!("{} has no UTF-8 name", path.display()))
}

/// Whether a folder beside the families holds no family.
fn beside_families(folder: &Path) -> bool {
    ["sources", inherit::RANKS].contains(&name(folder))
}

/// The rank files of the tree at `root`, by their path in it, sorted.
fn rank_files(root: &Path) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    let ranks_folder = root.join(inherit::RANKS);
    if ranks_folder.is_dir() {
        for rank in folders(&ranks_folder) {
            for file in json_files(&rank) {
                found.push((
                    format!("{}/{}/{}", inherit::RANKS, name(&rank), name(&file)),
                    file,
                ));
            }
        }
    }
    for family in folders(root) {
        if beside_families(&family) {
            continue;
        }
        if family.join("family.json").is_file() {
            found.push((
                format!("{}/family.json", name(&family)),
                family.join("family.json"),
            ));
        }
        for genus in folders(&family) {
            if genus.join("genus.json").is_file() {
                found.push((
                    format!("{}/{}/genus.json", name(&family), name(&genus)),
                    genus.join("genus.json"),
                ));
            }
        }
    }
    found.sort();
    found
}

/// The species' spec as compiled in: its own file, or where rank files
/// stand above it, its merged spec, written to `out_dir`.
fn effective(
    species: &Path,
    file: &str,
    above: &[&inherit::RankFile],
    programs: &inherit::Programs,
    out_dir: &Path,
) -> PathBuf {
    let spec = species.join("spec.json");
    let own = fs::read_to_string(&spec)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", spec.display()));
    match inherit::effective_text(file, &own, above, programs)
        .unwrap_or_else(|error| panic!("library/{error}"))
    {
        None => spec,
        Some(text) => {
            let folder = out_dir.join("inherited");
            fs::create_dir_all(&folder)
                .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
            let path = folder.join(format!("{}.json", name(species)));
            fs::write(&path, text)
                .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
            path
        }
    }
}

/// One `Species` per species folder of the tree at `root`, or per folder
/// of a taxon below a species, and how many.
fn species_entries(
    root: &Path,
    ranks: &BTreeMap<String, inherit::RankFile>,
    programs: &inherit::Programs,
    out_dir: &Path,
) -> (String, usize) {
    let mut entries = String::new();
    let mut count = 0_usize;
    for family in folders(root) {
        if beside_families(&family) {
            continue;
        }
        for genus in folders(&family) {
            let above = inherit::above(ranks, name(&family), name(&genus))
                .unwrap_or_else(|error| panic!("library/{error}"));
            for species in folders(&genus) {
                let below = folders(&species);
                if below.is_empty() {
                    entries.push_str(&species_entry(
                        &family, &genus, None, &species, &above, programs, out_dir,
                    ));
                    count += 1;
                    continue;
                }
                // A species folder holding the taxa below it holds nothing
                // of its own: how they would take its sections is not
                // settled.
                assert!(
                    json_files(&species).is_empty(),
                    "{} holds taxa below its species and files of its own",
                    species.display()
                );
                for taxon in below {
                    entries.push_str(&species_entry(
                        &family,
                        &genus,
                        Some(&species),
                        &taxon,
                        &above,
                        programs,
                        out_dir,
                    ));
                    count += 1;
                }
            }
        }
    }
    (entries, count)
}

/// The `Species` of the folder `taxon`: a species, or with `species`, a
/// taxon below that species.
fn species_entry(
    family: &Path,
    genus: &Path,
    species: Option<&Path>,
    taxon: &Path,
    above: &[&inherit::RankFile],
    programs: &inherit::Programs,
    out_dir: &Path,
) -> String {
    let spec = taxon.join("spec.json");
    assert!(spec.is_file(), "{} has no spec.json", taxon.display());
    let section = |name: &str| {
        let path = taxon.join(name);
        if path.is_file() {
            format!("Some(include_str!({:?}))", path.display().to_string())
        } else {
            "None".to_string()
        }
    };
    let conditions = section("conditions.json");
    let shed = section("shed.json");
    let niche = section("niche.json");
    let folder = species.map_or_else(
        || format!("{}/{}", name(family), name(genus)),
        |species| format!("{}/{}/{}", name(family), name(genus), name(species)),
    );
    let file = format!("{folder}/{}/spec.json", name(taxon));
    let source = effective(taxon, &file, above, programs, out_dir);
    format!(
        "    Species {{ id: {:?}, family: {:?}, genus: {:?}, species: {:?}, source: include_str!({:?}), own: include_str!({:?}), conditions: {conditions}, shed: {shed}, niche: {niche} }},\n",
        name(taxon),
        name(family),
        name(genus),
        species.map(name),
        source.display().to_string(),
        spec.display().to_string(),
    )
}

/// The aliases of `library/aliases.json`, old id and current id, sorted
/// by old id, and the file, if there is one.
fn aliases(root: &Path) -> (String, String) {
    let path = root.join("aliases.json");
    if !path.is_file() {
        return (String::new(), "None".to_string());
    }
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let file: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let aliases = file
        .get("aliases")
        .and_then(serde_json::Value::as_object)
        .unwrap_or_else(|| panic!("{} has no `aliases` object", path.display()));
    let mut rows = String::new();
    // A JSON object's keys come sorted.
    for (old, current) in aliases {
        let current = current
            .as_str()
            .unwrap_or_else(|| panic!("{}: the alias `{old}` is not an id", path.display()));
        writeln!(rows, "    ({old:?}, {current:?}),").expect("writing to a String");
    }
    (
        rows,
        format!("Some(include_str!({:?}))", path.display().to_string()),
    )
}

/// A table's rows: each key with its file's text.
fn table<'a>(rows: impl Iterator<Item = (&'a str, &'a Path)>) -> String {
    let mut table = String::new();
    for (key, path) in rows {
        writeln!(
            table,
            "    ({key:?}, include_str!({:?})),",
            path.display().to_string()
        )
        .expect("writing to a String");
    }
    table
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("set by Cargo"));
    let root = manifest.join("library");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("set by Cargo"));
    println!("cargo:rerun-if-changed={}", root.display());
    // The programs' parameters, which rules are checked against.
    let programs_folder = manifest.join("programs");
    println!("cargo:rerun-if-changed={}", programs_folder.display());
    let mut sources: Vec<(String, String)> = fs::read_dir(&programs_folder)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", programs_folder.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "lsys")
        })
        .map(|path| {
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_else(|| panic!("{} has no UTF-8 name", path.display()))
                .to_string();
            (stem, text)
        })
        .collect();
    sources.sort();
    let programs = inherit::program_params(
        sources
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str())),
    );
    let rank_files = rank_files(&root);
    let ranks: BTreeMap<String, inherit::RankFile> = rank_files
        .iter()
        .map(|(file, path)| {
            let text = fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            let rank = inherit::RankFile::parse(file, &text)
                .unwrap_or_else(|error| panic!("library/{file}: {error}"));
            (file.clone(), rank)
        })
        .collect();
    let (entries, count) = species_entries(&root, &ranks, &programs, &out_dir);
    let ranks_table = table(
        rank_files
            .iter()
            .map(|(file, path)| (file.as_str(), path.as_path())),
    );
    // The sources values cite by id, one file each.
    let sources_folder = root.join("sources");
    let source_files = if sources_folder.is_dir() {
        json_files(&sources_folder)
    } else {
        Vec::new()
    };
    let sources = table(source_files.iter().map(|file| {
        let id = file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_else(|| panic!("{} has no UTF-8 name", file.display()));
        (id, file.as_path())
    }));
    let (alias_rows, aliases_file) = aliases(&root);
    let out = out_dir.join("library.rs");
    fs::write(
        &out,
        format!(
            "/// Every species of the built-in library ({count}), sorted by family, genus and id.\n\
             pub const LIBRARY: &[Species] = &[\n{entries}];\n\
             /// Every source of the built-in library, by id, as JSON, sorted by id.\n\
             pub const SOURCES: &[(&str, &str)] = &[\n{sources}];\n\
             /// Every rank file of the built-in library (growth plan G2), by its path in the tree, as JSON, sorted by path.\n\
             pub const RANKS: &[(&str, &str)] = &[\n{ranks_table}];\n\
             /// Every old id of a renamed taxon of the built-in library, with its current id, sorted by old id.\n\
             pub const ALIASES: &[(&str, &str)] = &[\n{alias_rows}];\n\
             /// `library/aliases.json`, as JSON, if the built-in library has it.\n\
             const ALIASES_FILE: Option<&str> = {aliases_file};\n"
        ),
    )
    .unwrap_or_else(|error| panic!("cannot write {}: {error}", out.display()));
}

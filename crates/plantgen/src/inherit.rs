//! Specs inherited down the tree of life (growth plan G2).
//!
//! Beside its species, the library may hold a spec for any taxon above
//! them, a *rank file*:
//!
//! - `<family>/family.json` in a family's folder and
//!   `<family>/<genus>/genus.json` in a genus's;
//! - `_ranks/<rank>/<name>.json` for the ranks above families (orders,
//!   classes, phyla and unranked clades) and between a family and its
//!   genera (subfamilies, tribes), kept flat beside the families, each
//!   naming its parent, so a change in the classification is a one-field
//!   edit.
//!
//! A rank file is a spec's JSON with every key optional, plus `schema`,
//! `rank`, `name` (the taxon's; its file or folder is that name in lower
//! case, spaces as hyphens), `parent` and `forms`. `parent` names the rank
//! file it stands on as `<rank>/<name>`, such as `order/sapindales` or
//! `family/poaceae`; a genus stands on its family's file unless it names a
//! subfamily or tribe of that family. `forms` holds values for one growth
//! form only, by the form's name, laid over the file's other values. A
//! rank file never holds `id` or `taxon`: only a species' own `spec.json`
//! names a species.
//!
//! A species stands on a chain of rank files: its genus's, its family's,
//! and those they stand on, up to the top. Its effective spec is that
//! chain and its own `spec.json` merged from the top down by JSON Merge
//! Patch (RFC 7396), so the nearer file wins: objects merge key by key,
//! arrays and other values replace, and `null` deletes what the files
//! above set. Evidence travels with the values: each note of the
//! effective spec comes from one file of the chain, and a note is dropped
//! where a nearer file sets, replaces or deletes anything it describes. A
//! species with no rank file above it is its own `spec.json`, byte for
//! byte.
//!
//! The module reads nothing but JSON, so the build script compiles it too
//! and merges every built-in species' chain while compiling.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

/// The schema number of a rank file.
pub const RANK_SCHEMA: u64 = 1;

/// The folder beside the families that holds the ranks kept flat:
/// `_ranks/<rank>/<name>.json`.
pub const RANKS: &str = "_ranks";

/// An unranked group, such as the eudicots: it may stand anywhere above
/// families.
pub const CLADE: &str = "clade";

/// The ranks a rank file stands at, highest first; and [`CLADE`].
pub const LEVELS: [&str; 13] = [
    "kingdom",
    "phylum",
    "subphylum",
    "class",
    "subclass",
    "superorder",
    "order",
    "suborder",
    "family",
    "subfamily",
    "tribe",
    "subtribe",
    "genus",
];

const FAMILY: usize = 8;
const GENUS: usize = 12;

fn level(rank: &str) -> Option<usize> {
    LEVELS.iter().position(|level| *level == rank)
}

/// Values and the evidence notes on them, by path.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Part {
    /// A partial spec, without `evidence`.
    pub values: Map<String, Value>,
    /// Its `evidence`: each note's path and the note.
    pub notes: Map<String, Value>,
}

/// The spec of a taxon above species.
#[derive(Debug, Clone, PartialEq)]
pub struct RankFile {
    /// Its path in the library tree, which names it:
    /// `_ranks/order/sapindales.json`, `sapindaceae/family.json` or
    /// `sapindaceae/acer/genus.json`.
    pub file: String,
    /// One of [`LEVELS`], or [`CLADE`].
    pub rank: String,
    /// The taxon's name as written: `Sapindaceae`, `core eudicots`.
    pub name: String,
    /// For a family's or a genus's file, the family's folder.
    pub family: Option<String>,
    /// The rank file it stands on, by path: the one its `parent` names,
    /// or for a genus that names none, its family's `family.json`, which
    /// may be missing.
    pub parent: Option<String>,
    /// Whether it named its parent.
    pub named_parent: bool,
    /// Its values for every growth form.
    pub shared: Part,
    /// Its values for one growth form, by the form's name, laid over
    /// `shared`.
    pub forms: BTreeMap<String, Part>,
}

impl RankFile {
    /// The rank file at `file`, its path in the library tree, from its
    /// text.
    ///
    /// # Errors
    ///
    /// Fails on a file out of place, invalid JSON, a `schema`, `rank` or
    /// `name` that does not fit its place, a `parent` that is not a rank
    /// file's, a key only a species sets, `traits` (not read before
    /// growth plan G2.2), or an evidence note on a value the file does not
    /// set.
    pub fn parse(file: &str, text: &str) -> Result<Self, String> {
        let place = Place::of(file)?;
        let mut map = match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => map,
            Ok(_) => return Err("a rank file is a JSON object".into()),
            Err(error) => return Err(format!("invalid rank file: {error}")),
        };
        if map.remove("schema").and_then(|schema| schema.as_u64()) != Some(RANK_SCHEMA) {
            return Err(format!("`schema` must be {RANK_SCHEMA}"));
        }
        let rank = take_string(&mut map, "rank")?;
        if rank != place.rank {
            return Err(format!(
                "its place makes it a {} file; it says `rank` {rank}",
                place.rank
            ));
        }
        let name = take_string(&mut map, "name")?;
        if file_name(&name) != place.name {
            return Err(format!(
                "its `name` {name} would be named `{}`, not `{}`",
                file_name(&name),
                place.name
            ));
        }
        let named = match map.remove("parent") {
            None => None,
            Some(Value::String(parent)) => Some(parent_file(&parent)?),
            Some(_) => return Err("`parent` is a string, `<rank>/<name>`".into()),
        };
        let below_family = level(&rank).is_some_and(|level| level > FAMILY);
        if below_family && rank != "genus" && named.is_none() {
            return Err(format!(
                "a {rank} names its `parent`: its family, or the rank of that family it sits in"
            ));
        }
        let named_parent = named.is_some();
        let parent = named.or_else(|| {
            place
                .family
                .as_ref()
                .filter(|_| rank == "genus")
                .map(|family| format!("{family}/family.json"))
        });
        let forms = match map.remove("forms") {
            None => BTreeMap::new(),
            Some(Value::Object(forms)) => forms
                .into_iter()
                .map(|(form, part)| match part {
                    Value::Object(part) => {
                        Part::read(part, &format!("forms.{form}")).map(|part| (form, part))
                    }
                    _ => Err(format!("`forms.{form}` is an object, a partial spec")),
                })
                .collect::<Result<_, String>>()?,
            Some(_) => return Err("`forms` is an object: growth form → partial spec".into()),
        };
        let shared = Part::read(map, "")?;
        Ok(Self {
            file: file.to_string(),
            rank,
            name,
            family: place.family,
            parent,
            named_parent,
            shared,
            forms,
        })
    }

    /// Its parts: its shared values, then each form's, with where each
    /// sits in the file (`""` or `forms.<growth form>`).
    pub fn parts(&self) -> impl Iterator<Item = (String, &Part)> {
        std::iter::once((String::new(), &self.shared)).chain(
            self.forms
                .iter()
                .map(|(form, part)| (format!("forms.{form}"), part)),
        )
    }
}

impl Part {
    /// The part of a rank file at `at` (`""` for its shared values,
    /// `forms.<growth form>` for a form's) from its keys.
    fn read(mut values: Map<String, Value>, at: &str) -> Result<Self, String> {
        let within = |key: &str| {
            if at.is_empty() {
                format!("`{key}`")
            } else {
                format!("`{at}.{key}`")
            }
        };
        for key in ["id", "taxon"] {
            if values.contains_key(key) {
                return Err(format!(
                    "a rank file holds no {}: only a species' own spec.json sets it",
                    within(key)
                ));
            }
        }
        if values.contains_key("traits") {
            return Err(format!(
                "{} are not read before growth plan G2.2 (traits.toml)",
                within("traits")
            ));
        }
        if !at.is_empty() {
            for key in ["schema", "rank", "name", "parent", "forms", "growth_form"] {
                if values.contains_key(key) {
                    return Err(format!(
                        "{} belongs to the whole file, not to one form",
                        within(key)
                    ));
                }
            }
        }
        let notes = match values.remove("evidence") {
            None => Map::new(),
            Some(Value::Object(notes)) => notes,
            Some(_) => return Err(format!("{} is an object", within("evidence"))),
        };
        if let Some(path) = notes.keys().find(|path| find(&values, path).is_none()) {
            return Err(format!(
                "the evidence note on `{path}` in {} is on a value it does not set",
                if at.is_empty() { "the file" } else { at }
            ));
        }
        Ok(Self { values, notes })
    }
}

/// Where a rank file sits: the rank and name its place gives it.
struct Place {
    rank: String,
    name: String,
    family: Option<String>,
}

impl Place {
    fn of(file: &str) -> Result<Self, String> {
        let parts: Vec<&str> = file.split('/').collect();
        match parts.as_slice() {
            [RANKS, rank, name] => {
                let stem = name
                    .strip_suffix(".json")
                    .filter(|stem| is_name(stem))
                    .ok_or_else(|| {
                        format!(
                            "{file}: a rank file is named by its taxon, in lowercase letters, digits and hyphens, then `.json`"
                        )
                    })?;
                if *rank != CLADE
                    && !level(rank).is_some_and(|level| level != FAMILY && level != GENUS)
                {
                    return Err(format!(
                        "{file}: `{rank}` is not a rank kept in {RANKS}/; those are {CLADE} and the ranks of {} except family and genus",
                        LEVELS.join(", ")
                    ));
                }
                Ok(Self {
                    rank: (*rank).to_string(),
                    name: stem.to_string(),
                    family: None,
                })
            }
            [family, "family.json"] => Ok(Self {
                rank: "family".into(),
                name: (*family).to_string(),
                family: Some((*family).to_string()),
            }),
            [family, genus, "genus.json"] => Ok(Self {
                rank: "genus".into(),
                name: (*genus).to_string(),
                family: Some((*family).to_string()),
            }),
            _ => Err(format!(
                "{file} is not where a rank file goes: {RANKS}/<rank>/<name>.json, <family>/family.json or <family>/<genus>/genus.json"
            )),
        }
    }
}

/// A rank file's name: its taxon's in lower case, spaces as hyphens.
#[must_use]
pub fn file_name(name: &str) -> String {
    name.to_lowercase().replace(' ', "-")
}

/// 1 to 64 lowercase letters, digits and hyphens.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// The path of the rank file a `parent` of `<rank>/<name>` names.
fn parent_file(parent: &str) -> Result<String, String> {
    let (rank, name) = parent
        .split_once('/')
        .filter(|(rank, name)| {
            is_name(name) && (*rank == CLADE || level(rank).is_some_and(|level| level != GENUS))
        })
        .ok_or_else(|| {
            format!(
                "`parent` {parent} is not `<rank>/<name>` of a rank file, such as `order/sapindales` or `family/poaceae`"
            )
        })?;
    Ok(if rank == "family" {
        format!("{name}/family.json")
    } else {
        format!("{RANKS}/{rank}/{name}.json")
    })
}

fn take_string(map: &mut Map<String, Value>, key: &str) -> Result<String, String> {
    match map.remove(key) {
        Some(Value::String(value)) => Ok(value),
        _ => Err(format!("`{key}` is missing or not a string")),
    }
}

/// The value at `path` (keys joined by dots; an array's items by index).
#[must_use]
pub fn find<'a>(values: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let mut value = values.get(parts.next()?)?;
    for part in parts {
        value = match value {
            Value::Object(map) => map.get(part)?,
            Value::Array(items) => items.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(value)
}

/// The rank files a species of the genus `genus` in the family `family`
/// stands on, nearest first: its genus's file, or its family's when the
/// genus has none, and those it stands on.
///
/// # Errors
///
/// As [`chain`].
pub fn above<'a>(
    ranks: &'a BTreeMap<String, RankFile>,
    family: &str,
    genus: &str,
) -> Result<Vec<&'a RankFile>, String> {
    let genus_file = format!("{family}/{genus}/genus.json");
    if ranks.contains_key(&genus_file) {
        chain(ranks, &genus_file)
    } else {
        chain(ranks, &format!("{family}/family.json"))
    }
}

/// The rank file `file` and those it stands on, nearest first; none if
/// the library lacks `file`.
///
/// # Errors
///
/// Fails on a named parent the library lacks, parents that come back
/// round, a rank standing on one at or below it, a clade below a family,
/// or a file below a family that names its parent but does not reach its
/// own family through it.
pub fn chain<'a>(
    ranks: &'a BTreeMap<String, RankFile>,
    file: &str,
) -> Result<Vec<&'a RankFile>, String> {
    let mut found: Vec<&'a RankFile> = Vec::new();
    // The level of the lowest ranked file met so far, and whether a clade
    // has been met.
    let mut lowest: Option<usize> = None;
    let mut over_clade = false;
    let mut next = ranks.get(file);
    while let Some(rank) = next {
        if found.iter().any(|seen| seen.file == rank.file) {
            return Err(format!("{}: its parents come back round to it", rank.file));
        }
        if let Some(below) = found.last() {
            let fits = match level(&rank.rank) {
                Some(level) => {
                    lowest.is_none_or(|lowest| level < lowest) && !(over_clade && level >= FAMILY)
                }
                None => lowest.is_none_or(|lowest| lowest <= FAMILY),
            };
            if !fits {
                return Err(format!(
                    "{} ({}) cannot stand on {} ({})",
                    below.file, below.rank, rank.file, rank.rank
                ));
            }
        }
        match level(&rank.rank) {
            Some(level) => lowest = Some(level),
            None => over_clade = true,
        }
        found.push(rank);
        next = match &rank.parent {
            None => None,
            Some(parent) => match ranks.get(parent) {
                Some(next) => Some(next),
                None if rank.named_parent => {
                    return Err(format!(
                        "{}: its parent {parent} is not in the library",
                        rank.file
                    ));
                }
                None => None,
            },
        };
    }
    if let Some(first) = found.first()
        && first.named_parent
        && level(&first.rank).is_some_and(|level| level > FAMILY)
    {
        match found.iter().find(|rank| rank.rank == "family") {
            None => {
                return Err(format!("{}: its parents never reach a family", first.file));
            }
            Some(family) if first.family.is_some() && family.family != first.family => {
                return Err(format!(
                    "{}: its parents reach {}, not its own family's file",
                    first.file, family.file
                ));
            }
            Some(_) => {}
        }
    }
    Ok(found)
}

/// A species' spec as it inherits it.
#[derive(Debug, Clone, PartialEq)]
pub struct Inherited {
    /// The effective spec.
    pub spec: Value,
    /// The files it was merged from, nearest first: the species' own
    /// `spec.json`, then the rank files it stands on.
    pub chain: Vec<String>,
    /// Each value's path, such as `generator.params.whorl` (an array is
    /// one value), and the file that set it: its path in the tree, then
    /// `, forms.<growth form>` for a value from a form's part.
    pub origins: BTreeMap<String, String>,
    /// Each evidence note's path and the file it came from, written as in
    /// `origins`.
    pub notes: BTreeMap<String, String>,
}

/// The effective spec of a species whose own `spec.json`, at `file` in the
/// tree, holds `own`, on the rank files `above` it, nearest first as
/// [`above`] gives them.
///
/// The growth form whose parts apply is the nearest one set: the
/// species', else the nearest rank file's.
///
/// # Errors
///
/// Fails if `own` is not a JSON object or its `evidence` not an object.
pub fn inherit(file: &str, own: &Value, above: &[&RankFile]) -> Result<Inherited, String> {
    let Value::Object(own) = own else {
        return Err(format!("{file}: a spec is a JSON object"));
    };
    let mut values = own.clone();
    let notes = match values.remove("evidence") {
        None => Map::new(),
        Some(Value::Object(notes)) => notes,
        Some(_) => return Err(format!("{file}: `evidence` is an object")),
    };
    let own = Part { values, notes };
    let form = own
        .values
        .get("growth_form")
        .or_else(|| {
            above
                .iter()
                .find_map(|rank| rank.shared.values.get("growth_form"))
        })
        .and_then(Value::as_str);
    let mut layers: Vec<(String, &Part)> = Vec::new();
    for rank in above.iter().rev() {
        layers.push((rank.file.clone(), &rank.shared));
        if let Some((form, part)) = form.and_then(|form| Some((form, rank.forms.get(form)?))) {
            layers.push((format!("{}, forms.{form}", rank.file), part));
        }
    }
    layers.push((file.to_string(), &own));
    let mut spec = Map::new();
    let mut origins = BTreeMap::new();
    let mut notes: BTreeMap<String, (String, Value)> = BTreeMap::new();
    for (label, part) in &layers {
        let mut touched = Vec::new();
        merge(
            &mut spec,
            &part.values,
            "",
            label,
            &mut origins,
            &mut touched,
        );
        notes.retain(|path, _| !touched.iter().any(|set| overlaps(set, path)));
        for (path, note) in &part.notes {
            notes.insert(path.clone(), (label.clone(), note.clone()));
        }
    }
    if !notes.is_empty() {
        spec.insert(
            "evidence".into(),
            Value::Object(
                notes
                    .iter()
                    .map(|(path, (_, note))| (path.clone(), note.clone()))
                    .collect(),
            ),
        );
    }
    Ok(Inherited {
        spec: Value::Object(spec),
        chain: std::iter::once(file.to_string())
            .chain(above.iter().map(|rank| rank.file.clone()))
            .collect(),
        origins,
        notes: notes
            .into_iter()
            .map(|(path, (label, _))| (path, label))
            .collect(),
    })
}

/// The effective spec of a species, as the library stores it: `None` when
/// no rank file stands above it, so it is its own text; else the merged
/// spec as pretty JSON.
///
/// # Errors
///
/// As [`inherit`], and on an own text that is not JSON.
pub fn effective_text(
    file: &str,
    own: &str,
    above: &[&RankFile],
) -> Result<Option<String>, String> {
    if above.is_empty() {
        return Ok(None);
    }
    let value: Value =
        serde_json::from_str(own).map_err(|error| format!("{file}: invalid JSON: {error}"))?;
    let inherited = inherit(file, &value, above)?;
    let text = serde_json::to_string_pretty(&inherited.spec)
        .map_err(|error| format!("{file}: {error}"))?;
    // serde_json reads some numbers of 16 or more digits back a bit off
    // from how it wrote them: refuse a merged spec that does not read back
    // as merged, rather than change its package unseen.
    let back: Value =
        serde_json::from_str(&text).map_err(|error| format!("{file}: invalid JSON: {error}"))?;
    if let Some(path) = difference(&inherited.spec, &back, "") {
        return Err(format!(
            "{file}: `{path}` does not read back as written once merged; give it at most 15 significant digits"
        ));
    }
    Ok(Some(text))
}

/// The first path where two values differ, if they do.
fn difference(a: &Value, b: &Value, at: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) if a.len() == b.len() => {
            a.iter().find_map(|(key, inside)| {
                let path = if at.is_empty() {
                    key.clone()
                } else {
                    format!("{at}.{key}")
                };
                match b.get(key) {
                    Some(other) => difference(inside, other, &path),
                    None => Some(path),
                }
            })
        }
        _ => (a != b).then(|| at.to_string()),
    }
}

/// Lay `patch` over `target` by RFC 7396.
pub fn patch(target: &mut Map<String, Value>, patch: &Map<String, Value>) {
    merge(target, patch, "", "", &mut BTreeMap::new(), &mut Vec::new());
}

/// `values` with every null left out, at any depth.
#[must_use]
pub fn without_nulls(values: &Map<String, Value>) -> Map<String, Value> {
    values
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, value)| match value {
            Value::Object(inside) => (key.clone(), Value::Object(without_nulls(inside))),
            _ => (key.clone(), value.clone()),
        })
        .collect()
}

/// Merge `layer` into `target` by RFC 7396, noting the file `label` as
/// the origin of each value it sets and every path it sets, replaces or
/// deletes in `touched`.
fn merge(
    target: &mut Map<String, Value>,
    layer: &Map<String, Value>,
    at: &str,
    label: &str,
    origins: &mut BTreeMap<String, String>,
    touched: &mut Vec<String>,
) {
    for (key, value) in layer {
        let path = if at.is_empty() {
            key.clone()
        } else {
            format!("{at}.{key}")
        };
        match value {
            Value::Null => {
                target.remove(key);
                forget(origins, &path);
                touched.push(path);
            }
            Value::Object(inside) => {
                let entry = target.entry(key.clone()).or_insert(Value::Null);
                if !entry.is_object() {
                    forget(origins, &path);
                    *entry = Value::Object(Map::new());
                    touched.push(path.clone());
                }
                if let Value::Object(entry) = entry {
                    merge(entry, inside, &path, label, origins, touched);
                }
            }
            _ => {
                forget(origins, &path);
                target.insert(key.clone(), value.clone());
                origins.insert(path.clone(), label.to_string());
                touched.push(path);
            }
        }
    }
}

/// Forget the origins of the value at `path` and of everything inside it.
fn forget(origins: &mut BTreeMap<String, String>, path: &str) {
    origins.remove(path);
    // The paths inside `path` sort together: from `path.` up to `path/`.
    let inside: Vec<String> = origins
        .range(format!("{path}.")..format!("{path}/"))
        .map(|(inside, _)| inside.clone())
        .collect();
    for inside in inside {
        origins.remove(&inside);
    }
}

/// Whether one path is the other or lies inside it.
fn overlaps(a: &str, b: &str) -> bool {
    let inside = |path: &str, outer: &str| {
        path.len() > outer.len() && path.starts_with(outer) && path.as_bytes()[outer.len()] == b'.'
    };
    a == b || inside(a, b) || inside(b, a)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::{RankFile, above, chain, effective_text, file_name, inherit};

    fn ranks(files: &[(&str, Value)]) -> BTreeMap<String, RankFile> {
        files
            .iter()
            .map(|(file, value)| {
                (
                    (*file).to_string(),
                    RankFile::parse(file, &value.to_string())
                        .unwrap_or_else(|error| panic!("{file}: {error}")),
                )
            })
            .collect()
    }

    /// RFC 7396's own examples (its appendix A), each patch laid by a
    /// rank file over a species' value.
    #[test]
    fn values_merge_as_json_merge_patch() {
        let cases = [
            (json!({"a": "b"}), json!({"a": "c"}), json!({"a": "c"})),
            (
                json!({"a": "b"}),
                json!({"b": "c"}),
                json!({"a": "b", "b": "c"}),
            ),
            (json!({"a": "b"}), json!({"a": null}), json!({})),
            (
                json!({"a": "b", "b": "c"}),
                json!({"a": null}),
                json!({"b": "c"}),
            ),
            (json!({"a": ["b"]}), json!({"a": "c"}), json!({"a": "c"})),
            (json!({"a": "c"}), json!({"a": ["b"]}), json!({"a": ["b"]})),
            (
                json!({"a": {"b": "c"}}),
                json!({"a": {"b": "d", "c": null}}),
                json!({"a": {"b": "d"}}),
            ),
            (
                json!({"a": [{"b": "c"}]}),
                json!({"a": [1]}),
                json!({"a": [1]}),
            ),
            (
                json!({}),
                json!({"a": {"bb": {"ccc": null}}}),
                json!({"a": {"bb": {}}}),
            ),
        ];
        for (target, patch, wanted) in cases {
            // The family file holds the target, the species the patch.
            let mut family = json!({"schema": 1, "rank": "family", "name": "Testaceae"});
            family["generator"] = target.clone();
            let files = ranks(&[("testaceae/family.json", family)]);
            let chain = above(&files, "testaceae", "testa").unwrap();
            let own = json!({"generator": patch});
            let inherited = inherit("testaceae/testa/testa-one/spec.json", &own, &chain).unwrap();
            assert_eq!(
                inherited.spec,
                json!({"generator": wanted}),
                "{target} + {patch}"
            );
        }
    }

    /// A clade, an order, a family with a part per form, and a genus.
    fn sapindaceae() -> BTreeMap<String, RankFile> {
        ranks(&[
            (
                "_ranks/clade/eudicots.json",
                json!({"schema": 1, "rank": "clade", "name": "eudicots",
                    "tier": "procedural_proxy",
                    "generator": {"program": "broadleaf", "params": {"a": 1.0, "b": 1.0}},
                    "evidence": {"generator.params": {"evidence": "Authored", "note": "Clade."}}}),
            ),
            (
                "_ranks/order/sapindales.json",
                json!({"schema": 1, "rank": "order", "name": "Sapindales",
                    "parent": "clade/eudicots",
                    "generator": {"params": {"c": 1.0}},
                    "evidence": {"generator.params.c": {"evidence": "Authored", "note": "Order."}}}),
            ),
            (
                "sapindaceae/family.json",
                json!({"schema": 1, "rank": "family", "name": "Sapindaceae",
                "parent": "order/sapindales",
                "generator": {"params": {"b": 2.0}},
                "forms": {
                    "decurrent_tree": {"generator": {"program": "sapindaceae", "params": {"b": 3.0}}},
                    "shrub": {"generator": {"program": "shrub"}}
                }}),
            ),
            (
                "sapindaceae/acer/genus.json",
                json!({"schema": 1, "rank": "genus", "name": "Acer",
                    "growth_form": "decurrent_tree",
                    "generator": {"params": {"a": null}}}),
            ),
        ])
    }

    /// The chain from the top down, each file's values over those above,
    /// a form's part over its file's shared values; each value's origin
    /// and each note from the file that set what it describes.
    #[test]
    fn a_species_inherits_down_its_chain() {
        let files = sapindaceae();
        let chain = above(&files, "sapindaceae", "acer").unwrap();
        let names: Vec<&str> = chain.iter().map(|rank| rank.file.as_str()).collect();
        assert_eq!(
            names,
            [
                "sapindaceae/acer/genus.json",
                "sapindaceae/family.json",
                "_ranks/order/sapindales.json",
                "_ranks/clade/eudicots.json"
            ]
        );
        let file = "sapindaceae/acer/acer-test/spec.json";
        let own = json!({"id": "acer-test", "taxon": {"x": 1},
            "generator": {"params": {"d": 4.0}},
            "evidence": {"generator.params.d": {"evidence": "Authored", "note": "Own."}}});
        let inherited = inherit(file, &own, &chain).unwrap();
        assert_eq!(
            inherited.spec,
            json!({
                "id": "acer-test",
                "taxon": {"x": 1},
                "tier": "procedural_proxy",
                "growth_form": "decurrent_tree",
                "generator": {"program": "sapindaceae", "params": {"b": 3.0, "c": 1.0, "d": 4.0}},
                "evidence": {
                    "generator.params.c": {"evidence": "Authored", "note": "Order."},
                    "generator.params.d": {"evidence": "Authored", "note": "Own."}
                }
            })
        );
        let origin = |path: &str| inherited.origins[path].as_str();
        assert_eq!(origin("tier"), "_ranks/clade/eudicots.json");
        assert_eq!(origin("generator.params.c"), "_ranks/order/sapindales.json");
        assert_eq!(
            origin("generator.params.b"),
            "sapindaceae/family.json, forms.decurrent_tree"
        );
        assert_eq!(
            origin("generator.program"),
            "sapindaceae/family.json, forms.decurrent_tree"
        );
        assert_eq!(origin("growth_form"), "sapindaceae/acer/genus.json");
        assert_eq!(origin("generator.params.d"), file);
        assert!(!inherited.origins.contains_key("generator.params.a"));
        // The clade's note on all the params is dropped: files below it
        // set and delete params.
        assert_eq!(
            inherited.notes,
            BTreeMap::from([
                (
                    "generator.params.c".to_string(),
                    "_ranks/order/sapindales.json".to_string()
                ),
                ("generator.params.d".to_string(), file.to_string()),
            ])
        );
        assert_eq!(inherited.chain.len(), 5);
        assert_eq!(inherited.chain[0], file);
    }

    /// A shrub of the same family takes the shrub's part; with no genus
    /// file, it stands on the family's.
    #[test]
    fn each_growth_form_takes_its_own_part() {
        let files = sapindaceae();
        let shrub = json!({"id": "dodonaea-test", "growth_form": "shrub"});
        let chain = above(&files, "sapindaceae", "dodonaea").unwrap();
        assert_eq!(chain[0].file, "sapindaceae/family.json");
        let inherited = inherit(
            "sapindaceae/dodonaea/dodonaea-test/spec.json",
            &shrub,
            &chain,
        )
        .unwrap();
        assert_eq!(inherited.spec["generator"]["program"], "shrub");
        assert_eq!(
            inherited.spec["generator"]["params"],
            json!({"a": 1.0, "b": 2.0, "c": 1.0})
        );
    }

    /// A species with no rank file above it is its own text; one under
    /// a family file that sets nothing keeps its values.
    #[test]
    fn a_species_without_rank_files_is_its_own_spec() {
        let own = r#"{"id": "testa-one", "generator": {"params": {"x": 0.1}},
            "evidence": {"generator": {"evidence": "Authored", "note": "Own."}}}"#;
        assert_eq!(
            effective_text("f/g/testa-one/spec.json", own, &[]),
            Ok(None)
        );
        let files = ranks(&[(
            "testaceae/family.json",
            json!({"schema": 1, "rank": "family", "name": "Testaceae"}),
        )]);
        let chain = above(&files, "testaceae", "testa").unwrap();
        let text = effective_text("f/g/testa-one/spec.json", own, &chain)
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            serde_json::from_str::<Value>(own).unwrap()
        );
        // A number that would not read back as merged is refused, not
        // changed.
        let long = r#"{"id": "testa-one", "generator": {"params": {"x": 0.99634467830471407}}}"#;
        let error = effective_text("f/g/testa-one/spec.json", long, &chain).unwrap_err();
        assert!(
            error.contains("`generator.params.x` does not read back"),
            "{error}"
        );
    }

    #[test]
    fn rank_files_against_the_rules_are_refused() {
        let refused = |file: &str, value: Value, wanted: &str| {
            let error = RankFile::parse(file, &value.to_string()).unwrap_err();
            assert!(error.contains(wanted), "{file}: {error}");
        };
        let family = |extra: Value| {
            let mut value = json!({"schema": 1, "rank": "family", "name": "Testaceae"});
            for (key, item) in extra.as_object().unwrap() {
                value[key] = item.clone();
            }
            value
        };
        refused(
            "testaceae/x.json",
            family(json!({})),
            "not where a rank file goes",
        );
        refused(
            "_ranks/family/testaceae.json",
            family(json!({})),
            "not a rank kept",
        );
        refused(
            "_ranks/order/Test.json",
            family(json!({})),
            "named by its taxon",
        );
        refused(
            "testaceae/family.json",
            family(json!({"schema": 2})),
            "`schema` must be 1",
        );
        refused(
            "testaceae/family.json",
            family(json!({"rank": "order"})),
            "it says `rank` order",
        );
        refused(
            "otheraceae/family.json",
            family(json!({})),
            "would be named `testaceae`",
        );
        refused(
            "testaceae/family.json",
            family(json!({"parent": "orders"})),
            "is not `<rank>/<name>`",
        );
        refused(
            "testaceae/family.json",
            family(json!({"parent": "genus/testa"})),
            "is not `<rank>/<name>`",
        );
        refused(
            "testaceae/family.json",
            family(json!({"id": "testaceae"})),
            "holds no `id`",
        );
        refused(
            "testaceae/family.json",
            family(json!({"traits": {}})),
            "G2.2",
        );
        refused(
            "testaceae/family.json",
            family(json!({"forms": {"shrub": {"growth_form": "shrub"}}})),
            "`forms.shrub.growth_form` belongs to the whole file",
        );
        refused(
            "testaceae/family.json",
            family(json!({"forms": {"shrub": {"taxon": {}}}})),
            "holds no `forms.shrub.taxon`",
        );
        refused(
            "testaceae/family.json",
            family(json!({"tier": "calibrated",
                "evidence": {"generator": {"evidence": "Authored", "note": "Nothing."}}})),
            "the evidence note on `generator` in the file is on a value it does not set",
        );
        refused(
            "_ranks/tribe/testeae.json",
            json!({"schema": 1, "rank": "tribe", "name": "Testeae"}),
            "a tribe names its `parent`",
        );
        assert_eq!(file_name("core eudicots"), "core-eudicots");
        RankFile::parse(
            "_ranks/clade/core-eudicots.json",
            &json!({"schema": 1, "rank": "clade", "name": "core eudicots"}).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn chains_against_the_rules_are_refused() {
        let file = |rank: &str, name: &str, parent: Option<&str>| {
            let mut value = json!({"schema": 1, "rank": rank, "name": name});
            if let Some(parent) = parent {
                value["parent"] = parent.into();
            }
            value
        };
        let refused = |files: &[(&str, Value)], start: &str, wanted: &str| {
            let error = chain(&ranks(files), start).unwrap_err();
            assert!(error.contains(wanted), "{error}");
        };
        refused(
            &[(
                "testaceae/family.json",
                file("family", "Testaceae", Some("order/tests")),
            )],
            "testaceae/family.json",
            "its parent _ranks/order/tests.json is not in the library",
        );
        refused(
            &[
                ("_ranks/clade/a.json", file("clade", "a", Some("clade/b"))),
                ("_ranks/clade/b.json", file("clade", "b", Some("clade/a"))),
            ],
            "_ranks/clade/a.json",
            "come back round",
        );
        refused(
            &[
                ("_ranks/class/c.json", file("class", "c", Some("order/o"))),
                ("_ranks/order/o.json", file("order", "o", None)),
            ],
            "_ranks/class/c.json",
            "_ranks/class/c.json (class) cannot stand on _ranks/order/o.json (order)",
        );
        refused(
            &[
                ("_ranks/order/o.json", file("order", "o", Some("clade/k"))),
                ("_ranks/clade/k.json", file("clade", "k", Some("order/p"))),
                ("_ranks/order/p.json", file("order", "p", None)),
            ],
            "_ranks/order/o.json",
            "_ranks/clade/k.json (clade) cannot stand on _ranks/order/p.json (order)",
        );
        refused(
            &[
                ("_ranks/tribe/t.json", file("tribe", "t", Some("clade/k"))),
                ("_ranks/clade/k.json", file("clade", "k", None)),
            ],
            "_ranks/tribe/t.json",
            "_ranks/tribe/t.json (tribe) cannot stand on _ranks/clade/k.json (clade)",
        );
        refused(
            &[
                ("_ranks/tribe/t.json", file("tribe", "t", Some("order/o"))),
                ("_ranks/order/o.json", file("order", "o", None)),
            ],
            "_ranks/tribe/t.json",
            "never reach a family",
        );
        refused(
            &[
                (
                    "testaceae/testa/genus.json",
                    file("genus", "Testa", Some("tribe/t")),
                ),
                (
                    "_ranks/tribe/t.json",
                    file("tribe", "t", Some("family/otheraceae")),
                ),
                ("otheraceae/family.json", file("family", "Otheraceae", None)),
            ],
            "testaceae/testa/genus.json",
            "not its own family's file",
        );
        // A genus in a tribe of its family reaches the family through it;
        // a genus naming no parent stands on its family's file, if any.
        let files = ranks(&[
            (
                "testaceae/testa/genus.json",
                file("genus", "Testa", Some("tribe/t")),
            ),
            (
                "_ranks/tribe/t.json",
                file("tribe", "t", Some("family/testaceae")),
            ),
            (
                "testaceae/family.json",
                file("family", "Testaceae", Some("clade/k")),
            ),
            ("_ranks/clade/k.json", file("clade", "k", None)),
            ("testaceae/other/genus.json", file("genus", "Other", None)),
        ]);
        assert_eq!(
            chain(&files, "testaceae/testa/genus.json").unwrap().len(),
            4
        );
        assert_eq!(above(&files, "testaceae", "other").unwrap().len(), 3);
        assert_eq!(above(&files, "testaceae", "third").unwrap().len(), 2);
        assert!(above(&files, "nothingaceae", "third").unwrap().is_empty());
    }
}

//! The library as the window's browser lists it: every species with its
//! names, its growth form and its place in the tree of life, read from
//! the specs and the rank files they stand on (group, order, family,
//! genus), and the search over it.
//!
//! Nothing here is the window's own: the names come from each spec's
//! `taxon`, the form from its `growth_form`, and the order and group by
//! walking the family's rank file up through its `parent`s.

use std::collections::BTreeMap;

use plantgen::library::Library;
use serde_json::Value;

/// A growth form as the browser's filter groups them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Form {
    Tree,
    Shrub,
    Herb,
    Grass,
    Fern,
    Succulent,
    Palm,
    Vine,
    Other,
}

impl Form {
    /// The filter's chips, in order.
    pub const ALL: [Self; 9] = [
        Self::Tree,
        Self::Shrub,
        Self::Herb,
        Self::Grass,
        Self::Fern,
        Self::Succulent,
        Self::Palm,
        Self::Vine,
        Self::Other,
    ];

    /// The form of a spec's `growth_form`.
    #[must_use]
    pub fn of(growth_form: &str) -> Self {
        match growth_form {
            "excurrent_tree" | "decurrent_tree" | "scale_leaved_tree" => Self::Tree,
            "shrub" | "cushion" => Self::Shrub,
            "forb" => Self::Herb,
            "graminoid" => Self::Grass,
            "fern" => Self::Fern,
            "stem_succulent" | "rosette_succulent" => Self::Succulent,
            "palm" => Self::Palm,
            "vine" | "epiphyte" => Self::Vine,
            _ => Self::Other,
        }
    }

    /// Its chip's word, plural.
    #[must_use]
    pub const fn plural(self) -> &'static str {
        match self {
            Self::Tree => "Trees",
            Self::Shrub => "Shrubs",
            Self::Herb => "Herbs",
            Self::Grass => "Grasses",
            Self::Fern => "Ferns",
            Self::Succulent => "Succulents",
            Self::Palm => "Palms",
            Self::Vine => "Climbers",
            Self::Other => "Others",
        }
    }

    /// Its word for one plant.
    #[must_use]
    pub const fn one(self) -> &'static str {
        match self {
            Self::Tree => "tree",
            Self::Shrub => "shrub",
            Self::Herb => "herb",
            Self::Grass => "grass or sedge",
            Self::Fern => "fern",
            Self::Succulent => "succulent",
            Self::Palm => "palm or cycad",
            Self::Vine => "climber",
            Self::Other => "plant",
        }
    }
}

/// One species of the library.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    /// `Rosa nutkana`; the id where the spec gives none.
    pub scientific: String,
    /// `Nootka rose`; empty where the spec gives none.
    pub common: String,
    /// The spec's `growth_form`, as written (`decurrent_tree`).
    pub growth_form: String,
    pub form: Form,
    /// Each rank's name as the rank files write it.
    pub group: String,
    pub order: String,
    pub family: String,
    pub genus: String,
    /// What the search matches, lower case: the names, the id and the
    /// ranks.
    haystack: String,
}

impl Item {
    /// Its name first in a list: the common name, or the scientific one.
    #[must_use]
    pub fn title(&self) -> &str {
        if self.common.is_empty() {
            &self.scientific
        } else {
            &self.common
        }
    }

    /// Whether every word of `query` (lower case) is in its names or ranks.
    #[must_use]
    pub fn matches(&self, query: &str) -> bool {
        query
            .split_whitespace()
            .all(|word| self.haystack.contains(word))
    }
}

/// The groups the browser opens with: the large clades and classes a
/// species' rank files lead up to, nearest first.
const GROUPS: [(&str, &str); 6] = [
    ("monocots", "Monocots"),
    ("magnoliids", "Magnoliids"),
    ("eudicots", "Eudicots"),
    ("gymnosperms", "Gymnosperms"),
    ("angiosperms", "Early flowering plants"),
    ("euphyllophytes", "Ferns and allies"),
];

/// Every species, sorted by group, order, family, genus and name.
#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    pub items: Vec<Item>,
}

/// A node of the browser's tree: a rank's name and the items under it.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// `order`, `family` or `genus`.
    pub rank: &'static str,
    pub name: String,
    /// Indexes into [`Catalogue::items`].
    pub items: Vec<usize>,
    pub children: Vec<Node>,
}

impl Catalogue {
    /// The catalogue of `library`.
    #[must_use]
    pub fn of(library: &Library) -> Self {
        let mut items: Vec<Item> = library
            .species()
            .iter()
            .map(|entry| {
                let spec: Value = serde_json::from_str(entry.source()).unwrap_or(Value::Null);
                let taxon = &spec["taxon"];
                let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
                let scientific = Some(text(&taxon["scientific_name"]))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| entry.id.clone());
                let growth_form = text(&spec["growth_form"]);
                let family = Some(text(&taxon["family"]))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| capitalised(&entry.family));
                let genus = Some(text(&taxon["genus"]))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| capitalised(&entry.genus));
                let (group, order) = lineage(library, &entry.family);
                let common = text(&taxon["common_name"]);
                let haystack = [
                    entry.id.as_str(),
                    &scientific,
                    &common,
                    &family,
                    &genus,
                    &order,
                    &group,
                    &growth_form.replace('_', " "),
                ]
                .join(" ")
                .to_lowercase();
                Item {
                    id: entry.id.clone(),
                    scientific,
                    common,
                    form: Form::of(&growth_form),
                    growth_form,
                    group,
                    order,
                    family,
                    genus,
                    haystack,
                }
            })
            .collect();
        let rank = |group: &str| {
            GROUPS
                .iter()
                .position(|(_, name)| *name == group)
                .unwrap_or(GROUPS.len())
        };
        items.sort_by(|a, b| {
            (rank(&a.group), &a.group, &a.order, &a.family, &a.genus)
                .cmp(&(rank(&b.group), &b.group, &b.order, &b.family, &b.genus))
                .then_with(|| a.scientific.cmp(&b.scientific))
        });
        Self { items }
    }

    /// The item `id`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }

    /// The tree of the items `keep` keeps: a node per group, holding its
    /// orders, their families and their genera, each in the catalogue's
    /// order. A node holds the items of every node under it.
    #[must_use]
    pub fn tree(&self, keep: impl Fn(&Item) -> bool) -> Vec<Node> {
        let mut groups: Vec<Node> = Vec::new();
        for (index, item) in self.items.iter().enumerate() {
            if !keep(item) {
                continue;
            }
            let group = child(&mut groups, "group", &item.group);
            group.items.push(index);
            let order = child(&mut group.children, "order", &item.order);
            order.items.push(index);
            let family = child(&mut order.children, "family", &item.family);
            family.items.push(index);
            let genus = child(&mut family.children, "genus", &item.genus);
            genus.items.push(index);
        }
        groups
    }

    /// How many items each form has.
    #[must_use]
    pub fn forms(&self) -> BTreeMap<Form, usize> {
        let mut counts = BTreeMap::new();
        for item in &self.items {
            *counts.entry(item.form).or_insert(0) += 1;
        }
        counts
    }
}

/// The last node of `nodes` if it is `name`, else a new one.
fn child<'a>(nodes: &'a mut Vec<Node>, rank: &'static str, name: &str) -> &'a mut Node {
    if nodes.last().is_none_or(|node| node.name != name) {
        nodes.push(Node {
            rank,
            name: name.to_owned(),
            items: Vec::new(),
            children: Vec::new(),
        });
    }
    let last = nodes.len() - 1;
    &mut nodes[last]
}

/// A family's group and order, by walking its rank file up through its
/// parents: the order's name, and the nearest of [`GROUPS`], else the
/// highest class or phylum named. Unplaced where the files say nothing.
fn lineage(library: &Library, family: &str) -> (String, String) {
    let mut order = None;
    let mut group = None;
    let mut class = None;
    let mut file = Some(format!("{family}/family.json"));
    let mut seen = 0;
    while let Some(path) = file.take() {
        let Some(rank) = library.rank(&path) else {
            break;
        };
        let name = rank.name.to_lowercase();
        match rank.rank.as_str() {
            "order" if order.is_none() => order = Some(rank.name.clone()),
            "class" | "phylum" => class = Some(capitalised(&rank.name)),
            _ => {}
        }
        if group.is_none()
            && let Some((_, label)) = GROUPS.iter().find(|(clade, _)| *clade == name)
        {
            group = Some((*label).to_owned());
        }
        file.clone_from(&rank.parent);
        // A cycle in the files would be a bug elsewhere; never hang on one.
        seen += 1;
        if seen > 40 {
            break;
        }
    }
    (
        group.or(class).unwrap_or_else(|| "Other plants".into()),
        order.unwrap_or_else(|| "Order not placed".into()),
    )
}

/// `text` with its first letter capitalised.
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_species_is_placed_with_its_names_and_ranks() {
        let catalogue = Catalogue::of(Library::builtin());
        assert_eq!(catalogue.items.len(), Library::builtin().species().len());
        let rose = catalogue.get("rosa-nutkana").expect("Nootka rose");
        assert_eq!(rose.common, "Nootka rose");
        assert_eq!(rose.scientific, "Rosa nutkana");
        assert_eq!(
            (
                rose.order.as_str(),
                rose.family.as_str(),
                rose.genus.as_str()
            ),
            ("Rosales", "Rosaceae", "Rosa")
        );
        assert_eq!(rose.group, "Eudicots");
        assert_eq!(rose.form, Form::Shrub);
        let fir = catalogue.get("pseudotsuga-menziesii").expect("Douglas-fir");
        assert_eq!(fir.group, "Gymnosperms");
        assert_eq!(fir.form, Form::Tree);
    }

    #[test]
    fn the_search_matches_every_word_in_any_name_or_rank() {
        let catalogue = Catalogue::of(Library::builtin());
        let rose = catalogue.get("rosa-nutkana").unwrap();
        assert!(rose.matches("nootka"));
        assert!(rose.matches("rosaceae rose"));
        assert!(rose.matches("rosales"));
        assert!(!rose.matches("rose pine"));
    }

    #[test]
    fn the_tree_holds_every_kept_item_once_under_its_ranks() {
        let catalogue = Catalogue::of(Library::builtin());
        let tree = catalogue.tree(|_| true);
        let total: usize = tree.iter().map(|group| group.items.len()).sum();
        assert_eq!(total, catalogue.items.len());
        for group in &tree {
            let orders: usize = group.children.iter().map(|order| order.items.len()).sum();
            assert_eq!(orders, group.items.len(), "{}", group.name);
        }
        let trees = catalogue.tree(|item| item.form == Form::Tree);
        let kept: usize = trees.iter().map(|group| group.items.len()).sum();
        assert_eq!(kept, catalogue.forms()[&Form::Tree]);
    }
}

//! A compound leaf is one leaf drawn on one card (spec rework SEMLOD,
//! "counted once"): the light a plant grows by and the litter it drops
//! count the leaf once, at the area its card draws, and the census counts
//! the leaf as a leaf and its leaflets only as what the leaf holds.

use plantgen::divided;
use plantgen::grow::{Growth, GrowthSettings, grow};
use plantgen::looks::{Season, Shape};
use plantgen::lsys::{Limits, Program};
use plantgen::organs::{self, Tally};
use plantgen::spec::{self, PlantSpec};
use plantgen::templates::{self, Templates};

/// Midsummer, when every deciduous compound leaf is out.
const DAY: f64 = 180.0;

/// Years each species grows here: enough for a deciduous plant to drop a
/// few years of leaves.
const YEARS: f64 = 4.0;

/// The leaflets a compound look draws: none for a dissected blade.
fn leaflets(shape: &Shape) -> Option<u64> {
    let Shape::Compound(compound) = shape else {
        return None;
    };
    let laid = divided::layout(compound);
    Some(if laid.form.confluent > 0.0 {
        0
    } else {
        laid.leaflets.len() as u64
    })
}

/// The tallies termed `term`, anywhere in the census, each with whether it
/// is what the plant holds on its axes.
fn find<'a>(tallies: &'a [Tally], term: &str, top: bool, found: &mut Vec<(&'a Tally, bool)>) {
    for tally in tallies {
        if tally.term == term {
            found.push((tally, top));
        }
        find(&tally.parts, term, false, found);
    }
}

/// `spec` grown from its first variant for [`YEARS`] at most.
fn grown(spec: &PlantSpec, program: &Program, params: &[f64]) -> Growth {
    let variant = &spec.variant_list()[0];
    let years = spec.growth.years.min(YEARS);
    grow(
        program,
        params,
        &GrowthSettings {
            seed: variant.seed,
            dt: spec.growth.step,
            years,
            keyframes: vec![years],
            conditions: variant.conditions(),
            limits: Limits::default(),
            host: spec.host_geometry().unwrap(),
        },
    )
    .unwrap()
}

/// How many times `growth`'s compound leaves (organ types `compound`)
/// fell, each time pushing to `failures` unless what fell weighs one
/// leaf's area each: between all of them the size of the largest and all
/// the size of the mean.
fn litter(
    id: &str,
    growth: &Growth,
    names: &[&str],
    compound: &[usize],
    failures: &mut Vec<String>,
) -> usize {
    let mut fell = 0;
    for shed in &growth.shed().organs {
        let index = usize::from(shed.organ);
        if !compound.contains(&index) || shed.count == 0 {
            continue;
        }
        fell += 1;
        let area = growth.organ_types[index].area;
        let most = area * shed.size * shed.size;
        let least = most / f64::from(shed.count);
        if !(least * (1.0 - 1e-9) <= shed.area && shed.area <= most * (1.0 + 1e-9)) {
            failures.push(format!(
                "{id} `{}`: {} fell with {:.4} m², not between {least:.4} and {most:.4}",
                names[index], shed.count, shed.area
            ));
        }
    }
    fell
}

#[test]
fn a_compound_leaf_is_counted_once() {
    let mut species = 0;
    let mut fell = 0;
    let mut failures = Vec::new();
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        let (program, params) = spec.program().unwrap();
        let looks = spec.appearance.looks(program.organs());
        let names: Vec<&str> = program.organs().map(|(name, _)| name).collect();
        let compound: Vec<usize> = (0..names.len())
            .filter(|&index| matches!(looks[index].shape, Shape::Compound(_)))
            .collect();
        if compound.is_empty() {
            continue;
        }
        species += 1;

        // Light: the leaf shades with the area its one card draws.
        let templates = Templates::for_looks(&looks);
        let shading = spec::organ_areas(&program, &params).unwrap();
        for &index in &compound {
            let drawn = templates::drawn_area(&looks[index], &templates.templates[index]);
            if !spec::areas_agree(drawn, shading[index]) {
                failures.push(format!(
                    "{id} `{}` draws {drawn:.3} m² on its card but shades with {:.3} m²",
                    names[index], shading[index]
                ));
            }
        }

        let growth = grown(&spec, &program, &params);

        // Litter: each fell as one leaf.
        fell += litter(id, &growth, &names, &compound, &mut failures);

        // Census: each compound leaf is one leaf; its leaflets are only
        // what it holds.
        let graph = growth.keyframes.last().unwrap();
        let terms = organs::of_program(&program, &spec.appearance);
        let organ_types: Vec<(&str, &str)> = names.iter().copied().zip(terms.clone()).collect();
        let structure_types: Vec<(&str, &str)> = growth
            .structure_types
            .iter()
            .map(|kind| (kind.name.as_str(), kind.term.as_str()))
            .collect();
        let seasons: Vec<Option<&Season>> = names
            .iter()
            .map(|name| {
                spec.appearance
                    .organs
                    .get(*name)
                    .and_then(|look| look.season.as_ref())
            })
            .collect();
        let tallies = organs::census(graph, &organ_types, &structure_types, &looks, &seasons, DAY);
        let mut leaves = Vec::new();
        find(&tallies, "leaf", true, &mut leaves);
        let mut held = Vec::new();
        find(&tallies, "leaflet", true, &mut held);
        let grown_leaves = graph
            .organs
            .iter()
            .filter(|organ| terms[usize::from(organ.organ)] == "leaf")
            .count() as u64
            + graph
                .structures
                .iter()
                .filter(|structure| structure_types[usize::from(structure.structure)].1 == "leaf")
                .count() as u64;
        let counted: u64 = leaves.iter().map(|(tally, _)| tally.count).sum();
        if counted != grown_leaves {
            failures.push(format!(
                "{id}: the census counts {counted} leaves of the {grown_leaves} grown"
            ));
        }
        // Leaflets grown as organs of their own (a fern's pinnae) stand
        // on the axes; the leaflets a card draws are never there.
        let grown_leaflets = terms.contains(&"leaflet");
        let drawn: u64 = graph
            .organs
            .iter()
            .filter(|organ| compound.contains(&usize::from(organ.organ)))
            .filter(|organ| {
                let index = usize::from(organ.organ);
                let phase = organs::phase(organ, seasons[index], DAY, graph.age);
                !matches!(phase, Some(organs::Phase::Primordium | organs::Phase::Shed))
            })
            .map(|organ| leaflets(&looks[usize::from(organ.organ)].shape).unwrap_or(0))
            .sum();
        let within: u64 = held
            .iter()
            .filter(|(_, top)| !top)
            .map(|(tally, _)| tally.count)
            .sum();
        if held.iter().any(|(_, top)| *top) && !grown_leaflets {
            failures.push(format!(
                "{id}: the census counts drawn leaflets on the axes"
            ));
        }
        if !grown_leaflets && within != drawn {
            failures.push(format!(
                "{id}: the census holds {within} leaflets in its leaves, its cards draw {drawn}"
            ));
        }
    }
    assert!(species >= 15, "only {species} species draw compound leaves");
    assert!(fell > 0, "no compound leaf fell");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// No built-in palmate leaf, opened as far as its other values say
/// (its spread left to [`divided::open_enough`]), crowds its leaflets into
/// one another where they are widest (visual forms leaf blades): thirteen
/// leaflets in a half circle drew a scalloped fan, not a lupine's leaf. A
/// spread set by hand from photographs stands, touching leaflets and all.
#[test]
fn builtin_palmate_leaflets_open_apart() {
    let library = plantgen::library::Library::builtin();
    let mut crowded = Vec::new();
    let mut checked = 0;
    for entry in library.species() {
        let spec = PlantSpec::builtin(&entry.id).unwrap();
        for (organ, look) in &spec.appearance.organs {
            let Shape::Compound(compound) = &look.shape else {
                continue;
            };
            let opened = plantgen::looks::Compound {
                spread: None,
                ..compound.clone()
            };
            if let Some(crowding) = divided::crowding(&opened) {
                checked += 1;
                if crowding > 1.0 {
                    crowded.push(format!("{} `{organ}`: {crowding:.2}", entry.id));
                }
            }
        }
    }
    assert!(checked >= 5, "only {checked} palmate leaves");
    assert!(crowded.is_empty(), "leaflets overlap: {crowded:?}");
}

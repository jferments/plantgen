//! Plants grown in a place cut as Project After cuts it (CONTRACT "Places"):
//! a grid round the plant with cells from 0.025 m to 0.5 m, distances in
//! sixteenths of a cell. These tests use After's finest cells (0.025 m)
//! and about 0.1 m.

use plantgen::conditions::{Conditions, Intrusion};
use plantgen::graph::PlantGraph;
use plantgen::grow::{GrowthSettings, grow_in};
use plantgen::lsys::Limits;
use plantgen::math::Vec3;
use plantgen::spec::{Environment, PlantSpec};
use plantgen::substrate::{AIR, BUILT, ROCK, SOIL, Substrate, base64_encode};

/// A solid of a place: its signed distance and material.
enum Solid {
    /// Level ground, its top at `y`.
    Ground(f64),
    /// A boulder: centre and radius.
    Ball(Vec3, f64),
    /// A box: low and high corners.
    Block(Vec3, Vec3),
}

impl Solid {
    fn distance(&self, p: Vec3) -> f64 {
        match self {
            Self::Ground(y) => p.y - y,
            Self::Ball(centre, radius) => p.distance(*centre) - radius,
            Self::Block(low, high) => {
                let centre = (*low + *high) * 0.5;
                let half = (*high - *low) * 0.5;
                let q = Vec3::new(
                    (p.x - centre.x).abs() - half.x,
                    (p.y - centre.y).abs() - half.y,
                    (p.z - centre.z).abs() - half.z,
                );
                let outside = Vec3::new(q.x.max(0.0), q.y.max(0.0), q.z.max(0.0)).length();
                outside + q.x.max(q.y).max(q.z).min(0.0)
            }
        }
    }

    fn material(&self) -> u8 {
        match self {
            Self::Ground(_) => SOIL,
            Self::Ball(..) => ROCK,
            Self::Block(..) => BUILT,
        }
    }
}

/// The place's grid from `low` to `high` in cells `cell` wide, as After
/// writes it: an `i8` signed distance per cell in sixteenths of a cell.
fn grid(cell: f64, low: Vec3, high: Vec3, solids: &[Solid]) -> Substrate {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = |extent: f64| (extent / cell).ceil() as u32;
    let size = [
        count(high.x - low.x),
        count(high.y - low.y),
        count(high.z - low.z),
    ];
    let scale = cell / 16.0;
    let [nx, ny, nz] = size.map(|n| n as usize);
    let mut distance = Vec::with_capacity(nx * ny * nz);
    let mut material = Vec::with_capacity(nx * ny * nz);
    for y in 0..ny {
        for z in 0..nz {
            for x in 0..nx {
                #[allow(clippy::cast_precision_loss)]
                let centre = low + Vec3::new(x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5) * cell;
                let (d, m) = solids
                    .iter()
                    .map(|solid| (solid.distance(centre), solid.material()))
                    .fold((f64::INFINITY, AIR), |best, next| {
                        if next.0 < best.0 { next } else { best }
                    });
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let units = (d / scale).round().clamp(-127.0, 127.0) as i8;
                #[allow(clippy::cast_sign_loss)]
                distance.push(units as u8);
                material.push(if d < 0.0 { m } else { AIR });
            }
        }
    }
    Substrate {
        version: 1,
        preset: None,
        seed: None,
        scale: None,
        origin_m: Some([low.x, low.y, low.z]),
        cell_m: Some(cell),
        size: Some(size),
        distance_scale_m: Some(scale),
        distance: Some(base64_encode(&distance)),
        material: Some(base64_encode(&material)),
        up: Some([0.0, 1.0, 0.0]),
    }
}

fn place(substrate: Substrate) -> Conditions {
    let mut conditions = Conditions::from_json(r#"{"schema": 1}"#).unwrap();
    conditions.substrate = Some(substrate);
    conditions
}

/// `species`, its first open-grown variant, grown `years` in `place`.
fn grow(species: &str, place: &Conditions, years: f64) -> PlantGraph {
    let spec = PlantSpec::builtin(species).unwrap();
    let variant = spec
        .variant_list()
        .into_iter()
        .find(|variant| variant.environment == Environment::Open)
        .unwrap();
    let (program, params) = spec.program().unwrap();
    let mut growth = grow_in(
        &program,
        &spec.seeded_params(&program, &params, variant.seed),
        &GrowthSettings {
            seed: variant.seed,
            dt: spec.growth.step,
            years,
            keyframes: vec![years],
            conditions: variant.conditions(),
            limits: Limits::default(),
            host: None,
        },
        place,
        0.0,
        None,
    )
    .unwrap();
    growth.keyframes.remove(0)
}

fn intrusion(place: &Conditions, graph: &PlantGraph) -> Intrusion {
    place.intrusion(graph).unwrap()
}

fn leaf_count(graph: &PlantGraph) -> usize {
    graph
        .organs
        .iter()
        .filter(|organ| organ.shed.is_none_or(|shed| shed > graph.age))
        .count()
}

fn organs_centre(graph: &PlantGraph) -> Vec3 {
    #[allow(clippy::cast_precision_loss)]
    let count = graph.organs.len().max(1) as f64;
    graph
        .organs
        .iter()
        .fold(Vec3::ZERO, |sum, organ| sum + organ.position)
        / count
}

/// How far `graph` reaches from its stem and how high, metres.
fn extent(graph: &PlantGraph) -> (f64, f64) {
    let points = graph
        .segments
        .iter()
        .map(|segment| segment.end)
        .chain(graph.organs.iter().map(|organ| organ.position));
    points.fold((0.0_f64, 0.0_f64), |(reach, top), p| {
        (reach.max(p.x.hypot(p.z)), top.max(p.y))
    })
}

/// After's cell for a grid of this volume: the cube root of the volume
/// over 4 M cells, rounded up to a millimetre, 0.025 to 0.5 m.
fn after_cell(low: Vec3, high: Vec3) -> f64 {
    let size = high - low;
    let cell = (size.x * size.y * size.z / 4.0e6).cbrt();
    ((cell * 1000.0).ceil() / 1000.0).clamp(0.025, 0.5)
}

/// The place round a plant that reaches `reach` and stands `top` high, cut
/// as After cuts it (1 m past its reach, 1 m above its top and below its
/// foot): level ground, a boulder half buried east of the stem and a wall
/// to its north, both inside the open-grown crown. Its cells are `cell`
/// wide, or After's own size for the volume.
fn rock_and_wall(reach: f64, top: f64, cell: Option<f64>) -> (Conditions, f64) {
    let solids = [
        Solid::Ground(0.0),
        Solid::Ball(Vec3::new(0.7 * reach, 0.0, 0.0), 0.35 * reach),
        Solid::Block(
            Vec3::new(-reach - 1.0, -1.0, -reach - 1.0),
            Vec3::new(reach + 1.0, top + 1.0, -0.6 * reach),
        ),
    ];
    let low = Vec3::new(-reach - 1.0, -1.0, -reach - 1.0);
    let high = Vec3::new(reach + 1.0, top + 1.0, reach + 1.0);
    let cell = cell.unwrap_or_else(|| after_cell(low, high));
    (place(grid(cell, low, high, &solids)), cell)
}

/// A plant grown in the place at `cell` (or After's own cell), and its
/// and the open-grown plant's intrusions into the place.
struct Fitted {
    grown: PlantGraph,
    blind: Intrusion,
    seen: Intrusion,
    cell: f64,
}

fn fitted(species: &str, years: f64, cell: Option<f64>) -> Fitted {
    let open = Conditions::from_json(r#"{"schema": 1}"#).unwrap();
    let free = grow(species, &open, years);
    let (reach, top) = extent(&free);
    let (here, cell) = rock_and_wall(reach, top, cell);
    let grown = grow(species, &here, years);
    let (blind, seen) = (intrusion(&here, &free), intrusion(&here, &grown));
    Fitted {
        grown,
        blind,
        seen,
        cell,
    }
}

#[test]
fn plants_keep_out_of_rock_and_a_wall_at_after_s_cells() {
    // A sword fern at After's finest cells: its fronds lie over the
    // boulder and along the wall, none inside.
    let fern = fitted("polystichum-munitum", 8.0, Some(0.025));
    assert!(
        fern.blind.segments_in_solids > 50,
        "{}",
        fern.blind.describe()
    );
    assert!(
        fern.seen.segments_in_solids == 0 && fern.seen.organs_in_solids == 0,
        "{}",
        fern.seen.describe()
    );

    // A Douglas-fir at a tenth of a metre: a limb that comes near the
    // wall is shed there, so at most a few shoots touch it.
    // (Its limbs sag under load since pinidae 7, so fewer of the
    // open-grown fir's reach into the wall: 49.)
    let fir = fitted("pseudotsuga-menziesii", 15.0, Some(0.1));
    assert!(
        fir.blind.segments_in_solids > 40,
        "{}",
        fir.blind.describe()
    );
    assert!(
        fir.seen.segments_in_solids <= 5 && fir.seen.deepest_m < 0.1,
        "{}",
        fir.seen.describe()
    );

    // A bigleaf maple at a tenth of a metre: its buds find no goals in the
    // wall and do not grow once inside it, so at most a year's shoot goes
    // in, and fewer than half the open-grown crown's parts.
    // (The field reads at most 127 sixteenths of a cell, 0.79 m here, deep.)
    let maple = fitted("acer-macrophyllum", 15.0, Some(0.1));
    assert!(
        maple.blind.segments_in_solids > 40 && maple.blind.deepest_m > 0.6,
        "{}",
        maple.blind.describe()
    );
    assert!(
        maple.seen.segments_in_solids * 2 < maple.blind.segments_in_solids
            && maple.seen.deepest_m < 0.6,
        "{}",
        maple.seen.describe()
    );
}

#[test]
fn the_cell_size_hardly_changes_the_plant() {
    // The same place at After's finest cells and at 0.1 m, and at After's
    // own cell for a fir's grid and at 0.1 m and 0.2 m: the same plant to
    // within a percent of its leaves and two centimetres of their centre.
    for (species, years, cells) in [
        ("polystichum-munitum", 8.0, vec![Some(0.025), Some(0.1)]),
        (
            "pseudotsuga-menziesii",
            15.0,
            vec![None, Some(0.1), Some(0.2)],
        ),
    ] {
        let plants: Vec<Fitted> = cells
            .iter()
            .map(|cell| fitted(species, years, *cell))
            .collect();
        let first = &plants[0];
        for other in &plants[1..] {
            let (a, b) = (leaf_count(&first.grown), leaf_count(&other.grown));
            #[allow(clippy::cast_precision_loss)]
            let change = (a as f64 - b as f64).abs() / a as f64;
            let shift = organs_centre(&first.grown).distance(organs_centre(&other.grown));
            assert!(
                change < 0.01 && shift < 0.02,
                "{species}: {a} leaves at {} m cells, {b} at {} m; centres {shift:.3} m apart",
                first.cell,
                other.cell
            );
            assert_eq!(
                (first.seen.segments_in_solids, first.seen.organs_in_solids),
                (other.seen.segments_in_solids, other.seen.organs_in_solids),
                "{species} at {} m and {} m",
                first.cell,
                other.cell
            );
        }
        // And it is a plant fitted to the place, not the open-grown one.
        assert_ne!(leaf_count(&first.grown), 0);
        assert!(first.blind.segments_in_solids > first.seen.segments_in_solids);
    }
}

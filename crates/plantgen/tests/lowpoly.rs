//! Solid low-poly tiers (`plantgen::lowpoly`): each keeps its budget,
//! and every part it draws hangs from wood it draws or the ground.
//! `cargo test --release -p plantgen --test lowpoly`.

use plantgen::grow::{GrowthSettings, grow};
use plantgen::lowpoly::{self, SolidPlant, TIERS};
use plantgen::lsys::Limits;
use plantgen::math::Vec3;
use plantgen::package::DEFAULT_DAY;
use plantgen::quality::STANDARD;
use plantgen::spec::PlantSpec;
use plantgen::templates::Templates;

fn solid(id: &str) -> (f64, Vec<SolidPlant>) {
    let spec = PlantSpec::builtin(id).expect("a built-in species");
    let (program, params) = spec.program().expect("its program");
    let variant = spec.variant_list()[0];
    let age = spec.growth.keyframes.iter().copied().fold(0.0, f64::max);
    let growth = grow(
        &program,
        &params,
        &GrowthSettings {
            seed: variant.seed,
            dt: spec.growth.step,
            years: age,
            keyframes: vec![age],
            conditions: variant.conditions(),
            limits: Limits::default(),
            host: spec.host_geometry().expect("its host"),
        },
    )
    .expect("it grows");
    let graph = growth.keyframes.last().expect("a keyframe");
    let day = spec.appearance.has_seasons().then_some(DEFAULT_DAY);
    let (looks, sizes) = spec.appearance.looks_on(program.organs(), day);
    let bodies = spec.appearance.body_looks(program.bodies());
    let named: Vec<(&str, &_)> = program.bodies().zip(&bodies).collect();
    let templates = Templates::for_plant(&looks, &named).with_bark(spec.appearance.bark_params());
    let staged = plantgen::looks::staged(graph, &sizes, &looks);
    let drawn = staged.as_ref().unwrap_or(graph);
    let plants = TIERS
        .iter()
        .map(|tier| {
            lowpoly::build(
                drawn,
                &looks,
                &bodies,
                &spec.appearance,
                &templates,
                &STANDARD.lods,
                tier,
            )
        })
        .collect();
    (drawn.height, plants)
}

/// How far `p` lies from the nearest drawn path or ground point.
fn apart(plant: &SolidPlant, p: Vec3) -> f64 {
    let mut best = plant
        .grounds
        .iter()
        .map(|g| (*g - p).length())
        .fold(f64::INFINITY, f64::min);
    for path in &plant.paths {
        for pair in path.windows(2) {
            let (a, b) = (pair[0].0, pair[1].0);
            let ab = b - a;
            let t = if ab.dot(ab) > 0.0 {
                ((p - a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            best = best.min((a + ab * t - p).length());
        }
    }
    best
}

/// How far `p` lies outside the box round `points`.
fn boxed(points: &[Vec3], p: Vec3) -> f64 {
    if points.is_empty() {
        return f64::INFINITY;
    }
    let lo = points.iter().fold(points[0], |a, b| a.min(*b));
    let hi = points.iter().fold(points[0], |a, b| a.max(*b));
    let out = |v: f64, l: f64, h: f64| (l - v).max(v - h).max(0.0);
    let (x, y, z) = (
        out(p.x, lo.x, hi.x),
        out(p.y, lo.y, hi.y),
        out(p.z, lo.z, hi.z),
    );
    (x * x + y * y + z * z).sqrt()
}

const SPECIES: [&str; 8] = [
    "digitalis-purpurea",
    "epilobium-angustifolium",
    "deschampsia-cespitosa",
    "polystichum-munitum",
    "opuntia-engelmannii",
    "echinocereus-engelmannii",
    "agave-deserti",
    "acer-circinatum",
];

#[test]
fn tiers_keep_their_budgets() {
    for id in SPECIES {
        let (_, plants) = solid(id);
        for (tier, plant) in TIERS.iter().zip(plants) {
            let stats = &plant.stats;
            assert!(
                stats.triangles() > 0 && stats.triangles() <= stats.budget,
                "{id} {}: {stats:?}",
                tier.name
            );
            // Organs and wood are solid, opaque and untextured.
            assert!(plant.organs.colors.iter().all(|c| c[3] == 1.0));
            assert_eq!(plant.organs.indices.len() % 3, 0);
        }
    }
}

#[test]
fn nothing_floats() {
    for id in SPECIES {
        let (height, plants) = solid(id);
        for (tier, plant) in TIERS.iter().zip(plants) {
            // A part hangs within the drawn wood's straying and a stem's
            // girth of what bears it.
            let near = height * (tier.straying + 0.02);
            for (k, path) in plant.paths.iter().enumerate() {
                let others = SolidPlant {
                    paths: plant
                        .paths
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != k)
                        .map(|(_, p)| p.clone())
                        .collect(),
                    grounds: plant.grounds.clone(),
                    ..SolidPlant::default()
                };
                let d = apart(&others, path[0].0);
                assert!(
                    d <= near,
                    "{id} {}: stem {k} starts {d:.3} m from the plant",
                    tier.name
                );
            }
            for (k, piece) in plant.pieces.iter().enumerate() {
                assert!(
                    !piece.attach.is_empty(),
                    "{id} {}: piece {k} hangs from nothing",
                    tier.name
                );
                for &a in &piece.attach {
                    // On drawn wood, the ground, or another part it hangs
                    // from (a flower mass on its crown's).
                    let d = plant
                        .pieces
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != k)
                        .map(|(_, other)| boxed(&other.points, a))
                        .fold(apart(&plant, a), f64::min);
                    assert!(
                        d <= near,
                        "{id} {}: piece {k} hangs {d:.3} m from the plant",
                        tier.name
                    );
                }
                // And reaches where it hangs: the attach point is within
                // its box.
                let (mut lo, mut hi) = (
                    Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
                    Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
                );
                for p in &piece.points {
                    lo = lo.min(*p);
                    hi = hi.max(*p);
                }
                // A thin mass drawn flat may lose its thickness.
                let slack = near;
                for &a in &piece.attach {
                    let inside = [(a.x, lo.x, hi.x), (a.y, lo.y, hi.y), (a.z, lo.z, hi.z)]
                        .iter()
                        .all(|&(v, l, h)| v >= l - slack && v <= h + slack);
                    assert!(inside, "{id} {}: piece {k} does not reach {a:?}", tier.name);
                }
            }
        }
    }
}

#[test]
fn flowers_are_drawn() {
    for id in ["digitalis-purpurea", "epilobium-angustifolium"] {
        let (_, plants) = solid(id);
        for (tier, plant) in TIERS.iter().zip(plants) {
            assert!(
                plant.stats.masses > 0,
                "{id} {}: {:?}",
                tier.name,
                plant.stats
            );
        }
    }
}

//! `plantc scene`: grown variants laid out as instances and drawn together
//! (stands plan, step 1). Each species, environment and seed grows once,
//! to the oldest age its instances name; every instance is then a copy of
//! that variant's mesh at its age, turned, scaled and moved to its place,
//! as a host's renderer draws a forest of instances. Nothing grows
//! together here: the scene shows how a layout of pre-grown plants looks,
//! and how much of the ground its crowns cover from above.

use std::collections::BTreeMap;
use std::fs;

use plantgen::development::{self, Candidate};
use plantgen::math::Vec3;
use plantgen::mesh::{self, PlantMesh};
use plantgen::niche::Layer;
use plantgen::preview::{self, PreviewOptions, View};
use plantgen::raster;
use plantgen::spec::{Environment, PlantSpec};
use plantgen::stand_tile;
use plantgen::templates::Templates;

use super::{
    Failure, Options, bodies_of, describe, drawn, grow_variant, load_spec, looks_of, stand_number,
    stand_pair, write_line,
};

/// One instance of a scene document.
struct Instance {
    variant: usize,
    age: f64,
    at_m: [f64; 2],
    yaw_deg: f64,
    scale: f64,
    /// Metres east and north its crown leans per metre of height.
    lean: [f64; 2],
}

/// `PlantMesh` `plant` turned `yaw_deg` about its foot (anticlockwise
/// seen from above), scaled by `scale`, leaned by `lean` (metres east and
/// north per metre up, a shear that keeps its foot in place) and moved by
/// `by`.
fn placed(plant: &PlantMesh, yaw_deg: f64, scale: f64, lean: [f64; 2], by: Vec3) -> PlantMesh {
    #[allow(clippy::cast_possible_truncation)]
    let (sin, cos, scale_f) = {
        let (sin, cos) = yaw_deg.to_radians().sin_cos();
        (sin as f32, cos as f32, scale as f32)
    };
    let by = by.to_f32();
    #[allow(clippy::cast_possible_truncation)]
    let (east, north) = (lean[0] as f32, lean[1] as f32);
    let turn = |v: [f32; 3]| [v[0] * cos + v[2] * sin, v[1], -v[0] * sin + v[2] * cos];
    let point = |v: [f32; 3]| {
        let t = turn(v);
        let up = t[1] * scale_f;
        [
            t[0] * scale_f + east * up + by[0],
            up + by[1],
            t[2] * scale_f - north * up + by[2],
        ]
    };
    let mut out = plant.clone();
    out.wood.positions.iter_mut().for_each(|p| *p = point(*p));
    out.wood.normals.iter_mut().for_each(|n| *n = turn(*n));
    // Bark's alpha is minus the stem's radius, which scales with it.
    for colour in &mut out.wood.colors {
        if colour[3] < 0.0 {
            colour[3] *= scale_f;
        }
    }
    for card in &mut out.cards {
        card.base = point(card.base);
        card.heading = turn(card.heading);
        card.left = turn(card.left);
        card.length *= scale_f;
        card.width *= scale_f;
    }
    for tuft in &mut out.tufts {
        tuft.position = point(tuft.position);
    }
    out.hair_tubes.clear();
    out
}

/// `host` with `guest` appended, both already drawing with one list of
/// looks.
fn append(host: &mut PlantMesh, guest: PlantMesh) {
    let first = u32::try_from(host.wood.positions.len()).unwrap_or(u32::MAX);
    let wood = guest.wood;
    host.wood.positions.extend(wood.positions);
    host.wood.normals.extend(wood.normals);
    host.wood.uvs.extend(wood.uvs);
    host.wood.colors.extend(wood.colors);
    host.wood.births.extend(wood.births);
    host.wood.sheds.extend(wood.sheds);
    host.wood.levels.extend(wood.levels);
    host.wood
        .indices
        .extend(wood.indices.iter().map(|index| index + first));
    host.cards.extend(guest.cards);
    host.tufts.extend(guest.tufts);
}

/// The level of detail a scene's overview views (side, 45 degrees, from
/// the top, and the cover count) draw every plant at. Framing a stand
/// tile tens of metres across in a thousand or two pixels, a pixel spans
/// 3-6 cm, and level 1's 10 cm between a stem's ring vertices and 4 mm
/// thinnest twig (standard quality) lose nothing it can show; level 0
/// drew a 250-year Douglas-fir's wood in seven times the triangles.
const OVERVIEW_LEVEL: usize = 1;

/// The camera of a close-up from the side, for choosing each instance's
/// level of detail by what that close-up's pixels can resolve.
struct CloseUp {
    eye: Vec3,
    /// Level, unit, along the view and to its right.
    forward: [f64; 2],
    right: [f64; 2],
    /// Tangent of half the picture's width as seen from the eye.
    half_width: f64,
    /// Metres one pixel spans per metre from the eye.
    pixel_per_metre: f64,
}

impl CloseUp {
    fn new(focus: Vec3, span: f64, width: usize, height: usize) -> Self {
        let (eye, half_height) = preview::close_up_eye(View::Side, focus, span);
        let (x, z) = (focus.x - eye.x, focus.z - eye.z);
        let level = x.hypot(z).max(1e-9);
        #[allow(clippy::cast_precision_loss)]
        let (width, height) = (width as f64, height as f64);
        Self {
            eye,
            forward: [x / level, z / level],
            right: [-z / level, x / level],
            half_width: half_height * width / height,
            pixel_per_metre: 2.0 * half_height / height,
        }
    }

    /// The level of detail of an instance standing at `at_m` (east, north),
    /// `height` tall with a crown `crown` metres in radius: the coarsest of
    /// `levels` whose ring vertices and merged foliage cells stand at most
    /// three pixels apart, and whose thinnest twig is at most a pixel
    /// across, where the instance comes nearest the eye. One wholly outside
    /// the picture only casts shade into it and takes level 2.
    fn level(&self, at_m: [f64; 2], height: f64, crown: f64, levels: &[mesh::LodSpec; 4]) -> usize {
        let (x, z) = (at_m[0] - self.eye.x, -at_m[1] - self.eye.z);
        let along = x * self.forward[0] + z * self.forward[1];
        let across = (x * self.right[0] + z * self.right[1]).abs();
        // A margin for limbs past the crown's measured radius.
        let reach = crown.max(0.0) * 1.1 + 0.2;
        let beside = (across - self.half_width * along) / self.half_width.hypot(1.0);
        if along + reach <= 0.0 || beside > reach {
            return 2;
        }
        // Depth of its nearest part; nothing nearer than the macro lens.
        let depth = (along - reach).max(0.15);
        let pixel = depth * self.pixel_per_metre;
        (0..levels.len())
            .rev()
            .find(|&level| {
                let spec = levels[level].for_height(height);
                spec.ring_edge.max(spec.cluster) <= 3.0 * pixel && 2.0 * spec.min_radius <= pixel
            })
            .unwrap_or(0)
    }
}

/// The scene one view draws: every instance placed, each at the level of
/// detail `level` gives it, its variant's mesh at that level built once by
/// `build` and dropped once the scene holds its copies.
fn scene_of(
    instances: &[Instance],
    build: &dyn Fn(usize, f64, usize) -> Option<PlantMesh>,
    level: &dyn Fn(&Instance) -> usize,
) -> PlantMesh {
    let mut meshes: BTreeMap<(usize, u64, usize), PlantMesh> = BTreeMap::new();
    let key = |instance: &Instance| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let age = (instance.age * 1000.0).round() as u64;
        (instance.variant, age, level(instance))
    };
    for instance in instances {
        let key = key(instance);
        if !meshes.contains_key(&key)
            && let Some(plant) = build(instance.variant, instance.age, key.2)
        {
            meshes.insert(key, plant);
        }
    }
    let mut scene = PlantMesh::default();
    for instance in instances {
        let Some(plant) = meshes.get(&key(instance)) else {
            continue;
        };
        let [east, north] = instance.at_m;
        append(
            &mut scene,
            placed(
                plant,
                instance.yaw_deg,
                instance.scale,
                instance.lean,
                Vec3::new(east, 0.0, -north),
            ),
        );
    }
    scene
}

/// Lay the stand of `members` out on a tile by `laid`'s rules
/// (`plantgen::stand_tile`), drawn `repeat` tiles each way about the
/// middle, and print its stems, basal area, leaf area and cover.
#[allow(clippy::type_complexity)]
fn lay_out_stand(
    laid: &serde_json::Value,
    developed: bool,
    cover_default: Option<[f64; 4]>,
    members: &[(usize, f64, Layer, f64, f64)],
    names: &[String],
    sizes_at: &BTreeMap<(usize, u64), [f64; 5]>,
    instances: &mut Vec<Instance>,
) -> Result<(), Failure> {
    let defaults = stand_tile::Rules::default();
    let mut cover = cover_default.unwrap_or(defaults.cover);
    if let Some(given) = laid["cover"].as_array() {
        for (slot, value) in cover.iter_mut().zip(given) {
            *slot = value.as_f64().ok_or("stand cover must be numbers")?;
        }
    }
    // A developed stand's members are each cohort's grown dominant, so its
    // copies are that size or smaller: Wind River's tallest Douglas-fir
    // stands 1.24 times its Douglas-firs' mean height (Shaw et al. 2004).
    let mut scale = if developed {
        [0.7, 1.0]
    } else {
        defaults.scale
    };
    if let Some(given) = laid["scale"].as_array() {
        for (slot, value) in scale.iter_mut().zip(given) {
            *slot = value.as_f64().ok_or("stand scale must be numbers")?;
        }
    }
    let rules = stand_tile::Rules {
        tile_m: laid["tile_m"].as_f64().unwrap_or(defaults.tile_m),
        cover,
        overlap: laid["overlap"].as_f64().unwrap_or(defaults.overlap),
        scale,
        ..defaults
    };
    let seed = laid["seed"].as_u64().unwrap_or(1);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let key = |variant: usize, age: f64| (variant, (age * 1000.0).round() as u64);
    let tile_members: Vec<stand_tile::Member> = members
        .iter()
        .enumerate()
        .map(|(id, &(variant, age, layer, share, light))| {
            let [height, base, radius, ..] = sizes_at
                .get(&key(variant, age))
                .copied()
                .unwrap_or([0.0; 5]);
            stand_tile::Member {
                id: u32::try_from(id).unwrap_or(u32::MAX),
                layer,
                share,
                height_m: height,
                crown_base_m: base,
                crown_radius_m: radius,
                light_needed: light,
            }
        })
        .collect();
    let started = std::time::Instant::now();
    let placed = stand_tile::lay_out(&tile_members, &rules, seed);
    let seconds = started.elapsed().as_secs_f64();
    let area_ha = rules.tile_m * rules.tile_m / 10_000.0;
    let (mut basal, mut leaf) = (0.0, 0.0);
    let mut stems = [0_usize; 4];
    // By species, trees 5 cm or more through at breast height (as plots
    // count them): stems, summed height, basal area, tallest; and every
    // stem.
    let mut by_species: BTreeMap<&str, (usize, f64, f64, f64, usize)> = BTreeMap::new();
    for plant in &placed {
        let (variant, age, layer, ..) = members[plant.member];
        let [height, _, _, dbh, leaf_area] = sizes_at
            .get(&key(variant, age))
            .copied()
            .unwrap_or([0.0; 5]);
        let area = std::f64::consts::PI * (0.5 * dbh * plant.scale).powi(2);
        basal += area;
        leaf += leaf_area * plant.scale * plant.scale;
        if let Some(slot) = Layer::ALL.iter().position(|l| *l == layer) {
            stems[slot] += 1;
        }
        let entry = by_species
            .entry(names[variant].as_str())
            .or_insert((0, 0.0, 0.0, 0.0, 0));
        entry.4 += 1;
        if dbh * plant.scale >= 0.05 {
            entry.0 += 1;
            entry.1 += height * plant.scale;
            entry.2 += area;
            entry.3 = entry.3.max(height * plant.scale);
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let per_ha = |n: usize| n as f64 / area_ha;
    out!(
        "stand tile {} m laid out in {:.3} s: stems/ha canopy {:.0}, subcanopy {:.0}, shrub {:.0}, herb {:.0}; \
         basal area {:.1} m²/ha; leaf area index {:.2}; cover from above {:.1}%",
        rules.tile_m,
        seconds,
        per_ha(stems[0]),
        per_ha(stems[1]),
        per_ha(stems[2]),
        per_ha(stems[3]),
        basal / area_ha,
        leaf / (area_ha * 10_000.0),
        100.0 * stand_tile::cover(&tile_members, &placed, rules.tile_m)
    );
    for (name, (count, heights, area, tallest, all)) in &by_species {
        #[allow(clippy::cast_precision_loss)]
        let mean = heights / (*count).max(1) as f64;
        out!(
            "  {name}: {:.0} stems/ha of 5 cm or more (mean height {mean:.1} m, tallest {tallest:.1} m, basal area {:.1} m²/ha), {:.0} stems/ha in all",
            per_ha(*count),
            area / area_ha,
            per_ha(*all)
        );
    }
    let repeat = laid["repeat"].as_u64().unwrap_or(1).max(1);
    // Herbs are drawn only within `herb_radius_m` of the middle, when
    // given: a hectare's ferns are millions of cards, and only a close view
    // shows them one by one. They are counted above all the same.
    let herb_radius = laid["herb_radius_m"].as_f64().unwrap_or(f64::INFINITY);
    #[allow(clippy::cast_precision_loss)]
    let offset = -0.5 * rules.tile_m * repeat as f64;
    for copy_x in 0..repeat {
        for copy_y in 0..repeat {
            for plant in &placed {
                let (variant, age, layer, ..) = members[plant.member];
                #[allow(clippy::cast_precision_loss)]
                let (east, north) = (
                    plant.at_m[0] + offset + rules.tile_m * copy_x as f64,
                    plant.at_m[1] + offset + rules.tile_m * copy_y as f64,
                );
                if layer == Layer::Herb && east.hypot(north) > herb_radius {
                    continue;
                }
                // Lean it so its crown's middle stands where the layout put
                // its crown.
                let [height, base, ..] = sizes_at
                    .get(&key(variant, age))
                    .copied()
                    .unwrap_or([0.0; 5]);
                let middle = 0.5 * (base + height) * plant.scale;
                let shift = plant.crown_offset(rules.tile_m);
                let lean = if middle > 0.5 {
                    [shift[0] / middle, shift[1] / middle]
                } else {
                    [0.0, 0.0]
                };
                #[allow(clippy::cast_precision_loss)]
                instances.push(Instance {
                    lean,
                    variant,
                    age,
                    at_m: [east, north],
                    yaw_deg: plant.yaw_deg,
                    scale: plant.scale,
                });
            }
        }
    }
    Ok(())
}

fn view_named(name: &str) -> Result<View, Failure> {
    View::from_name(name).ok_or_else(|| format!("unknown view `{name}`").into())
}

/// `plantc scene`: instances of grown variants drawn together.
#[allow(clippy::too_many_lines)]
pub(super) fn scene_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(
        args,
        &[
            "out", "views", "size", "day", "quality", "focus", "span", "cover",
        ],
    )?;
    let given = options.one_positional("a scene document")?;
    let text =
        fs::read_to_string(given).map_err(|error| format!("cannot read {given}: {error}"))?;
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("invalid scene: {error}"))?;
    let empty = Vec::new();
    let list = document["instances"].as_array().unwrap_or(&empty);
    let develop = &document["develop"];
    let laid = if develop.is_object() {
        develop
    } else {
        &document["lay_out"]
    };
    let members_json = document["lay_out"]["members"].as_array().unwrap_or(&empty);
    // Variants by (species, environment, seed), each with its ages.
    let mut keys: BTreeMap<(String, String, u64), usize> = BTreeMap::new();
    let mut variants: Vec<(PlantSpec, Environment, u64, Vec<f64>)> = Vec::new();
    let mut instances = Vec::new();
    // Members of a laid-out stand: (variant, age, layer, share, light).
    let mut members = Vec::new();
    for (item, is_member) in list
        .iter()
        .map(|item| (item, false))
        .chain(members_json.iter().map(|item| (item, true)))
    {
        let id = item["species"]
            .as_str()
            .ok_or("each instance names its species")?;
        let env_name = item["env"].as_str().unwrap_or("open");
        let environment = Environment::from_name(env_name)
            .ok_or_else(|| format!("unknown environment `{env_name}`"))?;
        let age = stand_number(&item["age"], "age")?;
        let seed = item["seed"].as_u64();
        let key_seed = seed.unwrap_or(0);
        let key = (id.to_string(), env_name.to_string(), key_seed);
        let index = if let Some(index) = keys.get(&key) {
            *index
        } else {
            let spec = load_spec(id)?;
            let seed = seed.unwrap_or(spec.variants.seeds[0]);
            variants.push((spec, environment, seed, Vec::new()));
            keys.insert(key, variants.len() - 1);
            variants.len() - 1
        };
        let ages = &mut variants[index].3;
        if !ages.iter().any(|known| (known - age).abs() < 1e-9) {
            ages.push(age);
        }
        if is_member {
            let layer_name = item["layer"].as_str().unwrap_or("canopy");
            let layer = Layer::ALL
                .into_iter()
                .find(|layer| layer.name() == layer_name)
                .ok_or_else(|| format!("unknown layer `{layer_name}`"))?;
            members.push((
                index,
                age,
                layer,
                item["share"].as_f64().unwrap_or(1.0),
                item["light"].as_f64().unwrap_or(0.0),
            ));
            continue;
        }
        let at_m = stand_pair(&item["at"], "at")?;
        let yaw_deg = item["yaw"].as_f64().unwrap_or(0.0);
        let scale = item["scale"].as_f64().unwrap_or(1.0);
        instances.push(Instance {
            variant: index,
            age,
            at_m,
            yaw_deg,
            scale,
            lean: [0.0, 0.0],
        });
    }
    // A developing stand: its cohorts from its species' niches and
    // lifespans at its age (`plantgen::development`), each a member whose
    // layer its grown height decides.
    let mut candidates: Vec<Candidate> = Vec::new();
    // (member, candidate) of each developed member.
    let mut developed: Vec<(usize, usize)> = Vec::new();
    let mut cover_default = None;
    if let Some(pool) = develop["species"].as_array() {
        let stand_age = stand_number(&develop["stand_age"], "stand_age")?;
        // The site's factors, when given: each species' share follows how
        // well they suit it.
        let given = &develop["site"];
        let site = if given.is_object() {
            let number = |key: &str, default: f64| given[key].as_f64().unwrap_or(default);
            Some(plantgen::niche::SiteFactors {
                moisture: number("moisture", 0.5),
                heat_load: number("heat_load", 1.0),
                elevation_m: number("elevation_m", 0.0),
                slope_deg: number("slope_deg", 0.0),
            })
        } else {
            None
        };
        let mut entries = Vec::new();
        for item in pool {
            let id = item["species"]
                .as_str()
                .ok_or("each species of a developing stand names its species")?;
            let env_name = item["env"].as_str().unwrap_or("interior");
            let environment = Environment::from_name(env_name)
                .ok_or_else(|| format!("unknown environment `{env_name}`"))?;
            let spec = load_spec(id)?;
            let niche = plantgen::niche::Niche::builtin(id).map_err(|error| error.to_string())?;
            let mut ages = spec.growth.keyframes.clone();
            ages.extend(spec.old_ages());
            ages.sort_by(f64::total_cmp);
            ages.dedup();
            let suitability = site.map_or(1.0, |site| niche.site_response(site));
            candidates.push(Candidate {
                shade_tolerance: niche.shade_tolerance,
                layers: Layer::ALL.map(|layer| niche.layers.get(&layer).copied().unwrap_or(0.0)),
                lifespan: spec.growth.lifespan.unwrap_or(spec.growth.years),
                ages,
                suitability,
            });
            out!("  {id}: suits the site {suitability:.2}");
            let seeds: Vec<u64> = match item["seeds"].as_array() {
                Some(seeds) => seeds.iter().filter_map(serde_json::Value::as_u64).collect(),
                None => vec![spec.variants.seeds[0]],
            };
            entries.push((
                id.to_string(),
                env_name.to_string(),
                environment,
                seeds,
                development::light_needed(&niche),
            ));
        }
        let stand = development::develop(&candidates, stand_age);
        out!(
            "stand of {stand_age} years: {} (pioneers' lifespan {} years), cover canopy {:.2}, subcanopy {:.2}, shrub {:.2}, herb {:.2}",
            stand.stage.name(),
            stand.lifespan,
            stand.cover[0],
            stand.cover[1],
            stand.cover[2],
            stand.cover[3]
        );
        cover_default = Some(stand.cover);
        for cohort in &stand.cohorts {
            let (id, env_name, environment, seeds, light) = &entries[cohort.candidate];
            out!(
                "  cohort {id} {} years, weight {:.3}{}",
                cohort.age,
                cohort.weight,
                if cohort.pioneer { ", pioneer" } else { "" }
            );
            #[allow(clippy::cast_precision_loss)]
            let share = cohort.weight / seeds.len().max(1) as f64;
            for seed in seeds {
                let key = (id.clone(), env_name.clone(), *seed);
                let index = if let Some(index) = keys.get(&key) {
                    *index
                } else {
                    variants.push((load_spec(id)?, *environment, *seed, Vec::new()));
                    keys.insert(key, variants.len() - 1);
                    variants.len() - 1
                };
                let ages = &mut variants[index].3;
                if !ages.iter().any(|known| (known - cohort.age).abs() < 1e-9) {
                    ages.push(cohort.age);
                }
                members.push((index, cohort.age, Layer::Canopy, share, *light));
                developed.push((members.len() - 1, cohort.candidate));
            }
        }
    }
    if instances.is_empty() && members.is_empty() {
        return Err("a scene needs instances or a stand to lay out".into());
    }
    let day = options.day()?;
    let quality = options.quality()?;
    // Grow each variant once, every variant on its own thread.
    let started = std::time::Instant::now();
    for variant in &mut variants {
        variant.3.sort_by(f64::total_cmp);
    }
    let growths: Vec<Result<plantgen::grow::Growth, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = variants
            .iter()
            .map(|(spec, environment, seed, ages)| {
                scope.spawn(move || {
                    let years = ages.last().copied().unwrap_or(1.0);
                    grow_variant(spec, None, *environment, *seed, ages.clone(), years)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    });
    let growths = growths.into_iter().collect::<Result<Vec<_>, _>>()?;
    out!(
        "{} variants grown in {:.1} s for {} instances",
        variants.len(),
        started.elapsed().as_secs_f64(),
        instances.len()
    );
    // Each variant's looks, bodies and the shift of its looks in the
    // scene's one list of looks; its meshes are built for each view at the
    // level of detail that view needs (`scene_of`).
    let mut all_looks = Vec::new();
    let mut bark = None;
    #[allow(clippy::type_complexity)]
    let mut drawing: Vec<(
        Vec<plantgen::looks::Look>,
        Vec<f64>,
        Vec<plantgen::body::BodyLook>,
        u8,
    )> = Vec::new();
    // Each variant's size at each age: height, crown base, crown radius,
    // breast-height diameter and leaf area.
    let mut sizes_at: BTreeMap<(usize, u64), [f64; 5]> = BTreeMap::new();
    for (index, ((spec, environment, seed, ages), growth)) in
        variants.iter().zip(&growths).enumerate()
    {
        let (looks, sizes) = looks_of(spec, growth, day);
        let bodies = bodies_of(spec, growth);
        let shift = u8::try_from(all_looks.len()).map_err(|_| "too many organ types")?;
        for (keyframe, age) in ages.iter().enumerate() {
            let Some(graph) = growth.keyframes.get(keyframe) else {
                continue;
            };
            out!(
                "  {} {} seed {seed}: {}",
                spec.id,
                environment.name(),
                describe(graph, &growth.organ_types)
            );
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let key = (index, (age * 1000.0).round() as u64);
            sizes_at.insert(
                key,
                [
                    graph.height,
                    graph.crown_base().unwrap_or(0.0),
                    graph.crown_radius(),
                    graph.diameter_at_breast_height().unwrap_or(0.0),
                    graph.leaf_area(&growth.organ_types),
                ],
            );
        }
        all_looks.extend(looks.iter().cloned());
        drawing.push((looks, sizes, bodies, shift));
        if bark.is_none() {
            bark = spec.appearance.bark_params();
        }
    }
    if all_looks.len() > 255 {
        return Err("too many organ types to draw at once; draw fewer variants".into());
    }
    if !developed.is_empty() {
        // Each developed member's layer from its height among the stand's
        // tallest.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let height = |member: &(usize, f64, Layer, f64, f64)| {
            sizes_at
                .get(&(member.0, (member.1 * 1000.0).round() as u64))
                .map_or(0.0, |sizes| sizes[0])
        };
        let top = developed
            .iter()
            .filter(|(_, candidate)| candidates[*candidate].tree())
            .map(|(member, _)| height(&members[*member]))
            .fold(0.0_f64, f64::max);
        for (member, candidate) in &developed {
            let layer =
                development::layer_of(&candidates[*candidate], height(&members[*member]), top);
            members[*member].2 = layer;
        }
    }
    if !members.is_empty() {
        let names: Vec<String> = variants.iter().map(|(spec, ..)| spec.id.clone()).collect();
        lay_out_stand(
            laid,
            !developed.is_empty(),
            cover_default,
            &members,
            &names,
            &sizes_at,
            &mut instances,
        )?;
    }
    if !options.flags.contains_key("out") && !options.flags.contains_key("cover") {
        return Ok(());
    }
    // One variant's mesh at one age and level of detail.
    let build = |index: usize, age: f64, level: usize| -> Option<PlantMesh> {
        let (spec, _, seed, ages) = &variants[index];
        let keyframe = ages.iter().position(|known| (known - age).abs() < 1e-9)?;
        let graph = growths[index].keyframes.get(keyframe)?;
        let (looks, sizes, bodies, shift) = &drawing[index];
        let graph = &*drawn(graph, sizes, looks);
        let mut plant = mesh::build(
            graph,
            looks,
            bodies,
            &spec.appearance,
            &quality.lods[level].for_height(graph.height),
            level,
        );
        plant.vary_by_plant(looks, *seed);
        for card in &mut plant.cards {
            card.template = card.template.saturating_add(*shift);
        }
        Some(plant)
    };
    let templates = Templates::for_plant(&all_looks, &[]).with_bark(bark);
    let reach = instances
        .iter()
        .map(|i| i.at_m[0].abs().max(i.at_m[1].abs()))
        .fold(1.0, f64::max);
    let pixels: usize = options.number("size")?.unwrap_or(1200);
    // The overview views share one scene, built when one first needs it.
    let mut overview: Option<PlantMesh> = None;
    // `--cover HALF`: the share of the square HALF metres round the middle
    // that crowns cover seen straight down.
    if let Some(half) = options.number::<f64>("cover")? {
        let scene =
            overview.get_or_insert_with(|| scene_of(&instances, &build, &|_| OVERVIEW_LEVEL));
        let image = preview::render_on_site(
            scene,
            &[],
            &templates,
            &PreviewOptions {
                width: pixels,
                height: pixels,
                view: View::Top,
                supersample: 1,
                figure: false,
                frame_height: None,
                frame_reach: Some(reach),
                focus: None,
                fit_plant: false,
            },
        );
        // The top view frames 1.1 times the reach each way from the middle.
        #[allow(clippy::cast_precision_loss)]
        let metres_per_pixel = 2.2 * reach / pixels as f64;
        let (mut covered, mut all) = (0.0_f64, 0.0_f64);
        for row in 0..image.height {
            for column in 0..image.width {
                #[allow(clippy::cast_precision_loss)]
                let (x, z) = (
                    (column as f64 + 0.5) * metres_per_pixel - 1.1 * reach,
                    (row as f64 + 0.5) * metres_per_pixel - 1.1 * reach,
                );
                if x.abs() <= half && z.abs() <= half {
                    all += 1.0;
                    covered += f64::from(image.color[row * image.width + column][3]);
                }
            }
        }
        out!(
            "canopy cover from above over the middle {} m square: {:.1}%",
            2.0 * half,
            100.0 * covered / all.max(1.0)
        );
    }
    let Some(prefix) = options.flags.get("out") else {
        return Ok(());
    };
    let views = options
        .flags
        .get("views")
        .map_or("side,oblique", String::as_str);
    let focus = match options.flags.get("focus") {
        Some(text) => {
            let point: Vec<f64> = text
                .split(',')
                .map(|part| part.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| format!("`--focus` expects X,Y,Z in metres, found `{text}`"))?;
            let [x, y, z] = point[..] else {
                return Err(format!("`--focus` expects X,Y,Z in metres, found `{text}`").into());
            };
            let span: f64 = options.number("span")?.unwrap_or(4.0);
            Some((Vec3::new(x, y, -z), span))
        }
        None => None,
    };
    // The overview views first: they share one scene, and a close-up's
    // scene is built only once that one is dropped, so the two are never
    // held at once.
    let (closes, overviews): (Vec<&str>, Vec<&str>) =
        views.split(',').partition(|name| *name == "close");
    for name in overviews.into_iter().chain(closes) {
        let close = name == "close";
        let view = if close { View::Side } else { view_named(name)? };
        let shot = PreviewOptions {
            width: pixels,
            height: pixels * 3 / 4,
            view,
            supersample: 2,
            figure: !close && view != View::Top,
            frame_height: None,
            frame_reach: Some(reach * 1.05),
            focus: if close { focus } else { None },
            fit_plant: false,
        };
        let close_scene;
        let scene = match (close, focus) {
            (true, None) => return Err("the close view needs --focus X,Y,Z".into()),
            (true, Some((point, span))) => {
                let camera = CloseUp::new(point, span, shot.width, shot.height);
                // No overview view comes after a close-up.
                overview = None;
                close_scene = scene_of(&instances, &build, &|instance| {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let key = (instance.variant, (instance.age * 1000.0).round() as u64);
                    let [height, _, crown, ..] = sizes_at.get(&key).copied().unwrap_or_default();
                    camera.level(
                        instance.at_m,
                        height * instance.scale,
                        crown * instance.scale,
                        &quality.lods,
                    )
                });
                &close_scene
            }
            (false, _) => {
                &*overview.get_or_insert_with(|| scene_of(&instances, &build, &|_| OVERVIEW_LEVEL))
            }
        };
        let image = preview::render_with(scene, &[], &templates, &shot);
        let png = raster::encode_png(
            image.width,
            image.height,
            &image.to_srgb8(Some(preview::SKY)),
        )
        .map_err(|error| format!("cannot encode PNG: {error}"))?;
        let path = format!("{prefix}-{name}.png");
        fs::write(&path, png).map_err(|error| format!("cannot write {path}: {error}"))?;
        out!("wrote {path}: {} triangles", scene.triangle_count());
    }
    Ok(())
}

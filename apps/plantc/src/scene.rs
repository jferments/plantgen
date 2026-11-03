//! `plantc scene`: grown variants laid out as instances and drawn together
//! (stands plan, step 1). Each species, environment and seed grows once,
//! to the oldest age its instances name; every instance is then a copy of
//! that variant's mesh at its age, turned, scaled and moved to its place,
//! as a host's renderer draws a forest of instances. Nothing grows
//! together here: the scene shows how a layout of pre-grown plants looks,
//! and how much of the ground its crowns cover from above.

use std::collections::BTreeMap;
use std::fs;

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
}

/// `PlantMesh` `plant` turned `yaw_deg` about its foot (anticlockwise
/// seen from above), scaled by `scale` and moved by `by`.
fn placed(plant: &PlantMesh, yaw_deg: f64, scale: f64, by: Vec3) -> PlantMesh {
    #[allow(clippy::cast_possible_truncation)]
    let (sin, cos, scale_f) = {
        let (sin, cos) = yaw_deg.to_radians().sin_cos();
        (sin as f32, cos as f32, scale as f32)
    };
    let by = by.to_f32();
    let turn = |v: [f32; 3]| [v[0] * cos + v[2] * sin, v[1], -v[0] * sin + v[2] * cos];
    let point = |v: [f32; 3]| {
        let t = turn(v);
        [
            t[0] * scale_f + by[0],
            t[1] * scale_f + by[1],
            t[2] * scale_f + by[2],
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

/// Lay the stand of `members` out on a tile by `laid`'s rules
/// (`plantgen::stand_tile`), drawn `repeat` tiles each way about the
/// middle, and print its stems, basal area, leaf area and cover.
#[allow(clippy::type_complexity)]
fn lay_out_stand(
    laid: &serde_json::Value,
    members: &[(usize, f64, Layer, f64, f64)],
    sizes_at: &BTreeMap<(usize, u64), [f64; 5]>,
    instances: &mut Vec<Instance>,
) -> Result<(), Failure> {
    let defaults = stand_tile::Rules::default();
    let mut cover = defaults.cover;
    if let Some(given) = laid["cover"].as_array() {
        for (slot, value) in cover.iter_mut().zip(given) {
            *slot = value.as_f64().ok_or("stand cover must be numbers")?;
        }
    }
    let rules = stand_tile::Rules {
        tile_m: laid["tile_m"].as_f64().unwrap_or(defaults.tile_m),
        cover,
        overlap: laid["overlap"].as_f64().unwrap_or(defaults.overlap),
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
    for plant in &placed {
        let (variant, age, layer, ..) = members[plant.member];
        let [_, _, _, dbh, leaf_area] = sizes_at
            .get(&key(variant, age))
            .copied()
            .unwrap_or([0.0; 5]);
        basal += std::f64::consts::PI * (0.5 * dbh * plant.scale).powi(2);
        leaf += leaf_area * plant.scale * plant.scale;
        if let Some(slot) = Layer::ALL.iter().position(|l| *l == layer) {
            stems[slot] += 1;
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
    let repeat = laid["repeat"].as_u64().unwrap_or(1).max(1);
    #[allow(clippy::cast_precision_loss)]
    let offset = -0.5 * rules.tile_m * repeat as f64;
    for copy_x in 0..repeat {
        for copy_y in 0..repeat {
            for plant in &placed {
                let (variant, age, ..) = members[plant.member];
                #[allow(clippy::cast_precision_loss)]
                instances.push(Instance {
                    variant,
                    age,
                    at_m: [
                        plant.at_m[0] + offset + rules.tile_m * copy_x as f64,
                        plant.at_m[1] + offset + rules.tile_m * copy_y as f64,
                    ],
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
    let laid = &document["lay_out"];
    let members_json = laid["members"].as_array().unwrap_or(&empty);
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
        });
    }
    if instances.is_empty() && members.is_empty() {
        return Err("a scene needs instances or a stand to lay out".into());
    }
    let day = options.day()?;
    let quality = options.quality()?;
    let lod = &quality.lods[0];
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
    // One mesh per variant and age, its cards drawing from one list of
    // looks for the whole scene.
    let mut all_looks = Vec::new();
    let mut bark = None;
    let mut meshes: BTreeMap<(usize, u64), PlantMesh> = BTreeMap::new();
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
            let graph = &*drawn(graph, &sizes, &looks);
            let mut plant = mesh::build(
                graph,
                &looks,
                &bodies,
                &spec.appearance,
                &lod.for_height(graph.height),
                0,
            );
            plant.vary_by_plant(&looks, *seed);
            for card in &mut plant.cards {
                card.template = card.template.saturating_add(shift);
            }
            out!(
                "  {} {} seed {seed}: {}",
                spec.id,
                environment.name(),
                describe(graph, &growth.organ_types)
            );
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let key = (index, (age * 1000.0).round() as u64);
            meshes.insert(key, plant);
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
        all_looks.extend(looks);
        if bark.is_none() {
            bark = spec.appearance.bark_params();
        }
    }
    if all_looks.len() > 255 {
        return Err("too many organ types to draw at once; draw fewer variants".into());
    }
    if !members.is_empty() {
        lay_out_stand(laid, &members, &sizes_at, &mut instances)?;
    }
    let mut scene = PlantMesh::default();
    for instance in &instances {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let key = (instance.variant, (instance.age * 1000.0).round() as u64);
        let Some(plant) = meshes.get(&key) else {
            continue;
        };
        let [east, north] = instance.at_m;
        append(
            &mut scene,
            placed(
                plant,
                instance.yaw_deg,
                instance.scale,
                Vec3::new(east, 0.0, -north),
            ),
        );
    }
    let templates = Templates::for_plant(&all_looks, &[]).with_bark(bark);
    let reach = instances
        .iter()
        .map(|i| i.at_m[0].abs().max(i.at_m[1].abs()))
        .fold(1.0, f64::max);
    let pixels: usize = options.number("size")?.unwrap_or(1200);
    // `--cover HALF`: the share of the square HALF metres round the middle
    // that crowns cover seen straight down.
    if let Some(half) = options.number::<f64>("cover")? {
        let image = preview::render_on_site(
            &scene,
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
    for name in views.split(',') {
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
        if close && focus.is_none() {
            return Err("the close view needs --focus X,Y,Z".into());
        }
        let image = preview::render_with(&scene, &[], &templates, &shot);
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

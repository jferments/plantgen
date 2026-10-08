//! Every level of detail keeps the nearest one's widths (plant leftovers
//! L10 and L12): each level and the impostor rendered with one framing
//! from 32 directions (8 azimuths at 0, 8, 20 and 45 degrees up) and
//! compared with level 0 in crown width and height, silhouette area, wood
//! area and trunk widths (`docs/developer/PLANTS.md` section 6.4).
//!
//! `every_level_keeps_the_nearest_ones_widths` holds a set of species to
//! [`BOUNDS`]; retune the mesh, never the bounds. `lod_widths` measures
//! every built-in species:
//! `cargo test --release -p plantgen --test lod_widths -- --ignored --nocapture`;
//! `LOD_SPECIES=a,b` limits the species, `LOD_OUT` names the JSON-lines
//! output file and `LOD_THREADS` the threads.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use plantgen::graph::PlantGraph;
use plantgen::grow::{GrowthSettings, grow};
use plantgen::impostor;
use plantgen::lsys::Limits;
use plantgen::math::{self, Vec3};
use plantgen::mesh::{self, Mesh, PlantMesh};
use plantgen::package::{DEFAULT_DAY, IMPOSTOR_LOD, IMPOSTOR_SUPERSAMPLE};
use plantgen::quality::STANDARD;
use plantgen::raster::{self, Camera, Lighting, Material, Projection, RenderOptions};
use plantgen::spec::{PlantSpec, all_species};
use plantgen::templates::Templates;
use serde_json::{Value, json};

const SIZE: usize = 256;
const AZIMUTHS: usize = 8;
const ELEVATIONS: [f64; 4] = [0.0, 8.0, 20.0, 45.0];

fn options(size: usize) -> RenderOptions {
    RenderOptions {
        width: size,
        height: size,
        supersample: 1,
        shadows: false,
        shadow_bounds: None,
        shadow_texels: raster::SHADOW_TEXELS,
    }
}

fn wood_material(templates: &Templates) -> Material {
    templates.bark.map_or(Material::Opaque, Material::Bark)
}

/// Covered pixels: count, x extent, y extent, x p5..p95 width (pixels),
/// and the depth of each pixel.
struct Silhouette {
    count: usize,
    width: f64,
    height: f64,
    core: f64,
}

fn silhouette(image: &raster::Image) -> Silhouette {
    let (w, h) = (image.width, image.height);
    let mut xs = Vec::new();
    let (mut x0, mut x1, mut y0, mut y1) = (usize::MAX, 0, usize::MAX, 0);
    for y in 0..h {
        for x in 0..w {
            if image.color[y * w + x][3] >= 0.5 {
                xs.push(x);
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
    }
    if xs.is_empty() {
        return Silhouette {
            count: 0,
            width: 0.0,
            height: 0.0,
            core: 0.0,
        };
    }
    xs.sort_unstable();
    let p = |q: f64| xs[((xs.len() - 1) as f64 * q).round() as usize] as f64;
    Silhouette {
        count: xs.len(),
        width: (x1 - x0 + 1) as f64,
        height: (y1 - y0 + 1) as f64,
        core: p(0.95) - p(0.05) + 1.0,
    }
}

fn direction(azimuth: usize, elevation: f64) -> Vec3 {
    let a = 2.0 * math::PI * azimuth as f64 / AZIMUTHS as f64;
    let e = math::radians(elevation);
    Vec3::new(
        math::cos(e) * math::cos(a),
        math::sin(e),
        math::cos(e) * math::sin(a),
    )
}

fn ortho(direction: Vec3, center: Vec3, half: f64) -> Camera {
    Camera {
        eye: center + direction * (half * 4.0),
        target: center,
        up: Vec3::Y,
        projection: Projection::Orthographic { half_height: half },
    }
}

/// Point and radius of the plant's first stem at height `h`.
fn stem_at(graph: &PlantGraph, h: f64) -> Option<(Vec3, f64)> {
    let next = graph.continuations();
    let ends = graph.end_radii();
    let mut cursor = graph.segments.iter().position(|s| s.parent.is_none())?;
    loop {
        let s = &graph.segments[cursor];
        let (a, b) = (s.start.y, s.end.y);
        if (a.min(b)..=a.max(b)).contains(&h) && (b - a).abs() > 1e-9 {
            let t = (h - a) / (b - a);
            return Some((
                s.start.lerp(s.end, t),
                s.radius + (ends[cursor] - s.radius) * t,
            ));
        }
        cursor = next[cursor]? as usize;
    }
}

/// Width of the wood at the stem point `p` seen along horizontal
/// `direction`: the covered run through the image centre, counting only
/// surfaces within a few radii of the stem's plane.
fn trunk_width(
    wood: &Mesh,
    templates: &Templates,
    p: Vec3,
    r: f64,
    direction: Vec3,
) -> Option<f64> {
    const N: usize = 160;
    let half = (6.0 * r).max(0.002);
    let distance = 10.0 * half;
    let camera = Camera {
        eye: p + direction * distance,
        target: p,
        up: Vec3::Y,
        projection: Projection::Orthographic { half_height: half },
    };
    let image = raster::render(
        &[(wood, wood_material(templates))],
        &camera,
        &Lighting::flat(),
        templates,
        &options(N),
    );
    let row = N / 2;
    let ok = |x: usize| {
        let at = row * N + x;
        image.color[at][3] >= 0.5 && (f64::from(image.depth[at]) - distance).abs() <= 3.0 * r
    };
    // The covered pixel nearest the centre, then its run.
    let centre = (0..N)
        .filter(|&x| ok(x))
        .min_by_key(|&x| x.abs_diff(N / 2))?;
    if centre.abs_diff(N / 2) > N / 8 {
        return None;
    }
    let mut a = centre;
    while a > 0 && ok(a - 1) {
        a -= 1;
    }
    let mut b = centre;
    while b + 1 < N && ok(b + 1) {
        b += 1;
    }
    Some((b - a + 1) as f64 * 2.0 * half / N as f64)
}

fn median(mut v: Vec<f64>) -> Value {
    v.retain(|x| x.is_finite());
    if v.is_empty() {
        return Value::Null;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let m = if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    };
    json!((m * 1000.0).round() / 1000.0)
}

fn ratio(a: f64, b: f64) -> f64 {
    if b > 0.0 { a / b } else { f64::NAN }
}

fn measure(id: &str) -> Result<Value, String> {
    let spec = PlantSpec::builtin(id).map_err(|e| e.to_string())?;
    let (program, params) = spec.program().map_err(|e| e.to_string())?;
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
            host: spec.host_geometry().map_err(|e| e.to_string())?,
        },
    )
    .map_err(|e| e.to_string())?;
    let graph = growth.keyframes.last().ok_or("no keyframe")?;
    // As package::Inputs and bake_keyframe do.
    let day = spec.appearance.has_seasons().then_some(DEFAULT_DAY);
    let (looks, sizes) = spec.appearance.looks_on(program.organs(), day);
    let bodies = spec.appearance.body_looks(program.bodies());
    let named: Vec<(&str, &_)> = program.bodies().zip(&bodies).collect();
    let templates = Templates::for_plant(&looks, &named).with_bark(spec.appearance.bark_params());
    let mut parts = plantgen::parts::part_meshes(&looks);
    parts.retain(|part| sizes.get(part.template).is_none_or(|&size| size > 0.0));
    let part_types = plantgen::parts::types(&looks, &parts);
    let staged = plantgen::looks::staged(graph, &sizes);
    let drawn = staged.as_ref().unwrap_or(graph);
    let build = |g: &PlantGraph, level: usize| {
        mesh::build_with(
            g,
            &looks,
            &bodies,
            &spec.appearance,
            &STANDARD.lods[level].for_height(graph.height),
            level,
            Some(&part_types),
        )
    };
    let meshes: Vec<PlantMesh> = (0..4).map(|level| build(drawn, level)).collect();
    let mut bare = drawn.clone();
    bare.organs.clear();
    let woods: Vec<PlantMesh> = (0..4).map(|level| build(&bare, level)).collect();
    let cards: Vec<Mesh> = meshes.iter().map(PlantMesh::card_mesh).collect();
    let wood = wood_material(&templates);

    // One framing for every level: level 0's bounds.
    let (low, high) = raster::bounds(&[(&meshes[0].wood, wood), (&cards[0], Material::Card)])
        .ok_or("empty level 0")?;
    let center = (low + high) * 0.5;
    let half = (high - low).length() * 0.5 * 1.02;
    let pixel = 2.0 * half / SIZE as f64;

    // Per level, per direction: [area, width, height, core, wood area, wood width].
    let mut rows: Vec<Vec<[f64; 7]>> = vec![Vec::new(); 4];
    for (e, &elevation) in ELEVATIONS.iter().enumerate() {
        for azimuth in 0..AZIMUTHS {
            let camera = ortho(direction(azimuth, elevation), center, half);
            for level in 0..4 {
                let full = raster::render(
                    &[(&meshes[level].wood, wood), (&cards[level], Material::Card)],
                    &camera,
                    &Lighting::flat(),
                    &templates,
                    &options(SIZE),
                );
                let s = silhouette(&full);
                let w = silhouette(&raster::render(
                    &[(&woods[level].wood, wood)],
                    &camera,
                    &Lighting::flat(),
                    &templates,
                    &options(SIZE),
                ));
                rows[level].push([
                    s.count as f64,
                    s.width,
                    s.height,
                    s.core,
                    w.count as f64,
                    w.width,
                    e as f64,
                ]);
            }
        }
    }
    let mut levels = Vec::new();
    for level in 1..4 {
        let pick = |k: usize, low_only: bool| -> Vec<f64> {
            rows[level]
                .iter()
                .zip(&rows[0])
                .filter(|(a, _)| !low_only || a[6] <= 1.0)
                .map(|(a, b)| ratio(a[k], b[k]))
                .collect()
        };
        levels.push(json!({
            "area": median(pick(0, false)),
            "area_low": median(pick(0, true)),
            "width": median(pick(1, false)),
            "width_low": median(pick(1, true)),
            "height": median(pick(2, false)),
            "core": median(pick(3, false)),
            "wood_area": median(pick(4, false)),
            "wood_width": median(pick(5, false)),
        }));
    }
    let l0_area_m2 = median(rows[0].iter().map(|r| r[0] * pixel * pixel).collect());
    let l0_width_m = median(rows[0].iter().map(|r| r[1] * pixel).collect());
    let l0_wood_area_m2 = median(rows[0].iter().map(|r| r[4] * pixel * pixel).collect());

    // Trunk widths from the wood alone, eight level directions.
    let mut trunk = serde_json::Map::new();
    let heights = [
        ("1.3m", 1.3),
        ("10%", 0.1 * graph.height),
        ("30%", 0.3 * graph.height),
        ("50%", 0.5 * graph.height),
    ];
    for (name, h) in heights {
        if name == "1.3m" && graph.height < 2.6 {
            continue;
        }
        let Some((p, r)) = stem_at(drawn, h) else {
            continue;
        };
        let widths: Vec<Vec<f64>> = (0..4)
            .map(|level| {
                (0..AZIMUTHS)
                    .map(|a| {
                        trunk_width(&woods[level].wood, &templates, p, r, direction(a, 0.0))
                            .unwrap_or(f64::NAN)
                    })
                    .collect()
            })
            .collect();
        let per: Vec<Value> = (1..4)
            .map(|level| {
                median(
                    widths[level]
                        .iter()
                        .zip(&widths[0])
                        .map(|(a, b)| ratio(*a, *b))
                        .collect(),
                )
            })
            .collect();
        trunk.insert(
            name.to_string(),
            json!({
                "radius_m": (r * 1e4).round() / 1e4,
                "l0_width_over_2r": median(widths[0].iter().map(|w| w / (2.0 * r)).collect()),
                "l0_min_over_2r": median(vec![widths[0].iter().copied().filter(|w| w.is_finite()).fold(f64::INFINITY, f64::min) / (2.0 * r)]),
                "ratio": per,
                "min_ratio": (1..4).map(|level| {
                    let m = widths[level].iter().zip(&widths[0]).map(|(a, b)| ratio(*a, *b)).filter(|x| x.is_finite()).fold(f64::INFINITY, f64::min);
                    median(vec![m])
                }).collect::<Vec<_>>(),
            }),
        );
    }

    // Trunk sides per level, as mesh::wood counts them.
    let sides: Vec<u32> = (0..4)
        .map(|level| {
            let lod = STANDARD.lods[level].for_height(graph.height);
            let root = drawn.segments.iter().find(|s| s.parent.is_none());
            root.map_or(0, |s| {
                let base = s.radius.max(1e-4)
                    * (1.0 + spec.appearance.flare.as_ref().map_or(0.0, |f| f.amount))
                    * (1.0 + spec.appearance.bottle.as_ref().map_or(0.0, |b| b.amount));
                let around = 2.0 * math::PI * base / lod.ring_edge.max(1e-4);
                (around.ceil() as u32).clamp(lod.min_sides.max(3), lod.max_sides.max(3))
            })
        })
        .collect();

    // Card accounting per level: count, quad area, drawn (alpha >= 0.5)
    // area, the largest card, and how far cards reach from the axis.
    let crown_radius = drawn.crown_radius();
    let fraction = |t: usize| {
        templates.templates.get(t).map_or(1.0, |t| {
            t.coverage.iter().filter(|&&c| c >= 0.5).count() as f64 / t.coverage.len().max(1) as f64
        })
    };
    let card_stats: Vec<Value> = meshes
        .iter()
        .zip(&cards)
        .map(|(m, cm)| {
            let quad: f64 = m.cards.iter().map(|c| f64::from(c.length * c.width)).sum();
            let drawn_area: f64 = m
                .cards
                .iter()
                .map(|c| f64::from(c.length * c.width) * fraction(usize::from(c.template)))
                .sum();
            let longest = m
                .cards
                .iter()
                .map(|c| f64::from(c.length.max(c.width)))
                .fold(0.0, f64::max);
            let reach = cm
                .positions
                .iter()
                .map(|p| f64::from(p[0]).hypot(f64::from(p[2])))
                .fold(0.0, f64::max);
            let top = cm
                .positions
                .iter()
                .map(|p| f64::from(p[1]))
                .fold(f64::NEG_INFINITY, f64::max);
            json!({
                "cards": m.cards.len(),
                "quad_m2": (quad * 1000.0).round() / 1000.0,
                "drawn_m2": (drawn_area * 1000.0).round() / 1000.0,
                "longest_m": (longest * 1000.0).round() / 1000.0,
                "reach_over_crown_radius": (ratio(reach, crown_radius) * 1000.0).round() / 1000.0,
                "card_top_m": (top * 1000.0).round() / 1000.0,
                "wood_tris": m.wood.triangle_count(),
            })
        })
        .collect();

    // The impostor as package.rs bakes it, against level 0 and level 1
    // rendered with each low tile's own camera at 4x the tile resolution.
    let imp = impostor::bake(
        &meshes[IMPOSTOR_LOD],
        &templates,
        STANDARD.impostor_views,
        STANDARD.impostor_size,
        IMPOSTOR_SUPERSAMPLE,
    );
    let (views, size) = (imp.views, imp.size);
    let atlas = imp.atlas_size();
    let mut imp_area = Vec::new();
    let mut imp_strict = Vec::new();
    let mut imp_width = Vec::new();
    let mut l1_area = Vec::new();
    let mut l1_width = Vec::new();
    let reference = size * 4;
    for row in 0..views {
        for column in 0..views {
            let d = impostor::view_direction(column, row, views);
            if d.y > 0.5 {
                continue;
            }
            let (mut count, mut x0, mut x1) = (0usize, usize::MAX, 0usize);
            let mut strict = 0usize;
            for y in 0..size {
                for x in 0..size {
                    let at = ((row * size + y) * atlas + column * size + x) * 4;
                    if imp.albedo[at + 3] > 128 {
                        strict += 1;
                    }
                    if imp.albedo[at + 3] >= 128 {
                        count += 1;
                        x0 = x0.min(x);
                        x1 = x1.max(x);
                    }
                }
            }
            let tile_pixel = 2.0 * imp.radius / size as f64;
            let camera = impostor::view_camera(d, imp.center, imp.radius);
            let ref_pixel = 2.0 * imp.radius / reference as f64;
            let s0 = silhouette(&raster::render(
                &[(&meshes[0].wood, wood), (&cards[0], Material::Card)],
                &camera,
                &Lighting::flat(),
                &templates,
                &options(reference),
            ));
            let s1 = silhouette(&raster::render(
                &[(&meshes[1].wood, wood), (&cards[1], Material::Card)],
                &camera,
                &Lighting::flat(),
                &templates,
                &options(reference),
            ));
            let a0 = s0.count as f64 * ref_pixel * ref_pixel;
            let w0 = s0.width * ref_pixel;
            imp_area.push(ratio(count as f64 * tile_pixel * tile_pixel, a0));
            imp_strict.push(ratio(strict as f64 * tile_pixel * tile_pixel, a0));
            imp_width.push(ratio(
                if count > 0 {
                    (x1 - x0 + 1) as f64 * tile_pixel
                } else {
                    0.0
                },
                w0,
            ));
            l1_area.push(ratio(s1.count as f64 * ref_pixel * ref_pixel, a0));
            l1_width.push(ratio(s1.width * ref_pixel, w0));
        }
    }

    Ok(json!({
        "id": id,
        "form": format!("{:?}", spec.growth_form),
        "program": spec.generator.program,
        "age": age,
        "height_m": (graph.height * 100.0).round() / 100.0,
        "organs": drawn.organs.len(),
        "l0_area_m2": l0_area_m2,
        "l0_width_m": l0_width_m,
        "l0_wood_area_m2": l0_wood_area_m2,
        "pixel_m": (pixel * 1e4).round() / 1e4,
        "levels": levels,
        "trunk": trunk,
        "trunk_sides": sides,
        "cards": card_stats,
        "impostor": {
            "area": median(imp_area),
            "area_strict": median(imp_strict),
            "width": median(imp_width),
            "l1_area_same_camera": median(l1_area),
            "l1_width_same_camera": median(l1_width),
            "radius_m": (imp.radius * 100.0).round() / 100.0,
        },
    }))
}

#[test]
#[ignore = "every built-in species, half an hour on four cores"]
fn lod_widths() {
    let wanted: Option<Vec<String>> = std::env::var("LOD_SPECIES")
        .ok()
        .map(|s| s.split(',').map(str::to_string).collect());
    let ids: Vec<&str> = all_species()
        .map(|(id, _)| id)
        .filter(|id| wanted.as_ref().is_none_or(|w| w.iter().any(|x| x == id)))
        .collect();
    let out_path = std::env::var("LOD_OUT").unwrap_or_else(|_| "lod_widths.jsonl".to_string());
    let threads: usize = std::env::var("LOD_THREADS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    let next = AtomicUsize::new(0);
    let out = Mutex::new(std::fs::File::create(&out_path).unwrap());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(id) = ids.get(i) else { break };
                    let start = std::time::Instant::now();
                    let line = match measure(id) {
                        Ok(v) => v.to_string(),
                        Err(e) => json!({"id": id, "error": e}).to_string(),
                    };
                    eprintln!("{id}: {:.1} s", start.elapsed().as_secs_f64());
                    use std::io::Write;
                    let mut f = out.lock().unwrap();
                    writeln!(f, "{line}").unwrap();
                    f.flush().unwrap();
                }
            });
        }
    });
}

/// Species the test holds, one or more of each form whose coarse levels
/// drew wider or sparser than level 0 before generator revision 12: a fan
/// palm, rosettes, a scale-leaved conifer, twiggy desert trees and shrubs,
/// a cattail and broad-leaved herbs; and since revision 14 the rosettes
/// whose coarse levels became a cross of cards (render review S1): soaptree
/// yucca, desert agave and aloe. Conifers that take minutes to render are
/// measured by `lod_widths` only.
const HELD: [&str; 13] = [
    "washingtonia-filifera",
    "yucca-brevifolia",
    "yucca-elata",
    "agave-deserti",
    "aloe-vera",
    "dasylirion-wheeleri",
    "juniperus-scopulorum",
    "parkinsonia-microphylla",
    "prosopis-velutina",
    "larrea-tridentata",
    "typha-latifolia",
    "heracleum-maximum",
    "lysichiton-americanus",
];

/// Each measure of levels 1 to 3 over level 0's, as the median over the
/// directions, and the impostor's area: within these on every held
/// species. On generator revision 12 the held species measure widths 0.86
/// to 1.16, heights 0.98 to 1.04, areas 0.79 to 1.16 and impostor areas
/// 0.89 to 1.0; on revision 11 their coarse widths reached 2.9 and their
/// areas 3.8.
const BOUNDS: [(&str, f64, f64); 3] = [
    ("width", 0.82, 1.22),
    ("height", 0.9, 1.1),
    ("area", 0.75, 1.25),
];
const IMPOSTOR_AREA: (f64, f64) = (0.85, 1.1);

#[test]
fn every_level_keeps_the_nearest_ones_widths() {
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(id) = HELD.get(i) else { break };
                    let measured = measure(id).unwrap_or_else(|e| panic!("{id}: {e}"));
                    let mut wrong = Vec::new();
                    for (level, values) in measured["levels"].as_array().unwrap().iter().enumerate()
                    {
                        for (name, low, high) in BOUNDS {
                            let value = values[name].as_f64().unwrap();
                            if !(low..=high).contains(&value) {
                                wrong.push(format!("level {} {name} {value}", level + 1));
                            }
                        }
                    }
                    let impostor = measured["impostor"]["area"].as_f64().unwrap();
                    if !(IMPOSTOR_AREA.0..=IMPOSTOR_AREA.1).contains(&impostor) {
                        wrong.push(format!("impostor area {impostor}"));
                    }
                    if !wrong.is_empty() {
                        failures
                            .lock()
                            .unwrap()
                            .push(format!("{id}: {}", wrong.join(", ")));
                    }
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    assert!(
        failures.is_empty(),
        "levels unlike level 0:\n{}",
        failures.join("\n")
    );
}

//! Mosses grown over a site's surfaces (`plantgen::mosses`): the genera
//! the library describes, colonies that keep to the open faces of a wall
//! and favour its shaded, damp side, and covers that come out the same
//! every time.

use plantgen::library::Library;
use plantgen::math::Vec3;
use plantgen::mosses::{self, ColonyForm, Detail, DrawOptions, MossCover, MossKind, MossSettings};
use plantgen::site::{SiteMesh, Surface};
use plantgen::substrate::{BUILT, SOIL, WOOD};

/// A box from `low` to `high`, its faces wound outward.
fn cuboid(
    low: Vec3,
    high: Vec3,
    surface: Surface,
    out: &mut (Vec<Vec3>, Vec<[u32; 3]>, Vec<Surface>),
) {
    let first = u32::try_from(out.0.len()).unwrap();
    for i in 0..8 {
        out.0.push(Vec3::new(
            if i & 1 == 0 { low.x } else { high.x },
            if i & 2 == 0 { low.y } else { high.y },
            if i & 4 == 0 { low.z } else { high.z },
        ));
    }
    for [a, b, c, d] in [
        [0, 4, 6, 2],
        [1, 3, 7, 5],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 2, 3, 1],
        [4, 5, 7, 6],
    ] {
        out.1.push([first + a, first + b, first + c]);
        out.1.push([first + a, first + c, first + d]);
        out.2.extend([surface, surface]);
    }
}

/// Ground, an east-west wall 3 m long and 1 m high (its south face at
/// z = 0.1, its north at -0.1), and a slab lying on a board kept clear,
/// half a millimetre above it, as a tile lies on its deck.
fn wall_site() -> SiteMesh {
    let mut parts = (Vec::new(), Vec::new(), Vec::new());
    cuboid(
        Vec3::new(-3.0, -0.3, -3.0),
        Vec3::new(3.0, 0.0, 3.0),
        Surface::solid(SOIL),
        &mut parts,
    );
    cuboid(
        Vec3::new(-1.5, 0.0, -0.1),
        Vec3::new(1.5, 1.0, 0.1),
        Surface::solid(BUILT),
        &mut parts,
    );
    let mut board = Surface::solid(WOOD);
    board.clear = true;
    cuboid(
        Vec3::new(-2.5, 0.0, 1.0),
        Vec3::new(-1.5, 0.3, 2.0),
        board,
        &mut parts,
    );
    cuboid(
        Vec3::new(-2.5, 0.3005, 1.0),
        Vec3::new(-1.5, 0.32, 2.0),
        Surface::solid(BUILT),
        &mut parts,
    );
    SiteMesh::with_surfaces(&parts.0, &parts.1, &parts.2).unwrap()
}

fn library_kinds() -> Vec<MossKind> {
    mosses::kinds(&Library::builtin()).unwrap()
}

fn settings() -> MossSettings {
    MossSettings {
        years: 12,
        arrivals_per_m2: 60.0,
        ..MossSettings::default()
    }
}

#[test]
fn the_library_describes_wall_mosses_by_genus() {
    let kinds = library_kinds();
    let names: Vec<&str> = kinds.iter().map(|kind| kind.genus.as_str()).collect();
    for genus in [
        "Grimmia",
        "Tortula",
        "Bryum",
        "Ceratodon",
        "Hypnum",
        "Homalothecium",
        "Dicranoweisia",
        "Niphotrichum",
        "Rosulabryum",
    ] {
        assert!(names.contains(&genus), "{genus} missing from {names:?}");
    }
    let kind = |genus: &str| kinds.iter().find(|kind| kind.genus == genus).unwrap();
    // FNA vol. 27: Grimmia pulvinata in hoary cushions, its leaves awned.
    let grimmia = kind("Grimmia");
    assert_eq!(grimmia.form, ColonyForm::Cushion);
    assert!(!grimmia.pleurocarpous);
    assert!(grimmia.hair_point_m > 0.0);
    assert!(grimmia.seta_arcuate);
    // FNA vol. 28: Hypnum in mats, capsules on side branches.
    let hypnum = kind("Hypnum");
    assert_eq!(hypnum.form, ColonyForm::Mat);
    assert!(hypnum.pleurocarpous);
    assert!(hypnum.spread_m > grimmia.spread_m);
    // The roof mosses of western Oregon (FNA vol. 27, 28): Dicranoweisia
    // in cushions and fruiting freely, so its spores outnumber those of
    // Niphotrichum's mats, which seldom fruit; Rosulabryum in turfs.
    let dicranoweisia = kind("Dicranoweisia");
    let niphotrichum = kind("Niphotrichum");
    assert_eq!(dicranoweisia.form, ColonyForm::Cushion);
    assert_eq!(niphotrichum.form, ColonyForm::Mat);
    assert_eq!(kind("Rosulabryum").form, ColonyForm::Turf);
    assert!(dicranoweisia.spore_output() > 2.0 * niphotrichum.spore_output());
}

#[test]
fn moss_keeps_to_open_faces_and_favours_the_shaded_side() {
    let site = wall_site();
    let kinds = library_kinds();
    // An upright face carries few colonies, so the faces are compared
    // over four seeds: one large mat can decide a single seed.
    let (mut north, mut south) = (0.0, 0.0);
    for seed in 1..=4 {
        let cover = mosses::grow(&site, &kinds, &MossSettings { seed, ..settings() });
        assert!(
            cover.colonies.len() > 20,
            "only {} colonies",
            cover.colonies.len()
        );
        for colony in &cover.colonies {
            let [x, y, z] = colony.centre;
            // Never on the board kept clear, nor under the slab lying on it.
            let on_board = (-2.5..=-1.5).contains(&x) && (1.0..=2.0).contains(&z) && y < 0.3004;
            assert!(
                !on_board,
                "a colony at {:?} on the board or under the slab",
                colony.centre
            );
            let area = std::f64::consts::PI * colony.radius_m * colony.radius_m;
            if colony.normal[2] < -0.9 {
                north += area;
            } else if colony.normal[2] > 0.9 {
                south += area;
            }
        }
    }
    // Sun from the south dries the south face; the north stays damp.
    assert!(
        north > south * 1.5,
        "north {north:.4} m2, south {south:.4} m2"
    );
}

#[test]
fn the_same_site_grows_the_same_cover_and_it_reads_back() {
    let site = wall_site();
    let kinds = library_kinds();
    let a = mosses::grow(&site, &kinds, &settings());
    let b = mosses::grow(&site, &kinds, &settings());
    assert_eq!(a, b);
    let text = a.to_json().unwrap();
    assert_eq!(text, b.to_json().unwrap());
    // Read back, to the last digit or so JSON keeps.
    let back = MossCover::from_json(&text).unwrap();
    let genus = |cover: &MossCover| {
        cover
            .kinds
            .iter()
            .map(|kind| kind.genus.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(genus(&back), genus(&a));
    assert_eq!(back.colonies.len(), a.colonies.len());
    for (x, y) in back.colonies.iter().zip(&a.colonies) {
        assert_eq!(x.kind, y.kind);
        assert!((x.radius_m - y.radius_m).abs() < 1.0e-12);
        assert!(
            x.centre
                .iter()
                .zip(y.centre)
                .all(|(p, q)| (p - q).abs() < 1.0e-12)
        );
    }
}

#[test]
fn a_cover_draws_as_patches_far_shells_between_and_shoots_near() {
    let site = wall_site();
    let cover = mosses::grow(&site, &library_kinds(), &settings());
    let draw = |detail: Detail, near: Option<(Vec3, f64)>| {
        cover.draw(
            &site,
            &DrawOptions {
                detail,
                near,
                capsules: false,
                ..DrawOptions::default()
            },
        )
    };
    let patches = draw(Detail::Patch, None);
    let shells = draw(Detail::Shell, None);
    assert!(!patches.indices.is_empty());
    // A patch is a few tens of triangles, a shell a few hundred.
    assert!(
        patches.indices.len() * 3 < shells.indices.len(),
        "patches {} shells {}",
        patches.indices.len() / 3,
        shells.indices.len() / 3
    );
    // Round one colony, Near draws its shoots: many more triangles there.
    let at = cover.colonies[0].centre;
    let at = Vec3::new(at[0], at[1], at[2]);
    let near = draw(Detail::Near, Some((at, 0.02)));
    let close = |mesh: &plantgen::mesh::Mesh| {
        mesh.indices
            .iter()
            .filter(|i| {
                let p = mesh.positions[**i as usize];
                Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])).distance(at) < 0.03
            })
            .count()
    };
    assert!(close(&near) > close(&shells) * 3);
}

#[test]
fn moss_is_darker_and_greener_wet_than_dry() {
    let site = wall_site();
    let cover = mosses::grow(&site, &library_kinds(), &settings());
    let mean = |hydration: f64| {
        let mesh = cover.draw(
            &site,
            &DrawOptions {
                hydration,
                detail: Detail::Patch,
                ..DrawOptions::default()
            },
        );
        #[allow(clippy::cast_precision_loss)]
        let n = mesh.colors.len().max(1) as f32;
        mesh.colors.iter().fold([0.0f32; 3], |sum, c| {
            [sum[0] + c[0] / n, sum[1] + c[1] / n, sum[2] + c[2] / n]
        })
    };
    let (wet, dry) = (mean(0.95), mean(0.05));
    let luminance = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    // Drying, moss reflects more and turns duller and yellower.
    assert!(luminance(wet) < luminance(dry), "wet {wet:?} dry {dry:?}");
    assert!(wet[1] / wet[0] > dry[1] / dry[0], "wet {wet:?} dry {dry:?}");
}

#[test]
fn a_cover_tints_the_surface_under_it_with_no_triangles_of_its_own() {
    let site = wall_site();
    let cover = mosses::grow(&site, &library_kinds(), &settings());
    let tint = cover.tint(0.5);
    // At a colony's centre the moss hides the surface; far from any colony
    // there is none.
    let colony = cover
        .colonies
        .iter()
        .max_by(|a, b| a.radius_m.total_cmp(&b.radius_m))
        .unwrap();
    let at = Vec3::new(colony.centre[0], colony.centre[1], colony.centre[2]);
    let normal = Vec3::new(colony.normal[0], colony.normal[1], colony.normal[2]);
    let (colour, share) = tint.at(at, normal).unwrap();
    assert!(share > 0.6, "share {share}");
    assert!(colour[1] > colour[2], "moss is green, not blue: {colour:?}");
    assert!(tint.at(Vec3::new(50.0, 50.0, 50.0), Vec3::Y).is_none());
    // The site painted: the triangles near moss split, the far ones kept,
    // and some vertices greener than the bare surface.
    let (low, high) = site.bounds();
    let bare = site.drawn(low, high, Vec3::ZERO);
    let painted = tint.paint(&bare, 0.02, low, high);
    assert!(painted.indices.len() > bare.indices.len());
    let changed = painted
        .colors
        .iter()
        .filter(|c| !bare.colors.iter().any(|b| b[..3] == c[..3]))
        .count();
    assert!(changed > 0);
}

#[test]
fn a_view_named_for_a_genus_finds_that_genus() {
    let site = wall_site();
    let cover = mosses::grow(&site, &library_kinds(), &settings());
    let mut found = 0;
    for (index, kind) in cover.kinds.iter().enumerate() {
        let Some(colony) = cover.showpiece(&kind.genus) else {
            continue;
        };
        found += 1;
        assert_eq!(usize::from(colony.kind), index, "{}", kind.genus);
        // Its view point names it back: the nearest colony is itself.
        let (nearest, _) = cover
            .nearest(colony.centre)
            .expect("a cover with colonies has a nearest one");
        assert_eq!(nearest.kind, colony.kind, "{}", kind.genus);
        // No colony of the genus is wider and as vigorous.
        assert!(cover.colonies.iter().all(|other| other.kind != colony.kind
            || other.age < 3.0
            || other.radius_m * other.vigour.min(1.0)
                <= colony.radius_m * colony.vigour.min(1.0) + 1e-12));
    }
    assert!(found > 0, "the wall grows some genus");
    assert!(cover.showpiece("Sphagnum").is_none());
}

#[test]
fn moss_leaves_take_their_outline_from_leaf_shape() {
    let kinds = library_kinds();
    let kind = |genus: &str| {
        kinds
            .iter()
            .find(|kind| kind.genus == genus)
            .unwrap_or_else(|| panic!("the library describes {genus}"))
    };
    // FNA: Rosulabryum capillare and Tortula obovate, widest above the
    // middle; Grimmia pulvinata lanceolate, widest below it.
    for genus in ["Rosulabryum", "Tortula"] {
        assert!(!kind(genus).outline.is_empty(), "{genus}");
        assert!(kind(genus).blade_widest() > 0.5, "{genus}");
        assert!(
            kind(genus).blade_width(0.1) < kind(genus).blade_width(0.8),
            "{genus}"
        );
    }
    assert!(kind("Grimmia").blade_widest() < 0.5);
    assert!((kind("Grimmia").blade_width(kind("Grimmia").blade_widest()) - 1.0).abs() < 1e-9);
    assert!(kind("Grimmia").blade_width(1.0) < 0.05);
}

#[test]
fn a_drawn_cover_writes_as_a_glb_any_reader_opens() {
    let site = wall_site();
    let cover = mosses::grow(&site, &library_kinds(), &settings());
    let moss = cover.draw(
        &site,
        &DrawOptions {
            detail: Detail::Shell,
            capsules: false,
            ..DrawOptions::default()
        },
    );
    let (low, high) = site.bounds();
    let ground = site.drawn(low, high, Vec3::ZERO);
    let file = plantgen::gltf::coloured_glb(
        &[("site", &ground, false), ("moss", &moss, true)],
        "PlantGen moss",
        serde_json::json!({}),
    );
    let summary = plantgen::gltf::check(&file).expect("a valid glTF file");
    assert_eq!(summary.nodes, 2);
    assert_eq!(
        summary.triangles,
        (moss.indices.len() + ground.indices.len()) / 3
    );
    // Plain float positions, normals and colours: no extension a reader
    // must know, so Blender's importer opens it as it is.
    let glb = plantgen::gltf::Glb::read(&file).unwrap();
    assert!(glb.json.get("extensionsRequired").is_none());
    assert!(
        glb.json
            .pointer("/meshes/1/primitives/0/attributes/COLOR_0")
            .is_some()
    );
}

#[test]
fn a_tree_over_a_wooden_deck_lets_moss_grow_there() {
    use plantgen::conditions::Envelope;
    // A wooden deck 3 m square, 2 m up, in the open.
    let mut parts = (Vec::new(), Vec::new(), Vec::new());
    cuboid(
        Vec3::new(-4.0, -0.3, -4.0),
        Vec3::new(4.0, 0.0, 4.0),
        Surface::solid(SOIL),
        &mut parts,
    );
    cuboid(
        Vec3::new(-1.5, 1.9, -1.5),
        Vec3::new(1.5, 2.0, 1.5),
        Surface::solid(WOOD),
        &mut parts,
    );
    let site = SiteMesh::with_surfaces(&parts.0, &parts.1, &parts.2).unwrap();
    let kinds = library_kinds();
    let deck_moss = |cover: &MossCover| -> f64 {
        cover
            .colonies
            .iter()
            .filter(|colony| colony.centre[1] > 1.99 && colony.normal[1] > 0.9)
            .map(|colony| std::f64::consts::PI * colony.radius_m * colony.radius_m)
            .sum()
    };
    let open = mosses::grow(&site, &kinds, &settings());
    assert_eq!(
        mosses::grow_under(&site, &kinds, &settings(), &[]),
        open,
        "no canopy, the same cover"
    );
    // A broadleaf crown south of the deck, over it: 3 m across the
    // radius from 3 to 9 m up, shade 0.27 a metre.
    let crown = Envelope {
        centre_m: [0.0, -2.0],
        base_m: 3.0,
        top_m: 9.0,
        radius_m: 3.0,
        density: 0.27,
    };
    let shaded = mosses::grow_under(&site, &kinds, &settings(), std::slice::from_ref(&crown));
    assert_eq!(shaded.canopy, vec![crown]);
    // Out of the sun the boards stay damp longer (OSU Extension: roof
    // mosses do best where trees shade them).
    let (before, after) = (deck_moss(&open), deck_moss(&shaded));
    assert!(
        after > before * 2.0,
        "deck {before:.4} m2 open, {after:.4} m2 under the tree"
    );
}

#[test]
fn a_surface_may_say_its_face_is_neutral_where_its_material_is_basic() {
    // Asphalt shingles are built but neutral; mortar stays basic, shakes acid.
    assert_eq!(Surface::solid(BUILT).reaction(), 1);
    assert_eq!(Surface::solid(WOOD).reaction(), -1);
    assert_eq!(Surface::solid(SOIL).reaction(), 0);
    let asphalt = Surface {
        reaction: Some(0),
        ..Surface::solid(BUILT)
    };
    assert_eq!(asphalt.reaction(), 0);
}

#[test]
fn shade_lengthens_shoots_shortens_hair_points_and_sun_yellows_leaves() {
    let kinds = library_kinds();
    let grimmia = kinds
        .iter()
        .find(|kind| kind.genus == "Grimmia")
        .expect("the library describes Grimmia");
    // Part shade, where floras describe a genus, changes nothing.
    assert!(matches!(
        grimmia.in_light(0.5),
        std::borrow::Cow::Borrowed(_)
    ));
    // Glime vol. 1: hair-points shorter in shade (Racomitrium, p. 976),
    // shoots taller and leaves smaller in deep shade (pp. 946, 959-960).
    let deep = grimmia.in_light(0.02);
    assert!(grimmia.hair_point_m > 0.0);
    assert!(deep.hair_point_m < 0.5 * grimmia.hair_point_m);
    assert!(deep.height_m > 1.4 * grimmia.height_m);
    assert!(deep.leaf_length_m < grimmia.leaf_length_m);
    // Carotenoids in full sun (Ceratodon, p. 1735): less blue against red.
    let sun = grimmia.in_light(1.0);
    let ratio = |c: [f32; 3]| c[2] / c[0].max(1.0e-6);
    assert!(ratio(sun.colour) < 0.8 * ratio(grimmia.colour));
    assert!((sun.hair_point_m - grimmia.hair_point_m).abs() < 1e-12);
}

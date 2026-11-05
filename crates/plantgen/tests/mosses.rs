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
}

#[test]
fn moss_keeps_to_open_faces_and_favours_the_shaded_side() {
    let site = wall_site();
    let cover = mosses::grow(&site, &library_kinds(), &settings());
    assert!(
        cover.colonies.len() > 20,
        "only {} colonies",
        cover.colonies.len()
    );
    let (mut north, mut south) = (0.0, 0.0);
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

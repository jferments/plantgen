//! The plant-made ground looks: what plants lay on the ground between
//! them, as tiling textures drawn with the kit in [`crate::texture`].
//!
//! Needles, twigs and cones fall on dark duff; leaves of the organ
//! templates the plants wear (`palmate`, `simple`, `lobed`) fall on soil;
//! dead grass lies flat in swirls of thatch, and twigs, bark and small
//! leaves gather under desert shrubs; moss grows in cushions and sphagnum
//! in star-headed carpets; grass tufts spread their blades, tussocks stand
//! in clumps with bare ground between, and cushion plants in low domes.
//! Whatever lies higher hides what lies under it, and the relief records
//! how high each texel lies, so a viewer can blend looks by height and
//! light their bumps. A host keeps its own looks (Project After's soils,
//! stones, bedrock, lichens and crusts) and lists them all in its own
//! order; each look is drawn on its own ([`Recipe::look`]), so the list it
//! sits in never changes its pixels.

use crate::looks::{Fruit, Lobed, Palmate, Shape, Simple};
use crate::math::{self, PI};
use crate::texture::{Canvas, Draws, fbm, mix, organ_templates, recipe, scale, spot};
pub use crate::texture::{
    GROUND_LOOK_SIZE, GroundLook, Recipe, linear_to_srgb, mip_chain, relief_slopes, srgb_to_linear,
};

/// The plant-made looks: needles, leaves, thatch, twigs, moss, sphagnum,
/// grass, tussock and cushion.
pub static PLANT_RECIPES: [Recipe; 9] = [
    // Douglas-fir duff: rusty needles over dark humus.
    recipe("needles", 1.5, [0.105, 0.0675, 0.0375], 0.012, draw_needles),
    // Dry broadleaf litter: tan, brown and ochre.
    recipe("leaves", 2.0, [0.145, 0.10, 0.0525], 0.02, draw_leaves),
    // Dead grass lying flat in swirls.
    recipe("thatch", 1.5, [0.30, 0.26, 0.16], 0.02, draw_thatch),
    // Twigs, bark and small leaves under desert shrubs.
    recipe("twigs", 2.0, [0.17, 0.14, 0.11], 0.015, draw_twigs),
    recipe("moss", 1.0, [0.065, 0.11, 0.03], 0.03, draw_moss),
    recipe("sphagnum", 1.0, [0.13, 0.13, 0.05], 0.025, draw_sphagnum),
    recipe("grass", 1.0, [0.12, 0.175, 0.0725], 0.04, draw_grass),
    recipe("tussock", 2.0, [0.24, 0.21, 0.13], 0.25, draw_tussock),
    recipe("cushion", 1.0, [0.06, 0.13, 0.035], 0.06, draw_cushion),
];

/// Draw every plant-made look for `seed`, in [`PLANT_RECIPES`] order.
#[must_use]
pub fn plant_looks(seed: u64) -> Vec<GroundLook> {
    PLANT_RECIPES
        .iter()
        .map(|recipe| recipe.look(seed))
        .collect()
}

/// The recipe of the plant-made look `name`.
#[must_use]
pub fn plant_recipe(name: &str) -> Option<&'static Recipe> {
    PLANT_RECIPES.iter().find(|recipe| recipe.name == name)
}

fn draw_needles(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let humus = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(humus, 8, 4, x, y, size);
        (mix([0.07, 0.045, 0.03], [0.14, 0.09, 0.055], n), 0.15 * n)
    });
    // Fallen needles, 2.5 to 4 cm long at 341 texels a metre.
    let palette = [
        [0.36, 0.19, 0.09],
        [0.42, 0.29, 0.16],
        [0.25, 0.17, 0.11],
        [0.30, 0.22, 0.12],
        [0.27, 0.26, 0.13],
    ];
    let needles = draws.stream(2);
    let count = 14_000_u64;
    for i in 0..count {
        let a = spot(needles, i, size);
        let angle = needles.unit(i, 3) * 2.0 * PI;
        let length = needles.range(i, 4, 8.0, 14.0);
        let b = [
            a[0] + length * math::cos(angle),
            a[1] + length * math::sin(angle),
        ];
        let colour = scale(needles.pick(i, 5, &palette), needles.range(i, 6, 0.75, 1.2));
        #[allow(clippy::cast_precision_loss)]
        let layer = 0.25 + 0.75 * (i as f64 / count as f64);
        canvas.stroke(a, b, needles.range(i, 7, 1.0, 1.7), colour, layer);
    }
    // Twigs and a few cones.
    let twigs = draws.stream(3);
    for i in 0..10 {
        let a = spot(twigs, i, size);
        let angle = twigs.unit(i, 3) * 2.0 * PI;
        let length = twigs.range(i, 4, 50.0, 130.0);
        let bend = twigs.range(i, 5, -0.4, 0.4);
        let mid = [
            a[0] + 0.5 * length * math::cos(angle),
            a[1] + 0.5 * length * math::sin(angle),
        ];
        let end = [
            mid[0] + 0.5 * length * math::cos(angle + bend),
            mid[1] + 0.5 * length * math::sin(angle + bend),
        ];
        let colour = scale([0.22, 0.16, 0.11], twigs.range(i, 6, 0.8, 1.2));
        let width = twigs.range(i, 7, 2.0, 3.5);
        canvas.stroke(a, mid, width, colour, 1.15);
        canvas.stroke(mid, end, width * 0.8, colour, 1.15);
    }
    let cones = organ_templates(&[Shape::Fruit(Fruit {
        aspect: 0.45,
        cone: 1.0,
        ..Fruit::default()
    })]);
    let spots = draws.stream(4);
    for i in 0..4 {
        canvas.stamp(
            &cones,
            0,
            spot(spots, i, size),
            spots.range(i, 3, 22.0, 30.0),
            spots.unit(i, 4) * 2.0 * PI,
            [0.36, 0.22, 0.12],
            1.3,
            0.4,
        );
    }
}

fn draw_leaves(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let soil = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(soil, 8, 4, x, y, size);
        (mix([0.06, 0.04, 0.025], [0.12, 0.08, 0.045], n), 0.1 * n)
    });
    // Bigleaf maple, alder and cottonwood, and Oregon white oak: leaves
    // of 8 to 30 cm at 256 texels a metre.
    let templates = organ_templates(&[
        Shape::Palmate(Palmate::default()),
        Shape::Simple(Simple {
            width: 0.55,
            teeth: 12,
            ..Simple::default()
        }),
        Shape::Simple(Simple {
            width: 0.7,
            widest: 0.35,
            ..Simple::default()
        }),
        Shape::Lobed(Lobed::default()),
    ]);
    let lengths = [[45.0, 75.0], [18.0, 28.0], [22.0, 34.0], [26.0, 40.0]];
    let palette = [
        [0.48, 0.32, 0.14],
        [0.33, 0.19, 0.08],
        [0.52, 0.38, 0.11],
        [0.20, 0.12, 0.06],
        [0.40, 0.25, 0.10],
    ];
    let leaves = draws.stream(2);
    let count = 2_600_u64;
    for i in 0..count {
        let template = leaves.pick(i, 3, &[0_usize, 0, 1, 1, 1, 2, 2, 3]);
        let [low, high] = lengths[template];
        let colour = scale(leaves.pick(i, 5, &palette), leaves.range(i, 6, 0.75, 1.15));
        #[allow(clippy::cast_precision_loss)]
        let layer = 0.2 + 0.8 * (i as f64 / count as f64);
        canvas.stamp(
            &templates,
            template,
            spot(leaves, i, size),
            leaves.range(i, 4, low, high),
            leaves.unit(i, 7) * 2.0 * PI,
            colour,
            layer,
            leaves.range(i, 8, 0.0, 0.15),
        );
    }
    let twigs = draws.stream(3);
    for i in 0..6 {
        let a = spot(twigs, i, size);
        let angle = twigs.unit(i, 3) * 2.0 * PI;
        let length = twigs.range(i, 4, 40.0, 110.0);
        let b = [
            a[0] + length * math::cos(angle),
            a[1] + length * math::sin(angle),
        ];
        canvas.stroke(a, b, twigs.range(i, 5, 1.5, 3.0), [0.20, 0.15, 0.10], 1.1);
    }
}

fn draw_moss(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let ground = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(ground, 6, 3, x, y, size);
        (mix([0.05, 0.06, 0.02], [0.09, 0.10, 0.04], n), 0.0)
    });
    // Cushions 3 to 10 cm across at 512 texels a metre, each a dome of
    // fine shoots from yellow-green to deep green.
    let cushions = draws.stream(2);
    let shoots = draws.stream(3);
    let greens = [
        [0.13, 0.25, 0.05],
        [0.20, 0.30, 0.06],
        [0.10, 0.19, 0.05],
        [0.17, 0.24, 0.05],
    ];
    for i in 0..420 {
        let c = spot(cushions, i, size);
        let radius = cushions.range(i, 3, 12.0, 32.0);
        let green = cushions.pick(i, 4, &greens);
        let base = cushions.range(i, 5, 0.0, 0.3);
        canvas.dome(
            c,
            radius,
            radius * cushions.range(i, 6, 0.7, 1.0),
            cushions.unit(i, 7) * PI,
            base,
            0.7,
            |u, v, dome| {
                // Shoot tips speckle the cushion; its top catches the light.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let (ix, iy) = ((u * 40.0 + 100.0) as u64, (v * 40.0 + 100.0) as u64);
                let speckle = shoots.unit(ix * 1000 + iy, i);
                scale(green, (0.55 + 0.5 * dome) * (0.8 + 0.4 * speckle))
            },
        );
    }
}

fn draw_grass(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let thatch = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(thatch, 8, 4, x, y, size);
        (mix([0.10, 0.09, 0.05], [0.24, 0.20, 0.10], n), 0.1 * n)
    });
    // Tufts seen from above: blades 3 to 8 cm long at 512 texels a metre
    // spreading from each tuft's crown, green to straw.
    let tufts = draws.stream(2);
    let blades = draws.stream(3);
    let colours = [
        [0.16, 0.32, 0.08],
        [0.22, 0.38, 0.10],
        [0.30, 0.36, 0.13],
        [0.42, 0.38, 0.18],
        [0.12, 0.26, 0.07],
    ];
    let mut blade = 0_u64;
    for i in 0..260 {
        let c = spot(tufts, i, size);
        let count = 18 + tufts.below(i, 3, 26);
        let lean = tufts.unit(i, 4) * 2.0 * PI;
        for _ in 0..count {
            blade += 1;
            let angle = lean + blades.range(blade, 1, -1.6, 1.6) + blades.unit(blade, 6) * PI;
            let length = blades.range(blade, 2, 15.0, 42.0);
            let start = [
                c[0] + blades.range(blade, 3, -4.0, 4.0),
                c[1] + blades.range(blade, 4, -4.0, 4.0),
            ];
            let end = [
                start[0] + length * math::cos(angle),
                start[1] + length * math::sin(angle),
            ];
            let colour = scale(
                blades.pick(blade, 5, &colours),
                blades.range(blade, 7, 0.8, 1.2),
            );
            // Blades rise from the crown: higher near it.
            let mid = [
                f64::midpoint(start[0], end[0]),
                f64::midpoint(start[1], end[1]),
            ];
            canvas.stroke(start, mid, 2.2, colour, 0.95);
            canvas.stroke(mid, end, 1.6, colour, 0.7);
        }
    }
}

fn draw_thatch(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let soil = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(soil, 8, 4, x, y, size);
        (mix([0.09, 0.075, 0.05], [0.18, 0.15, 0.10], n), 0.1 * n)
    });
    // Blades 10 to 25 cm long at 341 texels a metre, laid flat by snow
    // and wind in swirls that follow a slowly turning direction.
    let flow = draws.stream(2);
    let blades = draws.stream(3);
    let colours = [
        [0.62, 0.55, 0.36],
        [0.55, 0.47, 0.30],
        [0.70, 0.64, 0.46],
        [0.46, 0.40, 0.28],
        [0.40, 0.42, 0.26],
    ];
    let count = 5_200_u64;
    for i in 0..count {
        let a = spot(blades, i, size);
        let swirl = fbm(flow, 3, 2, a[0], a[1], size) * 4.0 * PI;
        let angle = swirl + blades.range(i, 3, -0.5, 0.5);
        let length = blades.range(i, 4, 34.0, 85.0);
        let bend = blades.range(i, 5, -0.25, 0.25);
        let mid = [
            a[0] + 0.5 * length * math::cos(angle),
            a[1] + 0.5 * length * math::sin(angle),
        ];
        let end = [
            mid[0] + 0.5 * length * math::cos(angle + bend),
            mid[1] + 0.5 * length * math::sin(angle + bend),
        ];
        let colour = scale(blades.pick(i, 6, &colours), blades.range(i, 7, 0.8, 1.15));
        #[allow(clippy::cast_precision_loss)]
        let layer = 0.3 + 0.7 * (i as f64 / count as f64);
        let width = blades.range(i, 8, 1.4, 2.6);
        canvas.stroke(a, mid, width, colour, layer);
        canvas.stroke(mid, end, width * 0.8, colour, layer);
    }
}

fn draw_twigs(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let dust = draws.stream(1);
    let grain = draws.stream(2);
    canvas.fill(|x, y| {
        let n = fbm(dust, 6, 4, x, y, size);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let speck = grain.unit((x as u64) * 1024 + y as u64, 0);
        let colour = mix([0.36, 0.31, 0.24], [0.50, 0.44, 0.35], n);
        (scale(colour, 0.9 + 0.2 * speck), 0.2 * n)
    });
    // Small grey leaves of sagebrush and blackbrush, 1 to 3 cm at 256
    // texels a metre, flakes of shredded bark and twigs.
    let leaves = organ_templates(&[Shape::Simple(Simple {
        width: 0.4,
        ..Simple::default()
    })]);
    let fall = draws.stream(3);
    for i in 0..700 {
        canvas.stamp(
            &leaves,
            0,
            spot(fall, i, size),
            fall.range(i, 3, 3.0, 8.0),
            fall.unit(i, 4) * 2.0 * PI,
            scale([0.42, 0.42, 0.34], fall.range(i, 5, 0.7, 1.15)),
            fall.range(i, 6, 0.3, 0.8),
            0.05,
        );
    }
    let bark = draws.stream(4);
    for i in 0..260 {
        let c = spot(bark, i, size);
        let radius = bark.range(i, 3, 2.0, 6.0);
        let colour = scale([0.30, 0.24, 0.18], bark.range(i, 4, 0.7, 1.2));
        canvas.dome(
            c,
            radius,
            radius * 0.35,
            bark.unit(i, 5) * PI,
            0.35,
            0.25,
            |_, _, dome| scale(colour, 0.75 + 0.35 * dome),
        );
    }
    let twigs = draws.stream(5);
    for i in 0..380 {
        let a = spot(twigs, i, size);
        let angle = twigs.unit(i, 3) * 2.0 * PI;
        let length = twigs.range(i, 4, 12.0, 60.0);
        let bend = twigs.range(i, 5, -0.6, 0.6);
        let mid = [
            a[0] + 0.5 * length * math::cos(angle),
            a[1] + 0.5 * length * math::sin(angle),
        ];
        let end = [
            mid[0] + 0.5 * length * math::cos(angle + bend),
            mid[1] + 0.5 * length * math::sin(angle + bend),
        ];
        let grey = twigs.pick(
            i,
            6,
            &[[0.36, 0.33, 0.28], [0.26, 0.21, 0.16], [0.45, 0.42, 0.37]],
        );
        let colour = scale(grey, twigs.range(i, 7, 0.8, 1.2));
        let width = twigs.range(i, 8, 1.2, 2.8);
        canvas.stroke(a, mid, width, colour, 1.0);
        canvas.stroke(mid, end, width * 0.75, colour, 1.0);
    }
}

fn draw_sphagnum(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let water = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(water, 6, 3, x, y, size);
        (mix([0.03, 0.04, 0.02], [0.07, 0.07, 0.03], n), 0.0)
    });
    // Capitula, the star-shaped heads of the shoots, 1 to 2 cm across at
    // 512 texels a metre, packed into a carpet whose colour wanders from
    // green to ochre and red.
    let heads = draws.stream(2);
    let hue = draws.stream(3);
    let greens = [0.20, 0.30, 0.08];
    let ochres = [0.42, 0.34, 0.10];
    let reds = [0.40, 0.14, 0.10];
    for i in 0..5_200 {
        let c = spot(heads, i, size);
        let radius = heads.range(i, 3, 4.0, 9.0);
        let h = fbm(hue, 3, 3, c[0], c[1], size);
        let base = if h < 0.45 {
            mix(greens, ochres, h / 0.45)
        } else {
            mix(ochres, reds, ((h - 0.45) / 0.4).min(1.0))
        };
        let colour = scale(base, heads.range(i, 4, 0.75, 1.2));
        let turn = heads.unit(i, 5) * 2.0 * PI;
        canvas.dome(
            c,
            radius,
            radius,
            0.0,
            heads.range(i, 6, 0.0, 0.4),
            0.6,
            |u, v, dome| {
                // Five branch tufts radiate from the head's bud.
                let star = 0.5 + 0.5 * math::cos(5.0 * (libm::atan2(v, u) + turn));
                scale(colour, (0.55 + 0.45 * dome) * (0.75 + 0.35 * star))
            },
        );
    }
}

fn draw_tussock(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let soil = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(soil, 6, 4, x, y, size);
        (mix([0.10, 0.08, 0.055], [0.22, 0.18, 0.12], n), 0.05 * n)
    });
    // Bunchgrass and sedge tussocks 20 to 45 cm across at 256 texels a
    // metre: dense crowns of fine blades, straw outside and green at the
    // heart, with bare ground and a little thatch between.
    let litter = draws.stream(2);
    for i in 0..900 {
        let a = spot(litter, i, size);
        let angle = litter.unit(i, 3) * 2.0 * PI;
        let length = litter.range(i, 4, 10.0, 30.0);
        let b = [
            a[0] + length * math::cos(angle),
            a[1] + length * math::sin(angle),
        ];
        canvas.stroke(
            a,
            b,
            1.2,
            scale([0.48, 0.42, 0.28], litter.range(i, 5, 0.7, 1.1)),
            0.1,
        );
    }
    let tussocks = draws.stream(3);
    let blades = draws.stream(4);
    let straw = [[0.62, 0.55, 0.36], [0.55, 0.48, 0.32], [0.70, 0.62, 0.42]];
    let mut blade = 0_u64;
    for i in 0..30 {
        let c = spot(tussocks, i, size);
        let radius = tussocks.range(i, 3, 26.0, 58.0);
        let green = tussocks.range(i, 4, 0.0, 0.5);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = (radius * 6.0) as u64;
        for _ in 0..count {
            blade += 1;
            let angle = blades.unit(blade, 1) * 2.0 * PI;
            let start_r = blades.range(blade, 2, 0.0, 0.35) * radius;
            let start = [
                c[0] + start_r * math::cos(angle),
                c[1] + start_r * math::sin(angle),
            ];
            let length = radius * blades.range(blade, 3, 0.7, 1.25);
            let lean = angle + blades.range(blade, 4, -0.3, 0.3);
            let end = [
                start[0] + length * math::cos(lean),
                start[1] + length * math::sin(lean),
            ];
            let heart = 1.0 - start_r / radius;
            let base = blades.pick(blade, 5, &straw);
            let colour = mix(base, [0.20, 0.32, 0.10], green * heart);
            let colour = scale(colour, blades.range(blade, 6, 0.75, 1.15));
            let mid = [
                f64::midpoint(start[0], end[0]),
                f64::midpoint(start[1], end[1]),
            ];
            canvas.stroke(start, mid, 1.6, colour, 0.6 + 0.4 * heart);
            canvas.stroke(mid, end, 1.2, colour, 0.35 + 0.3 * heart);
        }
    }
}

fn draw_cushion(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let ground = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(ground, 6, 3, x, y, size);
        (mix([0.05, 0.045, 0.03], [0.12, 0.10, 0.07], n), 0.0)
    });
    // Cushion plants packed into firm mounds 3 to 12 cm across at 512
    // texels a metre: tiny rosettes of leaves, bright green, starred
    // with flowers here and there.
    let mounds = draws.stream(2);
    let rosettes = draws.stream(3);
    let greens = [
        [0.10, 0.26, 0.06],
        [0.14, 0.30, 0.07],
        [0.09, 0.22, 0.06],
        [0.16, 0.28, 0.09],
    ];
    for i in 0..700 {
        let c = spot(mounds, i, size);
        let radius = mounds.range(i, 3, 15.0, 60.0);
        let green = scale(mounds.pick(i, 4, &greens), mounds.range(i, 5, 0.8, 1.15));
        let flowers = mounds.unit(i, 6) > 0.8;
        canvas.dome(
            c,
            radius,
            radius * mounds.range(i, 7, 0.8, 1.0),
            mounds.unit(i, 8) * PI,
            0.0,
            1.0,
            |u, v, dome| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let (ix, iy) = ((u * 14.0 + 50.0) as u64, (v * 14.0 + 50.0) as u64);
                let rosette = rosettes.unit(ix * 1000 + iy, i);
                if flowers && rosette > 0.93 {
                    return [0.85, 0.80, 0.86];
                }
                scale(green, (0.55 + 0.5 * dome) * (0.8 + 0.35 * rosette))
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::texture::{to_byte, to_f64};

    #[test]
    fn looks_are_deterministic_and_average_to_their_means() {
        let looks = plant_looks(1);
        assert_eq!(looks.len(), PLANT_RECIPES.len());
        assert_eq!(looks, plant_looks(1));
        assert_ne!(looks[0].rgba, plant_looks(2)[0].rgba);
        for (look, name) in looks
            .iter()
            .zip(PLANT_RECIPES.iter().map(|recipe| recipe.name))
        {
            assert_eq!(look.name, name);
            assert_eq!(look.rgba.len(), GROUND_LOOK_SIZE * GROUND_LOOK_SIZE * 4);
            let texels = mip_chain(&look.rgba, GROUND_LOOK_SIZE);
            assert_eq!(texels.len(), 10);
            // The one texel of the last level is the texture's mean.
            let last = texels.last().unwrap();
            for (channel, &byte) in last.iter().enumerate().take(3) {
                let mean = srgb_to_linear(byte);
                let want = f64::from(look.mean[channel]);
                assert!(
                    (mean - want).abs() < 0.02 + 0.06 * want,
                    "{name}: channel {channel} averages {mean:.3}, wants {want:.3}"
                );
            }
            // Relief spans the alpha channel.
            let (texels, _) = look.rgba.as_chunks::<4>();
            let (low, high) = texels.iter().fold((255, 0), |(lo, hi), texel| {
                (lo.min(texel[3]), hi.max(texel[3]))
            });
            assert_eq!((low, high), (0, 255), "{name}");
        }
    }

    #[test]
    fn relief_slopes_are_how_the_relief_changes_over_a_tile() {
        // Relief rising and falling once along every row: half its span
        // times 2π cos along the rows, nothing down them.
        let size = 64;
        let mut rgba = vec![0_u8; size * size * 4];
        for y in 0..size {
            for x in 0..size {
                let phase = 2.0 * PI * to_f64(x) / to_f64(size);
                rgba[(y * size + x) * 4 + 3] = to_byte(0.5 + 0.5 * math::sin(phase));
            }
        }
        let levels = relief_slopes(&rgba, size);
        assert_eq!(levels.len(), 7);
        for (level, slopes) in levels.iter().enumerate() {
            assert_eq!(slopes.len(), (size >> level) * (size >> level));
        }
        for y in [0, 17, size - 1] {
            for x in 0..size {
                let want = PI * math::cos(2.0 * PI * to_f64(x) / to_f64(size));
                let [u, v] = levels[0][y * size + x];
                // A byte of relief is 1/255 of it; a step of one over two
                // texels is 0.125 per tile here.
                assert!((f64::from(u) - want).abs() < 0.15, "{x}: {u} wants {want}");
                assert!(v.abs() < 1.0e-6);
            }
        }
        // A coarser level is the mean of the finer.
        let [u, _] = levels[1][3];
        let finer = [6, 7, size + 6, size + 7].map(|at| levels[0][at][0]);
        assert!((u - finer.iter().sum::<f32>() / 4.0).abs() < 1.0e-6);
        // The looks wrap, so their slopes average to nothing.
        for look in plant_looks(5).iter().take(4) {
            let levels = relief_slopes(&look.rgba, GROUND_LOOK_SIZE);
            assert_eq!(levels.len(), 10);
            let [u, v] = levels[9][0];
            assert!(
                u.abs() < 1.0e-3 && v.abs() < 1.0e-3,
                "{}: {u} {v}",
                look.name
            );
        }
    }

    #[test]
    fn looks_tile_without_a_seam() {
        // Across the wrap, neighbouring texels differ no more than they do
        // anywhere else.
        for look in plant_looks(3) {
            let size = GROUND_LOOK_SIZE;
            let texel = |x: usize, y: usize| {
                let at = (y * size + x) * 4;
                [0, 1, 2, 3].map(|c| f64::from(look.rgba[at + c]))
            };
            // The mean change between column (or row) `i` and `j`.
            let step = |i: usize, j: usize, rows: bool| {
                (0..size)
                    .map(|k| {
                        let (a, b) = if rows {
                            (texel(k, i), texel(k, j))
                        } else {
                            (texel(i, k), texel(j, k))
                        };
                        (0..4).map(|c| (a[c] - b[c]).abs()).sum::<f64>()
                    })
                    .sum::<f64>()
                    / to_f64(size)
            };
            for (rows, edge) in [(false, "left and right"), (true, "top and bottom")] {
                let inside: f64 = (1..9).map(|k| step(k * 50, k * 50 + 1, rows)).sum::<f64>() / 8.0;
                let across = step(size - 1, 0, rows);
                assert!(
                    across < 2.0 * inside + 4.0,
                    "{}: {across:.1} across the {edge} edges, {inside:.1} inside",
                    look.name
                );
            }
        }
    }
}

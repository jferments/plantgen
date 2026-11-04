//! Stand tiles: where the plants of a stand stand, set by their crowns
//! (stands plan, steps 2 and 3). Nothing here grows a plant: each member
//! of a stand is a grown variant at one age whose size is known, and the
//! layout places copies of them on a square tile that wraps at its edges,
//! layer by layer from the canopy down, until each layer's crowns cover
//! the share of the ground its stand covers.
//!
//! Step 1 of the plan (plantgen-claude `project-files/stands/step1/`)
//! found After's closed conifer forest covering 31% of the ground seen
//! from above, against 60 to 96% in western Oregon stands past canopy
//! closure, rarely above 85% (McIntosh, Gray and Garman 2009,
//! PNW-GTR-794, 917 inventory plots), and about 70% in old growth at Wind
//! River. It placed a fixed number of stems a hectare whatever their size.
//! Here the number follows from the crowns: a stand of narrow crowns holds
//! more stems than a stand of wide ones, as yield tables find.
//!
//! How a plant is placed: tallest first, each member drawn by its share,
//! several places are tried and the one whose crown overlaps the crowns
//! already standing in its height band least is kept, if that overlap is
//! at most [`Rules::overlap`] of its crown. Crowns of canopy trees fill
//! the canopy's space and rarely grow into each other (Purves, Lichstein
//! and Pacala 2007, PLoS ONE 2:e870); a little overlap stands for the
//! crowns' irregular edges, which interlock. A plant below another's crown
//! base takes no room from it, but stands only where enough light passes
//! the crowns above for its shade tolerance. Every draw comes from hashes
//! of the seed, so a tile is the same on every machine.

use crate::niche::Layer;
use crate::rng::{hash_words, unit};

/// Edge of a cell of the cover raster, metres.
pub const CELL_M: f64 = 0.5;

/// Places tried for each plant before the least crowded is kept.
pub const TRIES: u32 = 16;

/// Plants in a row that find no room before a layer counts as full.
pub const GIVE_UP: u32 = 200;

/// How far a tree's crown may stand off its stem toward room, as a share
/// of its radius. Crowns grow toward gaps and away from their neighbours
/// (crown plasticity: Purves, Lichstein and Pacala 2007, PLoS ONE 2:e870;
/// Muth and Bazzaz 2003, Canadian Journal of Forest Research 33:1323), so
/// a canopy fills its space without its crowns growing into each other.
/// The share is this model's choice, kept small for conifers' strong
/// apical control.
pub const PLASTICITY: f64 = 0.35;

/// One member of a stand: a grown variant at one age.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    /// The caller's own number for it (a prototype's index).
    pub id: u32,
    pub layer: Layer,
    /// Its share of its layer's plants, relative to the layer's others.
    pub share: f64,
    pub height_m: f64,
    /// Height of its lowest living branch, metres.
    pub crown_base_m: f64,
    /// Its crown's widest reach from the stem, metres.
    pub crown_radius_m: f64,
    /// Share of full light, 0 to 1, it needs to stand: under a crown
    /// that lets less through, it does not.
    pub light_needed: f64,
}

/// How full a stand tile is.
#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    /// Edge of the square tile, metres; it wraps at its edges.
    pub tile_m: f64,
    /// Share of the ground each layer's crowns cover seen from above, by
    /// [`Layer::ALL`]'s order: canopy, subcanopy, shrub, herb.
    pub cover: [f64; 4],
    /// Most of a crown's area that may overlap crowns in its height band.
    pub overlap: f64,
    /// Share of light a crown lets through below it.
    pub crown_transmission: f64,
    /// Scale range of a copy of a member.
    pub scale: [f64; 2],
    /// How far a tree's crown may stand off its stem, as a share of its
    /// radius ([`PLASTICITY`]); shrubs and herbs keep theirs over their
    /// feet.
    pub plasticity: f64,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            tile_m: 40.0,
            // A closed stand past canopy closure (McIntosh et al. 2009:
            // 60-96%, rarely above 85%); its layers below are the model's
            // choice until sourced.
            cover: [0.8, 0.3, 0.3, 0.0],
            overlap: 0.25,
            crown_transmission: 0.1,
            scale: [0.85, 1.15],
            plasticity: PLASTICITY,
        }
    }
}

/// One plant standing on the tile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// Index of its member in the list laid out.
    pub member: usize,
    /// Metres east and north of the tile's south-west corner.
    pub at_m: [f64; 2],
    pub yaw_deg: f64,
    pub scale: f64,
    /// Where its crown's centre stands, metres east and north on the tile
    /// (wrapped): off its stem by at most [`Rules::plasticity`] of its
    /// radius, toward room. A host leans the plant so.
    pub crown_m: [f64; 2],
}

impl Placed {
    /// How far its crown stands off its stem, metres east and north, by
    /// the shorter way round the wrapping tile.
    #[must_use]
    pub fn crown_offset(&self, tile_m: f64) -> [f64; 2] {
        let short = |d: f64| d - tile_m * libm::round(d / tile_m);
        [
            short(self.crown_m[0] - self.at_m[0]),
            short(self.crown_m[1] - self.at_m[1]),
        ]
    }
}

/// The crowns standing so far, on a raster of [`CELL_M`] cells that wraps.
struct Raster {
    side: usize,
    /// For each cell, the crowns over it: (crown base, top) in metres.
    crowns: Vec<Vec<(f32, f32)>>,
    /// For each cell, a bit for each layer with a crown over it.
    layers: Vec<u8>,
    /// Cells under each layer's crowns, and under any crown.
    counts: [usize; 4],
    any: usize,
}

impl Raster {
    fn new(tile_m: f64) -> Self {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let side = (tile_m / CELL_M).round().max(1.0) as usize;
        Self {
            side,
            crowns: vec![Vec::new(); side * side],
            layers: vec![0; side * side],
            counts: [0; 4],
            any: 0,
        }
    }

    /// The cells of a crown of `radius` round `at`, wrapped.
    fn cells(&self, at: [f64; 2], radius: f64) -> Vec<usize> {
        #[allow(clippy::cast_precision_loss)]
        let side = self.side as f64;
        let reach = (radius / CELL_M).ceil();
        let (cx, cy) = (at[0] / CELL_M, at[1] / CELL_M);
        let mut out = Vec::new();
        #[allow(clippy::cast_possible_truncation)]
        let (low_x, high_x, low_y, high_y) = (
            (cx - reach).floor() as i64,
            (cx + reach).ceil() as i64,
            (cy - reach).floor() as i64,
            (cy + reach).ceil() as i64,
        );
        let r2 = (radius / CELL_M) * (radius / CELL_M);
        for y in low_y..high_y {
            for x in low_x..high_x {
                #[allow(clippy::cast_precision_loss)]
                let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
                if dx * dx + dy * dy > r2 {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let (wx, wy) = (
                    x.rem_euclid(side as i64) as usize,
                    y.rem_euclid(side as i64) as usize,
                );
                out.push(wy * self.side + wx);
            }
        }
        out
    }

    /// Share of `cells` already holding a crown whose height band meets
    /// [`base`, `top`], and the mean light reaching `top` through the
    /// crowns above it.
    fn crowding(&self, cells: &[usize], base: f64, top: f64, transmission: f64) -> (f64, f64) {
        if cells.is_empty() {
            return (0.0, 1.0);
        }
        let (mut crowded, mut light) = (0.0, 0.0);
        for &cell in cells {
            let mut meets = false;
            let mut through = 1.0;
            for &(low, high) in &self.crowns[cell] {
                let (low, high) = (f64::from(low), f64::from(high));
                if low < top && base < high {
                    meets = true;
                }
                if low >= top {
                    through *= transmission;
                }
            }
            if meets {
                crowded += 1.0;
            }
            light += through;
        }
        #[allow(clippy::cast_precision_loss)]
        let count = cells.len() as f64;
        (crowded / count, light / count)
    }

    fn stamp(&mut self, cells: &[usize], base: f64, top: f64, layer: usize) {
        #[allow(clippy::cast_possible_truncation)]
        let band = (base as f32, top as f32);
        let bit = 1_u8 << layer;
        for &cell in cells {
            self.crowns[cell].push(band);
            if self.layers[cell] == 0 {
                self.any += 1;
            }
            if self.layers[cell] & bit == 0 {
                self.counts[layer] += 1;
            }
            self.layers[cell] |= bit;
        }
    }

    /// Share of cells under a crown of `layer` (any layer when `None`).
    fn cover(&self, layer: Option<usize>) -> f64 {
        let covered = layer.map_or(self.any, |layer| self.counts[layer]);
        #[allow(clippy::cast_precision_loss)]
        let share = covered as f64 / self.crowns.len() as f64;
        share
    }
}

/// Lay `members` out on a tile by `rules`, from `seed`.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn lay_out(members: &[Member], rules: &Rules, seed: u64) -> Vec<Placed> {
    let mut raster = Raster::new(rules.tile_m);
    let mut placed = Vec::new();
    let mut draw = 0_u64;
    let mut next = |purpose: u64| {
        draw += 1;
        unit(hash_words(&[seed, purpose, draw]))
    };
    for (layer_index, layer) in Layer::ALL.iter().enumerate() {
        let target = rules.cover[layer_index].clamp(0.0, 1.0);
        let pool: Vec<usize> = (0..members.len())
            .filter(|&i| members[i].layer == *layer && members[i].share > 0.0)
            .collect();
        if target <= 0.0 || pool.is_empty() {
            continue;
        }
        let total: f64 = pool.iter().map(|&i| members[i].share).sum();
        let mut misses = 0;
        // Taller members first within a layer: draw a batch, then place it
        // tallest first, so short plants fill what the tall leave.
        while misses < GIVE_UP {
            let mut batch: Vec<(usize, f64)> = (0..32)
                .map(|_| {
                    let mut pick = next(1) * total;
                    let mut chosen = pool[pool.len() - 1];
                    for &i in &pool {
                        pick -= members[i].share;
                        if pick <= 0.0 {
                            chosen = i;
                            break;
                        }
                    }
                    let scale = rules.scale[0] + (rules.scale[1] - rules.scale[0]) * next(2);
                    (chosen, scale)
                })
                .collect();
            batch.sort_by(|a, b| {
                (members[b.0].height_m * b.1).total_cmp(&(members[a.0].height_m * a.1))
            });
            for (index, scale) in batch {
                if raster.cover(Some(layer_index)) >= target {
                    misses = GIVE_UP;
                    break;
                }
                let member = &members[index];
                let (radius, base, top) = (
                    member.crown_radius_m * scale,
                    member.crown_base_m * scale,
                    member.height_m * scale,
                );
                // A tree's crown may stand off its stem toward room.
                let reach = if layer_index < 2 {
                    rules.plasticity.max(0.0) * radius
                } else {
                    0.0
                };
                let offsets = crown_offsets(reach);
                let mut best: Option<([f64; 2], [f64; 2], Vec<usize>, f64)> = None;
                for _ in 0..TRIES {
                    let at = [next(3) * rules.tile_m, next(4) * rules.tile_m];
                    for (offset, off_by) in &offsets {
                        let crown = [
                            (at[0] + offset[0]).rem_euclid(rules.tile_m),
                            (at[1] + offset[1]).rem_euclid(rules.tile_m),
                        ];
                        let cells = raster.cells(crown, radius.max(CELL_M * 0.5));
                        let (crowded, light) =
                            raster.crowding(&cells, base, top, rules.crown_transmission);
                        if light < member.light_needed {
                            continue;
                        }
                        // Of two crowns as crowded, the one nearer its stem.
                        let score = crowded + 0.01 * off_by;
                        if best.as_ref().is_none_or(|b| score < b.3) {
                            best = Some((at, crown, cells, score));
                        }
                    }
                }
                match best {
                    Some((at, crown, cells, score)) if score <= rules.overlap + 0.01 => {
                        raster.stamp(&cells, base, top, layer_index);
                        placed.push(Placed {
                            member: index,
                            at_m: at,
                            yaw_deg: next(5) * 360.0,
                            scale,
                            crown_m: crown,
                        });
                        misses = 0;
                    }
                    _ => misses += 1,
                }
            }
        }
    }
    placed
}

/// The places a crown may stand off its stem, up to `reach` metres: over
/// it, and at half and all of the reach in eight directions, each with its
/// share of the reach.
fn crown_offsets(reach: f64) -> Vec<([f64; 2], f64)> {
    let mut offsets = vec![([0.0, 0.0], 0.0)];
    if reach > 0.0 {
        for share in [0.5, 1.0] {
            for step in 0..8 {
                let angle = f64::from(step) * std::f64::consts::FRAC_PI_4;
                offsets.push((
                    [
                        share * reach * libm::cos(angle),
                        share * reach * libm::sin(angle),
                    ],
                    share,
                ));
            }
        }
    }
    offsets
}

/// Share of the tile's ground covered by any crown of `placed`, seen
/// straight down, on the layout's raster.
#[must_use]
pub fn cover(members: &[Member], placed: &[Placed], tile_m: f64) -> f64 {
    let mut raster = Raster::new(tile_m);
    for plant in placed {
        let member = &members[plant.member];
        let cells = raster.cells(plant.crown_m, member.crown_radius_m * plant.scale);
        raster.stamp(&cells, 0.0, 1.0, 0);
    }
    raster.cover(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(id: u32, layer: Layer, height: f64, radius: f64) -> Member {
        Member {
            id,
            layer,
            share: 1.0,
            height_m: height,
            crown_base_m: height * 0.5,
            crown_radius_m: radius,
            light_needed: 0.0,
        }
    }

    #[test]
    fn a_layer_fills_to_its_cover_and_stems_follow_crown_size() {
        let rules = Rules {
            cover: [0.8, 0.0, 0.0, 0.0],
            ..Rules::default()
        };
        let narrow = [tree(0, Layer::Canopy, 30.0, 2.5)];
        let wide = [tree(0, Layer::Canopy, 30.0, 5.0)];
        let a = lay_out(&narrow, &rules, 1);
        let b = lay_out(&wide, &rules, 1);
        let (ca, cb) = (cover(&narrow, &a, 40.0), cover(&wide, &b, 40.0));
        assert!((0.75..0.9).contains(&ca), "narrow crowns cover {ca}");
        assert!((0.7..0.9).contains(&cb), "wide crowns cover {cb}");
        // Crown area a quarter as large: about four times the stems.
        assert!(a.len() > b.len() * 3, "{} against {}", a.len(), b.len());
    }

    #[test]
    fn a_tile_is_the_same_for_a_seed_and_differs_between_seeds() {
        let members = [
            tree(0, Layer::Canopy, 40.0, 4.0),
            tree(1, Layer::Subcanopy, 12.0, 2.0),
        ];
        let rules = Rules::default();
        assert_eq!(lay_out(&members, &rules, 5), lay_out(&members, &rules, 5));
        assert_ne!(lay_out(&members, &rules, 5), lay_out(&members, &rules, 6));
    }

    #[test]
    fn shade_intolerant_plants_stay_out_from_under_a_closed_canopy() {
        let mut low = tree(1, Layer::Subcanopy, 8.0, 1.5);
        low.light_needed = 0.9;
        let members = [tree(0, Layer::Canopy, 40.0, 4.0), low];
        let rules = Rules {
            cover: [0.8, 0.3, 0.0, 0.0],
            ..Rules::default()
        };
        let placed = lay_out(&members, &rules, 3);
        let under = placed.iter().filter(|p| p.member == 1).count();
        let canopy = placed.iter().filter(|p| p.member == 0).count();
        assert!(canopy > 0);
        // Only the gaps of the canopy take a plant that needs nearly full
        // light: under half what it would hold in the open.
        let open = lay_out(&members[1..], &Rules::default(), 3).len();
        assert!(
            under * 2 < open,
            "{under} under the canopy, {open} in the open"
        );
    }

    #[test]
    fn crowns_lean_into_room_and_fill_more_of_the_canopy() {
        let members = [tree(0, Layer::Canopy, 40.0, 4.0)];
        let full = Rules {
            cover: [0.99, 0.0, 0.0, 0.0],
            ..Rules::default()
        };
        let rigid = Rules {
            plasticity: 0.0,
            ..full.clone()
        };
        let leaning = lay_out(&members, &full, 2);
        let upright = lay_out(&members, &rigid, 2);
        assert!(upright.iter().all(|p| p.crown_offset(40.0) == [0.0, 0.0]));
        let offsets: Vec<f64> = leaning
            .iter()
            .map(|p| libm::hypot(p.crown_offset(40.0)[0], p.crown_offset(40.0)[1]))
            .collect();
        assert!(offsets.iter().any(|d| *d > 0.1));
        assert!(offsets.iter().all(|d| *d <= PLASTICITY * 4.0 * 1.15 + 1e-9));
        // Crowns that move toward room close more of the canopy.
        let (lean_cover, rigid_cover) = (
            cover(&members, &leaning, 40.0),
            cover(&members, &upright, 40.0),
        );
        assert!(
            lean_cover > rigid_cover,
            "{lean_cover} against {rigid_cover}"
        );
    }
}

//! Stands: plants that grow together (fitting plan PF5, `PlantGen` PG4).
//!
//! Each plant of a stand grows on its own [`Grower`], all of them a step
//! at a time on one clock. After each step every plant's crown
//! ([`Crown`]) becomes an envelope round its foot that its neighbours grow
//! among from the next step on: it shades them and takes its room
//! (`crate::conditions::Envelope`). A stand may be a tile that wraps: a
//! plant near one edge has the plants near the opposite edge as
//! neighbours, so tiles laid side by side meet without a seam.
//!
//! The plants step in parallel, but what each grows depends only on the
//! crowns the step before left, so a stand grows the same however many
//! threads grow it and in whatever order its plants are listed.

use crate::conditions::{Crown, Envelope, sort_envelopes};
use crate::grow::{Grower, Growth, GrowthSettings};
use crate::lsys::{GrowthError, Program};

/// One plant of a stand.
#[derive(Debug, Clone, Copy)]
pub struct Member<'a> {
    pub program: &'a Program,
    pub params: &'a [f64],
    /// Its own settings: seed, years, keyframes, and the conditions it
    /// grows in besides its neighbours (its place).
    pub settings: &'a GrowthSettings,
    /// Where its foot stands in the stand, metres east and north.
    pub at_m: [f64; 2],
}

/// Grow `members` together. With `tile_m` (east, north) the stand is a
/// tile that wraps at its edges, its plants' feet anywhere in it.
///
/// # Errors
///
/// When there are no members, their steps differ in length, one has a
/// layout (`crate::conditions::Layout`), the tile is not positive, a foot
/// stands outside the tile, or a member's growth fails.
pub fn grow_stand(
    members: &[Member<'_>],
    tile_m: Option<[f64; 2]>,
) -> Result<Vec<Growth>, GrowthError> {
    check(members, tile_m)?;
    let mut growers = members
        .iter()
        .map(|member| {
            Grower::new(member.program, member.params, member.settings).map(Grower::with_crown)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let own: Vec<Vec<Envelope>> = members
        .iter()
        .map(|member| member.settings.conditions.envelopes().to_vec())
        .collect();
    while growers.iter().any(|grower| !grower.is_over()) {
        let crowns: Vec<Option<Crown>> = growers.iter().map(Grower::crown).collect();
        for (index, grower) in growers.iter_mut().enumerate() {
            let mut envelopes = own[index].clone();
            envelopes.extend(neighbours(members, &crowns, index, tile_m));
            grower.set_envelopes(envelopes);
        }
        let stepped: Vec<Result<bool, GrowthError>> = std::thread::scope(|scope| {
            let handles: Vec<_> = growers
                .iter_mut()
                .map(|grower| scope.spawn(move || grower.step(None)))
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
        for result in stepped {
            result?;
        }
    }
    Ok(growers.into_iter().map(Grower::finish).collect())
}

fn check(members: &[Member<'_>], tile_m: Option<[f64; 2]>) -> Result<(), GrowthError> {
    let fail = |message: String| Err(GrowthError::Settings(message));
    let Some(first) = members.first() else {
        return fail("a stand needs at least one plant".into());
    };
    if members
        .iter()
        .any(|member| member.settings.dt.to_bits() != first.settings.dt.to_bits())
    {
        return fail("a stand's plants must all grow in steps of the same length".into());
    }
    if let Some(tile) = tile_m
        && !tile.iter().all(|side| side.is_finite() && *side > 0.0)
    {
        return fail(format!("a stand's tile must be positive, found {tile:?}"));
    }
    for member in members {
        if member.settings.conditions.layout().is_some() {
            return fail(
                "a stand's plants are their own neighbours; none may have a layout".into(),
            );
        }
        let [east, north] = member.at_m;
        if !(east.is_finite() && north.is_finite()) {
            return fail(format!("a plant stands at {:?}", member.at_m));
        }
        if let Some([width, depth]) = tile_m
            && !((0.0..=width).contains(&east) && (0.0..=depth).contains(&north))
        {
            return fail(format!(
                "a plant at {:?} stands outside the tile {width} by {depth} m",
                member.at_m
            ));
        }
    }
    Ok(())
}

/// The crowns round plant `index` that can shade it or reach its crown,
/// as envelopes in its frame: every other plant's, and in a tile every
/// plant's in the tiles round it too, its own included.
fn neighbours(
    members: &[Member<'_>],
    crowns: &[Option<Crown>],
    index: usize,
    tile_m: Option<[f64; 2]>,
) -> Vec<Envelope> {
    let here = members[index].at_m;
    let reach = crowns[index].map_or(0.0, |crown| crown.radius_m);
    let images: &[[f64; 2]] = match tile_m {
        Some(_) => &[
            [0.0, 0.0],
            [-1.0, -1.0],
            [-1.0, 0.0],
            [-1.0, 1.0],
            [0.0, -1.0],
            [0.0, 1.0],
            [1.0, -1.0],
            [1.0, 0.0],
            [1.0, 1.0],
        ],
        None => &[[0.0, 0.0]],
    };
    let [width, depth] = tile_m.unwrap_or([0.0, 0.0]);
    let mut envelopes = Vec::new();
    for (other, (member, crown)) in members.iter().zip(crowns).enumerate() {
        let Some(crown) = crown else {
            continue;
        };
        for [east, north] in images {
            if other == index && *east == 0.0 && *north == 0.0 {
                continue;
            }
            // The two plants' offset first, then the tile's: the middle
            // plant's own images then stand exactly a tile away, as a
            // layout's neighbours do.
            let centre = [
                (member.at_m[0] - here[0]) + east * width,
                (member.at_m[1] - here[1]) + north * depth,
            ];
            if crown.reaches(centre, reach) {
                envelopes.push(crown.envelope(centre));
            }
        }
    }
    sort_envelopes(&mut envelopes);
    envelopes
}

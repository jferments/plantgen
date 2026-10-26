// How a baked card turns to the eye in a vertex shader: the reference for
// `plantgen::raster::turn_cards`, which draws PlantGen's own renders of the
// card tiers (`plantgen::billboards`). An engine includes this file in its
// vertex shader and defines
//
//     fn card_picture(index: u32) -> CardPicture
//
// reading the tier's pictures, in the package's order, from its own
// buffer. Each card vertex then goes through `turn_card` with its position,
// normal and picture (`card-mesh`), and the result's picture picks the
// atlas rectangle its uv is read from. `tests/turn_cards.json` holds
// vectors from the Rust; within 1e-4 of them, this file and an engine's
// copy of it draw the cards as PlantGen does.

// A tier's picture as the package's manifest records it (`PictureRecord`).
struct CardPicture {
    // The point its card turns about, in the plant's frame.
    pivot: vec3<f32>,
    // 0: it stays as baked. 1: it faces the eye. 2: it faces the eye and
    // shows, of `count` pictures from `first`, the one baked from nearest
    // the eye's way (an impostor). 3: likewise, the one baked from nearest
    // the eye's height (a crown's card).
    kind: u32,
    // How far what it shows spreads, root mean square, along its card's
    // right, up and normal as baked.
    extent: vec3<f32>,
    first: u32,
    // The way toward the eye it was baked from.
    seen_from: vec3<f32>,
    count: u32,
};

struct TurnedCard {
    position: vec3<f32>,
    normal: vec3<f32>,
    // The picture shown: the vertex's own, or another of its views.
    picture: u32,
};

const CARD_FIXED: u32 = 0u;
const CARD_FACING: u32 = 1u;
const CARD_VIEWS: u32 = 2u;
const CARD_HEIGHTS: u32 = 3u;

// A picture baked from higher than this sine of elevation was baked from
// straight above, and keeps its frame on the ground.
const CARD_STRAIGHT_UP: f32 = 0.999;

fn card_normalize_or(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    let n = length(v);
    if (n > 1e-12) {
        return v / n;
    }
    return fallback;
}

fn card_any_perpendicular(v: vec3<f32>) -> vec3<f32> {
    var helper = vec3<f32>(0.0, 0.0, 1.0);
    if (abs(v.x) < 0.9) {
        helper = vec3<f32>(1.0, 0.0, 0.0);
    }
    return card_normalize_or(cross(v, helper), vec3<f32>(0.0, 0.0, 1.0));
}

// The up of a card facing `normal`: the world's up laid on it, else
// `fallback` laid on it, else any; its right is up × normal.
fn card_up(normal: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    let world = vec3<f32>(0.0, 1.0, 0.0) - normal * normal.y;
    if (length(world) > 1e-12) {
        return normalize(world);
    }
    let laid = fallback - normal * dot(fallback, normal);
    if (length(laid) > 1e-12) {
        return normalize(laid);
    }
    return card_any_perpendicular(normal);
}

fn card_elevation(d: vec3<f32>) -> f32 {
    return asin(clamp(card_normalize_or(d, vec3<f32>(0.0, 0.0, 1.0)).y, -1.0, 1.0));
}

// How wide what a picture shows is along `axis`, as `card_turn` narrows
// a card turned to an eye beside it.
fn card_across(axis: vec3<f32>, right: vec3<f32>, up: vec3<f32>, normal: vec3<f32>, extent: vec3<f32>) -> f32 {
    let a = dot(axis, right) * extent.x;
    let b = dot(axis, up) * extent.y;
    let c = dot(axis, normal) * extent.z;
    return sqrt(a * a + b * b + c * c);
}

// Vertex `position` of a card facing `normal` and showing `picture`, seen
// from `eye` looking along `forward`: perspective, the card faces the eye
// itself; orthographic, it faces back along `forward`.
fn turn_card(
    position: vec3<f32>,
    normal_in: vec3<f32>,
    picture: u32,
    eye: vec3<f32>,
    forward: vec3<f32>,
    perspective: bool,
) -> TurnedCard {
    let own = card_picture(picture);
    var out = TurnedCard(position, normal_in, picture);
    if (own.kind == CARD_FIXED) {
        return out;
    }
    let pivot = own.pivot;
    var toward = -forward;
    if (perspective) {
        toward = card_normalize_or(eye - pivot, -forward);
    }
    // The card's own frame, as it was baked.
    let normal = card_normalize_or(normal_in, vec3<f32>(0.0, 0.0, 1.0));
    let up = card_up(normal, vec3<f32>(0.0, 0.0, 1.0));
    let right = cross(up, normal);
    let facing = toward;
    // The up, way and spread of the picture shown.
    var up_then = up;
    var normal_then = normal;
    var extent = own.extent;
    var from_above = false;
    if (own.kind == CARD_HEIGHTS) {
        let eye_height = card_elevation(toward);
        var best = own.first;
        var off = 1e30;
        for (var v = own.first; v < own.first + own.count; v++) {
            let o = abs(card_elevation(card_picture(v).seen_from) - eye_height);
            // The first of equals.
            if (o < off) {
                off = o;
                best = v;
            }
        }
        let shown = card_picture(best);
        out.picture = best;
        let way = card_normalize_or(shown.seen_from, normal);
        up_then = cross(way, right);
        normal_then = way;
        extent = shown.extent;
        from_above = way.y > CARD_STRAIGHT_UP;
    } else if (own.kind == CARD_VIEWS) {
        var best = own.first;
        var along = -1e30;
        for (var v = own.first; v < own.first + own.count; v++) {
            let a = dot(card_picture(v).seen_from, toward);
            // The last of equals.
            if (a >= along) {
                along = a;
                best = v;
            }
        }
        out.picture = best;
    }
    var up_now: vec3<f32>;
    var right_now: vec3<f32>;
    if (from_above) {
        // Seen from above, a picture baked from above lies on the ground as
        // it was baked, and turns as the eye goes round.
        up_now = card_normalize_or(up_then - facing * dot(up_then, facing), up_then);
        right_now = cross(up_now, facing);
    } else {
        up_now = card_up(facing, up);
        right_now = cross(up_now, facing);
    }
    // A clump spreads less through its depth than across: turned to an
    // eye beside it, its card narrows to what the eye would see.
    var scale_right = 1.0;
    if (extent.x > 1e-9) {
        scale_right = min(card_across(right_now, right, up_then, normal_then, extent) / extent.x, 1.0);
    }
    var scale_up = 1.0;
    if (extent.y > 1e-9) {
        scale_up = min(card_across(up_now, right, up_then, normal_then, extent) / extent.y, 1.0);
    }
    let offset = position - pivot;
    out.position = pivot
        + right_now * (dot(offset, right) * scale_right)
        + up_now * (dot(offset, up) * scale_up);
    out.normal = facing;
    return out;
}

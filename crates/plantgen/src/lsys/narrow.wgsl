// Space colonization's per-point work narrowed in 32-bit floats with a
// safety margin; the CPU decides every point in 64-bit (see narrow.rs, whose
// `on_cpu` does the same as this shader).

struct Settings {
    count: u32,
    kill_sq: f32,
    influence_sq: f32,
    cone: f32,
    margin_sq: f32,
    margin_facing: f32,
    near_sq: f32,
    unused: u32,
    plant_dims: vec4<i32>,
    apex_dims: vec4<i32>,
};

@group(0) @binding(0) var<uniform> settings: Settings;
@group(0) @binding(1) var<storage, read> points: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> plant_keys: array<vec4<i32>>;
@group(0) @binding(3) var<storage, read> apex_keys: array<vec4<i32>>;
@group(0) @binding(4) var<storage, read> plant_starts: array<u32>;
@group(0) @binding(5) var<storage, read> plant_items: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> apex_starts: array<u32>;
// Two per bud: its position, then its heading.
@group(0) @binding(7) var<storage, read> apex_items: array<vec4<f32>>;
@group(0) @binding(8) var<storage, read_write> narrowed: array<u32>;

const CANDIDATES: u32 = 8u;
const STRIDE: u32 = 10u;
const CONSUMED: u32 = 1u;
const LEFT: u32 = 2u;
const GROUP: u32 = 64u;

// The cell's index, or -1 if the grid does not hold it.
fn cell(key: vec3<i32>, dims: vec4<i32>) -> i32 {
    if (key.x < 0 || key.y < 0 || key.z < 0 || key.x >= dims.x || key.y >= dims.y || key.z >= dims.z) {
        return -1;
    }
    return (key.y * dims.z + key.z) * dims.x + key.x;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = id.x + id.y * groups.x * GROUP;
    if (index >= settings.count) {
        return;
    }
    let point = points[index].xyz;
    let base = index * STRIDE;

    // The nearest plant point in the 27 cells round the point's.
    var nearest = 3.0e38;
    let plant_key = plant_keys[index].xyz;
    for (var dz = -1; dz <= 1; dz++) {
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let at = cell(plant_key + vec3<i32>(dx, dy, dz), settings.plant_dims);
                if (at < 0) {
                    continue;
                }
                for (var item = plant_starts[at]; item < plant_starts[at + 1]; item++) {
                    let d = plant_items[item].xyz - point;
                    nearest = min(nearest, dot(d, d));
                }
            }
        }
    }
    var status = 0u;
    if (nearest <= settings.kill_sq - settings.margin_sq) {
        status = CONSUMED;
    } else if (nearest > settings.kill_sq + settings.margin_sq) {
        status = LEFT;
    }
    narrowed[base] = status;
    if (status == CONSUMED) {
        narrowed[base + 1u] = 0u;
        return;
    }

    // One walk over the buds near the point: the CANDIDATES nearest that
    // may take it, sorted by squared distance (a later bud after an equal
    // one), and the nearest that certainly does.
    let apex_key = apex_keys[index].xyz;
    var sure = 3.0e38;
    var kept_d2: array<f32, 8>;
    var kept_bud: array<u32, 8>;
    var kept = 0u;
    var dropped = false;
    for (var dz = -1; dz <= 1; dz++) {
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let at = cell(apex_key + vec3<i32>(dx, dy, dz), settings.apex_dims);
                if (at < 0) {
                    continue;
                }
                for (var bud = apex_starts[at]; bud < apex_starts[at + 1]; bud++) {
                    let d = point - apex_items[2u * bud].xyz;
                    let d2 = dot(d, d);
                    if (d2 > settings.influence_sq + settings.margin_sq) {
                        continue;
                    }
                    if (d2 >= settings.near_sq) {
                        let facing = dot(apex_items[2u * bud + 1u].xyz, d) / sqrt(d2);
                        if (facing < settings.cone - settings.margin_facing) {
                            continue;
                        }
                        if (facing >= settings.cone + settings.margin_facing && d2 <= settings.influence_sq - settings.margin_sq) {
                            sure = min(sure, d2);
                        }
                    }
                    if (kept == CANDIDATES) {
                        dropped = true;
                        if (d2 >= kept_d2[CANDIDATES - 1u]) {
                            continue;
                        }
                    } else {
                        kept += 1u;
                    }
                    var slot = kept - 1u;
                    while (slot > 0u && kept_d2[slot - 1u] > d2) {
                        kept_d2[slot] = kept_d2[slot - 1u];
                        kept_bud[slot] = kept_bud[slot - 1u];
                        slot -= 1u;
                    }
                    kept_d2[slot] = d2;
                    kept_bud[slot] = bud;
                }
            }
        }
    }
    // The candidates: those kept no farther than the sure one plus twice
    // the margin. If a bud was dropped and every kept one is that near, a
    // dropped one may be too: the full search decides.
    let limit = sure + 2.0 * settings.margin_sq;
    var count = 0u;
    while (count < kept && kept_d2[count] <= limit) {
        count += 1u;
    }
    if (dropped && count == kept) {
        narrowed[base + 1u] = CANDIDATES + 1u;
        return;
    }
    for (var slot = 0u; slot < count; slot++) {
        narrowed[base + 2u + slot] = kept_bud[slot];
    }
    narrowed[base + 1u] = count;
}

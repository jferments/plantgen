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

    // The nearest bud that certainly perceives the point.
    let apex_key = apex_keys[index].xyz;
    var sure = 3.0e38;
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
                    if (d2 < settings.near_sq || d2 > settings.influence_sq - settings.margin_sq) {
                        continue;
                    }
                    let facing = dot(apex_items[2u * bud + 1u].xyz, d) / sqrt(d2);
                    if (facing >= settings.cone + settings.margin_facing) {
                        sure = min(sure, d2);
                    }
                }
            }
        }
    }
    // Every bud that may be the nearest that perceives it.
    var count = 0u;
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
                    if (d2 > settings.influence_sq + settings.margin_sq || d2 > sure + 2.0 * settings.margin_sq) {
                        continue;
                    }
                    if (d2 >= settings.near_sq) {
                        let facing = dot(apex_items[2u * bud + 1u].xyz, d) / sqrt(d2);
                        if (facing < settings.cone - settings.margin_facing) {
                            continue;
                        }
                    }
                    if (count < CANDIDATES) {
                        narrowed[base + 2u + count] = bud;
                    }
                    count += 1u;
                }
            }
        }
    }
    narrowed[base + 1u] = count;
}

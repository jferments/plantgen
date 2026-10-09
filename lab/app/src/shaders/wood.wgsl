// PlantLab's wood: a standard material in the wood's vertex colours, with
// the species' bark pattern drawn on it pixel by pixel. The functions below
// are PlantGen's `bark::sample` in f32, operation for operation, as every
// renderer of PlantGen's plants runs it: the pattern at `u` round a stem
// (the first texture coordinate's x), `v` metres along it (its y), on a
// stem of radius `r` (the second texture coordinate's x; 0 off bark), for
// a pixel `footprint` metres across. The sample darkens or lightens the
// colour, turns it toward the peeled inner bark, and tilts the normal by
// the relief's slope round and along the stem, as `plantgen::raster` does.

#import bevy_pbr::pbr_fragment::pbr_input_from_standard_material
#import bevy_pbr::mesh_view_bindings::view
#import bevy_pbr::forward_io::{VertexOutput, FragmentOutput}
#import bevy_pbr::pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing}

// `plantgen::bark::BarkParams`.
struct Bark {
    inner: vec4<f32>,
    kind: u32,
    scale: f32,
    elongation: f32,
    fissure: f32,
    depth: f32,
    contrast: f32,
    variation: f32,
    mean: f32,
    smooth_below: f32,
    lenticels: f32,
    peeled: f32,
    roughness: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> bark: Bark;

// BarkKind's numbers, and bark.rs's salts.
const BARK_SCALY: u32 = 4u;
const BARK_SMOOTH: u32 = 5u;
const BARK_PEELING: u32 = 6u;
const BARK_CELL_SALT: u32 = 0x68e31da4u;
const BARK_SHADE_SALT: u32 = 0x5bd1e995u;
const BARK_PEEL_SALT: u32 = 0x2c1b3c6du;
const BARK_MOTTLE_SALT: u32 = 0x297a2d39u;
const BARK_LENTICEL_SALT: u32 = 0x7feb352du;
const BARK_GRAIN_SALT: u32 = 0x1b873593u;
const BARK_FINE_SALT: u32 = 0xcc9e2d51u;
const BARK_TAU: f32 = 6.2831855;

// PlantGen's `spines::pcg`.
fn pcg(value: u32) -> u32 {
    let state = value * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn bark_unit(hash: u32) -> f32 {
    return f32(hash >> 8u) / 16777216.0;
}

fn bark_hash(cell: vec3<i32>, salt: u32) -> u32 {
    return pcg(bitcast<u32>(cell.x) ^ pcg(bitcast<u32>(cell.y) ^ pcg(bitcast<u32>(cell.z) ^ salt)));
}

struct Cells {
    near: f32,
    second: f32,
    near_point: vec3<f32>,
    second_point: vec3<f32>,
    id: u32,
}

// bark.rs `cells`.
fn bark_cells(q: vec3<f32>, salt: u32) -> Cells {
    let base = vec3<i32>(floor(q));
    var found: Cells;
    found.near = 1.0e9;
    found.second = 1.0e9;
    found.near_point = q;
    found.second_point = q;
    found.id = 0u;
    for (var dz = -1; dz <= 1; dz++) {
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let cell = base + vec3<i32>(dx, dy, dz);
                let hash = bark_hash(cell, salt);
                let a = pcg(hash);
                let b = pcg(a);
                let point = vec3<f32>(cell) + 0.1
                    + 0.8 * vec3<f32>(bark_unit(hash), bark_unit(a), bark_unit(b));
                let distance = length(q - point);
                if distance < found.near {
                    found.second = found.near;
                    found.second_point = found.near_point;
                    found.near = distance;
                    found.near_point = point;
                    found.id = hash;
                } else if distance < found.second {
                    found.second = distance;
                    found.second_point = point;
                }
            }
        }
    }
    return found;
}

// bark.rs `noise`.
fn bark_noise(q: vec3<f32>, salt: u32) -> f32 {
    let base = vec3<i32>(floor(q));
    let f = q - floor(q);
    let w = f * f * (3.0 - 2.0 * f);
    var corner: array<f32, 8>;
    for (var k = 0; k < 8; k++) {
        corner[k] = bark_unit(bark_hash(base + vec3<i32>(k & 1, (k >> 1u) & 1, (k >> 2u) & 1), salt));
    }
    let xa = corner[0] + (corner[1] - corner[0]) * w.x;
    let xb = corner[2] + (corner[3] - corner[2]) * w.x;
    let xc = corner[4] + (corner[5] - corner[4]) * w.x;
    let xd = corner[6] + (corner[7] - corner[6]) * w.x;
    let ya = xa + (xb - xa) * w.y;
    let yb = xc + (xd - xc) * w.y;
    return ya + (yb - ya) * w.z;
}

// bark.rs `noise_gradient`: (value, gradient).
fn bark_noise_gradient(q: vec3<f32>, salt: u32) -> vec4<f32> {
    let base = vec3<i32>(floor(q));
    let f = q - floor(q);
    let w = f * f * (3.0 - 2.0 * f);
    let dw = 6.0 * f * (1.0 - f);
    var corner: array<f32, 8>;
    for (var k = 0; k < 8; k++) {
        corner[k] = bark_unit(bark_hash(base + vec3<i32>(k & 1, (k >> 1u) & 1, (k >> 2u) & 1), salt));
    }
    let kx = corner[1] - corner[0];
    let ky = corner[2] - corner[0];
    let kz = corner[4] - corner[0];
    let kxy = corner[0] - corner[1] - corner[2] + corner[3];
    let kxz = corner[0] - corner[1] - corner[4] + corner[5];
    let kyz = corner[0] - corner[2] - corner[4] + corner[6];
    let kxyz = -corner[0] + corner[1] + corner[2] - corner[3] + corner[4] - corner[5] - corner[6]
        + corner[7];
    let value = corner[0] + kx * w.x + ky * w.y + kz * w.z + kxy * w.x * w.y + kxz * w.x * w.z
        + kyz * w.y * w.z + kxyz * w.x * w.y * w.z;
    return vec4<f32>(
        value,
        dw.x * (kx + kxy * w.y + kxz * w.z + kxyz * w.y * w.z),
        dw.y * (ky + kxy * w.x + kyz * w.z + kxyz * w.x * w.z),
        dw.z * (kz + kxz * w.x + kyz * w.y + kxyz * w.x * w.y),
    );
}

// bark.rs `octave`.
fn bark_octave(
    c: vec3<f32>,
    sine: f32,
    cosine: f32,
    footprint: f32,
    frequency: f32,
    weight: f32,
    salt: u32,
) -> vec3<f32> {
    let fade = 1.0 - smoothstep(0.3, 0.8, footprint * frequency / bark.scale);
    if fade <= 0.0 {
        return vec3<f32>(0.0);
    }
    let k = frequency / bark.scale;
    let stretch = sqrt(max(bark.elongation, 1.0e-3));
    let g = vec3<f32>(c.x * k, c.y * k, c.z * k / stretch);
    let n = bark_noise_gradient(g, salt);
    let a = weight * fade;
    return vec3<f32>(a * (n.x - 0.5), a * k * (-sine * n.y + cosine * n.z), a * k / stretch * n.w);
}

struct Height {
    h: f32,
    gradient: vec3<f32>,
    id: u32,
}

// bark.rs `height`.
fn bark_height(q: vec3<f32>) -> Height {
    let found = bark_cells(q, BARK_CELL_SALT);
    let edge = found.second - found.near;
    let w = max(bark.fissure, 1.0e-4);
    let t = clamp(edge / w, 0.0, 1.0);
    var out: Height;
    out.h = t * t * (3.0 - 2.0 * t);
    let to_near = q - found.near_point;
    let to_second = q - found.second_point;
    let near_length = max(length(to_near), 1.0e-6);
    let second_length = max(length(to_second), 1.0e-6);
    var slope = 0.0;
    if t > 0.0 && t < 1.0 {
        slope = 6.0 * t * (1.0 - t) / w;
    }
    out.gradient = slope * (to_second / second_length - to_near / near_length);
    if bark.kind == BARK_SCALY {
        let rise = clamp(0.5 - to_near.z, 0.0, 1.0);
        let factor = 0.65 + 0.35 * rise;
        var inside = 0.0;
        if 0.5 - to_near.z > 0.0 && 0.5 - to_near.z < 1.0 {
            inside = -0.35 * out.h;
        }
        out.gradient = out.gradient * factor;
        out.gradient.z += inside;
        out.h *= factor;
    }
    out.id = found.id;
    return out;
}

struct Sample {
    shade: f32,
    inner: f32,
    slope: vec2<f32>,
}

// bark.rs `sample`.
fn bark_sample(u: f32, v: f32, r: f32, footprint: f32) -> Sample {
    var none: Sample;
    none.shade = 1.0;
    none.inner = 0.0;
    none.slope = vec2<f32>(0.0);
    if bark.kind == 0u || r <= 0.0 || bark.scale <= 0.0 {
        return none;
    }
    var young = 1.0;
    if bark.smooth_below > 0.0 {
        young = smoothstep(bark.smooth_below, 2.0 * bark.smooth_below, r);
    }
    let fade = 1.0 - smoothstep(0.3, 0.8, footprint / bark.scale);
    let amount = young * fade;
    if amount <= 0.0 {
        return none;
    }
    let theta = BARK_TAU * u;
    let sine = sin(theta);
    let cosine = cos(theta);
    let c = vec3<f32>(r * cosine, r * sine, v);
    let s = bark.scale;
    let along = s * max(bark.elongation, 1.0e-3);
    let q = vec3<f32>(c.x / s, c.y / s, c.z / along);
    var shade = 1.0;
    var inner = 0.0;
    var gradient = vec3<f32>(0.0);
    var grain_slope = vec2<f32>(0.0);
    if bark.kind == BARK_SMOOTH {
        shade = 1.0 + bark.variation * (bark_noise(q, BARK_MOTTLE_SALT) - 0.5);
    } else {
        var height = bark_height(q);
        let cell = bark_unit(pcg(height.id ^ BARK_SHADE_SALT));
        let peeled = bark.kind == BARK_PEELING
            && bark_unit(pcg(height.id ^ BARK_PEEL_SALT)) < bark.peeled;
        // Peeling bark is flat but where a patch has peeled (`surface`).
        if bark.kind == BARK_PEELING && !peeled {
            height.h = 1.0;
            height.gradient = vec3<f32>(0.0);
        }
        var rough = height.h;
        if bark.roughness > 0.0 {
            let d = bark_octave(c, sine, cosine, footprint, 4.0, 1.0, BARK_GRAIN_SALT)
                + bark_octave(c, sine, cosine, footprint, 9.0, 0.5, BARK_FINE_SALT);
            let rise = 1.0 + bark.roughness * d.x;
            rough = height.h * rise;
            height.gradient = height.gradient * rise;
            grain_slope = vec2<f32>(height.h * bark.roughness * d.y, height.h * bark.roughness * d.z);
        }
        shade = (1.0 + bark.contrast * (rough - bark.mean)) * (1.0 + bark.variation * (cell - 0.5));
        if peeled {
            inner = height.h;
        }
        gradient = height.gradient;
    }
    if bark.lenticels > 0.0 {
        let spacing = 1.0 / sqrt(bark.lenticels);
        let found = bark_cells(c / spacing, BARK_LENTICEL_SALT);
        let offset = c / spacing - found.near_point;
        let round = -sine * offset.x + cosine * offset.y;
        let reach = (round / 0.22) * (round / 0.22) + (offset.z / 0.035) * (offset.z / 0.035);
        shade *= 1.0 - 0.45 * (1.0 - smoothstep(0.6, 1.0, reach));
    }
    let relief = bark.depth * s;
    let round_q = vec3<f32>(-sine / s, cosine / s, 0.0);
    var out: Sample;
    out.shade = 1.0 + (shade - 1.0) * amount;
    out.inner = inner * amount;
    out.slope = vec2<f32>(
        relief * (gradient.x * round_q.x + gradient.y * round_q.y) + relief * grain_slope.x,
        relief * gradient.z / along + relief * grain_slope.y,
    ) * amount;
    return out;
}

fn unit_or_zero(a: vec3<f32>) -> vec3<f32> {
    let length_squared = dot(a, a);
    if length_squared < 1.0e-20 {
        return vec3<f32>(0.0);
    }
    return a * inverseSqrt(length_squared);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // The directions u and v grow in across the surface, from the screen
    // derivatives (the triangle's edges, as `plantgen::raster` takes them),
    // before anything branches.
    let dp1 = dpdx(in.world_position.xyz);
    let dp2 = dpdy(in.world_position.xyz);
    let dt1 = dpdx(in.uv);
    let dt2 = dpdy(in.uv);
    var pbr_input = pbr_input_from_standard_material(in, is_front);
#ifdef VERTEX_UVS_B
    let radius = in.uv_b.x;
#else
    let radius = 0.0;
#endif
    // A pixel's size in metres where it lies: 2 w / (P[1][1] h), for a
    // perspective or an orthographic view alike.
    let clip = view.clip_from_world * vec4<f32>(in.world_position.xyz, 1.0);
    let footprint = 2.0 * clip.w / (view.clip_from_view[1][1] * view.viewport.w);
    let sample = bark_sample(in.uv.x, in.uv.y, radius, footprint);
    let shaded = pbr_input.material.base_color.rgb * sample.shade;
    pbr_input.material.base_color = vec4<f32>(mix(shaded, bark.inner.rgb, sample.inner), 1.0);
    let det = dt1.x * dt2.y - dt2.x * dt1.y;
    if abs(det) > 1.0e-12 {
        let round = unit_or_zero((dp1 * dt2.y - dp2 * dt1.y) / det);
        let along = unit_or_zero((dp2 * dt1.x - dp1 * dt2.x) / det);
        let n = pbr_input.N - round * sample.slope.x - along * sample.slope.y;
        if dot(n, n) > 1.0e-12 {
            pbr_input.N = normalize(n);
        }
    }
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}

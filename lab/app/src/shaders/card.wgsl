// PlantLab's organ cards: a standard material whose base colour comes from
// the card's organ template. The template layer plus one is the vertex
// colour's alpha (plus one, so the standard material's alpha mask never
// cuts a card), the card's colour its RGB, and the darkening of the template's
// accent colour for this card (`Templates::albedo`, computed on the CPU in
// `plantlab-scene`) the second texture coordinate's x. The texel holds
// brightness / 2 in red, the accent weight in green and coverage in alpha.
// The blend below is `Templates::albedo` exactly; nothing else about the
// plant is computed here.

#import bevy_pbr::pbr_fragment::pbr_input_from_standard_material

#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io::{VertexOutput, FragmentOutput}
#import bevy_pbr::pbr_deferred_functions::deferred_output
#else
#import bevy_pbr::forward_io::{VertexOutput, FragmentOutput}
#import bevy_pbr::pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing}
#endif

struct CardAccents {
    // Per template: its look's accent colour, linear RGB.
    accents: array<vec4<f32>, 64>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> card: CardAccents;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var templates: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var templates_sampler: sampler;

fn texel(uv: vec2<f32>, layer: f32) -> vec4<f32> {
    // Row 0 of a template is the card's base, where v = 0.
    return textureSampleLevel(templates, templates_sampler, uv, i32(round(layer)) - 1, 0.0);
}

#ifdef PREPASS_PIPELINE
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) {
#ifdef VERTEX_COLORS
    let t = texel(in.uv, in.color.a);
    if t.a < 0.5 {
        discard;
    }
#endif
}
#else
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let t = texel(in.uv, in.color.a);
    if t.a < 0.5 {
        discard;
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let layer = u32(round(in.color.a)) - 1u;
    let accent = card.accents[layer].rgb * in.uv_b.x;
    let albedo = mix(in.color.rgb, accent, t.g) * (2.0 * t.r);
    pbr_input.material.base_color = vec4<f32>(albedo, 1.0);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
#endif

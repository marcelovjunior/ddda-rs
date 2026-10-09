// StandardMaterial plus the original's FTransparencyMap rule: the alpha is
// multiplied by one channel of tTransparencyMap, sampled with its own UV
// set (the mesh's second UV, ATTRIBUTE_UV_1, untransformed). See
// docs/SHADING.md and crates/ddview/src/stage.rs.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> transparency_channel: u32;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var transparency_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var transparency_sampler: sampler;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

#ifdef VERTEX_UVS_B
    let uv = in.uv_b;
#else
    let uv = in.uv;
#endif
    let t = textureSample(transparency_texture, transparency_sampler, uv);
    pbr_input.material.base_color.a *= t[transparency_channel];

    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}

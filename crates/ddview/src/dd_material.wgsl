// Stage material: StandardMaterial plus the original's rules that Bevy's
// PBR does not have. See docs/SHADING.md and crates/ddview/src/stage.rs.
//
// Vertex: swing billboards (rSwingModel nodes with flag 0x200). The mesh
// carries the swing pivot (model space, viewer units) in its vertex colour
// with w = -1. As in the stage swing vertex shaders, the pivot goes through
// the model transform and the vertex offset from the pivot is mapped onto
// the camera's right / up / back axes (fViewI rows), so each card faces the
// camera. Wind sway is not applied.
//
// Fragment: optional transparency map (FTransparencyMap: alpha *= map
// channel on UV 1), and, when DD_LIGHTING is set, the original's lighting:
//   colour = albedo * (SH(N) + sun colour * max(N.L, 0) * shadow)
//   colour = lerp(colour, fog colour, saturate((depth - start) * inv_range) * density)
// with fSHCoef evaluated exactly as the stage pixel shaders do.

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    mesh_functions,
    prepass_io::{Vertex, VertexOutput},
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}
#else
#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings as view_bindings,
    mesh_view_types,
    mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT,
    shadows,
}
#endif

struct DdParams {
    flags: u32,
    map_channel: u32,
    pad0: u32,
    pad1: u32,
    sh: array<vec4<f32>, 7>,
    // xyz: direction to the light (unit), w unused
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    // rgb: fog colour, w: fog density
    fog_color: vec4<f32>,
    // x: fog start (cm), y: 1 / (end - start) (1/cm), z: cm per viewer unit
    fog_range: vec4<f32>,
    // xyz: fReflectiveColor, w: Fresnel F0
    reflect: vec4<f32>,
};

const FLAG_MAP: u32 = 1u;
const FLAG_DD_LIGHTING: u32 = 2u;
const FLAG_REFLECT: u32 = 4u;
const FLAG_SPEC_MAP: u32 = 8u;
// Shadow caster only (the primitive's draw mode lacks the main pass).
const FLAG_SHADOW_ONLY: u32 = 16u;

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> dd: DdParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var transparency_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var transparency_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var spec_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var spec_sampler: sampler;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    var world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
#ifdef VERTEX_NORMALS
    var normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#endif
#ifdef VERTEX_COLORS
    var color = vertex.color;
    if (vertex.color.w < 0.0) {
        // Billboard: pivot through the model transform; the offset (model
        // space, not transformed) along the camera axes.
        let pivot = (world_from_local * vec4<f32>(vertex.color.xyz, 1.0)).xyz;
        let d = vertex.position - vertex.color.xyz;
        let right = view.world_from_view[0].xyz;
        let up = view.world_from_view[1].xyz;
        let back = view.world_from_view[2].xyz;
        world = vec4<f32>(pivot + d.x * right + d.y * up + d.z * back, 1.0);
#ifdef VERTEX_NORMALS
        normal = normalize(vertex.normal.x * right + vertex.normal.y * up + vertex.normal.z * back);
#endif
        color = vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }
#endif
    out.world_position = world;
    out.position = position_world_to_clip(world.xyz);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif
#ifdef PREPASS_PIPELINE
#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
#ifdef VERTEX_NORMALS
    out.world_normal = normal;
#endif
#endif
#else
#ifdef VERTEX_NORMALS
    out.world_normal = normal;
#endif
#endif
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_COLORS
    out.color = color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}

#ifndef PREPASS_PIPELINE
// fSHCoef evaluation of the stage pixel shaders (e.g. sc\dx9\stage100 PS
// at file offset 0x3F0490): r = dp4(c1..c3, (N, 1)) + dp4(c4..c6,
// N.yzzx * N.xyzz) + c7 * (N.x^2 - N.y^2).
fn sh_irradiance(n: vec3<f32>) -> vec3<f32> {
    let n1 = vec4<f32>(n, 1.0);
    let q = n.yzzx * n.xyzz;
    var r = vec3<f32>(dot(dd.sh[0], n1), dot(dd.sh[1], n1), dot(dd.sh[2], n1));
    r += vec3<f32>(dot(dd.sh[3], q), dot(dd.sh[4], q), dot(dd.sh[5], q));
    r += dd.sh[6].xyz * (n.x * n.x - n.y * n.y);
    return r;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    if ((dd.flags & FLAG_SHADOW_ONLY) != 0u) {
        discard;
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    if ((dd.flags & FLAG_MAP) != 0u) {
#ifdef VERTEX_UVS_B
        let uv = in.uv_b;
#else
        let uv = in.uv;
#endif
        let t = textureSample(transparency_texture, transparency_sampler, uv);
        pbr_input.material.base_color.a *= t[dd.map_channel];
    }

    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    if ((dd.flags & FLAG_DD_LIGHTING) != 0u) {
        let n = pbr_input.N;
        var shadow = 1.0;
        let view_z = dot(vec4<f32>(
            view_bindings::view.view_from_world[0].z,
            view_bindings::view.view_from_world[1].z,
            view_bindings::view.view_from_world[2].z,
            view_bindings::view.view_from_world[3].z
        ), in.world_position);
        if (view_bindings::lights.n_directional_lights > 0u
                && (pbr_input.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u
                && (view_bindings::lights.directional_lights[0].flags
                    & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
            shadow = shadows::fetch_directional_shadow(0u, in.world_position, in.world_normal, view_z, in.position.xy);
        }
        let light = sh_irradiance(n) + dd.sun_color.rgb * max(dot(n, dd.sun_dir.xyz), 0.0) * shadow;
        var color = pbr_input.material.base_color.rgb * light;
        if ((dd.flags & FLAG_REFLECT) != 0u) {
            // Environment reflection as in the stage shaders (e.g. file
            // offset 0x88068C): env * fReflectiveColor * specular map G *
            // mean ambient * Schlick Fresnel lerp(F0, 1, (1 - N.V)^5). The
            // env is a run-time render (sky cube / planar water reflection);
            // the fog colour stands in for it here (*not traced*).
            var mask = 1.0;
            if ((dd.flags & FLAG_SPEC_MAP) != 0u) {
#ifdef VERTEX_UVS_A
                mask = textureSample(spec_texture, spec_sampler, in.uv).g;
#endif
            }
            let sh = sh_irradiance(n);
            let ambient = (sh.r + sh.g + sh.b) / 3.0;
            let nv = clamp(dot(n, pbr_input.V), 0.0, 1.0);
            let fres = mix(dd.reflect.w, 1.0, pow(1.0 - nv, 5.0));
            color += dd.fog_color.rgb * dd.reflect.rgb * mask * ambient * fres;
        }
        // Linear fog on view depth in game units (cm).
        let depth = -view_z * dd.fog_range.z;
        let f = clamp((depth - dd.fog_range.x) * dd.fog_range.y, 0.0, 1.0) * dd.fog_color.w;
        color = mix(color, dd.fog_color.rgb, f);
        out.color = vec4<f32>(color, pbr_input.material.base_color.a);
    } else {
        out.color = apply_pbr_lighting(pbr_input);
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
#endif

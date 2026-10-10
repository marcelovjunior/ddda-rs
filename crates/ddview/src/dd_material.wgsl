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

#import ddview::sky_scatter::sky_color
#import bevy_pbr::skinning

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
    forward_io::{Vertex, FragmentOutput},
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings as view_bindings,
    mesh_view_types,
    mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT,
    shadows,
}
#import bevy_pbr::forward_io as forward_io

// Keep the original's interpolated bitangent: reconstructing it per pixel
// from normalized axes changes its length and loses mirrored-basis behavior.
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
#ifdef VERTEX_UVS_A
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_UVS_B
    @location(3) uv_b: vec2<f32>,
#endif
#ifdef VERTEX_TANGENTS
    @location(4) world_tangent: vec4<f32>,
    @location(8) world_bitangent: vec3<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(5) color: vec4<f32>,
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    @location(6) @interpolate(flat) instance_index: u32,
#endif
#ifdef VISIBILITY_RANGE_DITHER
    @location(7) @interpolate(flat) visibility_range_dither: i32,
#endif
}
#endif

struct DdParams {
    flags: u32,
    map_channel: u32,
    alpha_compare: u32,
    alpha_reference: u32,
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
    // xyz: environment cube colour, w: cube index
    env: vec4<f32>,
    // spot light: position (viewer units, w falloff), direction (from the
    // light), colour (w 1 when lit), attenuation (start cm, 1/(end-start),
    // cos outer, 1/(cos inner - cos outer))
    spot_pos: vec4<f32>,
    spot_dir: vec4<f32>,
    spot_color: vec4<f32>,
    spot_atten: vec4<f32>,
    // rgb: constant light (FLAG_CONST_LIGHT)
    const_light: vec4<f32>,
    // rgb: fAlbedoBlendColor.rgb * .w * fDiffuseColor (FLAG_ALBEDO_ADD)
    albedo_blend: vec4<f32>,
    // rgb: fSpecularColor, w: fShininess (FLAG_CHARACTER)
    spec: vec4<f32>,
    normal_uv0: vec4<f32>,
    normal_uv1: vec4<f32>,
};

const FLAG_MAP: u32 = 1u;
const FLAG_DD_LIGHTING: u32 = 2u;
const FLAG_REFLECT: u32 = 4u;
const FLAG_SPEC_MAP: u32 = 8u;
// Shadow caster only (the primitive's draw mode lacks the main pass).
const FLAG_SHADOW_ONLY: u32 = 16u;
// The environment reflection evaluates the stage sky.
const FLAG_SKY_ENV: u32 = 32u;
// The environment reflection samples the environment cube.
const FLAG_ENV_CUBE: u32 = 64u;
// No light buffer: unlit, the vertex colour is the light.
const FLAG_NO_LIGHT_BUFFER: u32 = 128u;
// Alpha blended (keeps its alpha in the frame).
const FLAG_BLENDED: u32 = 256u;
// Lit by a constant colour (the sky's cloud domes).
const FLAG_CONST_LIGHT: u32 = 512u;
// FAlbedoMapAdd on the view-normal UV (characters).
const FLAG_ALBEDO_ADD: u32 = 1024u;
// The character pixel shaders' specular and environment terms.
const FLAG_CHARACTER: u32 = 2048u;
const FLAG_ALPHA_TEST: u32 = 4096u;
const FLAG_NORMAL_MAP: u32 = 8192u;

struct SkyEnvParams {
    cb: array<vec4<f32>, 11>,
    eye: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> dd: DdParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var transparency_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var transparency_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var spec_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var spec_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var<uniform> sky_env: SkyEnvParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var sky_rayleigh: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var sky_mie: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var env_cube: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(109) var env_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(110) var albedo_blend_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(111) var albedo_blend_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(112) var normal_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(113) var normal_sampler: sampler;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
#ifdef SKINNED
    // Characters (character.rs): skinned as Bevy's mesh shader does.
    let world_from_local = skinning::skin_model(vertex.joint_indices, vertex.joint_weights, vertex.instance_index);
#else
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
#endif
    var world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
#ifdef VERTEX_NORMALS
#ifdef SKINNED
    var normal = skinning::skin_normals(world_from_local, vertex.normal);
#else
    var normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#endif
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
#ifdef VERTEX_TANGENTS
    // Translated from sc/DX9/Stage200 VS 0x1381d4. Biased-byte XYZ/W
    // were decoded once by mtmod. Preserve the raw transformed T and B;
    // normalize only N before interpolation, as the source does.
    let imported_n = (world_from_local * vec4<f32>(vertex.normal, 0.0)).xyz;
    let imported_t = (world_from_local * vec4<f32>(vertex.tangent.xyz, 0.0)).xyz;
    out.world_tangent = vec4<f32>(imported_t, vertex.tangent.w);
    out.world_bitangent = cross(imported_t, imported_n) * vertex.tangent.w;
    if ((dd.flags & FLAG_NORMAL_MAP) != 0u) {
        out.world_normal = normalize(imported_n);
    }
#endif
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
    var standard: forward_io::VertexOutput;
    standard.position = in.position;
    standard.world_position = in.world_position;
    standard.world_normal = in.world_normal;
#ifdef VERTEX_UVS_A
    standard.uv = in.uv;
#endif
#ifdef VERTEX_UVS_B
    standard.uv_b = in.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    standard.world_tangent = in.world_tangent;
#endif
#ifdef VERTEX_COLORS
    standard.color = in.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    standard.instance_index = in.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    standard.visibility_range_dither = in.visibility_range_dither;
#endif
    var pbr_input = pbr_input_from_standard_material(standard, is_front);
    let geometric_n = pbr_input.N;
#ifdef VERTEX_TANGENTS
#ifdef VERTEX_UVS_A
    if ((dd.flags & FLAG_NORMAL_MAP) != 0u) {
        let source_uv = vec4<f32>(in.uv, 0.0, 1.0);
        let uv = vec2<f32>(dot(dd.normal_uv0, source_uv), dot(dd.normal_uv1, source_uv));
        let rgba = textureSample(normal_texture, normal_sampler, uv);
        // Translated from sc/DX9/Stage200 PS 0x6171f8 (also 0x318e0c,
        // 0x5f3000): DXT5 alpha/green, midpoint 127/255, strict Z branch.
        let xy = (rgba.ag - vec2<f32>(127.0 / 255.0)) * 2.0;
        let zz = 1.0 - dot(xy, xy);
        let z = select(0.01, sqrt(max(zz, 0.0)), zz > 0.0001);
        // Retain the source's interpolated N length. Use the existing
        // material's face convention without normalizing N before the sum.
        let geometric = in.world_normal * select(-1.0, 1.0, dot(in.world_normal, pbr_input.N) >= 0.0);
        pbr_input.N = normalize(in.world_tangent.xyz * xy.x
            + in.world_bitangent * xy.y + geometric * z);
    }
#endif
#endif

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
    // rMaterial::vf09 0x0112e4d0 / draw 0x00fee320: alpha testing
    // remains enabled independently of blending and depth writes.
    if ((dd.flags & FLAG_ALPHA_TEST) != 0u) {
        let a = pbr_input.material.base_color.a;
        let r = f32(dd.alpha_reference) / 255.0;
        var alpha_passes = true;
        switch dd.alpha_compare {
            case 0u: { alpha_passes = false; }
            case 1u: { alpha_passes = a < r; }
            case 2u: { alpha_passes = a == r; }
            case 3u: { alpha_passes = a <= r; }
            case 4u: { alpha_passes = a > r; }
            case 5u: { alpha_passes = a != r; }
            case 6u: { alpha_passes = a >= r; }
            default: {}
        }
        if (!alpha_passes) { discard; }
    }

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
        var light = sh_irradiance(n) + dd.sun_color.rgb * max(dot(n, dd.sun_dir.xyz), 0.0) * shadow;
        if (dd.spot_color.w > 0.0) {
            // Deferred spot light (pixel shader at file offset 0x9B1A70 of
            // sc\dx9\root): colour x (1 - sat((d - start) / (end - start)))^2
            // x sat((dot(-dir, L) - cos outer) / (cos inner - cos outer))^falloff
            // x max(N.L, 0); no shadow.
            let to_light = dd.spot_pos.xyz - in.world_position.xyz;
            let d = length(to_light) * dd.fog_range.z;
            let l = normalize(to_light);
            var range = 1.0 - clamp((d - dd.spot_atten.x) * dd.spot_atten.y, 0.0, 1.0);
            range = range * range;
            let cone = pow(clamp((dot(-dd.spot_dir.xyz, l) - dd.spot_atten.z) * dd.spot_atten.w, 0.0, 1.0), dd.spot_pos.w);
            light += dd.spot_color.rgb * range * cone * max(dot(n, l), 0.0);
        }
        if ((dd.flags & FLAG_NO_LIGHT_BUFFER) != 0u) {
            // Unlit: the vertex colour (in the base colour) is the light.
            light = vec3<f32>(1.0);
        }
        if ((dd.flags & FLAG_CONST_LIGHT) != 0u) {
            light = dd.const_light.rgb;
        }
        var albedo = pbr_input.material.base_color.rgb;
        if ((dd.flags & FLAG_ALBEDO_ADD) != 0u) {
            // FAlbedoMapAdd / FUVViewNormal (character pixel shaders, file
            // offsets 0x31BEA8 and 0x40C494 of sc\dx9\root): the normal
            // through the 3x3 of fView, normalized; UV = (0.5 + 0.5 x,
            // 0.5 - 0.5 y); albedo += blend.rgb * blend.a * colour. The
            // game's view axes are *assumed* to match Bevy's (x right, y
            // up). The normal is the vertex normal (normal maps are not
            // applied).
            let vn = normalize((view_bindings::view.view_from_world * vec4<f32>(geometric_n, 0.0)).xyz);
            let buv = vec2<f32>(0.5 + 0.5 * vn.x, 0.5 - 0.5 * vn.y);
            let b = textureSample(albedo_blend_texture, albedo_blend_sampler, buv);
            albedo += b.rgb * b.a * dd.albedo_blend.rgb;
        }
        var color = albedo * light;
        if ((dd.flags & FLAG_CHARACTER) != 0u) {
            // The character pixel shaders' specular (same shaders):
            //   spec = (DL0 colour x (N.H)^fShininess x fSpecularColor + env)
            //          x tSpecularMap.rgb x occlusion
            //   colour += spec x lerp(fFresnelSchlick, 1, (1 - N.V)^5)
            // DL0 (sLightParamDL0, the forward infinite light, uInfiniteLight
            // 0x00e15c70) is taken to be the sun with its shadow (*inferred*
            // from the light groups, not traced at run time). env =
            // cube(reflect) x fReflectiveColor x (1 + 7 x
            // fAppShadowLightScale x (N.normalize(fAppReflectShadowDir +
            // V))^fShininess - fEnvMapMask x shadow mask), at the MFX
            // defaults fAppShadowLightScale 1, fAppReflectShadowDir (0, 1, 0),
            // fEnvMapMask 1 (their run-time writers are not found); shadow
            // groups 1. *Not done:* the light buffer's specular (deferred
            // lights), the vertex occlusion (1).
            let v = pbr_input.V;
            let shine = dd.spec.w;
            let h = normalize(dd.sun_dir.xyz + v);
            var spec = dd.sun_color.rgb * shadow * pow(max(dot(n, h), 1e-5), shine) * dd.spec.rgb;
            if ((dd.flags & FLAG_ENV_CUBE) != 0u) {
                let r = reflect(-v, n);
                let hs = normalize(vec3<f32>(0.0, 1.0, 0.0) + v);
                let boost = 1.0 + 7.0 * pow(max(dot(n, hs), 1e-5), shine) - (1.0 - shadow);
                spec += textureSample(env_cube, env_sampler, r).rgb * dd.env.rgb * dd.reflect.rgb * boost;
            }
            var mask = vec3<f32>(1.0);
            if ((dd.flags & FLAG_SPEC_MAP) != 0u) {
#ifdef VERTEX_UVS_A
                mask = textureSample(spec_texture, spec_sampler, in.uv).rgb;
#endif
            }
            // N.V is not clamped in the original.
            let fres = mix(dd.reflect.w, 1.0, pow(max(1.0 - dot(n, v), 0.0), 5.0));
            color += spec * mask * fres;
        }
        if ((dd.flags & FLAG_REFLECT) != 0u) {
            // Environment reflection as in the stage shaders (e.g. file
            // offsets 0x88068C, 0x4A0980): env * fReflectiveColor *
            // specular map G * mean ambient * Schlick Fresnel lerp(F0, 1,
            // (1 - N.V)^5). The env is the sky's reflection cube
            // (DDCube0_CM): the stage sky evaluated along the reflected
            // view ray. *Not done:* the cube's other contents (EnvCube pass
            // geometry), the normal and detail normal maps; without a sky
            // the fog colour stands in.
            var mask = 1.0;
            if ((dd.flags & FLAG_SPEC_MAP) != 0u) {
#ifdef VERTEX_UVS_A
                mask = textureSample(spec_texture, spec_sampler, in.uv).g;
#endif
            }
            let sh = sh_irradiance(n);
            var ambient = (sh.r + sh.g + sh.b) / 3.0;
            if ((dd.flags & FLAG_NO_LIGHT_BUFFER) != 0u) {
                // mean(VertexColor.rgb) x Occlusion
                ambient = 1.0;
#ifdef VERTEX_COLORS
                ambient = (in.color.r + in.color.g + in.color.b) / 3.0;
#endif
            }
            let nv = clamp(dot(n, pbr_input.V), 0.0, 1.0);
            let fres = mix(dd.reflect.w, 1.0, pow(1.0 - nv, 5.0));
            var env = dd.fog_color.rgb;
            let r = reflect(-pbr_input.V, n);
            if ((dd.flags & FLAG_ENV_CUBE) != 0u) {
                // DDCube<i>_CM = DDBaseCube<i>_CM x the weather's colour
                // (0x912060; its HDR encode is a no-op below 1).
                env = textureSample(env_cube, env_sampler, r).rgb * dd.env.rgb;
            } else if ((dd.flags & FLAG_SKY_ENV) != 0u) {
                // Planar water reflection (FReflectWaterMap): the sky
                // stands in for the mirrored scene.
                env = sky_color(sky_env.cb, sky_rayleigh, sky_mie, sky_env.eye.xyz, r);
            }
            color += env * dd.reflect.rgb * mask * ambient * fres;
        }
        // Linear fog on view depth in game units (cm).
        let depth = -view_z * dd.fog_range.z;
        let f = clamp((depth - dd.fog_range.x) * dd.fog_range.y, 0.0, 1.0) * dd.fog_color.w;
        color = mix(color, dd.fog_color.rgb, f);
        out.color = vec4<f32>(color, pbr_input.material.base_color.a);
        if ((dd.flags & FLAG_BLENDED) == 0u) {
            // The frame's alpha carries the view depth (m) for the depth of
            // field (dd_post.wgsl); blended surfaces keep their alpha.
            out.color.a = -view_z;
        }
    } else {
        out.color = apply_pbr_lighting(pbr_input);
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
#endif

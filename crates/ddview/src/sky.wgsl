// Sky dome (docs/SHADING.md, "Sky"; crates/ddview/src/sky.rs).
//
// The original renders 32 x 32 Rayleigh and Mie scatter maps over the screen
// (pixel shaders at file offsets 0x5F9134 and 0x657A54 of sc\dx9\root, the
// 10-step variants for mLoopCount 10) and lights them on the sky dome (file
// offset 0x5B8D54). Here the scatter functions run per pixel on the pixel's
// own view ray instead of the screen grid (their prologue, which builds the
// ray from the grid position with a horizon-weighted row spacing, is left
// out). The two function bodies below are generated, instruction for
// instruction, from the disassembly: registers rN are the shader's
// registers, cb[k] is CBSky packed k (crates/mtweather/src/sky.rs), and the
// depth maps are read with point sampling as SSPoint does. Stars and the
// starry sky texture are not drawn.
// Translated from the pixel shaders at file offsets 0x5F9134, 0x657A54 and 0x5B8D54 of sc\dx9\root (DDDA 2017-12-13 Steam)

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
}
#import ddview::sky_scatter::sky_color

struct SkyParams {
    // CBSky, 11 float4s
    cb: array<vec4<f32>, 11>,
    // xyz: camera position in game units (fCameraPos)
    eye: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: SkyParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var rayleigh_depth: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var mie_depth: texture_2d<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.world_position.xyz - view.world_position);
    // Alpha: the sky's depth for the depth of field (far).
    return vec4<f32>(sky_color(sky.cb, rayleigh_depth, mie_depth, sky.eye.xyz, dir), 60000.0);
}

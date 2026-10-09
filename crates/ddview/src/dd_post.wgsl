// The stage filter chain (crates/ddview/src/post.rs): vignette from the
// ImagePlaneFilter blended in the sRGB frame, then the ColorCorrectFilter
// tone curve on the linear colour.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct DdPost {
    lut: array<vec4<f32>, 64>,
    vignette: array<vec4<f32>, 512>,
    flags: vec4<f32>,
};

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;
@group(0) @binding(2) var<uniform> post: DdPost;

fn lut_at(i: i32) -> f32 {
    let k = clamp(i, 0, 255);
    return post.lut[k / 4][k % 4];
}

fn vig_at(x: i32, y: i32) -> f32 {
    let k = clamp(y, 0, 31) * 64 + clamp(x, 0, 63);
    return post.vignette[k / 4][k % 4];
}

fn to_srgb(c: f32) -> f32 {
    if (c <= 0.0031308) { return c * 12.92; }
    return 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}

fn to_linear(c: f32) -> f32 {
    if (c <= 0.04045) { return c / 12.92; }
    return pow((c + 0.055) / 1.055, 2.4);
}

// Point-sampled table, as the original's SSPoint sampler.
fn curve(c: f32) -> f32 {
    return lut_at(i32(clamp(c, 0.0, 1.0) * 255.0 + 0.5));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    var c = textureSample(screen_texture, texture_sampler, in.uv);
    if (post.flags.y > 0.0) {
        // Bilinear vignette alpha at this pixel.
        let p = in.uv * vec2<f32>(64.0, 32.0) - 0.5;
        let i = vec2<i32>(floor(p));
        let f = fract(p);
        let a = mix(
            mix(vig_at(i.x, i.y), vig_at(i.x + 1, i.y), f.x),
            mix(vig_at(i.x, i.y + 1), vig_at(i.x + 1, i.y + 1), f.x),
            f.y,
        );
        let s = vec3<f32>(to_srgb(c.r), to_srgb(c.g), to_srgb(c.b)) * (1.0 - a);
        c = vec4<f32>(to_linear(s.r), to_linear(s.g), to_linear(s.b), c.a);
    }
    if (post.flags.x > 0.0) {
        c = vec4<f32>(curve(c.r), curve(c.g), curve(c.b), c.a);
    }
    return c;
}

// The stage filter chain (crates/ddview/src/post.rs): vignette from the
// ImagePlaneFilter blended in the sRGB frame, then the ColorCorrectFilter
// tone curve on the linear colour.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct DdPost {
    lut: array<vec4<f32>, 64>,
    vignette: array<vec4<f32>, 512>,
    flags: vec4<f32>,
    // (zf, zn, zfar, blur size), (near limit, far limit, 1/(zf-zn),
    // 1/(zfar-zf)), (camera near cm, far cm, low-res w, h), (gradate rgb, -)
    dof: array<vec4<f32>, 4>,
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

fn srgb3(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(to_srgb(c.r), to_srgb(c.g), to_srgb(c.b));
}

// srgb2lin of the DOF shaders (threshold 0.03928).
fn lin3(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.03928));
}

// The DOF downscale (0x89E474): one point tap of the frame at the low-res
// texel's centre, sRGB encoded.
fn dof_low(i: vec2<i32>) -> vec3<f32> {
    let low = vec2<i32>(post.dof[2].zw);
    let t = clamp(i, vec2<i32>(0), low - vec2<i32>(1));
    let size = vec2<i32>(textureDimensions(screen_texture));
    let uv = (vec2<f32>(t) + 0.5) / post.dof[2].zw;
    let p = clamp(vec2<i32>(uv * vec2<f32>(size)), vec2<i32>(0), size - vec2<i32>(1));
    return srgb3(clamp(textureLoad(screen_texture, p, 0).rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
}

// The H and V Gaussian passes (0x775888, 0x611C08): 5 point taps at
// (k - 2) x mBlurSize low-res texels, weights exp(-i^2/2) normalised.
fn dof_blurred(i: vec2<i32>) -> vec3<f32> {
    let w = array<f32, 5>(0.054489, 0.244201, 0.40262, 0.244201, 0.054489);
    let step = i32(post.dof[0].w);
    var sum = vec3<f32>(0.0);
    for (var b = 0; b < 5; b++) {
        for (var a = 0; a < 5; a++) {
            sum += w[a] * w[b] * dof_low(i + vec2<i32>((a - 2) * step, (b - 2) * step));
        }
    }
    return sum;
}

// Post-projection depth (z/w) of a view depth in cm.
fn post_depth(d: f32) -> f32 {
    let n = post.dof[2].x;
    let f = post.dof[2].y;
    return clamp(f / (f - n) * (1.0 - n / d), 0.0, 1.0);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    var c = textureSample(screen_texture, texture_sampler, in.uv);
    // The frame is float (for the depth in alpha); its colour is clamped as
    // the 8-bit frame buffer of the original (and of this viewer before)
    // clamps it.
    c = vec4<f32>(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), c.a);
    if (post.flags.z > 0.0) {
        // The DOF composite (0x466ADC): the blurred low-res image, read
        // bilinearly, over the frame with the depth-based alpha. The frame's
        // alpha holds the view depth in metres.
        let p = in.uv * post.dof[2].zw - 0.5;
        let i = vec2<i32>(floor(p));
        let f = fract(p);
        let blurred = mix(
            mix(dof_blurred(i), dof_blurred(i + vec2<i32>(1, 0)), f.x),
            mix(dof_blurred(i + vec2<i32>(0, 1)), dof_blurred(i + vec2<i32>(1, 1)), f.x),
            f.y,
        );
        let z = post_depth(c.a * 100.0);
        let t = z - post.dof[0].x;
        var a = min(post.dof[1].y, t * post.dof[1].w);
        if (t < 0.0) {
            a = min(post.dof[1].x, -t * post.dof[1].z);
        }
        c = vec4<f32>(mix(c.rgb, lin3(blurred) * post.dof[3].rgb, a), c.a);
    }
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
    return vec4<f32>(c.rgb, 1.0);
}

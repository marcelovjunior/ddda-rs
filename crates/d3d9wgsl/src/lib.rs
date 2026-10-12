//! Direct3D 9 shader model 3 (vs_3_0, ps_3_0) shaders translated to WGSL for the
//! runtime's GPU backend (docs/RUNTIME.md, "Graphics").
//!
//! The translation is literal: every D3D9 register is a WGSL variable,
//! every instruction a statement, with the semantics Microsoft documents
//! for it (Microsoft Learn, "Asm Shader Reference"). It is checked by
//! running the game's shaders both ways: on the host's D3D9 driver
//! (crates/d3d9ref) and translated on wgpu (tests/against_d3d9.rs).
//!
//! Interface of a translated vertex shader:
//! - `@group(0) @binding(2)`: the constants, as below with 256 float
//!   registers (`VertexConstants`).
//! - `@group(2) @binding(2n)` / `@binding(2n + 1)`: vertex texture fetch.
//! - inputs: the k-th declared input at `@location(k)`.
//! - outputs: the position output as `@builtin(position)` (D3D9 clip
//!   space; the half-pixel offset is the backend's job), every other
//!   declared output at `@location(m)`, m counting the declarations other
//!   than position in order ([`Interface::outputs_vs`]).
//!
//! Interface of a translated pixel shader:
//! - `@group(0) @binding(0)`: the constants, `struct Constants { c:
//!   array<vec4<f32>, 224>, i: array<vec4<i32>, 16>, b: array<vec4<u32>,
//!   16> }` (a boolean is its `.x`); the shader's own `def`s override these
//!   registers, as in D3D9.
//! - `@group(1) @binding(2n)` / `@binding(2n + 1)`: texture and sampler for
//!   sampler register s_n.
//! - inputs: the k-th declared input (`dcl_<usage> v#`) at `@location(k)`.
//! - outputs: oC_n at `@location(n)`, oDepth as `frag_depth`.
//!
//! Arithmetic follows what the host's D3D9 driver computes, bit for bit
//! (see [`HELPERS`]): D3D9's multiplication rule (0 * anything = 0, with
//! denormals counting as 0, NaN kept; shader model 3 hardware, and DXVK
//! emulates the same), and fused multiply-adds where the driver fuses them
//! (mad, the dot products, the matrix macros, dp2add, lrp, crs).
//!
//! Relative constant reads outside the register file return 0 (as the
//! reference driver does; WGSL would clamp the index).
//!
//! Checked (tests/against_d3d9.rs): every arithmetic instruction of both
//! stages bit for bit on random and special operands; the game's shaders
//! with random constants, inputs and textures (the `sweep` test; results in
//! docs/tasks/L23-shader-sweep.md).
//!
//! Pixel-shader texture reads use implicit derivatives (texld, texldp,
//! texldb's bias, texldd's gradients) as D3D9 does; vertex shaders and
//! texldl use an explicit level.
use d3d9shader::{op, reg, Instruction, Param, Shader, Stage};
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NotShaderModel3,
    Unsupported(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotShaderModel3 => write!(f, "only vs_3_0 and ps_3_0 are translated"),
            Error::Unsupported(s) => write!(f, "unsupported: {s}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureKind {
    Flat,
    Cube,
    Volume,
}

/// What a translated pixel shader needs bound.
#[derive(Clone, Debug, Default)]
pub struct Interface {
    /// Declared inputs in location order: (usage, usage index, register).
    pub inputs: Vec<(u32, u32, u32)>,
    /// Sampler registers and their texture kinds.
    pub samplers: Vec<(u32, TextureKind)>,
    /// Colour outputs written (oC#).
    pub outputs: Vec<u32>,
    pub writes_depth: bool,
    /// Vertex shaders: outputs other than position in location order:
    /// (usage, usage index, register, write mask of the declaration).
    pub outputs_vs: Vec<(u32, u32, u32, u8)>,
    /// Vertex shaders: the register of the position output.
    pub position: Option<u32>,
}

/// Options for testing: a vertex shader's position output can also be
/// captured at the location after the others, with the rasterised
/// position replaced by a fixed value (the oracle reads every output that
/// way).
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub capture_position: Option<[f32; 4]>,
    /// Pixel shaders: the location of each declared input in declaration
    /// order, from linking with the vertex stage's outputs by usage and
    /// index; None for an input the vertex stage does not write (it reads
    /// 0). Without this, input k is at location k.
    pub ps_input_locations: Option<Vec<Option<u32>>>,
    /// Vertex shaders: the class of each declared input's vertex format
    /// (integer formats arrive as integers and are converted, as D3D9 does).
    pub vs_input_types: Option<Vec<InputType>>,
    /// Vertex shaders: apply D3D9's half-pixel offset, position.xy +=
    /// hp.xy * position.w, with `hp` = (1/width, -1/height) of the viewport
    /// (VertexConstants.hp).
    pub half_pixel: bool,
    /// Pixel shaders: D3D9's fixed-function alpha test, from
    /// Constants.alpha: x enabled, y D3DCMPFUNC, z the reference (0..255).
    pub alpha_test: bool,
    /// Pixel shaders: each sampler's D3DSAMP_MIPMAPLODBIAS added to the
    /// level texld and texldb pick (wgpu samplers have no LOD bias), from
    /// Constants.bias (sampler n at bias[n / 4][n % 4], after `alpha`).
    pub lod_bias: bool,
    /// Pixel shaders that do not write oDepth: D3D9's depth bias, depth +
    /// DEPTHBIAS + SLOPESCALEDEPTHBIAS * max(|dz/dx|, |dz/dy|), written as
    /// frag_depth from Constants.zbias (x constant, y slope; after `bias`
    /// if present, else after `alpha`). Hardware depth bias scales the
    /// constant by an implementation-defined step, so it is done here.
    pub depth_bias: bool,
}

/// The class of a vertex attribute format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputType {
    #[default]
    Float,
    Uint,
    Sint,
}

pub struct Translation {
    pub wgsl: String,
    pub interface: Interface,
}

struct Ctx<'a> {
    shader: &'a Shader,
    defs: BTreeMap<u32, [f32; 4]>,
    int_defs: BTreeMap<u32, [i32; 4]>,
    bool_defs: BTreeMap<u32, bool>,
    /// constants are read with relative addressing: a local copy with defs applied
    relative_constants: bool,
    temps: u32,
    loop_depth: u32,
    out: String,
    indent: usize,
    lod_bias: bool,
}

const COMPONENTS: [char; 4] = ['x', 'y', 'z', 'w'];

/// D3D9's arithmetic, as the host driver computes it (measured against it
/// on random and special operands, tests/against_d3d9.rs `rule_*`):
/// - multiplication: a zero or denormal factor gives +0, whatever the other
///   factor (also inf and NaN); zero is tested on the bits because GPU
///   compilers may fold float compares as if there were no NaNs, and NaN *
///   x must stay NaN;
/// - mad is one fused multiply-add (rounded once), with the same zero rule
///   for its product (+0 + c: -0 and denormal addends give +0; written as
///   an fma of zeros because compilers fold `0.0 + c` to `c`);
/// - dp3/dp4 (and the matrix macros) are a fused chain in component order:
///   x*x', then fma(y, y', .), fma(z, z', .), fma(w, w', .);
/// - dp2add is fma(y, y', x*x') + c, lrp is fma(a, b - c, c);
/// - crs's x is fma(a.y, b.z, -(a.z * b.y)) in pixel shaders but
///   fma(-a.z, b.y, a.y * b.z) in vertex shaders (and so on cyclically);
/// - frc reads a denormal operand as 0, and so does sincos in pixel shaders
///   (sin(-0) is +0 there, -0 in vertex shaders);
/// - lit clamps its exponent to [-128, 128] (not the reference
///   rasteriser's 127.9961).
const HELPERS: &str = "fn zero9(a: vec4<f32>) -> vec4<bool> { return (bitcast<vec4<u32>>(a) & vec4<u32>(0x7f800000u)) == vec4<u32>(0u); }
fn zero1(a: f32) -> bool { return (bitcast<u32>(a) & 0x7f800000u) == 0u; }
fn m9(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> { return select(a * b, vec4<f32>(0.0), zero9(a) | zero9(b)); }
fn m1(a: f32, b: f32) -> f32 { return select(a * b, 0.0, zero1(a) || zero1(b)); }
fn fma9(a: vec4<f32>, b: vec4<f32>, c: vec4<f32>) -> vec4<f32> { let z = zero9(a) | zero9(b); return fma(select(a, vec4<f32>(0.0), z), select(b, vec4<f32>(0.0), z), c); }
fn fma1(a: f32, b: f32, c: f32) -> f32 { let z = zero1(a) || zero1(b); return fma(select(a, 0.0, z), select(b, 0.0, z), c); }
fn dot3_9(a: vec4<f32>, b: vec4<f32>) -> f32 { return fma1(a.z, b.z, fma1(a.y, b.y, m1(a.x, b.x))); }
fn dot4_9(a: vec4<f32>, b: vec4<f32>) -> f32 { return fma1(a.w, b.w, fma1(a.z, b.z, fma1(a.y, b.y, m1(a.x, b.x)))); }
fn dp2add9(a: vec4<f32>, b: vec4<f32>, c: f32) -> f32 { return fma1(a.y, b.y, m1(a.x, b.x)) + c; }
fn crs9(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> { return vec4<f32>(fma1(a.y, b.z, -m1(a.z, b.y)), fma1(a.z, b.x, -m1(a.x, b.z)), fma1(a.x, b.y, -m1(a.y, b.x)), 0.0); }
fn crs9v(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> { return vec4<f32>(fma1(-a.z, b.y, m1(a.y, b.z)), fma1(-a.x, b.z, m1(a.z, b.x)), fma1(-a.y, b.x, m1(a.x, b.y)), 0.0); }
fn sincos9(a: f32) -> vec4<f32> { let x = select(a, 0.0, zero1(a)); return vec4<f32>(cos(x), sin(x), 0.0, 0.0); }
fn frc9(a: vec4<f32>) -> vec4<f32> { let f = select(a, vec4<f32>(0.0), zero9(a)); return f - floor(f); }";

enum Sampling {
    Implicit,
    Bias(String),
    Grad(String, String),
    Level(String),
}

/// Translate a vs_3_0 or ps_3_0 shader.
pub fn translate(shader: &Shader) -> Result<Translation, Error> {
    translate_with(shader, Options::default())
}

pub fn translate_with(shader: &Shader, options: Options) -> Result<Translation, Error> {
    if shader.major != 3 {
        return Err(Error::NotShaderModel3);
    }
    let vertex = shader.stage == Stage::Vertex;
    let mut cx = Ctx {
        shader,
        defs: BTreeMap::new(),
        int_defs: BTreeMap::new(),
        bool_defs: BTreeMap::new(),
        relative_constants: false,
        temps: 0,
        loop_depth: 0,
        out: String::new(),
        indent: 1,
        lod_bias: options.lod_bias && !vertex,
    };
    let mut iface = Interface::default();
    for ins in &shader.instructions {
        match ins.opcode {
            op::DEF => {
                let d = ins.dst.as_ref().unwrap();
                let v: Vec<f32> = ins.data.iter().map(|&t| f32::from_bits(t)).collect();
                cx.defs.insert(d.index, [v[0], v[1], v[2], v[3]]);
            }
            op::DEFI => {
                let d = ins.dst.as_ref().unwrap();
                let v: Vec<i32> = ins.data.iter().map(|&t| t as i32).collect();
                cx.int_defs.insert(d.index, [v[0], v[1], v[2], v[3]]);
            }
            op::DEFB => {
                cx.bool_defs.insert(ins.dst.as_ref().unwrap().index, ins.data[0] != 0);
            }
            op::DCL => {
                let d = ins.dst.as_ref().unwrap();
                let t = ins.decl.unwrap_or(0);
                match d.kind {
                    reg::INPUT => iface.inputs.push((t & 0x1F, (t >> 16) & 0xF, d.index)),
                    reg::OUTPUT if t & 0x1F == 0 && (t >> 16) & 0xF == 0 => iface.position = Some(d.index),
                    reg::OUTPUT => iface.outputs_vs.push((t & 0x1F, (t >> 16) & 0xF, d.index, d.mask_or_swizzle)),
                    reg::SAMPLER => iface.samplers.push((d.index, match (t >> 27) & 0xF {
                        3 => TextureKind::Cube,
                        4 => TextureKind::Volume,
                        _ => TextureKind::Flat,
                    })),
                    _ => {}
                }
            }
            _ => {}
        }
        for p in ins.src.iter().chain(ins.dst.iter()) {
            if p.kind == reg::CONST && p.relative.is_some() {
                cx.relative_constants = true;
            }
        }
        if let Some(d) = &ins.dst {
            if d.kind == reg::COLOROUT && !iface.outputs.contains(&d.index) {
                iface.outputs.push(d.index);
            }
            if d.kind == reg::DEPTHOUT {
                iface.writes_depth = true;
            }
        }
    }
    iface.outputs.sort();

    // main body and subroutines (label .. ret)
    let mut main = Vec::new();
    let mut subs: Vec<(u32, Vec<&Instruction>)> = Vec::new();
    let mut current: Option<usize> = None;
    for ins in &shader.instructions {
        match ins.opcode {
            op::COMMENT | op::DCL | op::DEF | op::DEFI | op::DEFB => {}
            op::LABEL => {
                subs.push((ins.src[0].index, vec![]));
                current = Some(subs.len() - 1);
            }
            op::RET if current.is_some() => current = None,
            op::RET => {}
            _ => match current {
                Some(k) => subs[k].1.push(ins),
                None => main.push(ins),
            },
        }
    }

    let mut w = String::new();
    // D3D9 allows texld/dsx/dsy anywhere (their derivatives are undefined
    // only in divergent branches); WGSL's uniformity check would refuse them
    if !vertex {
        writeln!(w, "diagnostic(off, derivative_uniformity);").unwrap();
    }
    let floats = if vertex { 256 } else { 224 };
    if vertex {
        writeln!(w, "struct VertexConstants {{ c: array<vec4<f32>, 256>, i: array<vec4<i32>, 16>, b: array<vec4<u32>, 16>, hp: vec4<f32> }}").unwrap();
        writeln!(w, "@group(0) @binding(2) var<uniform> k: VertexConstants;").unwrap();
    } else {
        let bias = if cx.lod_bias { ", bias: array<vec4<f32>, 4>" } else { "" };
        let zbias = if options.depth_bias { ", zbias: vec4<f32>" } else { "" };
        writeln!(w, "struct Constants {{ c: array<vec4<f32>, 224>, i: array<vec4<i32>, 16>, b: array<vec4<u32>, 16>, alpha: vec4<u32>{bias}{zbias} }}").unwrap();
        writeln!(w, "@group(0) @binding(0) var<uniform> k: Constants;").unwrap();
    }
    writeln!(w, "{HELPERS}").unwrap();
    for &(n, kind) in &iface.samplers {
        let t = match kind {
            TextureKind::Flat => "texture_2d<f32>",
            TextureKind::Cube => "texture_cube<f32>",
            TextureKind::Volume => "texture_3d<f32>",
        };
        let group = if vertex { 2 } else { 1 };
        writeln!(w, "@group({group}) @binding({}) var t{n}: {t};", 2 * n).unwrap();
        writeln!(w, "@group({group}) @binding({}) var s{n}: sampler;", 2 * n + 1).unwrap();
    }
    // registers: module-scope so subroutines share them
    for n in 0..32 {
        writeln!(w, "var<private> r{n}: vec4<f32>;").unwrap();
    }
    for &(_, _, r) in &iface.inputs {
        writeln!(w, "var<private> v{r}: vec4<f32>;").unwrap();
    }
    for n in 0..4 {
        writeln!(w, "var<private> oC{n}: vec4<f32>;").unwrap();
    }
    for n in 0..12 {
        writeln!(w, "var<private> o{n}: vec4<f32>;").unwrap();
    }
    writeln!(w, "var<private> a0: vec4<i32>;").unwrap();
    writeln!(w, "var<private> oDepth: vec4<f32>;").unwrap();
    writeln!(w, "var<private> p0: vec4<bool>;").unwrap();
    writeln!(w, "var<private> aL: i32;").unwrap();
    writeln!(w, "var<private> vPos: vec4<f32>;").unwrap();
    writeln!(w, "var<private> vFace: vec4<f32>;").unwrap();
    if cx.relative_constants {
        writeln!(w, "var<private> cc: array<vec4<f32>, {floats}>;").unwrap();
        writeln!(w, "fn rc(i: i32) -> vec4<f32> {{ if (i < 0 || i >= {floats}) {{ return vec4<f32>(0.0); }} return cc[i]; }}").unwrap();
    }
    // subroutines
    for (label, body) in &subs {
        cx.out.clear();
        cx.indent = 1;
        for ins in body {
            cx.instruction(ins)?;
        }
        writeln!(w, "fn l{label}() {{\n{}}}", cx.out).unwrap();
    }
    if vertex {
        return finish_vertex(w, cx, &main, iface, &options);
    }
    // entry point
    writeln!(w, "struct In {{").unwrap();
    writeln!(w, "  @builtin(position) position: vec4<f32>,").unwrap();
    writeln!(w, "  @builtin(front_facing) front: bool,").unwrap();
    let locations: Vec<Option<u32>> = match &options.ps_input_locations {
        Some(l) => l.clone(),
        None => (0..iface.inputs.len() as u32).map(Some).collect(),
    };
    for (k, &(_, _, r)) in iface.inputs.iter().enumerate() {
        if let Some(Some(l)) = locations.get(k) {
            writeln!(w, "  @location({l}) v{r}: vec4<f32>,").unwrap();
        }
    }
    writeln!(w, "}}").unwrap();
    writeln!(w, "struct Out {{").unwrap();
    for &n in &iface.outputs {
        writeln!(w, "  @location({n}) oC{n}: vec4<f32>,").unwrap();
    }
    let biased_depth = options.depth_bias && !iface.writes_depth;
    if iface.writes_depth || biased_depth {
        writeln!(w, "  @builtin(frag_depth) depth: f32,").unwrap();
    }
    if iface.outputs.is_empty() && !iface.writes_depth {
        return Err(Error::Unsupported("a pixel shader without outputs".into()));
    }
    writeln!(w, "}}").unwrap();
    cx.out.clear();
    cx.indent = 1;
    for ins in &main {
        cx.instruction(ins)?;
    }
    writeln!(w, "@fragment\nfn main(input: In) -> Out {{").unwrap();
    for (k, &(_, _, r)) in iface.inputs.iter().enumerate() {
        if let Some(Some(_)) = locations.get(k) {
            writeln!(w, "  v{r} = input.v{r};").unwrap();
        }
    }
    // D3D9's vPos is the pixel's integer position (no half-pixel offset)
    writeln!(w, "  vPos = vec4<f32>(floor(input.position.xy), 0.0, 0.0);").unwrap();
    writeln!(w, "  vFace = vec4<f32>(select(-1.0, 1.0, input.front));").unwrap();
    if cx.relative_constants {
        writeln!(w, "  cc = k.c;").unwrap();
        for (n, v) in &cx.defs {
            writeln!(w, "  cc[{n}] = {};", vec4_literal(v)).unwrap();
        }
    }
    w.push_str(&cx.out);
    if options.alpha_test && iface.outputs.contains(&0) {
        // D3DCMPFUNC: 1 never, 2 less, 3 equal, 4 less-equal, 5 greater,
        // 6 not-equal, 7 greater-equal, 8 always; failing pixels are
        // discarded. The driver compares the alpha rounded to 8 bits with
        // ALPHAREF (measured, ddgpu explore_d3d9_semantics: 47.6/255 equals
        // 48, 47.4/255 does not)
        writeln!(w, "  if (k.alpha.x != 0u) {{").unwrap();
        writeln!(w, "    let a = u32(floor(clamp(oC0.w, 0.0, 1.0) * 255.0 + 0.5)); let r = k.alpha.z; let f = k.alpha.y;").unwrap();
        writeln!(w, "    let passed = (f == 8u) || (f == 2u && a < r) || (f == 3u && a == r) || (f == 4u && a <= r) || (f == 5u && a > r) || (f == 6u && a != r) || (f == 7u && a >= r);").unwrap();
        writeln!(w, "    if (!passed) {{ discard; }}").unwrap();
        writeln!(w, "  }}").unwrap();
    }
    write!(w, "  return Out(").unwrap();
    let mut fields: Vec<String> = iface.outputs.iter().map(|n| format!("oC{n}")).collect();
    if iface.writes_depth {
        fields.push("oDepth.x".into());
    } else if biased_depth {
        fields.push("input.position.z + k.zbias.x + k.zbias.y * max(abs(dpdx(input.position.z)), abs(dpdy(input.position.z)))".into());
    }
    writeln!(w, "{});\n}}", fields.join(", ")).unwrap();
    Ok(Translation { wgsl: w, interface: iface })
}

fn finish_vertex(mut w: String, mut cx: Ctx, main: &[&Instruction], iface: Interface, options: &Options) -> Result<Translation, Error> {
    let position = iface.position.ok_or_else(|| Error::Unsupported("a vertex shader without a position output".into()))?;
    // (WGSL has no empty structs: a shader without inputs takes none)
    if !iface.inputs.is_empty() {
        writeln!(w, "struct In {{").unwrap();
        for (k, &(_, _, r)) in iface.inputs.iter().enumerate() {
            let ty = match options.vs_input_types.as_ref().and_then(|t| t.get(k)).copied().unwrap_or_default() {
                InputType::Float => "vec4<f32>",
                InputType::Uint => "vec4<u32>",
                InputType::Sint => "vec4<i32>",
            };
            writeln!(w, "  @location({k}) v{r}: {ty},").unwrap();
        }
        writeln!(w, "}}").unwrap();
    }
    writeln!(w, "struct Out {{").unwrap();
    writeln!(w, "  @builtin(position) position: vec4<f32>,").unwrap();
    for (m, &(_, _, r, _)) in iface.outputs_vs.iter().enumerate() {
        writeln!(w, "  @location({m}) o{r}: vec4<f32>,").unwrap();
    }
    if options.capture_position.is_some() {
        writeln!(w, "  @location({}) captured: vec4<f32>,", iface.outputs_vs.len()).unwrap();
    }
    writeln!(w, "}}").unwrap();
    cx.out.clear();
    cx.indent = 1;
    for ins in main {
        cx.instruction(ins)?;
    }
    if iface.inputs.is_empty() {
        writeln!(w, "@vertex\nfn main() -> Out {{").unwrap();
    } else {
        writeln!(w, "@vertex\nfn main(input: In) -> Out {{").unwrap();
    }
    for &(_, _, r) in &iface.inputs {
        writeln!(w, "  v{r} = vec4<f32>(input.v{r});").unwrap();
    }
    if cx.relative_constants {
        writeln!(w, "  cc = k.c;").unwrap();
        for (n, v) in &cx.defs {
            writeln!(w, "  cc[{n}] = {};", vec4_literal(v)).unwrap();
        }
    }
    w.push_str(&cx.out);
    if options.half_pixel {
        writeln!(w, "  o{position} = vec4<f32>(o{position}.xy + k.hp.xy * o{position}.w, o{position}.zw);").unwrap();
    }
    let mut fields = vec![match options.capture_position {
        Some(p) => vec4_literal(&p),
        None => format!("o{position}"),
    }];
    fields.extend(iface.outputs_vs.iter().map(|&(_, _, r, _)| format!("o{r}")));
    if options.capture_position.is_some() {
        fields.push(format!("o{position}"));
    }
    writeln!(w, "  return Out({});\n}}", fields.join(", ")).unwrap();
    Ok(Translation { wgsl: w, interface: iface })
}

fn float_literal(v: f32) -> String {
    if v.is_nan() {
        "bitcast<f32>(0x7fc00000u)".into()
    } else if v.is_infinite() {
        if v > 0.0 { "bitcast<f32>(0x7f800000u)".into() } else { "bitcast<f32>(0xff800000u)".into() }
    } else {
        // exact: the bit pattern as an f32 literal
        format!("bitcast<f32>({:#010x}u)", v.to_bits())
    }
}

fn vec4_literal(v: &[f32; 4]) -> String {
    format!("vec4<f32>({}, {}, {}, {})", float_literal(v[0]), float_literal(v[1]), float_literal(v[2]), float_literal(v[3]))
}

fn swizzle(sw: u8) -> String {
    if sw == 0xE4 {
        return String::new();
    }
    let s: String = (0..4).map(|k| COMPONENTS[((sw >> (2 * k)) & 3) as usize]).collect();
    format!(".{s}")
}

impl Ctx<'_> {
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn temp(&mut self) -> String {
        self.temps += 1;
        format!("tmp{}", self.temps)
    }

    /// The value of a register (before swizzle and modifier) as vec4<f32>.
    fn register(&self, p: &Param) -> Result<String, Error> {
        let n = p.index;
        Ok(match p.kind {
            reg::TEMP => format!("r{n}"),
            reg::INPUT => match &p.relative {
                Some(_) => return Err(Error::Unsupported("relative input addressing".into())),
                None => format!("v{n}"),
            },
            reg::CONST => match &p.relative {
                // out of range reads 0 (WGSL would clamp the index)
                Some(r) => format!("rc(i32({n}) + {})", self.address(r)?),
                None => match self.defs.get(&n) {
                    Some(v) => vec4_literal(v),
                    None => format!("k.c[{n}]"),
                },
            },
            reg::CONSTINT => match self.int_defs.get(&n) {
                Some(v) => format!("vec4<f32>(vec4<i32>({}, {}, {}, {}))", v[0], v[1], v[2], v[3]),
                None => format!("vec4<f32>(k.i[{n}])"),
            },
            reg::MISCTYPE => if n == 0 { "vPos".into() } else { "vFace".into() },
            reg::LOOP => "vec4<f32>(f32(aL))".into(),
            reg::ADDR if self.shader.stage == Stage::Vertex => "vec4<f32>(a0)".into(),
            reg::COLOROUT => format!("oC{n}"),
            reg::DEPTHOUT => "oDepth".into(),
            k => return Err(Error::Unsupported(format!("source register type {k}"))),
        })
    }

    /// An address register as an i32 expression (aL, or a component of a0).
    fn address(&self, r: &Param) -> Result<String, Error> {
        match r.kind {
            reg::LOOP => Ok("aL".into()),
            reg::ADDR => Ok(format!("a0.{}", COMPONENTS[(r.mask_or_swizzle & 3) as usize])),
            k => Err(Error::Unsupported(format!("relative addressing through register type {k}"))),
        }
    }

    /// A source operand: register, swizzle, modifier.
    fn source(&self, p: &Param) -> Result<String, Error> {
        if p.kind == reg::PREDICATE {
            return Err(Error::Unsupported("predicate as a float source".into()));
        }
        let e = format!("{}{}", self.register(p)?, swizzle(p.mask_or_swizzle));
        Ok(match p.modifier {
            0 => e,
            1 => format!("(-{e})"),
            2 => format!("({e} - vec4<f32>(0.5))"),
            3 => format!("(-({e} - vec4<f32>(0.5)))"),
            4 => format!("(2.0 * ({e} - vec4<f32>(0.5)))"),
            5 => format!("(-2.0 * ({e} - vec4<f32>(0.5)))"),
            6 => format!("(vec4<f32>(1.0) - {e})"),
            7 => format!("(2.0 * {e})"),
            8 => format!("(-2.0 * {e})"),
            11 => format!("abs({e})"),
            12 => format!("(-abs({e}))"),
            m => return Err(Error::Unsupported(format!("source modifier {m}"))),
        })
    }

    /// The first component of a source (scalar instructions use a
    /// replicate swizzle, so every component is the selected one).
    fn scalar(&self, p: &Param) -> Result<String, Error> {
        Ok(format!("{}.x", self.source(p)?))
    }

    fn dst_name(&self, d: &Param) -> Result<String, Error> {
        Ok(match d.kind {
            reg::TEMP => format!("r{}", d.index),
            reg::COLOROUT => format!("oC{}", d.index),
            reg::DEPTHOUT => "oDepth".into(),
            reg::PREDICATE => "p0".into(),
            reg::OUTPUT if d.relative.is_none() => format!("o{}", d.index),
            reg::ADDR => "a0".into(),
            k => return Err(Error::Unsupported(format!("destination register type {k}"))),
        })
    }

    /// Write `value` (a vec4<f32> expression) through `d`'s mask, with
    /// saturation, under the instruction's predicate.
    fn write(&mut self, ins: &Instruction, d: &Param, value: String) -> Result<(), Error> {
        let name = self.dst_name(d)?;
        let t = self.temp();
        let value = if d.modifier & 1 != 0 { format!("clamp({value}, vec4<f32>(0.0), vec4<f32>(1.0))") } else { value };
        self.line(&format!("{{ let {t} = {value};"));
        let pred = match &ins.predicate {
            Some(p) => Some((p.mask_or_swizzle, p.modifier == 13)),
            None => None,
        };
        for (k, c) in COMPONENTS.iter().enumerate() {
            if d.mask_or_swizzle & (1 << k) == 0 {
                continue;
            }
            let assign = if name == "a0" {
                // mova rounds to the nearest integer
                format!("a0.{c} = i32(floor({t}.{c} + 0.5));")
            } else {
                format!("{name}.{c} = {t}.{c};")
            };
            match pred {
                Some((sw, not)) => {
                    let pc = COMPONENTS[((sw >> (2 * k)) & 3) as usize];
                    self.line(&format!("  if ({}p0.{pc}) {{ {assign} }}", if not { "!" } else { "" }));
                }
                None => self.line(&format!("  {assign}")),
            }
        }
        self.line("}");
        Ok(())
    }

    fn comparison(control: u8) -> Result<&'static str, Error> {
        Ok(match control {
            1 => ">",
            2 => "==",
            3 => ">=",
            4 => "<",
            5 => "!=",
            6 => "<=",
            c => return Err(Error::Unsupported(format!("comparison {c}"))),
        })
    }

    /// A boolean condition from `if`/`callnz`/`breakp` style operands.
    fn condition(&self, p: &Param) -> Result<String, Error> {
        match p.kind {
            reg::CONSTBOOL => Ok(match self.bool_defs.get(&p.index) {
                Some(b) => b.to_string(),
                None => format!("(k.b[{}].x != 0u)", p.index),
            }),
            reg::PREDICATE => {
                let c = COMPONENTS[(p.mask_or_swizzle & 3) as usize];
                Ok(format!("({}p0.{c})", if p.modifier == 13 { "!" } else { "" }))
            }
            k => Err(Error::Unsupported(format!("condition on register type {k}"))),
        }
    }

    fn int_constant(&self, p: &Param) -> String {
        match self.int_defs.get(&p.index) {
            Some(v) => format!("vec4<i32>({}, {}, {}, {})", v[0], v[1], v[2], v[3]),
            None => format!("k.i[{}]", p.index),
        }
    }

    /// A texture read. Pixel shaders sample with implicit derivatives
    /// (D3D9's texld picks the mip level from them; `Sampling::Bias` and
    /// `Sampling::Grad` are texldb and texldd), vertex shaders and texldl at
    /// an explicit level.
    fn sample(&self, coord: &str, sampler: &Param, how: Sampling) -> Result<String, Error> {
        let n = sampler.index;
        let kind = self.shader.instructions.iter()
            .find(|i| i.opcode == op::DCL && i.dst.as_ref().is_some_and(|d| d.kind == reg::SAMPLER && d.index == n))
            .map(|i| (i.decl.unwrap_or(0) >> 27) & 0xF)
            .ok_or_else(|| Error::Unsupported(format!("sampler s{n} not declared")))?;
        let c = match kind {
            3 | 4 => format!("({coord}).xyz"),
            _ => format!("({coord}).xy"),
        };
        let grad = |g: &str| match kind {
            3 | 4 => format!("({g}).xyz"),
            _ => format!("({g}).xy"),
        };
        let sampler_bias = format!("k.bias[{}].{}", n / 4, COMPONENTS[(n % 4) as usize]);
        let call = match how {
            Sampling::Level(lod) => format!("textureSampleLevel(t{n}, s{n}, {c}, {lod})"),
            _ if self.shader.stage == Stage::Vertex => format!("textureSampleLevel(t{n}, s{n}, {c}, 0.0)"),
            Sampling::Implicit if self.lod_bias => format!("textureSampleBias(t{n}, s{n}, {c}, {sampler_bias})"),
            Sampling::Implicit => format!("textureSample(t{n}, s{n}, {c})"),
            Sampling::Bias(b) if self.lod_bias => format!("textureSampleBias(t{n}, s{n}, {c}, {b} + {sampler_bias})"),
            Sampling::Bias(b) => format!("textureSampleBias(t{n}, s{n}, {c}, {b})"),
            Sampling::Grad(x, y) => format!("textureSampleGrad(t{n}, s{n}, {c}, {}, {})", grad(&x), grad(&y)),
        };
        // the sampler's swizzle applies to the result
        Ok(format!("{call}{}", swizzle(sampler.mask_or_swizzle)))
    }

    fn instruction(&mut self, ins: &Instruction) -> Result<(), Error> {
        let src = |k: usize| -> Result<String, Error> {
            ins.src.get(k).ok_or_else(|| Error::Unsupported(format!("operand {k} of opcode {}", ins.opcode))).and_then(|p| self.source(p))
        };
        let v4 = |e: String| format!("vec4<f32>({e})");
        let value = match ins.opcode {
            op::NOP => return Ok(()),
            op::MOV => src(0)?,
            op::ADD => format!("({} + {})", src(0)?, src(1)?),
            op::SUB => format!("({} - {})", src(0)?, src(1)?),
            op::MUL => format!("m9({}, {})", src(0)?, src(1)?),
            op::MAD => format!("fma9({}, {}, {})", src(0)?, src(1)?, src(2)?),
            op::DP3 => v4(format!("dot3_9({}, {})", src(0)?, src(1)?)),
            op::DP4 => v4(format!("dot4_9({}, {})", src(0)?, src(1)?)),
            op::DP2ADD => v4(format!("dp2add9({}, {}, ({}).x)", src(0)?, src(1)?, src(2)?)),
            op::MIN => format!("min({}, {})", src(0)?, src(1)?),
            op::MAX => format!("max({}, {})", src(0)?, src(1)?),
            op::SLT => format!("select(vec4<f32>(0.0), vec4<f32>(1.0), {} < {})", src(0)?, src(1)?),
            op::SGE => format!("select(vec4<f32>(0.0), vec4<f32>(1.0), {} >= {})", src(0)?, src(1)?),
            op::RCP => v4(format!("1.0 / {}", self.scalar(&ins.src[0])?)),
            op::RSQ => v4(format!("inverseSqrt(abs({}))", self.scalar(&ins.src[0])?)),
            op::EXP => v4(format!("exp2({})", self.scalar(&ins.src[0])?)),
            op::LOG => v4(format!("log2(abs({}))", self.scalar(&ins.src[0])?)),
            op::POW => v4(format!("exp2(m9(vec4<f32>({}), vec4<f32>(log2(abs({})))).x)", self.scalar(&ins.src[1])?, self.scalar(&ins.src[0])?)),
            op::FRC => format!("frc9({})", src(0)?),
            op::ABS => format!("abs({})", src(0)?),
            op::NRM => {
                let a = src(0)?;
                format!("m9({a}, vec4<f32>(inverseSqrt(dot3_9({a}, {a}))))")
            }
            op::LRP => {
                let (a, b, c) = (src(0)?, src(1)?, src(2)?);
                format!("fma9({a}, {b} - {c}, {c})")
            }
            op::CMP => format!("select({}, {}, {} >= vec4<f32>(0.0))", src(2)?, src(1)?, src(0)?),
            op::CND => format!("select({}, {}, {} > vec4<f32>(0.5))", src(2)?, src(1)?, src(0)?),
            op::CRS if self.shader.stage == Stage::Vertex => format!("crs9v({}, {})", src(0)?, src(1)?),
            op::CRS => format!("crs9({}, {})", src(0)?, src(1)?),
            op::SINCOS => {
                let x = self.scalar(&ins.src[0])?;
                if self.shader.stage == Stage::Vertex {
                    format!("vec4<f32>(cos({x}), sin({x}), 0.0, 0.0)")
                } else {
                    // pixel shaders: a zero or denormal angle is +0 (sin(-0) = +0)
                    format!("sincos9({x})")
                }
            }
            op::MOVA => src(0)?,
            op::SGN => format!("sign({})", src(0)?),
            op::EXPP => v4(format!("exp2({})", self.scalar(&ins.src[0])?)),
            op::LOGP => v4(format!("log2(abs({}))", self.scalar(&ins.src[0])?)),
            op::LIT => {
                let a = src(0)?;
                format!("vec4<f32>(1.0, max(({a}).x, 0.0), select(0.0, exp2(m9(vec4<f32>(clamp(({a}).w, -128.0, 128.0)), vec4<f32>(log2(({a}).y))).x), ({a}).x > 0.0 && ({a}).y > 0.0), 1.0)")
            }
            op::DST => format!("vec4<f32>(1.0, m9({}, {}).y, ({}).z, ({}).w)", src(0)?, src(1)?, src(0)?, src(1)?),
            op::DSX => format!("dpdx({})", src(0)?),
            op::DSY => format!("dpdy({})", src(0)?),
            op::M4X4 | op::M4X3 | op::M3X4 | op::M3X3 | op::M3X2 => {
                let (rows, three) = match ins.opcode {
                    op::M4X4 => (4, false),
                    op::M4X3 => (3, false),
                    op::M3X4 => (4, true),
                    op::M3X3 => (3, true),
                    _ => (2, true),
                };
                let a = src(0)?;
                let mut parts = Vec::new();
                for row in 0..4 {
                    if row >= rows {
                        parts.push("0.0".to_string());
                        continue;
                    }
                    let mut m = ins.src[1].clone();
                    m.index += row;
                    let b = self.source(&m)?;
                    parts.push(if three { format!("dot3_9({a}, {b})") } else { format!("dot4_9({a}, {b})") });
                }
                format!("vec4<f32>({})", parts.join(", "))
            }
            op::TEX => {
                let coord = src(0)?;
                match ins.control {
                    // texldp: projective
                    1 => self.sample(&format!("({coord} / ({coord}).w)"), &ins.src[1], Sampling::Implicit)?,
                    // texldb: level-of-detail bias from .w
                    2 => self.sample(&coord, &ins.src[1], Sampling::Bias(format!("({coord}).w")))?,
                    _ => self.sample(&coord, &ins.src[1], Sampling::Implicit)?,
                }
            }
            op::TEXLDL => {
                let coord = src(0)?;
                self.sample(&coord, &ins.src[1], Sampling::Level(format!("({coord}).w")))?
            }
            op::TEXLDD => self.sample(&src(0)?, &ins.src[1], Sampling::Grad(src(2)?, src(3)?))?,
            op::TEXKILL => {
                let d = ins.dst.as_ref().unwrap();
                let name = match d.kind {
                    reg::TEMP => format!("r{}", d.index),
                    reg::INPUT => format!("v{}", d.index),
                    k => return Err(Error::Unsupported(format!("texkill of register type {k}"))),
                };
                let tests: Vec<String> = (0..4).filter(|k| d.mask_or_swizzle & (1 << k) != 0)
                    .map(|k| format!("{name}.{} < 0.0", COMPONENTS[k])).collect();
                self.line(&format!("if ({}) {{ discard; }}", tests.join(" || ")));
                return Ok(());
            }
            op::SETP => {
                let cmp = Self::comparison(ins.control)?;
                let (a, b) = (src(0)?, src(1)?);
                let d = ins.dst.as_ref().unwrap();
                for (k, c) in COMPONENTS.iter().enumerate() {
                    if d.mask_or_swizzle & (1 << k) != 0 {
                        self.line(&format!("p0.{c} = ({a}).{c} {cmp} ({b}).{c};"));
                    }
                }
                return Ok(());
            }
            op::IF => {
                let c = self.condition(&ins.src[0])?;
                self.line(&format!("if {c} {{"));
                self.indent += 1;
                return Ok(());
            }
            op::IFC => {
                let cmp = Self::comparison(ins.control)?;
                self.line(&format!("if (({}).x {cmp} ({}).x) {{", src(0)?, src(1)?));
                self.indent += 1;
                return Ok(());
            }
            op::ELSE => {
                self.indent -= 1;
                self.line("} else {");
                self.indent += 1;
                return Ok(());
            }
            op::ENDIF | op::ENDREP => {
                self.indent -= 1;
                self.line("}");
                return Ok(());
            }
            op::ENDLOOP => {
                self.line("aL = aL + step;");
                self.indent -= 1;
                self.line("}");
                self.indent -= 1;
                self.line("aL = saved_aL;");
                self.line("}");
                self.loop_depth -= 1;
                return Ok(());
            }
            op::REP => {
                let i = self.int_constant(&ins.src[0]);
                let j = self.temp();
                self.line(&format!("for (var {j} = 0; {j} < ({i}).x; {j} = {j} + 1) {{"));
                self.indent += 1;
                return Ok(());
            }
            op::LOOP => {
                // loop aL, i#: count i.x, start i.y, step i.z
                let i = self.int_constant(&ins.src[1]);
                let j = self.temp();
                self.line("{");
                self.indent += 1;
                self.line("let saved_aL = aL;");
                self.line(&format!("let step = ({i}).z;"));
                self.line(&format!("aL = ({i}).y;"));
                self.line(&format!("for (var {j} = 0; {j} < ({i}).x; {j} = {j} + 1) {{"));
                self.indent += 1;
                self.loop_depth += 1;
                return Ok(());
            }
            op::BREAK => {
                self.line("break;");
                return Ok(());
            }
            op::BREAKC => {
                let cmp = Self::comparison(ins.control)?;
                self.line(&format!("if (({}).x {cmp} ({}).x) {{ break; }}", src(0)?, src(1)?));
                return Ok(());
            }
            op::BREAKP => {
                let c = self.condition(&ins.src[0])?;
                self.line(&format!("if {c} {{ break; }}"));
                return Ok(());
            }
            op::CALL => {
                self.line(&format!("l{}();", ins.src[0].index));
                return Ok(());
            }
            op::CALLNZ => {
                let c = self.condition(&ins.src[1])?;
                self.line(&format!("if {c} {{ l{}(); }}", ins.src[0].index));
                return Ok(());
            }
            o => return Err(Error::Unsupported(format!("opcode {o}"))),
        };
        let d = ins.dst.as_ref().ok_or_else(|| Error::Unsupported(format!("opcode {} without destination", ins.opcode)))?;
        self.write(ins, d, value)
    }
}

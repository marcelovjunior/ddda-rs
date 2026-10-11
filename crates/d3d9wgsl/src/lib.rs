//! Direct3D 9 shader model 3 pixel shaders translated to WGSL for the
//! runtime's GPU backend (docs/RUNTIME.md, "Graphics").
//!
//! The translation is literal: every D3D9 register is a WGSL variable,
//! every instruction a statement, with the semantics Microsoft documents
//! for it (Microsoft Learn, "Asm Shader Reference"). It is checked by
//! running the game's shaders both ways: on the host's D3D9 driver
//! (crates/d3d9ref) and translated on wgpu (tests/against_d3d9.rs).
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
//! Multiplication follows D3D9's rule, not IEEE's: 0 * anything is 0, also
//! 0 * inf and 0 * NaN (shader model 3 hardware; the reference driver
//! shows it, and DXVK emulates the same). It applies to mul, mad, the dot
//! products, the matrix macros, nrm, lrp and pow (exp2(y * log2|x|)).
//!
//! Texture reads use explicit level 0 for now (exact for single-level
//! textures); derivative-based level selection comes with the backend.
use d3d9shader::{op, reg, Instruction, Param, Shader, Stage};
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NotPixelShader3,
    Unsupported(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotPixelShader3 => write!(f, "only ps_3_0 is translated"),
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
}

const COMPONENTS: [char; 4] = ['x', 'y', 'z', 'w'];

/// Translate a ps_3_0 shader.
pub fn translate(shader: &Shader) -> Result<Translation, Error> {
    if shader.stage != Stage::Pixel || shader.major != 3 {
        return Err(Error::NotPixelShader3);
    }
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
    writeln!(w, "struct Constants {{ c: array<vec4<f32>, 224>, i: array<vec4<i32>, 16>, b: array<vec4<u32>, 16> }}").unwrap();
    writeln!(w, "@group(0) @binding(0) var<uniform> k: Constants;").unwrap();
    // D3D9 multiplication: 0 * anything = 0
    // (zero tested on the bits: GPU compilers may fold float compares as if
    // there were no NaNs, and NaN * x must stay NaN)
    writeln!(w, "fn zero9(a: vec4<f32>) -> vec4<bool> {{ return (bitcast<vec4<u32>>(a) & vec4<u32>(0x7fffffffu)) == vec4<u32>(0u); }}").unwrap();
    writeln!(w, "fn m9(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {{ return select(a * b, vec4<f32>(0.0), zero9(a) | zero9(b)); }}").unwrap();
    writeln!(w, "fn dot3_9(a: vec4<f32>, b: vec4<f32>) -> f32 {{ let p = m9(a, b); return p.x + p.y + p.z; }}").unwrap();
    writeln!(w, "fn dot4_9(a: vec4<f32>, b: vec4<f32>) -> f32 {{ let p = m9(a, b); return p.x + p.y + p.z + p.w; }}").unwrap();
    for &(n, kind) in &iface.samplers {
        let t = match kind {
            TextureKind::Flat => "texture_2d<f32>",
            TextureKind::Cube => "texture_cube<f32>",
            TextureKind::Volume => "texture_3d<f32>",
        };
        writeln!(w, "@group(1) @binding({}) var t{n}: {t};", 2 * n).unwrap();
        writeln!(w, "@group(1) @binding({}) var s{n}: sampler;", 2 * n + 1).unwrap();
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
    writeln!(w, "var<private> oDepth: vec4<f32>;").unwrap();
    writeln!(w, "var<private> p0: vec4<bool>;").unwrap();
    writeln!(w, "var<private> aL: i32;").unwrap();
    writeln!(w, "var<private> vPos: vec4<f32>;").unwrap();
    writeln!(w, "var<private> vFace: vec4<f32>;").unwrap();
    if cx.relative_constants {
        writeln!(w, "var<private> cc: array<vec4<f32>, 224>;").unwrap();
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
    // entry point
    writeln!(w, "struct In {{").unwrap();
    writeln!(w, "  @builtin(position) position: vec4<f32>,").unwrap();
    writeln!(w, "  @builtin(front_facing) front: bool,").unwrap();
    for (k, &(_, _, r)) in iface.inputs.iter().enumerate() {
        writeln!(w, "  @location({k}) v{r}: vec4<f32>,").unwrap();
    }
    writeln!(w, "}}").unwrap();
    writeln!(w, "struct Out {{").unwrap();
    for &n in &iface.outputs {
        writeln!(w, "  @location({n}) oC{n}: vec4<f32>,").unwrap();
    }
    if iface.writes_depth {
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
    for &(_, _, r) in &iface.inputs {
        writeln!(w, "  v{r} = input.v{r};").unwrap();
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
    write!(w, "  return Out(").unwrap();
    let mut fields: Vec<String> = iface.outputs.iter().map(|n| format!("oC{n}")).collect();
    if iface.writes_depth {
        fields.push("oDepth.x".into());
    }
    writeln!(w, "{});\n}}", fields.join(", ")).unwrap();
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
                Some(r) => format!("cc[i32({n}) + {}]", self.address(r)?),
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
            reg::COLOROUT => format!("oC{n}"),
            reg::DEPTHOUT => "oDepth".into(),
            k => return Err(Error::Unsupported(format!("source register type {k}"))),
        })
    }

    /// An address register as an i32 expression (aL, or a component of a0).
    fn address(&self, r: &Param) -> Result<String, Error> {
        match r.kind {
            reg::LOOP => Ok("aL".into()),
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
            let assign = format!("{name}.{c} = {t}.{c};");
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

    fn sample(&self, coord: &str, sampler: &Param, lod: Option<String>) -> Result<String, Error> {
        let n = sampler.index;
        let kind = self.shader.instructions.iter()
            .find(|i| i.opcode == op::DCL && i.dst.as_ref().is_some_and(|d| d.kind == reg::SAMPLER && d.index == n))
            .map(|i| (i.decl.unwrap_or(0) >> 27) & 0xF)
            .ok_or_else(|| Error::Unsupported(format!("sampler s{n} not declared")))?;
        let c = match kind {
            3 | 4 => format!("({coord}).xyz"),
            _ => format!("({coord}).xy"),
        };
        // the sampler's swizzle applies to the result
        let lod = lod.unwrap_or_else(|| "0.0".into());
        Ok(format!("textureSampleLevel(t{n}, s{n}, {c}, {lod}){}", swizzle(sampler.mask_or_swizzle)))
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
            op::MAD => format!("(m9({}, {}) + {})", src(0)?, src(1)?, src(2)?),
            op::DP3 => v4(format!("dot3_9({}, {})", src(0)?, src(1)?)),
            op::DP4 => v4(format!("dot4_9({}, {})", src(0)?, src(1)?)),
            op::DP2ADD => {
                let p = format!("m9({}, {})", src(0)?, src(1)?);
                v4(format!("({p}).x + ({p}).y + ({}).x", src(2)?))
            }
            op::MIN => format!("min({}, {})", src(0)?, src(1)?),
            op::MAX => format!("max({}, {})", src(0)?, src(1)?),
            op::SLT => format!("select(vec4<f32>(0.0), vec4<f32>(1.0), {} < {})", src(0)?, src(1)?),
            op::SGE => format!("select(vec4<f32>(0.0), vec4<f32>(1.0), {} >= {})", src(0)?, src(1)?),
            op::RCP => v4(format!("1.0 / {}", self.scalar(&ins.src[0])?)),
            op::RSQ => v4(format!("inverseSqrt(abs({}))", self.scalar(&ins.src[0])?)),
            op::EXP => v4(format!("exp2({})", self.scalar(&ins.src[0])?)),
            op::LOG => v4(format!("log2(abs({}))", self.scalar(&ins.src[0])?)),
            op::POW => v4(format!("exp2(m9(vec4<f32>({}), vec4<f32>(log2(abs({})))).x)", self.scalar(&ins.src[1])?, self.scalar(&ins.src[0])?)),
            op::FRC => format!("fract({})", src(0)?),
            op::ABS => format!("abs({})", src(0)?),
            op::NRM => {
                let a = src(0)?;
                format!("m9({a}, vec4<f32>(inverseSqrt(dot3_9({a}, {a}))))")
            }
            op::LRP => {
                let (a, b, c) = (src(0)?, src(1)?, src(2)?);
                format!("(m9({a}, {b} - {c}) + {c})")
            }
            op::CMP => format!("select({}, {}, {} >= vec4<f32>(0.0))", src(2)?, src(1)?, src(0)?),
            op::CND => format!("select({}, {}, {} > vec4<f32>(0.5))", src(2)?, src(1)?, src(0)?),
            op::CRS => format!("vec4<f32>(cross(({}).xyz, ({}).xyz), 0.0)", src(0)?, src(1)?),
            op::SINCOS => {
                let x = self.scalar(&ins.src[0])?;
                format!("vec4<f32>(cos({x}), sin({x}), 0.0, 0.0)")
            }
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
                let coord = if ins.control == 1 { format!("({coord} / ({coord}).w)") } else { coord };
                self.sample(&coord, &ins.src[1], None)?
            }
            op::TEXLDL => {
                let coord = src(0)?;
                self.sample(&coord, &ins.src[1], Some(format!("({coord}).w")))?
            }
            op::TEXLDD => self.sample(&src(0)?, &ins.src[1], None)?,
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

//! Full-target quad draws run as compute dispatches (L113, `DDGPU_NATIVE_SIM=1`).
//!
//! The game's GPU soft-body solver (docs/SOFTBODY.md) draws ~900 one-quad
//! passes a gameplay frame into tiny float targets; each is a render pass of
//! its own here, and render passes are what costs. A draw that meets all of
//! the following runs instead as a dispatch in one compute pass, which wgpu
//! orders and synchronises dispatch by dispatch:
//!
//! - the pixel shader's interpolated inputs only address texture fetches
//!   (`d3d9shader::flow`), every such fetch samples one texel (point
//!   filtering, one level) and none uses an implicit level: so no result
//!   bit depends on how the interpolants are computed, as long as each
//!   lies in its pixel's texel;
//! - the vertex shader, evaluated here for the 4 vertices of the strip,
//!   draws an axis-aligned rectangle with w = 1 covering every pixel centre
//!   of the viewport, which is the whole target, with no culling;
//! - the targets are A32B32G32R32F render-target textures of one size, no
//!   depth, no blending, alpha test, scissor or sRGB write, all channels
//!   written, and no target is also sampled; no occlusion query is counting.
//!
//! The dispatch runs the pixel shader's translation with the options the
//! draw path uses (the same arithmetic) and the same constant block; the
//! interpolants are evaluated at each pixel centre from the plane through
//! the evaluated vertices.
use super::*;
use d3d9shader::{op, reg, Shader};

/// A32B32G32R32F
const F32X4: u32 = 116;

/// Whether `DDGPU_NATIVE_SIM=1`.
pub(crate) fn enabled() -> bool {
    std::env::var("DDGPU_NATIVE_SIM").is_ok_and(|v| v == "1")
}

/// What a pixel shader needs to run as a dispatch.
pub(crate) struct QuadPs {
    pipeline: wgpu::ComputePipeline,
    out_layout: wgpu::BindGroupLayout,
    /// colour outputs written (oC#), in order
    outputs: Vec<u32>,
    /// declared inputs: (usage, usage index)
    inputs: Vec<(u32, u32)>,
    /// samplers whose coordinates come from the interpolants
    addressed: u32,
    /// the pixel textures (sampler, kind) and their layout, as the draw path has them
    kinds: Vec<(u32, u8)>,
    layout1: wgpu::BindGroupLayout,
    /// float constants the translation reads
    floats: usize,
}

/// A vertex shader the CPU can evaluate: its instructions and outputs.
pub(crate) struct QuadVs {
    shader: Shader,
    /// the def constants by register
    defs: Vec<Option<[f32; 4]>>,
    /// declared inputs: (usage, usage index, register)
    inputs: Vec<(u32, u32, u32)>,
    /// declared outputs: (usage, usage index, register)
    outputs: Vec<(u32, u32, u32)>,
}

/// The per-shader results, by the shader's identity (its tokens, as the
/// draw path keys shaders: a replayed capture brings every draw its own copy).
#[derive(Default)]
pub(crate) struct QuadCache {
    ps: HashMap<u64, Option<Arc<QuadPs>>>,
    vs: HashMap<u64, Option<Arc<QuadVs>>>,
    /// output bind groups by (pixel shader identity, target resources)
    outputs: HashMap<(u64, Vec<u32>), wgpu::BindGroup>,
    pub(crate) dispatches: u64,
}

fn declarations(s: &Shader, kind: u8) -> Vec<(u32, u32, u32)> {
    s.instructions
        .iter()
        .filter(|i| i.opcode == op::DCL)
        .filter_map(|i| match (&i.dst, i.decl) {
            (Some(d), Some(t)) if d.kind == kind => Some((t & 0x1F, (t >> 16) & 0xF, d.index)),
            _ => None,
        })
        .collect()
}

/// Evaluate a vertex shader made only of mov/add/mul/mad/rcp/dp3/dp4 on
/// temporaries, inputs and float constants: its output registers.
fn evaluate(vs: &QuadVs, inputs: &[[f32; 4]; 16], constants: &d3d9cmd::Constants) -> Result<[[f32; 4]; 16], String> {
    let mut temps = [[0.0f32; 4]; 32];
    let mut outs = [[0.0f32; 4]; 16];
    for ins in &vs.shader.instructions {
        match ins.opcode {
            op::COMMENT | op::DCL | op::NOP | op::END | op::DEF => continue,
            op::MOV | op::ADD | op::MUL | op::MAD | op::RCP | op::DP3 | op::DP4 => {}
            o => return Err(format!("vertex shader opcode {o}")),
        }
        if ins.predicated || ins.coissue {
            return Err("predicated vertex shader instruction".into());
        }
        let mut src = [[0.0f32; 4]; 3];
        for (n, p) in ins.src.iter().enumerate().take(3) {
            if p.relative.is_some() {
                return Err("relative addressing in the vertex shader".into());
            }
            let i = p.index as usize;
            let v = match p.kind {
                reg::TEMP => *temps.get(i).ok_or("temporary register")?,
                reg::INPUT => *inputs.get(i).ok_or("input register")?,
                reg::CONST => match vs.defs.get(i).copied().flatten() {
                    Some(v) => v,
                    None => constants.f.get(i).map_or([0.0; 4], |c| c.map(f32::from_bits)),
                },
                k => return Err(format!("vertex shader source register kind {k}")),
            };
            let mut v: [f32; 4] = std::array::from_fn(|k| v[((p.mask_or_swizzle >> (2 * k)) & 3) as usize]);
            match p.modifier {
                0 => {}
                1 => v = v.map(|x| -x),
                11 => v = v.map(f32::abs),
                12 => v = v.map(|x| -x.abs()),
                m => return Err(format!("vertex shader source modifier {m}")),
            }
            src[n] = v;
        }
        let [a, b, c] = src;
        let r: [f32; 4] = match ins.opcode {
            op::MOV => a,
            op::ADD => std::array::from_fn(|k| a[k] + b[k]),
            op::MUL => std::array::from_fn(|k| a[k] * b[k]),
            op::MAD => std::array::from_fn(|k| a[k].mul_add(b[k], c[k])),
            op::RCP => [1.0 / a[0]; 4],
            op::DP3 => [a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; 4],
            _ => [a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]; 4],
        };
        let d = ins.dst.as_ref().ok_or("no destination")?;
        if d.shift != 0 || d.modifier & !1 != 0 {
            return Err("vertex shader result modifier".into());
        }
        let r = if d.modifier & 1 != 0 { r.map(|x| x.clamp(0.0, 1.0)) } else { r };
        let target = match d.kind {
            reg::TEMP => temps.get_mut(d.index as usize).ok_or("temporary register")?,
            reg::OUTPUT => outs.get_mut(d.index as usize).ok_or("output register")?,
            k => return Err(format!("vertex shader destination register kind {k}")),
        };
        for k in 0..4 {
            if d.mask_or_swizzle & (1 << k) != 0 {
                target[k] = r[k];
            }
        }
    }
    Ok(outs)
}

/// A vertex element's value (FLOAT1..4 and D3DCOLOR).
fn element(bytes: &[u8], ty: u8) -> Option<[f32; 4]> {
    let f = |k: usize| bytes.get(4 * k..4 * k + 4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    Some(match ty {
        0 => [f(0)?, 0.0, 0.0, 1.0],
        1 => [f(0)?, f(1)?, 0.0, 1.0],
        2 => [f(0)?, f(1)?, f(2)?, 1.0],
        3 => [f(0)?, f(1)?, f(2)?, f(3)?],
        4 => {
            let c = bytes.get(..4)?;
            // D3DCOLOR (BGRA bytes) arrives as RGBA
            [c[2] as f32 / 255.0, c[1] as f32 / 255.0, c[0] as f32 / 255.0, c[3] as f32 / 255.0]
        }
        _ => return None,
    })
}

impl QuadCache {
    /// Drop the output groups that bind a freed resource.
    pub(crate) fn forget(&mut self, resource: u32) {
        self.outputs.retain(|k, _| !k.1.contains(&resource));
    }
}

/// What one eligible draw dispatches.
struct Plan {
    ps: Arc<QuadPs>,
    /// the pixel shader's identity
    ps_key: u64,
    width: u32,
    height: u32,
    /// per pixel-shader input: the plane a + b * x + c * y in pixel units
    planes: Vec<[[f32; 4]; 3]>,
    targets: Vec<u32>,
}

impl WgpuBackend {
    fn quad_ps(&mut self, d: &DrawCall, tokens: &Arc<[u32]>) -> Option<Arc<QuadPs>> {
        let key = self.shader_identities.key(tokens);
        if let Some(q) = self.quads.ps.get(&key) {
            return q.clone();
        }
        let q = self.build_quad_ps(d, tokens).map(Arc::new);
        if let Err(e) = &q {
            if self.errors.len() < 1000 && std::env::var_os("DDGPU_NATIVE_SIM_WHY").is_some() {
                self.errors.push(format!("not a quad dispatch: {e}"));
            }
        }
        self.quads.ps.insert(key, q.as_ref().ok().cloned());
        q.ok()
    }

    fn build_quad_ps(&mut self, d: &DrawCall, tokens: &Arc<[u32]>) -> Result<QuadPs, String> {
        let shader = d3d9shader::parse(tokens).map_err(|e| e.to_string())?;
        let flow = d3d9shader::flow::interpolants_only_address(&shader)?;
        if flow.implicit_level != 0 {
            return Err("a fetch with an implicit level".into());
        }
        if shader.instructions.iter().any(|i| i.src.iter().chain(i.dst.iter()).any(|p| p.kind == reg::MISCTYPE)) {
            return Err("vPos or vFace".into());
        }
        let inputs: Vec<(u32, u32)> = declarations(&shader, reg::INPUT).iter().map(|&(u, n, _)| (u, n)).collect();
        // the translation the draw path makes, with inputs at their declaration
        // order and without the alpha test (an eligible draw has it off, and
        // its discard is not allowed in a compute stage)
        let options = d3d9wgsl::Options {
            ps_input_locations: Some((0..inputs.len() as u32).map(Some).collect()),
            alpha_test: false,
            lod_bias: true,
            sampler_ones: sampler_ones(d),
            compact_constants: true,
            ..Default::default()
        };
        let t = d3d9wgsl::translate_with(&shader, options).map_err(|e| e.to_string())?;
        if t.interface.writes_depth || t.interface.outputs.is_empty() {
            return Err("writes depth or no colour".into());
        }
        let entry = "@fragment\nfn main(input: In) -> Out {";
        if !t.wgsl.contains(entry) {
            return Err("unexpected translation entry".into());
        }
        let mut wgsl = t.wgsl.replace(entry, "fn ps_main(input: In) -> Out {");
        let n = inputs.len().max(1);
        use std::fmt::Write as _;
        writeln!(wgsl, "struct QuadParams {{ size: vec4<u32>, plane: array<vec4<f32>, {}> }}", 3 * n).unwrap();
        writeln!(wgsl, "@group(2) @binding(8) var<uniform> quad: QuadParams;").unwrap();
        for &o in &t.interface.outputs {
            writeln!(wgsl, "@group(2) @binding({o}) var out{o}: texture_storage_2d<rgba32float, write>;").unwrap();
        }
        writeln!(wgsl, "@compute @workgroup_size(8, 8)\nfn quad_main(@builtin(global_invocation_id) id: vec3<u32>) {{").unwrap();
        writeln!(wgsl, "  if (id.x >= quad.size.x || id.y >= quad.size.y) {{ return; }}").unwrap();
        writeln!(wgsl, "  let x = f32(id.x) + 0.5;\n  let y = f32(id.y) + 0.5;\n  var input: In;").unwrap();
        for (k, &(_, _, r)) in declarations(&shader, reg::INPUT).iter().enumerate() {
            writeln!(wgsl, "  input.v{r} = quad.plane[{}] + quad.plane[{}] * x + quad.plane[{}] * y;", 3 * k, 3 * k + 1, 3 * k + 2).unwrap();
        }
        writeln!(wgsl, "  let o = ps_main(input);").unwrap();
        for &o in &t.interface.outputs {
            writeln!(wgsl, "  textureStore(out{o}, vec2<i32>(id.xy), o.oC{o});").unwrap();
        }
        writeln!(wgsl, "}}").unwrap();
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("quad dispatch"), source: wgpu::ShaderSource::Wgsl(wgsl.into()) });
        // the layouts: the draw path's constants and pixel textures, then the outputs
        let kinds: Vec<(u32, u8)> = t.interface.samplers.iter().map(|&(n, k)| (n, match k {
            d3d9wgsl::TextureKind::Flat => 0,
            d3d9wgsl::TextureKind::Cube => 1,
            d3d9wgsl::TextureKind::Volume => 2,
        })).collect();
        let layout1 = self.tex_layout(false, &kinds);
        let mut entries: Vec<wgpu::BindGroupLayoutEntry> = t.interface.outputs.iter().map(|&o| wgpu::BindGroupLayoutEntry {
            binding: o,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba32Float, view_dimension: wgpu::TextureViewDimension::D2 },
            count: None,
        }).collect();
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 8,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: None },
            count: None,
        });
        let out_layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("quad outputs"), entries: &entries });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad dispatch"),
            bind_group_layouts: &[Some(&self.layout0), Some(&layout1), Some(&out_layout)],
            immediate_size: 0,
        });
        let pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("quad dispatch"),
            layout: Some(&layout),
            module: &module,
            entry_point: Some("quad_main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(e) = block_on(scope.pop()) {
            return Err(format!("compute pipeline: {e}"));
        }
        Ok(QuadPs { pipeline, out_layout, outputs: t.interface.outputs.clone(), inputs, addressed: flow.samplers, kinds, layout1, floats: t.interface.float_constants })
    }

    fn quad_vs(&mut self, tokens: &Arc<[u32]>) -> Option<Arc<QuadVs>> {
        let key = self.shader_identities.key(tokens);
        if let Some(q) = self.quads.vs.get(&key) {
            return q.clone();
        }
        let q = d3d9shader::parse(tokens).ok().filter(|s| s.stage == d3d9shader::Stage::Vertex).map(|shader| {
            let mut defs = vec![None; 256];
            for i in shader.instructions.iter().filter(|i| i.opcode == op::DEF) {
                if let (Some(d), true) = (&i.dst, i.data.len() >= 4) {
                    if let Some(slot) = defs.get_mut(d.index as usize) {
                        *slot = Some(std::array::from_fn(|k| f32::from_bits(i.data[k])));
                    }
                }
            }
            Arc::new(QuadVs { inputs: declarations(&shader, reg::INPUT), outputs: declarations(&shader, reg::OUTPUT), defs, shader })
        });
        self.quads.vs.insert(key, q.clone());
        q
    }

    /// The dispatch an eligible draw makes (see the module documentation).
    fn quad_plan(&mut self, d: &DrawCall) -> Result<Plan, &'static str> {
        let state = |s: u32| d.render_states.binary_search_by_key(&s, |&(k, _)| k).ok().map(|i| d.render_states[i].1);
        // a dispatch counts no occlusion samples
        if !self.active_queries.is_empty() {
            return Err("occlusion query");
        }
        // targets: float textures of one size, no depth
        let targets: Vec<&Target> = d.targets.iter().flatten().collect();
        let first = targets.first().ok_or("no target")?;
        let (w, h) = (first.width, first.height);
        if d.depth.is_some() || targets.iter().any(|t| t.format != F32X4 || t.texture.is_none() || t.level != 0 || t.samples > 1 || (t.width, t.height) != (w, h)) {
            return Err("target format, size or depth");
        }
        // the state: no blending, alpha test, scissor, sRGB write or culling;
        // every channel written (COLORWRITEENABLE, ..1..3)
        if state(27).unwrap_or(0) != 0 || state(15).unwrap_or(0) != 0 || state(174).unwrap_or(0) != 0
            || state(194).unwrap_or(0) != 0 || state(22).unwrap_or(3) != 1 {
            return Err("blend, alpha test, scissor, sRGB or cull");
        }
        let masks = [168, 190, 191, 192];
        if d.targets.iter().enumerate().any(|(k, t)| t.is_some() && state(masks[k]).unwrap_or(15) & 15 != 15) {
            return Err("colour write mask");
        }
        // the viewport is the whole target
        if d.viewport[..4] != [0, 0, w, h] {
            return Err("viewport");
        }
        // two triangles of a strip, not indexed
        if d.primitive != 5 || d.primitives != 2 || !matches!(d.indices, Indices::None) {
            return Err("primitive");
        }
        let resources: Vec<u32> = targets.iter().map(|t| t.texture.unwrap().resource).collect();
        if d.textures.iter().any(|t| resources.contains(&t.resource)) {
            return Err("target also sampled");
        }
        let ps_tokens = d.pixel_shader.as_ref().ok_or("no pixel shader")?;
        let ps_key = self.shader_identities.key(ps_tokens);
        let ps = self.quad_ps(d, ps_tokens).ok_or("pixel shader")?;
        if ps.outputs.iter().any(|&o| d.targets.get(o as usize).is_none_or(|t| t.is_none())) {
            return Err("output without a target");
        }
        // the addressed samplers read one texel: point filters, one level
        for t in d.textures.iter().filter(|t| t.sampler < 16 && ps.addressed & (1 << t.sampler) != 0) {
            if t.states[5] != 1 || t.states[6] != 1 || (t.levels != 1 && t.states[7] != 0) || t.kind != d3d9cmd::TexKind::Flat {
                return Err("addressed sampler not point");
            }
        }
        // the vertices through the vertex shader
        let vs = self.quad_vs(d.vertex_shader.as_ref().ok_or("no vertex shader")?).ok_or("vertex shader")?;
        let mut vertices = Vec::with_capacity(4);
        for i in 0..4u32 {
            let mut inputs = [[0.0f32; 4]; 16];
            for &(usage, index, r) in &vs.inputs {
                let e = d.declaration.iter().find(|e| e[6] as u32 == usage && e[7] as u32 == index).ok_or("vertex element")?;
                let (stream, offset, ty) = (u16::from_le_bytes([e[0], e[1]]) as u32, u16::from_le_bytes([e[2], e[3]]) as usize, e[4]);
                let bytes: &[u8] = match &d.vertices {
                    Vertices::Streams(v) => {
                        let &(_, res, off, stride, _) = v.iter().find(|x| x.0 == stream).ok_or("stream")?;
                        let b = self.small_buffers.get(&res).ok_or("vertex buffer bytes")?;
                        b.get(off as usize + (d.start + i) as usize * stride as usize + offset..).ok_or("vertex out of the buffer")?
                    }
                    Vertices::Inline { bytes, stride } => bytes.get((d.start + i) as usize * *stride as usize + offset..).ok_or("vertex out of the data")?,
                };
                *inputs.get_mut(r as usize).ok_or("input register")? = element(bytes, ty).ok_or("vertex element type")?;
            }
            vertices.push(evaluate(&vs, &inputs, &d.vs_constants).map_err(|_| "vertex shader evaluation")?);
        }
        let position = vs.outputs.iter().find(|o| o.0 == 0 && o.1 == 0).ok_or("no position output")?.2;
        // the draw path's half-pixel move (d3d9wgsl Options::half_pixel),
        // then pixel coordinates
        let (vw, vh) = (w as f32, h as f32);
        let mut screen = Vec::new();
        for v in &vertices {
            let p = *v.get(position as usize).ok_or("position register")?;
            if p[3] != 1.0 || p[2] < 0.0 || p[2] > 1.0 {
                return Err("w or z");
            }
            let (x, y) = (p[0] + 1.0 / vw, p[1] - 1.0 / vh);
            screen.push(((x * 0.5 + 0.5) * vw, (0.5 - y * 0.5) * vh));
        }
        // an axis-aligned rectangle over every pixel centre
        let xs: Vec<f32> = screen.iter().map(|s| s.0).collect();
        let ys: Vec<f32> = screen.iter().map(|s| s.1).collect();
        let (x0, x1) = (xs.iter().cloned().fold(f32::MAX, f32::min), xs.iter().cloned().fold(f32::MIN, f32::max));
        let (y0, y1) = (ys.iter().cloned().fold(f32::MAX, f32::min), ys.iter().cloned().fold(f32::MIN, f32::max));
        let corner = |s: &(f32, f32)| (s.0 == x0 || s.0 == x1) && (s.1 == y0 || s.1 == y1);
        let corners: std::collections::HashSet<(bool, bool)> = screen.iter().map(|s| (s.0 == x1, s.1 == y1)).collect();
        if !screen.iter().all(corner) || corners.len() != 4 || x0 > 0.25 || y0 > 0.25 || x1 < vw - 0.25 || y1 < vh - 0.25 {
            return Err("not a covering rectangle");
        }
        // the interpolants: the plane through three corners, per input
        let (a, b, c) = (0, (1..4).find(|&k| screen[k].1 == screen[0].1).ok_or("corner")?, (1..4).find(|&k| screen[k].0 == screen[0].0).ok_or("corner")?);
        let (dx, dy) = (screen[b].0 - screen[a].0, screen[c].1 - screen[a].1);
        let mut planes = Vec::new();
        for &(usage, index) in &ps.inputs {
            let value = |k: usize| vs.outputs.iter().find(|o| o.0 == usage && o.1 == index).and_then(|o| vertices[k].get(o.2 as usize).copied()).unwrap_or([0.0; 4]);
            let (va, vb, vc) = (value(a), value(b), value(c));
            let gx: [f32; 4] = std::array::from_fn(|k| (vb[k] - va[k]) / dx);
            let gy: [f32; 4] = std::array::from_fn(|k| (vc[k] - va[k]) / dy);
            let base: [f32; 4] = std::array::from_fn(|k| va[k] - gx[k] * screen[a].0 - gy[k] * screen[a].1);
            planes.push([base, gx, gy]);
        }
        if planes.len() > 10 {
            return Err("too many inputs");
        }
        Ok(Plan { ps, ps_key, width: w, height: h, planes, targets: resources })
    }

    /// Run `d` as a dispatch if it is eligible; whether it ran.
    pub(crate) fn try_quad(&mut self, d: &DrawCall) -> bool {
        let plan = match self.quad_plan(d) {
            Ok(p) => p,
            Err(why) => {
                if self.errors.len() < 50 && std::env::var_os("DDGPU_NATIVE_SIM_WHY").is_some() {
                    self.errors.push(format!("not a quad dispatch: {why}"));
                }
                return false;
            }
        };
        match self.dispatch_quad(d, plan) {
            Ok(()) => true,
            Err(e) => {
                if self.errors.len() < 1000 {
                    self.errors.push(format!("quad dispatch: {e}"));
                }
                false
            }
        }
    }

    /// The pixel textures' bind group, as the draw path builds and caches
    /// it (the same key, so the two paths share groups).
    fn quad_texture_group(&mut self, d: &DrawCall, kinds: &[(u32, u8)], layout: &wgpu::BindGroupLayout) -> Result<wgpu::BindGroup, String> {
        let mut entries = Vec::new();
        for &(n, kind) in kinds {
            let b = d.textures.iter().find(|t| t.sampler == n).ok_or_else(|| format!("sampler s{n} has no texture bound"))?;
            entries.push((n, kind, b.resource, b.states[11] != 0, b.states));
        }
        let key: TexGroupKey = (false, entries);
        if let Some(g) = self.tex_groups.get(&key) {
            return Ok(g.clone());
        }
        let mut views = Vec::new();
        for &(n, kind, _, srgb_read, states) in &key.1 {
            let b = d.textures.iter().find(|t| t.sampler == n).expect("checked above");
            let view_dimension = match kind {
                0 => wgpu::TextureViewDimension::D2,
                1 => wgpu::TextureViewDimension::Cube,
                _ => wgpu::TextureViewDimension::D3,
            };
            let tx = self.texture_for(b).ok_or_else(|| format!("texture format {:#x} is not supported", b.format))?;
            let format = if srgb_read { tx.gpu_format.add_srgb_suffix() } else { tx.gpu_format };
            let view = tx.texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(view_dimension), format: Some(format), ..Default::default() });
            views.push((n, view, self.sampler(&states)?));
        }
        let mut bind = Vec::new();
        for (n, v, s) in &views {
            bind.push(wgpu::BindGroupEntry { binding: 2 * n, resource: wgpu::BindingResource::TextureView(v) });
            bind.push(wgpu::BindGroupEntry { binding: 2 * n + 1, resource: wgpu::BindingResource::Sampler(s) });
        }
        let g = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &bind });
        self.tex_groups.insert(key, g.clone());
        Ok(g)
    }

    /// A render-target texture created with storage use (DDGPU_NATIVE_SIM).
    fn storage_texture(&mut self, t: &Target) -> Option<wgpu::Texture> {
        let tt = t.texture?;
        self.create_texture(&tt, t.format)?;
        let tx = self.textures.get(&tt.resource)?;
        tx.texture.usage().contains(wgpu::TextureUsages::STORAGE_BINDING).then(|| tx.texture.clone())
    }

    fn dispatch_quad(&mut self, d: &DrawCall, plan: Plan) -> Result<(), String> {
        let group1 = self.quad_texture_group(d, &plan.ps.kinds, &plan.ps.layout1)?;
        // storage views of the targets, by target set
        let key = (plan.ps_key, plan.targets.clone());
        if !self.quads.outputs.contains_key(&key) {
            let mut views = Vec::new();
            for &o in &plan.ps.outputs {
                let t = d.targets[o as usize].as_ref().ok_or("output without a target")?;
                let tex = self.storage_texture(t).ok_or("target is not a storage texture")?;
                views.push((o, tex.create_view(&wgpu::TextureViewDescriptor { format: Some(wgpu::TextureFormat::Rgba32Float), mip_level_count: Some(1), base_mip_level: 0, ..Default::default() })));
            }
            let mut entries: Vec<wgpu::BindGroupEntry> = views.iter().map(|(o, v)| wgpu::BindGroupEntry { binding: *o, resource: wgpu::BindingResource::TextureView(v) }).collect();
            let params = wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &self.uniforms.borrow().buffer, offset: 0, size: std::num::NonZeroU64::new(QUAD_BLOCK) });
            entries.push(wgpu::BindGroupEntry { binding: 8, resource: params });
            let g = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("quad outputs"), layout: &plan.ps.out_layout, entries: &entries });
            self.quads.outputs.insert(key.clone(), g);
        }
        let group2 = self.quads.outputs[&key].clone();
        // the blocks (a ring restart flushes, so before the pass is opened)
        if !self.uniforms.borrow().fits(PS_BLOCK + VS_BLOCK + QUAD_BLOCK + 2 * UNIFORM_ALIGN, UNIFORM_ALIGN) {
            self.flush();
            self.uniforms.borrow_mut().restart();
        }
        // each pixel sampler's MIPMAPLODBIAS (no alpha-test row: see build_quad_ps)
        let mut ps_extra = [[0u32; 4]; 4];
        for t in d.textures.iter().filter(|t| t.sampler < 16) {
            ps_extra[t.sampler as usize / 4][t.sampler as usize % 4] = t.states[8];
        }
        let ps_off = self.constants(&d.ps_constants, plan.ps.floats, &ps_extra, PS_BLOCK, 0);
        let mut params: Vec<u8> = Vec::with_capacity(16 + 48 * plan.planes.len());
        for v in [plan.width, plan.height, 0, 0] {
            params.extend_from_slice(&v.to_le_bytes());
        }
        for p in &plan.planes {
            for v in p.iter().flatten() {
                params.extend_from_slice(&v.to_le_bytes());
            }
        }
        params.resize(QUAD_BLOCK as usize, 0);
        let q_off = self.ring_alloc(&self.uniforms, &params, QUAD_BLOCK, UNIFORM_ALIGN) as u32;
        {
            let mut uses = self.open_uses.borrow_mut();
            uses.extend(d.textures.iter().map(|t| t.resource));
            uses.extend(plan.targets.iter().copied());
        }
        // the open compute pass, or a new one after whatever was recorded
        if self.cpass.borrow().is_none() {
            self.end_pass();
            let mut enc = self.open.borrow_mut().take().unwrap_or_else(|| self.device.create_command_encoder(&Default::default()));
            let pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("quad dispatches"), timestamp_writes: None }).forget_lifetime();
            *self.open.borrow_mut() = Some(enc);
            *self.cpass.borrow_mut() = Some(pass);
            self.passes_begun.set(self.passes_begun.get() + 1);
        }
        let mut cpass = self.cpass.borrow_mut();
        let pass = cpass.as_mut().expect("opened above");
        pass.set_pipeline(&plan.ps.pipeline);
        pass.set_bind_group(0, &self.group0, &[ps_off, 0]);
        pass.set_bind_group(1, &group1, &[]);
        pass.set_bind_group(2, &group2, &[q_off]);
        pass.dispatch_workgroups(plan.width.div_ceil(8), plan.height.div_ceil(8), 1);
        drop(cpass);
        self.quads.dispatches += 1;
        self.draws_recorded.set(self.draws_recorded.get() + 1);
        self.open_draws.set(self.open_draws.get() + 1);
        if self.open_draws.get() >= self.draws_per_submit {
            self.flush_async();
        }
        Ok(())
    }
}

/// The quad parameters' block: the size and up to 10 input planes.
pub(crate) const QUAD_BLOCK: u64 = 16 + 48 * 10;

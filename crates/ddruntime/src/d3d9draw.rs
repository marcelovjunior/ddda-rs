//! Draws (docs/RUNTIME.md, "Graphics backend design", step 5): the D3D9
//! draw, clear and present calls turned into self-contained commands for a
//! [`GraphicsBackend`]. The runtime does not depend on a GPU API: a host
//! crate implements the trait with wgpu; the default backend records the
//! commands (headless sessions, tests).
//!
//! Before a draw, every resource it reads whose guest-memory shadow is
//! dirty (written through Lock since the last upload) is sent as an
//! [`Upload`]. A [`DrawCall`] then carries the complete state the draw
//! depends on: shaders (bytecode), vertex declaration or FVF, streams,
//! indices, constants, bound textures with their sampler states, render
//! states, viewport, scissor and targets.
use crate::d3d9::{D3DERR_INVALIDCALL, D3D_OK};
use crate::d3d9dev::{objects, with_device, Kind};
use crate::d3d9res::Image;
pub use d3d9cmd::{ClearCall, CopyCall, DrawCall, FillCall, GammaRamp, GraphicsBackend, Indices, Target, TargetTexture, TextureBinding, Upload, Vertices};
use crate::services::{arg, ret};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

/// The default backend: keeps the commands (tests and headless sessions
/// read them).
#[derive(Default)]
pub struct Recorder {
    pub uploads: Vec<Upload>,
    pub draws: Vec<DrawCall>,
    pub clears: Vec<ClearCall>,
    pub presents: u64,
    pub copies: Vec<CopyCall>,
    pub fills: Vec<FillCall>,
    /// keep only counts after this many draws (long headless runs)
    pub limit: usize,
    /// the gamma ramp last given (None: identity)
    pub gamma: Option<Box<GammaRamp>>,
    /// objects the runtime freed, in order
    pub released: Vec<u32>,
    /// occlusion queries begun and ended, in order
    pub queries: Vec<(u32, bool)>,
}

impl GraphicsBackend for Recorder {
    fn upload(&mut self, upload: Upload) {
        if self.limit == 0 || self.uploads.len() < self.limit {
            self.uploads.push(upload);
        }
    }
    fn draw(&mut self, draw: &DrawCall) {
        if self.limit == 0 || self.draws.len() < self.limit {
            self.draws.push(draw.clone());
        }
    }
    fn clear(&mut self, clear: &ClearCall) {
        self.clears.push(clear.clone());
    }
    fn present(&mut self, _back_buffer: &Target) {
        self.presents += 1;
    }
    fn copy(&mut self, copy: &CopyCall) {
        self.copies.push(copy.clone());
    }
    fn fill(&mut self, fill: &FillCall) {
        self.fills.push(fill.clone());
    }
    /// A recorder draws nothing, so it has nothing to read back.
    fn read_back(&mut self, _target: &Target) -> Option<Vec<u8>> {
        None
    }
    fn query_begin(&mut self, query: u32) {
        self.queries.push((query, true));
    }
    fn query_end(&mut self, query: u32) {
        self.queries.push((query, false));
    }
    /// Nothing is drawn, so no samples are counted.
    fn query_result(&mut self, _query: u32) -> Option<u64> {
        None
    }
    fn gamma(&mut self, ramp: Option<&GammaRamp>) {
        self.gamma = ramp.map(|r| Box::new(*r));
    }
    fn release(&mut self, resource: u32) {
        if self.limit == 0 || self.released.len() < self.limit {
            self.released.push(resource);
        }
    }
}

pub struct Graphics(pub Box<dyn GraphicsBackend>);

impl Default for Graphics {
    fn default() -> Self {
        Graphics(d3d9frame::Capture::from_env(Box::new(Recorder { limit: 10_000, ..Default::default() })))
    }
}

/// Install the backend the session draws with. With `DDDA_CAPTURE` set,
/// every command it is given is also written to that file (the frame
/// oracle's capture, docs/FRAME_ORACLE.md; `DDDA_CAPTURE_FRAMES` stops
/// recording after that many Presents).
pub fn set_backend(backend: Box<dyn GraphicsBackend>) {
    // the backend replaced (the default recorder's capture included) is
    // dropped, and its file closed, before the new capture creates it again
    crate::runtime::with(|rt| graphics(rt).0 = Box::new(Recorder::default()));
    let backend = d3d9frame::Capture::from_env(backend);
    crate::runtime::with(|rt| graphics(rt).0 = backend);
}

fn graphics(rt: &crate::runtime::Runtime) -> std::sync::MutexGuard<'_, Graphics> {
    rt.graphics.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn backend<R>(f: impl FnOnce(&mut dyn GraphicsBackend) -> R) -> R {
    crate::runtime::with(|rt| f(graphics(rt).0.as_mut()))
}

/// Backend work that reads no runtime state (a draw's or Present's commands,
/// complete in their arguments), in parallel mode without the guest lock
/// (crate::threads::unlocked; L64: Present waits for the display there).
/// The backend's own lock is taken first, under the guest lock, so commands
/// reach the backend in the order the guest issued them; it is released
/// before the guest lock is taken back, so the two locks never wait on each
/// other.
pub(crate) fn backend_unlocked<R>(f: impl FnOnce(&mut dyn GraphicsBackend) -> R) -> R {
    let rt = crate::runtime::current();
    let mut g = graphics(&rt);
    crate::threads::unlocked(move || {
        let r = f(g.0.as_mut());
        drop(g);
        r
    })
}

/// Run `f` on the recording backend (tests); None if another is installed.
pub fn recorded<R>(f: impl FnOnce(&Recorder) -> R) -> Option<R> {
    crate::runtime::with(|rt| {
        let g = graphics(rt);
        let any: &dyn std::any::Any = g.0.as_ref();
        any.downcast_ref::<Recorder>().map(f)
    })
}

// --- uploads -------------------------------------------------------------------

fn read_bytes(m: &dyn Mem, at: u32, len: u32) -> Vec<u8> {
    // the same bytes as reading each with m.u8, copied page by page
    let mut v = vec![0; len as usize];
    m.read_bytes(at, &mut v);
    v
}

fn image_upload(m: &dyn Mem, resource: u32, index: u32, format: u32, img: &Image) -> Upload {
    Upload::Image {
        resource, index, format, width: img.width, height: img.height, depth: img.depth, pitch: img.pitch, slice: img.slice,
        bytes: read_bytes(m, img.shadow, img.size),
    }
}

/// Upload what `resource` has dirty (texture levels, a surface, a buffer).
pub(crate) fn flush(m: &dyn Mem, resource: u32) {
    // collect dirty images, clearing the flags: a texture's volumes, then
    // its level surfaces in order (L78: in one pass, without copying the
    // texture's surface list; a draw flushes everything it reads)
    let out = objects(|o| {
        let mut out = Vec::new();
        let surfaces = match o.live.get_mut(&resource).map(|x| &mut x.kind) {
            Some(Kind::Texture { data, .. }) => {
                for (k, v) in data.volumes.iter_mut().enumerate() {
                    if std::mem::take(&mut v.dirty) && v.shadow != 0 {
                        out.push((k as u32, data.format, v.clone()));
                    }
                }
                std::mem::take(&mut data.surfaces)
            }
            Some(Kind::Surface { desc, image, .. }) => {
                if std::mem::take(&mut image.dirty) && image.shadow != 0 {
                    out.push((0, desc.format, image.clone()));
                }
                return out;
            }
            Some(Kind::Buffer { data, .. }) => {
                if std::mem::take(&mut data.image.dirty) && data.image.shadow != 0 {
                    out.push((u32::MAX, 0, data.image.clone()));
                }
                return out;
            }
            _ => return out,
        };
        for (k, s) in surfaces.iter().enumerate() {
            if let Some(Kind::Surface { desc, image, .. }) = o.live.get_mut(s).map(|x| &mut x.kind) {
                if std::mem::take(&mut image.dirty) && image.shadow != 0 {
                    out.push((k as u32, desc.format, image.clone()));
                }
            }
        }
        if let Some(Kind::Texture { data, .. }) = o.live.get_mut(&resource).map(|x| &mut x.kind) {
            data.surfaces = surfaces;
        }
        out
    });
    for (index, format, img) in out {
        let u = if index == u32::MAX {
            Upload::Buffer { resource, bytes: read_bytes(m, img.shadow, img.size) }
        } else {
            image_upload(m, resource, index, format, &img)
        };
        backend(|b| b.upload(u));
    }
}

// --- snapshots -------------------------------------------------------------------

pub(crate) fn target(surface: u32) -> Option<Target> {
    if surface == 0 {
        return None;
    }
    objects(|o| {
        let (desc, owner) = match o.live.get(&surface).map(|x| &x.kind) {
            Some(Kind::Surface { desc, owner, .. }) => (desc.clone(), *owner),
            _ => return None,
        };
        let (level, texture) = match o.live.get(&owner).map(|x| &x.kind) {
            Some(Kind::Texture { data, .. }) => (
                data.surfaces.iter().position(|&s| s == surface).unwrap_or(0) as u32,
                Some(TargetTexture {
                    resource: owner, kind: data.kind, width: data.width, height: data.height, depth: data.depth,
                    levels: data.levels, usage: data.usage,
                }),
            ),
            _ => (0, None),
        };
        Some(Target {
            surface, format: desc.format, width: desc.width, height: desc.height,
            texture, level,
            samples: desc.multisample, quality: desc.quality,
        })
    })
}

fn texture_binding(sampler: u32, resource: u32, states: [u32; 14]) -> Option<TextureBinding> {
    objects(|o| match o.live.get(&resource).map(|x| &x.kind) {
        Some(Kind::Texture { data, .. }) => Some(TextureBinding {
            sampler, resource, kind: data.kind, format: data.format, width: data.width, height: data.height,
            depth: data.depth, levels: data.levels, usage: data.usage, states,
        }),
        _ => None,
    })
}

fn shader_tokens(shader: u32) -> Option<Vec<u32>> {
    if shader == 0 {
        return None;
    }
    objects(|o| match o.live.get(&shader).map(|x| &x.kind) {
        Some(Kind::Shader { tokens, .. }) => Some(tokens.clone()),
        _ => None,
    })
}

fn declaration(decl: u32) -> Vec<[u8; 8]> {
    objects(|o| match o.live.get(&decl).map(|x| &x.kind) {
        Some(Kind::Declaration { elements, .. }) => elements.clone(),
        _ => vec![],
    })
}

/// Vertices a draw of `primitives` of `primitive` type uses.
pub fn vertex_count(primitive: u32, primitives: u32) -> Option<u32> {
    Some(match primitive {
        1 => primitives,          // point list
        2 => 2 * primitives,      // line list
        3 => primitives + 1,      // line strip
        4 => 3 * primitives,      // triangle list
        5 | 6 => primitives + 2,  // triangle strip, fan
        _ => return None,
    })
}

/// The draw's state snapshot, after flushing every dirty resource it reads.
fn snapshot(m: &dyn Mem, dev: u32, primitive: u32, primitives: u32, start: u32, base_vertex: i32, min_index: u32,
            vertex_count: u32, vertices: Option<Vertices>, indices: Option<Indices>) -> DrawCall {
    // what the draw reads, flushed first (flushing changes no device state)
    let (reads, rts, ds) = with_device(dev, |d| {
        let state = &d.state;
        let mut reads: Vec<u32> = state.textures.values().copied().collect();
        reads.extend(state.streams.iter().map(|s| s.0).filter(|&b| b != 0));
        // the device's index buffer only for a draw that uses it (indexed, not UP)
        if state.indices != 0 && indices.is_none() {
            reads.push(state.indices);
        }
        reads.extend(d.render_targets.iter().copied().filter(|&s| s != 0));
        (reads, d.render_targets, d.depth_stencil)
    });
    for r in reads {
        flush(m, r);
    }
    // the state the draw carries, read in place (L78: the whole device
    // state, every map of it, was cloned for each draw)
    struct Read {
        textures: Vec<(u32, u32, [u32; 14])>,
        render_states: Vec<(u32, u32)>,
        vertices: Vertices,
        indices: u32,
        shaders: (u32, u32),
        declaration: u32,
        fvf: u32,
        vs: d3d9cmd::Constants,
        ps: d3d9cmd::Constants,
        viewport: [u32; 6],
        scissor: [i32; 4],
    }
    let r = with_device(dev, |d| {
        let state = &d.state;
        let mut textures: Vec<(u32, u32, [u32; 14])> = state.textures.iter().map(|(&smp, &res)| {
            (smp, res, std::array::from_fn(|t| state.sampler.get(&(smp, t as u32)).copied().unwrap_or(0)))
        }).collect();
        textures.sort_unstable_by_key(|t| t.0);
        Read {
            textures,
            // in state order already (a BTreeMap)
            render_states: state.render.iter().map(|(&k, &v)| (k, v)).collect(),
            vertices: vertices.unwrap_or_else(|| Vertices::Streams(
                state.streams.iter().enumerate().filter(|(_, s)| s.0 != 0)
                    .map(|(k, s)| (k as u32, s.0, s.1, s.2, state.stream_freq[k])).collect(),
            )),
            indices: state.indices,
            shaders: (state.vertex_shader, state.pixel_shader),
            declaration: state.declaration,
            fvf: state.fvf,
            vs: state.vs.clone(),
            ps: state.ps.clone(),
            viewport: state.viewport,
            scissor: state.scissor,
        }
    });
    let textures: Vec<TextureBinding> = r.textures.iter().filter_map(|&(smp, res, states)| texture_binding(smp, res, states)).collect();
    let indices = indices.unwrap_or_else(|| if r.indices == 0 {
        Indices::None
    } else {
        let format = objects(|o| match o.live.get(&r.indices).map(|x| &x.kind) {
            Some(Kind::Buffer { data, .. }) => data.format,
            _ => 101,
        });
        Indices::Buffer(r.indices, format)
    });
    DrawCall {
        primitive, primitives, start, base_vertex, min_index, vertex_count, vertices: r.vertices, indices,
        vertex_shader: shader_tokens(r.shaders.0),
        pixel_shader: shader_tokens(r.shaders.1),
        declaration: declaration(r.declaration),
        fvf: r.fvf,
        vs_constants: r.vs,
        ps_constants: r.ps,
        textures,
        render_states: r.render_states,
        viewport: r.viewport,
        scissor: r.scissor,
        targets: std::array::from_fn(|k| target(rts[k])),
        depth: target(ds),
    }
}

fn in_scene(dev: u32) -> bool {
    with_device(dev, |d| d.state.in_scene)
}

// --- device methods --------------------------------------------------------------

/// DrawPrimitive(this, type, start vertex, primitive count): nonindexed
/// primitives (Microsoft Learn, IDirect3DDevice9::DrawPrimitive), so the
/// device's index buffer, if one is set, is neither read nor drawn with
/// (L21: the game sets indices and then draws triangle strips with it; a
/// draw carrying them reached wgpu as an indexed strip and failed).
pub fn draw_primitive(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, ty, start, count) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    cpu.eax = match vertex_count(ty, count) {
        Some(n) if in_scene(dev) => {
            let d = snapshot(m, dev, ty, count, start, 0, 0, n, None, Some(Indices::None));
            backend_unlocked(|b| b.draw(&d));
            D3D_OK
        }
        _ => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 16);
}

/// DrawIndexedPrimitive(this, type, base vertex (signed), min index, vertex
/// count, start index, primitive count).
pub fn draw_indexed_primitive(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (0..7).map(|k| arg(m, cpu, k)).collect();
    let (dev, ty) = (a[0], a[1]);
    cpu.eax = match vertex_count(ty, a[6]) {
        Some(_) if in_scene(dev) && with_device(dev, |d| d.state.indices != 0) => {
            let d = snapshot(m, dev, ty, a[6], a[5], a[2] as i32, a[3], a[4], None, None);
            backend_unlocked(|b| b.draw(&d));
            D3D_OK
        }
        _ => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 28);
}

/// After a UP draw, D3D9 leaves stream 0 (and for the indexed form the
/// index buffer) unset.
fn reset_after_up(dev: u32, indices: bool) {
    let (old_vb, old_ib) = with_device(dev, |d| {
        let vb = std::mem::replace(&mut d.state.streams[0], (0, 0, 0)).0;
        let ib = if indices { std::mem::replace(&mut d.state.indices, 0) } else { 0 };
        (vb, ib)
    });
    for o in [old_vb, old_ib] {
        if o != 0 {
            crate::d3d9dev::release_of(o);
        }
    }
}

/// DrawPrimitiveUP(this, type, primitive count, vertex data*, stride).
pub fn draw_primitive_up(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, ty, count, data, stride) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3), arg(m, cpu, 4));
    cpu.eax = match vertex_count(ty, count) {
        Some(n) if in_scene(dev) && data != 0 => {
            let bytes = read_bytes(m, data, n * stride);
            let d = snapshot(m, dev, ty, count, 0, 0, 0, n, Some(Vertices::Inline { bytes, stride }), Some(Indices::None));
            backend_unlocked(|b| b.draw(&d));
            reset_after_up(dev, false);
            D3D_OK
        }
        _ => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 20);
}

/// DrawIndexedPrimitiveUP(this, type, min index, vertex count, primitive
/// count, index data*, index format, vertex data*, stride).
pub fn draw_indexed_primitive_up(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (0..9).map(|k| arg(m, cpu, k)).collect();
    let (dev, ty, min, nverts, count, idata, ifmt, vdata, stride) = (a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8]);
    cpu.eax = match vertex_count(ty, count) {
        Some(n) if in_scene(dev) && idata != 0 && vdata != 0 && matches!(ifmt, 101 | 102) => {
            let isize = if ifmt == 101 { 2 } else { 4 };
            let indices = read_bytes(m, idata, n * isize);
            let vertices = read_bytes(m, vdata, (min + nverts) * stride);
            let d = snapshot(m, dev, ty, count, 0, 0, min, nverts,
                             Some(Vertices::Inline { bytes: vertices, stride }), Some(Indices::Inline { bytes: indices, format: ifmt }));
            backend_unlocked(|b| b.draw(&d));
            reset_after_up(dev, true);
            D3D_OK
        }
        _ => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 36);
}

/// Clear(this, rect count, D3DRECT*, flags, colour, z, stencil).
pub fn clear(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (0..7).map(|k| arg(m, cpu, k)).collect();
    let dev = a[0];
    let rects: Vec<[i32; 4]> = (0..if a[2] != 0 { a[1] } else { 0 })
        .map(|k| std::array::from_fn(|c| m.u32(a[2] + 16 * k + 4 * c as u32) as i32)).collect();
    let (rts, ds, viewport, scissor, srgb) = with_device(dev, |d| {
        let s = &d.state;
        let on = |k: u32| s.render.get(&k).copied().unwrap_or(0) != 0;
        (d.render_targets, d.depth_stencil, s.viewport, on(174).then_some(s.scissor), on(194))
    });
    let c = ClearCall {
        rects, flags: a[3], colour: a[4], z: f32::from_bits(a[5]), stencil: a[6],
        targets: std::array::from_fn(|k| target(rts[k])), depth: target(ds), viewport, scissor, srgb,
    };
    backend_unlocked(|b| b.clear(&c));
    cpu.eax = D3D_OK;
    ret(m, cpu, 28);
}

/// Present the swap chain's first back buffer.
pub fn present_back_buffer(m: &dyn Mem, back_buffer: u32) {
    flush(m, back_buffer);
    if let Some(t) = target(back_buffer) {
        // read under the guest lock for the host's sink (d3d9dev::presented)
        crate::d3d9dev::set_presented(crate::d3d9dev::presentation());
        backend_unlocked(|b| b.present(&t));
    }
}

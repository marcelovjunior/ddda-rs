//! Whole draws on the host's D3D9 driver: the same commands the runtime
//! gives a graphics backend (`d3d9cmd`: uploads, a clear, draws), executed
//! on a real IDirect3DDevice9, the target read back. The reference for the
//! wgpu backend (crates/ddgpu tests compare frames pixel by pixel).
use super::*;
use d3d9cmd::{ClearCall, CopyCall, DrawCall, Indices, Target, TexKind as Kind, Upload, Vertices};
use std::collections::HashMap;

const CREATE_VERTEX_BUFFER: usize = 26;
const CREATE_INDEX_BUFFER: usize = 27;
const CREATE_DEPTH_STENCIL_SURFACE: usize = 29;
const SET_DEPTH_STENCIL_SURFACE: usize = 39;
const SET_VIEWPORT: usize = 47;
const SET_SCISSOR_RECT: usize = 75;
const DRAW_PRIMITIVE: usize = 81;
const DRAW_INDEXED_PRIMITIVE: usize = 82;
const DRAW_INDEXED_PRIMITIVE_UP: usize = 84;
const SET_STREAM_SOURCE: usize = 100;
const SET_STREAM_SOURCE_FREQ: usize = 102;
const SET_INDICES: usize = 104;
const STRETCH_RECT: usize = 34;
const CREATE_QUERY: usize = 118;
// IDirect3DQuery9 slots
const QUERY_ISSUE: usize = 6;
const QUERY_GET_DATA: usize = 7;
const QUERYTYPE_OCCLUSION: u32 = 9;
const ISSUE_END: u32 = 1;
const ISSUE_BEGIN: u32 = 2;
const GETDATA_FLUSH: u32 = 1;

/// A frame read back from a target: RGBA8, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Oracle {
    /// Execute `clear` then `draws` with `uploads` as the resources' contents
    /// and read `read` (an A8R8G8B8/X8R8G8B8 target) back.
    pub fn render(&mut self, uploads: &[Upload], clear: Option<&ClearCall>, draws: &[DrawCall], read: &Target) -> Result<Frame, String> {
        self.render_counted(uploads, clear, draws, read, &[]).map(|(f, _)| f)
    }

    /// [`Oracle::render`] with D3DQUERYTYPE_OCCLUSION queries: query k is
    /// issued with D3DISSUE_BEGIN before draw `spans[k].start` and
    /// D3DISSUE_END after draw `spans[k].end - 1` (an empty span begins and
    /// ends at once). Returns the frame and each query's sample count.
    pub fn render_counted(&mut self, uploads: &[Upload], clear: Option<&ClearCall>, draws: &[DrawCall], read: &Target,
                          spans: &[std::ops::Range<usize>]) -> Result<(Frame, Vec<u32>), String> {
        let (raw, counts) = self.render_all(uploads, clear.as_slice(), draws, std::slice::from_ref(read), spans)?;
        Ok((rgba_frame(read, &raw[0])?, counts))
    }

    /// [`Oracle::render`] reading every target of `reads` back as it is
    /// stored: D3D9's layout and bits (float formats as their raw words),
    /// rows tightly packed, top to bottom. The first target is render
    /// target 0 for the clear; the clear's other targets are bound at their
    /// indices, so a clear covers an MRT set.
    pub fn render_targets(&mut self, uploads: &[Upload], clear: Option<&ClearCall>, draws: &[DrawCall], reads: &[Target]) -> Result<Vec<Vec<u8>>, String> {
        self.render_all(uploads, clear.as_slice(), draws, reads, &[]).map(|(r, _)| r)
    }

    /// [`Oracle::render_targets`] with several clears in order before the
    /// draws.
    pub fn render_cleared(&mut self, uploads: &[Upload], clears: &[ClearCall], draws: &[DrawCall], reads: &[Target]) -> Result<Vec<Vec<u8>>, String> {
        self.render_copied(uploads, clears, draws, &[], reads)
    }

    /// [`Oracle::render_cleared`] followed by StretchRect `copies` (after
    /// EndScene, as depth-stencil copies require), then the reads. The
    /// copies' surfaces are the targets named by surface number (created
    /// with their format, size and multisample type).
    pub fn render_copied(&mut self, uploads: &[Upload], clears: &[ClearCall], draws: &[DrawCall], copies: &[CopyCall], reads: &[Target]) -> Result<Vec<Vec<u8>>, String> {
        let placed: Vec<(usize, CopyCall)> = copies.iter().map(|c| (draws.len(), c.clone())).collect();
        self.render_placed(uploads, clears, draws, &placed, reads)
    }

    /// [`Oracle::render_copied`] with each copy placed after the given
    /// number of draws (0: before the first draw). Every copy runs outside
    /// the scene (EndScene, StretchRect, BeginScene).
    pub fn render_placed(&mut self, uploads: &[Upload], clears: &[ClearCall], draws: &[DrawCall], copies: &[(usize, CopyCall)], reads: &[Target]) -> Result<Vec<Vec<u8>>, String> {
        let refs: Vec<&ClearCall> = clears.iter().collect();
        self.copies = copies.to_vec();
        let r = self.render_all(uploads, &refs, draws, reads, &[]).map(|(r, _)| r);
        self.copies.clear();
        r
    }

    fn render_all(&mut self, uploads: &[Upload], clears: &[&ClearCall], draws: &[DrawCall], reads: &[Target],
                  spans: &[std::ops::Range<usize>]) -> Result<(Vec<Vec<u8>>, Vec<u32>), String> {
        unsafe {
            let mut created: Vec<Obj> = Vec::new();
            let r = self.render_inner(uploads, clears, draws, reads, spans, &mut created);
            // unbind what the device holds before releasing it
            let d = self.device;
            for k in 0..16 {
                com!(d, SET_TEXTURE, (u32, Obj), k, std::ptr::null_mut::<c_void>());
                com!(d, SET_STREAM_SOURCE, (u32, Obj, u32, u32), k, std::ptr::null_mut::<c_void>(), 0, 0);
            }
            com!(d, SET_INDICES, (Obj), std::ptr::null_mut::<c_void>());
            com!(d, SET_DEPTH_STENCIL_SURFACE, (Obj), std::ptr::null_mut::<c_void>());
            com!(d, SET_RENDER_TARGET, (u32, Obj), 0, self.targets[0]);
            for k in 1..4u32 {
                com!(d, SET_RENDER_TARGET, (u32, Obj), k, std::ptr::null_mut::<c_void>());
            }
            for o in created {
                release(o);
            }
            r
        }
    }

    unsafe fn render_inner(&mut self, uploads: &[Upload], clears: &[&ClearCall], draws: &[DrawCall], reads: &[Target],
                           spans: &[std::ops::Range<usize>], created: &mut Vec<Obj>) -> Result<(Vec<Vec<u8>>, Vec<u32>), String> {
        let d = self.device;
        // render target 0: the first read, else the first clear's target
        let read = reads.first().or_else(|| clears.first().and_then(|c| c.targets[0].as_ref())).ok_or("no render target")?;
        // targets by surface
        let targets: std::cell::RefCell<HashMap<u32, Obj>> = Default::default();
        let target = |t: &Target, depth: bool, created: &mut Vec<Obj>| -> Result<Obj, String> {
            if let Some(&o) = targets.borrow().get(&t.surface) {
                return Ok(o);
            }
            let mut s: Obj = std::ptr::null_mut();
            if depth {
                check(com!(d, CREATE_DEPTH_STENCIL_SURFACE, (u32, u32, u32, u32, u32, i32, *mut Obj, Obj),
                           t.width, t.height, t.format, t.samples, t.quality, 0, &mut s, std::ptr::null_mut()), "CreateDepthStencilSurface")?;
            } else {
                check(com!(d, CREATE_RENDER_TARGET, (u32, u32, u32, u32, u32, i32, *mut Obj, Obj),
                           t.width, t.height, t.format, t.samples, t.quality, 0, &mut s, std::ptr::null_mut()), "CreateRenderTarget")?;
            }
            created.push(s);
            targets.borrow_mut().insert(t.surface, s);
            Ok(s)
        };
        let rt = target(read, false, created)?;
        check(com!(d, SET_RENDER_TARGET, (u32, Obj), 0, rt), "SetRenderTarget")?;
        for k in 1..4u32 {
            com!(d, SET_RENDER_TARGET, (u32, Obj), k, std::ptr::null_mut::<c_void>());
        }
        // every texture first: a format the driver refuses fails before
        // any state of this call reaches the device
        let mut textures: HashMap<u32, Obj> = HashMap::new();
        for b in draws.iter().flat_map(|d| &d.textures) {
            if let std::collections::hash_map::Entry::Vacant(e) = textures.entry(b.resource) {
                let t = self.texture_from(b, uploads)?;
                created.push(t);
                e.insert(t);
            }
        }
        check(com!(d, BEGIN_SCENE, ()), "BeginScene")?;
        for c in clears {
            if let Some(t) = &c.depth {
                let ds = target(t, true, created)?;
                com!(d, SET_DEPTH_STENCIL_SURFACE, (Obj), ds);
            }
            // the clear's own targets (render target 0 included), the
            // others unbound
            for (k, t) in c.targets.iter().enumerate() {
                match t {
                    Some(t) => {
                        let s = target(t, false, created)?;
                        com!(d, SET_RENDER_TARGET, (u32, Obj), k as u32, s);
                    }
                    None if k > 0 => {
                        com!(d, SET_RENDER_TARGET, (u32, Obj), k as u32, std::ptr::null_mut::<c_void>());
                    }
                    None => {}
                }
            }
            // Clear follows the viewport, SRGBWRITEENABLE and the scissor
            // test (D3D9 behaviour): the call's own, not an earlier call's
            com!(d, SET_RENDER_STATE, (u32, u32), 194, c.srgb as u32);
            com!(d, SET_RENDER_STATE, (u32, u32), 174, c.scissor.is_some() as u32);
            if let Some(r) = c.scissor {
                com!(d, SET_SCISSOR_RECT, (*const i32), r.as_ptr());
            }
            com!(d, SET_VIEWPORT, (*const u32), c.viewport.as_ptr());
            let flat: Vec<i32> = c.rects.iter().flatten().copied().collect();
            let p = if c.rects.is_empty() { std::ptr::null() } else { flat.as_ptr() };
            check(com!(d, CLEAR, (u32, *const i32, u32, u32, f32, u32), c.rects.len() as u32, p, c.flags, c.colour, c.z, c.stencil), "Clear")?;
        }
        let buffers: HashMap<u32, &Vec<u8>> = uploads.iter().filter_map(|u| match u {
            Upload::Buffer { resource, bytes } => Some((*resource, bytes)),
            _ => None,
        }).collect();
        let mut queries: Vec<Obj> = Vec::new();
        for _ in spans {
            let mut q: Obj = std::ptr::null_mut();
            check(com!(d, CREATE_QUERY, (u32, *mut Obj), QUERYTYPE_OCCLUSION, &mut q), "CreateQuery")?;
            created.push(q);
            queries.push(q);
        }
        let issue = |at: usize, end: bool| {
            for (k, s) in spans.iter().enumerate() {
                if end && s.end == at {
                    com!(queries[k], QUERY_ISSUE, (u32), ISSUE_END);
                } else if !end && s.start == at {
                    com!(queries[k], QUERY_ISSUE, (u32), ISSUE_BEGIN);
                    if s.end == at {
                        com!(queries[k], QUERY_ISSUE, (u32), ISSUE_END);
                    }
                }
            }
        };
        let copies = std::mem::take(&mut self.copies);
        let run_copies = |at: usize, created: &mut Vec<Obj>| -> Result<(), String> {
            for (_, c) in copies.iter().filter(|(k, _)| *k == at) {
                let depth = |f: u32| matches!(f, 70..=83);
                let (src, dst) = (target(&c.src, depth(c.src.format), created)?, target(&c.dst, depth(c.dst.format), created)?);
                let rect = |r: [i32; 4]| if r == [0; 4] { None } else { Some(r) };
                let (sr, dr) = (rect(c.src_rect), rect(c.dst_rect));
                let p = |r: &Option<[i32; 4]>| r.as_ref().map_or(std::ptr::null(), |r| r.as_ptr());
                com!(d, END_SCENE, ());
                let hr = com!(d, STRETCH_RECT, (Obj, *const i32, Obj, *const i32, u32), src, p(&sr), dst, p(&dr), c.filter);
                com!(d, BEGIN_SCENE, ());
                check(hr, "StretchRect")?;
            }
            Ok(())
        };
        run_copies(0, created)?;
        for (k, draw) in draws.iter().enumerate() {
            issue(k, false);
            let r = self.one_draw(draw, uploads, &buffers, &mut textures, &mut |t, depth, c| target(t, depth, c), created);
            if let Err(e) = r {
                com!(d, END_SCENE, ());
                return Err(e);
            }
            issue(k + 1, true);
            run_copies(k + 1, created)?;
        }
        issue(draws.len(), false);
        com!(d, END_SCENE, ());
        let mut counts = Vec::new();
        for &q in &queries {
            let mut n = 0u32;
            loop {
                let hr = com!(q, QUERY_GET_DATA, (*mut u32, u32, u32), &mut n, 4, GETDATA_FLUSH);
                check(hr, "GetData")?;
                if hr == 0 {
                    break;
                }
            }
            counts.push(n);
        }
        let mut raw = Vec::new();
        for t in reads {
            let s = target(t, false, created)?;
            raw.push(self.read_raw(s, t, created)?);
        }
        let _ = rt;
        Ok((raw, counts))
    }

    /// What an occlusion query's GetData returns before any Issue (HRESULT
    /// and the value left in a buffer holding 0xDEAD), between
    /// D3DISSUE_BEGIN and D3DISSUE_END, and after an END with no draw
    /// (HRESULT and count).
    /// What Clear writes with render states set: a 4x4 A8R8G8B8 target
    /// cleared to 0, then `states` (render state, value) and the scissor
    /// rectangle set, then cleared to `colour` (D3DCLEAR_TARGET); the raw
    /// BGRA bytes.
    pub fn clear_with_states(&mut self, colour: u32, states: &[(u32, u32)], scissor: [i32; 4]) -> Result<Vec<u8>, String> {
        self.clear_probe(colour, states, scissor, [0, 0, 4, 4, 0, 1f32.to_bits()], &[])
    }

    /// [`Oracle::clear_with_states`] with a viewport (X, Y, W, H, MinZ,
    /// MaxZ bits) and rectangles (none: Count 0).
    pub fn clear_probe(&mut self, colour: u32, states: &[(u32, u32)], scissor: [i32; 4], viewport: [u32; 6], rects: &[[i32; 4]]) -> Result<Vec<u8>, String> {
        unsafe {
            let d = self.device;
            let t = Target { surface: 0, format: 21, width: 4, height: 4, texture: None, level: 0, samples: 0, quality: 0 };
            let mut created = Vec::new();
            let mut rt: Obj = std::ptr::null_mut();
            check(com!(d, CREATE_RENDER_TARGET, (u32, u32, u32, u32, u32, i32, *mut Obj, Obj), 4, 4, 21, 0, 0, 0, &mut rt, std::ptr::null_mut()), "CreateRenderTarget")?;
            created.push(rt);
            com!(d, SET_RENDER_TARGET, (u32, Obj), 0, rt);
            for (s, v) in [(194u32, 0u32), (174, 0), (168, 0xF)] {
                com!(d, SET_RENDER_STATE, (u32, u32), s, v);
            }
            check(com!(d, CLEAR, (u32, Obj, u32, u32, f32, u32), 0, std::ptr::null_mut(), 1, 0, 1.0, 0), "Clear")?;
            for &(s, v) in states {
                com!(d, SET_RENDER_STATE, (u32, u32), s, v);
            }
            com!(d, SET_SCISSOR_RECT, (*const i32), scissor.as_ptr());
            com!(d, SET_VIEWPORT, (*const u32), viewport.as_ptr());
            let flat: Vec<i32> = rects.iter().flatten().copied().collect();
            let p = if rects.is_empty() { std::ptr::null() } else { flat.as_ptr() };
            check(com!(d, CLEAR, (u32, *const i32, u32, u32, f32, u32), rects.len() as u32, p, 1, colour, 1.0, 0), "Clear")?;
            let full = [0u32, 0, 4, 4, 0, 1f32.to_bits()];
            com!(d, SET_VIEWPORT, (*const u32), full.as_ptr());
            for (s, v) in [(194u32, 0u32), (174, 0), (168, 0xF)] {
                com!(d, SET_RENDER_STATE, (u32, u32), s, v);
            }
            let r = self.read_raw(rt, &t, &mut created);
            com!(d, SET_RENDER_TARGET, (u32, Obj), 0, self.targets[0]);
            for o in created {
                release(o);
            }
            r
        }
    }

    pub fn occlusion_states(&mut self) -> Result<[i32; 5], String> {
        unsafe {
            let d = self.device;
            let mut q: Obj = std::ptr::null_mut();
            check(com!(d, CREATE_QUERY, (u32, *mut Obj), QUERYTYPE_OCCLUSION, &mut q), "CreateQuery")?;
            let mut n = 0xDEADu32;
            let before = com!(q, QUERY_GET_DATA, (*mut u32, u32, u32), &mut n, 4, GETDATA_FLUSH);
            let left = n as i32;
            com!(q, QUERY_ISSUE, (u32), ISSUE_BEGIN);
            let building = com!(q, QUERY_GET_DATA, (*mut u32, u32, u32), &mut n, 4, GETDATA_FLUSH);
            com!(q, QUERY_ISSUE, (u32), ISSUE_END);
            let mut after;
            loop {
                after = com!(q, QUERY_GET_DATA, (*mut u32, u32, u32), &mut n, 4, GETDATA_FLUSH);
                if after != 1 {
                    break;
                }
            }
            release(q);
            Ok([before, left, building, after, n as i32])
        }
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn one_draw(&mut self, draw: &DrawCall, uploads: &[Upload], buffers: &HashMap<u32, &Vec<u8>>, textures: &mut HashMap<u32, Obj>,
                       target: &mut dyn FnMut(&Target, bool, &mut Vec<Obj>) -> Result<Obj, String>, created: &mut Vec<Obj>) -> Result<(), String> {
        let d = self.device;
        // targets
        for (k, t) in draw.targets.iter().enumerate() {
            match t {
                Some(t) => {
                    let s = target(t, false, created)?;
                    com!(d, SET_RENDER_TARGET, (u32, Obj), k as u32, s);
                }
                None if k > 0 => {
                    com!(d, SET_RENDER_TARGET, (u32, Obj), k as u32, std::ptr::null_mut::<c_void>());
                }
                None => {}
            }
        }
        match &draw.depth {
            Some(t) => {
                let ds = target(t, true, created)?;
                com!(d, SET_DEPTH_STENCIL_SURFACE, (Obj), ds);
            }
            None => {
                com!(d, SET_DEPTH_STENCIL_SURFACE, (Obj), std::ptr::null_mut::<c_void>());
            }
        }
        // shaders and declaration
        let (mut vs, mut ps, mut decl): (Obj, Obj, Obj) = (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
        let vt = draw.vertex_shader.as_ref().ok_or("no vertex shader")?;
        let pt = draw.pixel_shader.as_ref().ok_or("no pixel shader")?;
        check(com!(d, CREATE_VERTEX_SHADER, (*const u32, *mut Obj), vt.as_ptr(), &mut vs), "CreateVertexShader")?;
        created.push(vs);
        check(com!(d, CREATE_PIXEL_SHADER, (*const u32, *mut Obj), pt.as_ptr(), &mut ps), "CreatePixelShader")?;
        created.push(ps);
        let mut elements: Vec<u8> = draw.declaration.iter().flatten().copied().collect();
        elements.extend_from_slice(&[0xFF, 0, 0, 0, 17, 0, 0, 0]);
        check(com!(d, CREATE_VERTEX_DECLARATION, (*const u8, *mut Obj), elements.as_ptr(), &mut decl), "CreateVertexDeclaration")?;
        created.push(decl);
        com!(d, SET_VERTEX_SHADER, (Obj), vs);
        com!(d, SET_PIXEL_SHADER, (Obj), ps);
        com!(d, SET_VERTEX_DECLARATION, (Obj), decl);
        // constants
        let floats = |c: &d3d9cmd::Constants, n: usize| -> Vec<u32> {
            let mut v: Vec<u32> = c.f.iter().flatten().copied().collect();
            v.resize(n * 4, 0);
            v
        };
        let vf = floats(&draw.vs_constants, 256);
        com!(d, SET_VERTEX_SHADER_CONSTANT_F, (u32, *const u32, u32), 0, vf.as_ptr(), 256);
        com!(d, SET_VERTEX_SHADER_CONSTANT_I, (u32, *const i32, u32), 0, draw.vs_constants.i.as_ptr() as *const i32, 16);
        com!(d, SET_VERTEX_SHADER_CONSTANT_B, (u32, *const u32, u32), 0, draw.vs_constants.b.as_ptr(), 16);
        let pf = floats(&draw.ps_constants, 224);
        com!(d, SET_PIXEL_SHADER_CONSTANT_F, (u32, *const u32, u32), 0, pf.as_ptr(), 224);
        com!(d, SET_PIXEL_SHADER_CONSTANT_I, (u32, *const i32, u32), 0, draw.ps_constants.i.as_ptr() as *const i32, 16);
        com!(d, SET_PIXEL_SHADER_CONSTANT_B, (u32, *const u32, u32), 0, draw.ps_constants.b.as_ptr(), 16);
        // states
        for &(s, v) in &draw.render_states {
            com!(d, SET_RENDER_STATE, (u32, u32), s, v);
        }
        let vp = draw.viewport;
        com!(d, SET_VIEWPORT, (*const u32), vp.as_ptr());
        com!(d, SET_SCISSOR_RECT, (*const i32), draw.scissor.as_ptr());
        // textures
        for b in &draw.textures {
            let tex = match textures.get(&b.resource) {
                Some(&t) => t,
                None => {
                    let t = self.texture_from(b, uploads)?;
                    created.push(t);
                    textures.insert(b.resource, t);
                    t
                }
            };
            com!(d, SET_TEXTURE, (u32, Obj), b.sampler, tex);
            for (k, &v) in b.states.iter().enumerate().skip(1) {
                com!(d, SET_SAMPLER_STATE, (u32, u32, u32), b.sampler, k as u32, v);
            }
        }
        // vertices, indices, the draw
        let up_stride;
        let hr = match (&draw.vertices, &draw.indices) {
            (Vertices::Inline { bytes, stride }, Indices::None) => {
                up_stride = *stride;
                let _ = up_stride;
                com!(d, DRAW_PRIMITIVE_UP, (u32, u32, *const u8, u32), draw.primitive, draw.primitives, bytes.as_ptr(), *stride)
            }
            (Vertices::Inline { bytes, stride }, Indices::Inline { bytes: ib, format }) => {
                com!(d, DRAW_INDEXED_PRIMITIVE_UP, (u32, u32, u32, u32, *const u8, u32, *const u8, u32),
                     draw.primitive, draw.min_index, draw.vertex_count, draw.primitives, ib.as_ptr(), *format, bytes.as_ptr(), *stride)
            }
            (Vertices::Streams(streams), indices) => {
                for &(n, res, off, stride, freq) in streams {
                    let data = buffers.get(&res).ok_or_else(|| format!("no upload for vertex buffer {res:#x}"))?;
                    let vb = self.buffer(CREATE_VERTEX_BUFFER, data, 0)?;
                    created.push(vb);
                    com!(d, SET_STREAM_SOURCE, (u32, Obj, u32, u32), n, vb, off, stride);
                    com!(d, SET_STREAM_SOURCE_FREQ, (u32, u32), n, freq.max(1));
                }
                match indices {
                    Indices::None => com!(d, DRAW_PRIMITIVE, (u32, u32, u32), draw.primitive, draw.start, draw.primitives),
                    Indices::Buffer(res, format) => {
                        let data = buffers.get(res).ok_or_else(|| format!("no upload for index buffer {res:#x}"))?;
                        let ib = self.buffer(CREATE_INDEX_BUFFER, data, *format)?;
                        created.push(ib);
                        com!(d, SET_INDICES, (Obj), ib);
                        com!(d, DRAW_INDEXED_PRIMITIVE, (u32, i32, u32, u32, u32, u32), draw.primitive, draw.base_vertex,
                             draw.min_index, draw.vertex_count, draw.start, draw.primitives)
                    }
                    Indices::Inline { .. } => return Err("inline indices with stream vertices".into()),
                }
            }
            _ => return Err("unsupported vertex/index combination".into()),
        };
        for k in 0..16 {
            com!(d, SET_STREAM_SOURCE_FREQ, (u32, u32), k, 1);
        }
        check(hr, "Draw")
    }

    /// A managed vertex or index buffer holding `data`.
    unsafe fn buffer(&mut self, slot: usize, data: &[u8], index_format: u32) -> Result<Obj, String> {
        let d = self.device;
        let mut b: Obj = std::ptr::null_mut();
        if slot == CREATE_VERTEX_BUFFER {
            check(com!(d, slot, (u32, u32, u32, u32, *mut Obj, Obj), data.len() as u32, 0, 0, POOL_MANAGED, &mut b, std::ptr::null_mut()), "CreateVertexBuffer")?;
        } else {
            check(com!(d, slot, (u32, u32, u32, u32, *mut Obj, Obj), data.len() as u32, 0, index_format, POOL_MANAGED, &mut b, std::ptr::null_mut()), "CreateIndexBuffer")?;
        }
        // Lock(offset, size, void**, flags) slot 11, Unlock 12
        let mut p: *mut u8 = std::ptr::null_mut();
        check(com!(b, 11, (u32, u32, *mut *mut u8, u32), 0, 0, &mut p, 0), "Lock")?;
        std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
        com!(b, 12, ());
        Ok(b)
    }

    /// A managed texture with every uploaded level of `b.resource`.
    unsafe fn texture_from(&mut self, b: &d3d9cmd::TextureBinding, uploads: &[Upload]) -> Result<Obj, String> {
        let d = self.device;
        let mut tex: Obj = std::ptr::null_mut();
        match b.kind {
            Kind::Flat => check(com!(d, CREATE_TEXTURE, (u32, u32, u32, u32, u32, u32, *mut Obj, Obj),
                                     b.width, b.height, b.levels, 0, b.format, POOL_MANAGED, &mut tex, std::ptr::null_mut()), "CreateTexture")?,
            Kind::Cube => check(com!(d, CREATE_CUBE_TEXTURE, (u32, u32, u32, u32, u32, *mut Obj, Obj),
                                     b.width, b.levels, 0, b.format, POOL_MANAGED, &mut tex, std::ptr::null_mut()), "CreateCubeTexture")?,
            Kind::Volume => check(com!(d, CREATE_VOLUME_TEXTURE, (u32, u32, u32, u32, u32, u32, u32, *mut Obj, Obj),
                                       b.width, b.height, b.depth, b.levels, 0, b.format, POOL_MANAGED, &mut tex, std::ptr::null_mut()), "CreateVolumeTexture")?,
        }
        for u in uploads {
            let Upload::Image { resource, index, height, pitch, slice, bytes, depth, .. } = u else { continue };
            if *resource != b.resource {
                continue;
            }
            let rows = (bytes.len() as u32 / (*slice).max(1)).max(1);
            let _ = rows;
            let block_rows = if *pitch > 0 { slice / pitch } else { *height };
            match b.kind {
                Kind::Volume => {
                    let mut boxed = [0u8; 24];
                    check(com!(tex, 19, (u32, *mut u8, Obj, u32), *index, boxed.as_mut_ptr(), std::ptr::null_mut(), 0), "LockBox")?;
                    let row = i32::from_le_bytes(boxed[0..4].try_into().unwrap()) as usize;
                    let sl = i32::from_le_bytes(boxed[4..8].try_into().unwrap()) as usize;
                    let bits = u64::from_le_bytes(boxed[8..16].try_into().unwrap()) as *mut u8;
                    for z in 0..*depth as usize {
                        for y in 0..block_rows as usize {
                            let src = z * *slice as usize + y * *pitch as usize;
                            std::ptr::copy_nonoverlapping(bytes[src..].as_ptr(), bits.add(z * sl + y * row), *pitch as usize);
                        }
                    }
                    com!(tex, 20, (u32), *index);
                }
                Kind::Cube => {
                    let (face, level) = (index / b.levels, index % b.levels);
                    let mut locked = [0u64; 2];
                    check(com!(tex, 19, (u32, u32, *mut u64, Obj, u32), face, level, locked.as_mut_ptr(), std::ptr::null_mut(), 0), "LockRect")?;
                    copy_rows(locked, bytes, *pitch, block_rows);
                    com!(tex, 20, (u32, u32), face, level);
                }
                Kind::Flat => {
                    let mut locked = [0u64; 2];
                    check(com!(tex, 19, (u32, *mut u64, Obj, u32), *index, locked.as_mut_ptr(), std::ptr::null_mut(), 0), "LockRect")?;
                    copy_rows(locked, bytes, *pitch, block_rows);
                    com!(tex, 20, (u32), *index);
                }
            }
        }
        Ok(tex)
    }

    /// A target's contents as stored: GetRenderTargetData into a
    /// system-memory surface of the same format, rows tightly packed.
    unsafe fn read_raw(&mut self, rt: Obj, t: &Target, created: &mut Vec<Obj>) -> Result<Vec<u8>, String> {
        let d = self.device;
        let bpp = bytes_per_pixel(t.format).ok_or_else(|| format!("reading format {} back", t.format))?;
        let mut sys: Obj = std::ptr::null_mut();
        check(com!(d, CREATE_OFFSCREEN_PLAIN_SURFACE, (u32, u32, u32, u32, *mut Obj, Obj), t.width, t.height, t.format, POOL_SYSTEMMEM, &mut sys, std::ptr::null_mut()), "CreateOffscreenPlainSurface")?;
        created.push(sys);
        check(com!(d, GET_RENDER_TARGET_DATA, (Obj, Obj), rt, sys), "GetRenderTargetData")?;
        let mut locked = [0u64; 2];
        check(com!(sys, 13, (*mut u64, Obj, u32), locked.as_mut_ptr(), std::ptr::null_mut(), 0x10), "LockRect")?;
        let pitch = locked[0] as u32 as usize;
        let bits = locked[1] as *const u8;
        let row = (t.width * bpp) as usize;
        let mut out = Vec::with_capacity(row * t.height as usize);
        for y in 0..t.height as usize {
            out.extend_from_slice(std::slice::from_raw_parts(bits.add(y * pitch), row));
        }
        com!(sys, 14, ());
        Ok(out)
    }
}

/// Bytes per pixel of the render-target formats the oracle reads back.
pub fn bytes_per_pixel(format: u32) -> Option<u32> {
    Some(match format {
        21 | 22 | 31 | 32 | 33 | 34 | 112 | 114 => 4,
        23 | 24 | 25 | 26 | 111 | 81 => 2,
        36 | 113 | 115 => 8,
        116 => 16,
        28 | 50 => 1,
        _ => return None,
    })
}

/// An A8R8G8B8/X8R8G8B8 target's raw bytes as RGBA8 (X8 reads alpha 255).
fn rgba_frame(t: &Target, raw: &[u8]) -> Result<Frame, String> {
    if !matches!(t.format, 21 | 22) {
        return Err(format!("format {} is not an 8-bit BGRA target (use render_targets)", t.format));
    }
    let rgba = raw.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], if t.format == 22 { 255 } else { p[3] }]).collect();
    Ok(Frame { width: t.width, height: t.height, rgba })
}

/// Copy rows of `pitch` bytes into a D3DLOCKED_RECT (x64: Pitch, pBits).
unsafe fn copy_rows(locked: [u64; 2], bytes: &[u8], pitch: u32, rows: u32) {
    let dst_pitch = locked[0] as u32 as usize;
    let bits = locked[1] as *mut u8;
    for y in 0..rows as usize {
        let s = y * pitch as usize;
        let n = (pitch as usize).min(dst_pitch).min(bytes.len().saturating_sub(s));
        std::ptr::copy_nonoverlapping(bytes[s..].as_ptr(), bits.add(y * dst_pitch), n);
    }
}

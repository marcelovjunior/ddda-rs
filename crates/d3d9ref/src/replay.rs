//! Whole draws and whole frames on the host's D3D9 driver: the same
//! commands the runtime gives a graphics backend (`d3d9cmd`: uploads,
//! clears, draws, copies, fills), executed on a real IDirect3DDevice9, the
//! targets read back. The reference for the wgpu backend (crates/ddgpu
//! tests compare draws pixel by pixel; the frame oracle compares whole
//! captured frames, docs/FRAME_ORACLE.md).
//!
//! A [`Session`] keeps the device objects a command stream creates (targets,
//! textures, buffers, shaders, queries) across commands. [`Driver`] is a
//! session as a `d3d9cmd::GraphicsBackend`; the `Oracle::render*` calls run
//! one batch of commands in a session of their own.
use super::*;
use d3d9cmd::{
    ClearCall, CopyCall, DrawCall, FillCall, GammaRamp, GraphicsBackend, Indices, Target, TargetTexture, TexKind as Kind, TextureBinding, Upload,
    Vertices, USAGE_DEPTHSTENCIL, USAGE_RENDERTARGET,
};
use std::collections::{BTreeMap, HashMap};

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
const COLOR_FILL: usize = 35;
const CREATE_QUERY: usize = 118;
// IDirect3DTexture9::GetSurfaceLevel, IDirect3DCubeTexture9::GetCubeMapSurface
const GET_SURFACE_LEVEL: usize = 18;
// IDirect3DQuery9 slots
const QUERY_ISSUE: usize = 6;
const QUERY_GET_DATA: usize = 7;
const QUERYTYPE_OCCLUSION: u32 = 9;
const ISSUE_END: u32 = 1;
const ISSUE_BEGIN: u32 = 2;
const GETDATA_FLUSH: u32 = 1;
const POOL_DEFAULT: u32 = 0;

/// A frame read back from a target: RGBA8, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Whether a D3D9 format is a depth-stencil format (D3DFMT_D16_LOCKABLE
/// through D3DFMT_D16, D3DFMT_D32F_LOCKABLE, D3DFMT_D24FS8, and the INTZ,
/// DF24 and DF16 FOURCCs).
fn is_depth(format: u32) -> bool {
    let fourcc = |b: &[u8; 4]| u32::from_le_bytes(*b);
    matches!(format, 70..=83) || format == fourcc(b"INTZ") || format == fourcc(b"DF24") || format == fourcc(b"DF16")
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
        let (raw, counts) = self.render_all(uploads, clear.as_slice(), draws, std::slice::from_ref(read), spans, &[])?;
        Ok((rgba_frame(read, &raw[0])?, counts))
    }

    /// [`Oracle::render`] reading every target of `reads` back as it is
    /// stored: D3D9's layout and bits (float formats as their raw words),
    /// rows tightly packed, top to bottom. The first target is render
    /// target 0 for the clear; the clear's other targets are bound at their
    /// indices, so a clear covers an MRT set.
    pub fn render_targets(&mut self, uploads: &[Upload], clear: Option<&ClearCall>, draws: &[DrawCall], reads: &[Target]) -> Result<Vec<Vec<u8>>, String> {
        self.render_all(uploads, clear.as_slice(), draws, reads, &[], &[]).map(|(r, _)| r)
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
        self.render_all(uploads, &refs, draws, reads, &[], copies).map(|(r, _)| r)
    }

    fn render_all(&mut self, uploads: &[Upload], clears: &[&ClearCall], draws: &[DrawCall], reads: &[Target],
                  spans: &[std::ops::Range<usize>], copies: &[(usize, CopyCall)]) -> Result<(Vec<Vec<u8>>, Vec<u32>), String> {
        let mut s = Session::new(self.device);
        let r = unsafe { s.batch(uploads, clears, draws, reads, spans, copies) };
        unsafe { s.finish(self.targets[0]) };
        r
    }

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
            let mut rt: Obj = std::ptr::null_mut();
            check(com!(d, CREATE_RENDER_TARGET, (u32, u32, u32, u32, u32, i32, *mut Obj, Obj), 4, 4, 21, 0, 0, 0, &mut rt, std::ptr::null_mut()), "CreateRenderTarget")?;
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
            let r = read_raw(d, rt, &t);
            com!(d, SET_RENDER_TARGET, (u32, Obj), 0, self.targets[0]);
            release(rt);
            r
        }
    }

    /// What an occlusion query's GetData returns before any Issue (HRESULT
    /// and the value left in a buffer holding 0xDEAD), between
    /// D3DISSUE_BEGIN and D3DISSUE_END, and after an END with no draw
    /// (HRESULT and count).
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
}

/// The device objects a command stream created, kept across commands, by
/// the runtime's resource numbers.
pub(crate) struct Session {
    device: Obj,
    /// target surfaces by surface number: standalone render targets and
    /// depth-stencil surfaces, or levels of render-target textures
    targets: HashMap<u32, Obj>,
    /// the texture each texture-level surface of `targets` belongs to
    target_owner: HashMap<u32, u32>,
    /// textures, and whether each is a render-target or depth-stencil one
    textures: HashMap<u32, (Obj, bool)>,
    /// each texture's latest image uploads, by level index
    images: HashMap<u32, BTreeMap<u32, Upload>>,
    buffers: HashMap<u32, Vec<u8>>,
    vertex_buffers: HashMap<u32, Obj>,
    index_buffers: HashMap<(u32, u32), Obj>,
    vertex_shaders: HashMap<Vec<u32>, Obj>,
    pixel_shaders: HashMap<Vec<u32>, Obj>,
    declarations: HashMap<Vec<[u8; 8]>, Obj>,
    queries: HashMap<u32, Obj>,
    in_scene: bool,
}

impl Session {
    pub(crate) fn new(device: Obj) -> Session {
        Session {
            device,
            targets: HashMap::new(),
            target_owner: HashMap::new(),
            textures: HashMap::new(),
            images: HashMap::new(),
            buffers: HashMap::new(),
            vertex_buffers: HashMap::new(),
            index_buffers: HashMap::new(),
            vertex_shaders: HashMap::new(),
            pixel_shaders: HashMap::new(),
            declarations: HashMap::new(),
            queries: HashMap::new(),
            in_scene: false,
        }
    }

    /// One batch: `uploads`, render target 0 bound (the first read, else
    /// the first clear's target), every texture created before any state
    /// (a format the driver refuses fails first), the clears, the draws
    /// with the queries of `spans` and the `copies` at their positions,
    /// then the queries' counts and the reads.
    unsafe fn batch(&mut self, uploads: &[Upload], clears: &[&ClearCall], draws: &[DrawCall], reads: &[Target],
                    spans: &[std::ops::Range<usize>], copies: &[(usize, CopyCall)]) -> Result<(Vec<Vec<u8>>, Vec<u32>), String> {
        let d = self.device;
        for u in uploads {
            self.upload(u.clone())?;
        }
        let read = reads.first().or_else(|| clears.first().and_then(|c| c.targets[0].as_ref())).ok_or("no render target")?;
        let rt = self.target(read)?;
        check(com!(d, SET_RENDER_TARGET, (u32, Obj), 0, rt), "SetRenderTarget")?;
        for k in 1..4u32 {
            com!(d, SET_RENDER_TARGET, (u32, Obj), k, std::ptr::null_mut::<c_void>());
        }
        for b in draws.iter().flat_map(|d| &d.textures) {
            self.texture(b)?;
        }
        for c in clears {
            self.clear(c)?;
        }
        // query k: D3DISSUE_BEGIN before draw spans[k].start, D3DISSUE_END
        // after draw spans[k].end - 1 (an empty span at once)
        let issue = |s: &mut Session, at: usize, end: bool| -> Result<(), String> {
            for (k, span) in spans.iter().enumerate() {
                if end && span.end == at {
                    s.query_end(k as u32)?;
                } else if !end && span.start == at {
                    s.query_begin(k as u32)?;
                    if span.end == at {
                        s.query_end(k as u32)?;
                    }
                }
            }
            Ok(())
        };
        let run_copies = |s: &mut Session, at: usize| -> Result<(), String> {
            for (_, c) in copies.iter().filter(|(k, _)| *k == at) {
                s.copy(c)?;
            }
            Ok(())
        };
        run_copies(self, 0)?;
        for (k, draw) in draws.iter().enumerate() {
            issue(self, k, false)?;
            self.draw(draw)?;
            issue(self, k + 1, true)?;
            run_copies(self, k + 1)?;
        }
        issue(self, draws.len(), false)?;
        self.scene(false)?;
        let mut counts = Vec::new();
        for k in 0..spans.len() {
            counts.push(self.query_result(k as u32)? as u32);
        }
        let mut raw = Vec::new();
        for t in reads {
            raw.push(self.read(t)?);
        }
        Ok((raw, counts))
    }

    /// Inside or outside BeginScene/EndScene: draws and clears run inside,
    /// copies, fills and reads outside.
    unsafe fn scene(&mut self, inside: bool) -> Result<(), String> {
        if inside != self.in_scene {
            let d = self.device;
            if inside {
                check(com!(d, BEGIN_SCENE, ()), "BeginScene")?;
            } else {
                check(com!(d, END_SCENE, ()), "EndScene")?;
            }
            self.in_scene = inside;
        }
        Ok(())
    }

    /// New contents for a resource: an image replaces its level (the
    /// texture is created again from its images when next used); a buffer
    /// replaces the buffer's bytes.
    pub(crate) unsafe fn upload(&mut self, u: Upload) -> Result<(), String> {
        match u {
            Upload::Image { resource, index, .. } => {
                match self.textures.get(&resource) {
                    Some(&(_, true)) => return Err(format!("an image upload into render-target texture {resource:#x}")),
                    Some(&(t, false)) => {
                        release(t);
                        self.textures.remove(&resource);
                    }
                    None => {}
                }
                self.images.entry(resource).or_default().insert(index, u);
            }
            Upload::Buffer { resource, bytes } => {
                if let Some(b) = self.vertex_buffers.remove(&resource) {
                    release(b);
                }
                self.index_buffers.retain(|&(r, _), &mut b| {
                    if r == resource {
                        release(b);
                    }
                    r != resource
                });
                self.buffers.insert(resource, bytes);
            }
        }
        Ok(())
    }

    /// The texture a binding names: a render-target or depth-stencil
    /// texture (D3DPOOL_DEFAULT, its levels targets), or a managed texture
    /// holding the resource's image uploads.
    unsafe fn texture(&mut self, b: &TextureBinding) -> Result<Obj, String> {
        if let Some(&(t, _)) = self.textures.get(&b.resource) {
            return Ok(t);
        }
        let target = b.usage & (USAGE_RENDERTARGET | USAGE_DEPTHSTENCIL) != 0;
        let t = if target {
            let desc = TargetTexture { resource: b.resource, kind: b.kind, width: b.width, height: b.height, depth: b.depth, levels: b.levels, usage: b.usage };
            target_texture(self.device, &desc, b.format)?
        } else {
            managed_texture(self.device, b, self.images.get(&b.resource).into_iter().flat_map(|m| m.values()))?
        };
        self.textures.insert(b.resource, (t, target));
        Ok(t)
    }

    /// The surface of a target: a level of a render-target texture (the
    /// texture created when it is drawn into before it is bound), or a
    /// standalone render target or depth-stencil surface with the target's
    /// format, size and multisample type.
    unsafe fn target(&mut self, t: &Target) -> Result<Obj, String> {
        if let Some(&s) = self.targets.get(&t.surface) {
            return Ok(s);
        }
        let d = self.device;
        let mut s: Obj = std::ptr::null_mut();
        match t.texture {
            Some(tt) => {
                let tex = match self.textures.get(&tt.resource) {
                    Some(&(x, true)) => x,
                    Some(_) => return Err(format!("texture {:#x} is not a render-target texture", tt.resource)),
                    None => {
                        let x = target_texture(d, &tt, t.format)?;
                        self.textures.insert(tt.resource, (x, true));
                        x
                    }
                };
                match tt.kind {
                    Kind::Flat => check(com!(tex, GET_SURFACE_LEVEL, (u32, *mut Obj), t.level, &mut s), "GetSurfaceLevel")?,
                    Kind::Cube => check(com!(tex, GET_SURFACE_LEVEL, (u32, u32, *mut Obj), t.level / tt.levels, t.level % tt.levels, &mut s), "GetCubeMapSurface")?,
                    Kind::Volume => return Err("a volume texture as a target".into()),
                }
                self.target_owner.insert(t.surface, tt.resource);
            }
            None if is_depth(t.format) => {
                check(com!(d, CREATE_DEPTH_STENCIL_SURFACE, (u32, u32, u32, u32, u32, i32, *mut Obj, Obj),
                           t.width, t.height, t.format, t.samples, t.quality, 0, &mut s, std::ptr::null_mut()), "CreateDepthStencilSurface")?;
            }
            None => {
                check(com!(d, CREATE_RENDER_TARGET, (u32, u32, u32, u32, u32, i32, *mut Obj, Obj),
                           t.width, t.height, t.format, t.samples, t.quality, 0, &mut s, std::ptr::null_mut()), "CreateRenderTarget")?;
            }
        }
        self.targets.insert(t.surface, s);
        Ok(s)
    }

    pub(crate) unsafe fn clear(&mut self, c: &ClearCall) -> Result<(), String> {
        let d = self.device;
        self.scene(true)?;
        if let Some(t) = &c.depth {
            let ds = self.target(t)?;
            com!(d, SET_DEPTH_STENCIL_SURFACE, (Obj), ds);
        }
        // the clear's own targets (render target 0 included), the others
        // unbound
        for (k, t) in c.targets.iter().enumerate() {
            match t {
                Some(t) => {
                    let s = self.target(t)?;
                    com!(d, SET_RENDER_TARGET, (u32, Obj), k as u32, s);
                }
                None if k > 0 => {
                    com!(d, SET_RENDER_TARGET, (u32, Obj), k as u32, std::ptr::null_mut::<c_void>());
                }
                None => {}
            }
        }
        // Clear follows the viewport, SRGBWRITEENABLE and the scissor test
        // (D3D9 behaviour): the call's own, not an earlier call's
        com!(d, SET_RENDER_STATE, (u32, u32), 194, c.srgb as u32);
        com!(d, SET_RENDER_STATE, (u32, u32), 174, c.scissor.is_some() as u32);
        if let Some(r) = c.scissor {
            com!(d, SET_SCISSOR_RECT, (*const i32), r.as_ptr());
        }
        com!(d, SET_VIEWPORT, (*const u32), c.viewport.as_ptr());
        let flat: Vec<i32> = c.rects.iter().flatten().copied().collect();
        let p = if c.rects.is_empty() { std::ptr::null() } else { flat.as_ptr() };
        check(com!(d, CLEAR, (u32, *const i32, u32, u32, f32, u32), c.rects.len() as u32, p, c.flags, c.colour, c.z, c.stencil), "Clear")
    }

    /// StretchRect, outside the scene (as depth-stencil copies require).
    pub(crate) unsafe fn copy(&mut self, c: &CopyCall) -> Result<(), String> {
        let d = self.device;
        self.scene(false)?;
        let (src, dst) = (self.target(&c.src)?, self.target(&c.dst)?);
        let rect = |r: [i32; 4]| if r == [0; 4] { None } else { Some(r) };
        let (sr, dr) = (rect(c.src_rect), rect(c.dst_rect));
        let p = |r: &Option<[i32; 4]>| r.as_ref().map_or(std::ptr::null(), |r| r.as_ptr());
        check(com!(d, STRETCH_RECT, (Obj, *const i32, Obj, *const i32, u32), src, p(&sr), dst, p(&dr), c.filter), "StretchRect")
    }

    /// ColorFill, outside the scene.
    pub(crate) unsafe fn fill(&mut self, f: &FillCall) -> Result<(), String> {
        let d = self.device;
        self.scene(false)?;
        let s = self.target(&f.dst)?;
        let p = if f.rect == [0; 4] { std::ptr::null() } else { f.rect.as_ptr() };
        check(com!(d, COLOR_FILL, (Obj, *const i32, u32), s, p, f.colour), "ColorFill")
    }

    /// A target's contents as stored (outside the scene).
    pub(crate) unsafe fn read(&mut self, t: &Target) -> Result<Vec<u8>, String> {
        self.scene(false)?;
        let s = self.target(t)?;
        read_raw(self.device, s, t)
    }

    /// The occlusion query object for a runtime query (created when first
    /// issued).
    unsafe fn query(&mut self, id: u32) -> Result<Obj, String> {
        if let Some(&q) = self.queries.get(&id) {
            return Ok(q);
        }
        let mut q: Obj = std::ptr::null_mut();
        check(com!(self.device, CREATE_QUERY, (u32, *mut Obj), QUERYTYPE_OCCLUSION, &mut q), "CreateQuery")?;
        self.queries.insert(id, q);
        Ok(q)
    }

    pub(crate) unsafe fn query_begin(&mut self, id: u32) -> Result<(), String> {
        let q = self.query(id)?;
        check(com!(q, QUERY_ISSUE, (u32), ISSUE_BEGIN), "Issue")
    }

    /// D3DISSUE_END (also on a query never begun, as D3D9 allows).
    pub(crate) unsafe fn query_end(&mut self, id: u32) -> Result<(), String> {
        let q = self.query(id)?;
        check(com!(q, QUERY_ISSUE, (u32), ISSUE_END), "Issue")
    }

    /// An ended query's sample count (GetData with D3DGETDATA_FLUSH until
    /// it is ready).
    pub(crate) unsafe fn query_result(&mut self, id: u32) -> Result<u64, String> {
        let q = *self.queries.get(&id).ok_or_else(|| format!("query {id:#x} was never issued"))?;
        let mut n = 0u32;
        loop {
            let hr = com!(q, QUERY_GET_DATA, (*mut u32, u32, u32), &mut n, 4, GETDATA_FLUSH);
            check(hr, "GetData")?;
            if hr == 0 {
                return Ok(n as u64);
            }
        }
    }

    /// Drop what the session keeps for a resource the runtime freed.
    pub(crate) unsafe fn release(&mut self, resource: u32) {
        if let Some((t, _)) = self.textures.remove(&resource) {
            let levels: Vec<u32> = self.target_owner.iter().filter(|(_, &o)| o == resource).map(|(&s, _)| s).collect();
            for s in levels {
                self.target_owner.remove(&s);
                if let Some(o) = self.targets.remove(&s) {
                    release(o);
                }
            }
            release(t);
        }
        if let Some(s) = self.targets.remove(&resource) {
            self.target_owner.remove(&resource);
            release(s);
        }
        self.images.remove(&resource);
        self.buffers.remove(&resource);
        if let Some(b) = self.vertex_buffers.remove(&resource) {
            release(b);
        }
        self.index_buffers.retain(|&(r, _), &mut b| {
            if r == resource {
                release(b);
            }
            r != resource
        });
        if let Some(q) = self.queries.remove(&resource) {
            release(q);
        }
    }

    /// End the scene, unbind what the device holds (render target 0 back to
    /// `default_target`) and release every object of the session.
    pub(crate) unsafe fn finish(&mut self, default_target: Obj) {
        let d = self.device;
        let _ = self.scene(false);
        for k in 0..16 {
            com!(d, SET_TEXTURE, (u32, Obj), k, std::ptr::null_mut::<c_void>());
            com!(d, SET_STREAM_SOURCE, (u32, Obj, u32, u32), k, std::ptr::null_mut::<c_void>(), 0, 0);
        }
        for k in 257..261 {
            com!(d, SET_TEXTURE, (u32, Obj), k, std::ptr::null_mut::<c_void>());
        }
        com!(d, SET_INDICES, (Obj), std::ptr::null_mut::<c_void>());
        com!(d, SET_DEPTH_STENCIL_SURFACE, (Obj), std::ptr::null_mut::<c_void>());
        com!(d, SET_RENDER_TARGET, (u32, Obj), 0, default_target);
        for k in 1..4u32 {
            com!(d, SET_RENDER_TARGET, (u32, Obj), k, std::ptr::null_mut::<c_void>());
        }
        com!(d, SET_VERTEX_SHADER, (Obj), std::ptr::null_mut::<c_void>());
        com!(d, SET_PIXEL_SHADER, (Obj), std::ptr::null_mut::<c_void>());
        com!(d, SET_VERTEX_DECLARATION, (Obj), std::ptr::null_mut::<c_void>());
        // level surfaces before their textures
        for (_, o) in self.targets.drain() {
            release(o);
        }
        for (_, (o, _)) in self.textures.drain() {
            release(o);
        }
        for o in self.vertex_buffers.drain().map(|(_, o)| o)
            .chain(self.index_buffers.drain().map(|(_, o)| o))
            .chain(self.vertex_shaders.drain().map(|(_, o)| o))
            .chain(self.pixel_shaders.drain().map(|(_, o)| o))
            .chain(self.declarations.drain().map(|(_, o)| o))
            .chain(self.queries.drain().map(|(_, o)| o)) {
            release(o);
        }
        self.target_owner.clear();
        self.images.clear();
        self.buffers.clear();
    }

    unsafe fn shader(&mut self, tokens: &[u32], vertex: bool) -> Result<Obj, String> {
        let cache = if vertex { &mut self.vertex_shaders } else { &mut self.pixel_shaders };
        if let Some(&s) = cache.get(tokens) {
            return Ok(s);
        }
        let mut s: Obj = std::ptr::null_mut();
        if vertex {
            check(com!(self.device, CREATE_VERTEX_SHADER, (*const u32, *mut Obj), tokens.as_ptr(), &mut s), "CreateVertexShader")?;
        } else {
            check(com!(self.device, CREATE_PIXEL_SHADER, (*const u32, *mut Obj), tokens.as_ptr(), &mut s), "CreatePixelShader")?;
        }
        cache.insert(tokens.to_vec(), s);
        Ok(s)
    }

    unsafe fn declaration(&mut self, elements: &[[u8; 8]]) -> Result<Obj, String> {
        if let Some(&o) = self.declarations.get(elements) {
            return Ok(o);
        }
        let mut bytes: Vec<u8> = elements.iter().flatten().copied().collect();
        bytes.extend_from_slice(&[0xFF, 0, 0, 0, 17, 0, 0, 0]);
        let mut o: Obj = std::ptr::null_mut();
        check(com!(self.device, CREATE_VERTEX_DECLARATION, (*const u8, *mut Obj), bytes.as_ptr(), &mut o), "CreateVertexDeclaration")?;
        self.declarations.insert(elements.to_vec(), o);
        Ok(o)
    }

    unsafe fn vertex_buffer(&mut self, resource: u32) -> Result<Obj, String> {
        if let Some(&b) = self.vertex_buffers.get(&resource) {
            return Ok(b);
        }
        let data = self.buffers.get(&resource).ok_or_else(|| format!("no upload for vertex buffer {resource:#x}"))?;
        let b = buffer(self.device, CREATE_VERTEX_BUFFER, data, 0)?;
        self.vertex_buffers.insert(resource, b);
        Ok(b)
    }

    unsafe fn index_buffer(&mut self, resource: u32, format: u32) -> Result<Obj, String> {
        if let Some(&b) = self.index_buffers.get(&(resource, format)) {
            return Ok(b);
        }
        let data = self.buffers.get(&resource).ok_or_else(|| format!("no upload for index buffer {resource:#x}"))?;
        let b = buffer(self.device, CREATE_INDEX_BUFFER, data, format)?;
        self.index_buffers.insert((resource, format), b);
        Ok(b)
    }

    /// One draw with its complete state.
    pub(crate) unsafe fn draw(&mut self, draw: &DrawCall) -> Result<(), String> {
        let d = self.device;
        self.scene(true)?;
        // targets
        for (k, t) in draw.targets.iter().enumerate() {
            match t {
                Some(t) => {
                    let s = self.target(t)?;
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
                let ds = self.target(t)?;
                com!(d, SET_DEPTH_STENCIL_SURFACE, (Obj), ds);
            }
            None => {
                com!(d, SET_DEPTH_STENCIL_SURFACE, (Obj), std::ptr::null_mut::<c_void>());
            }
        }
        // shaders and declaration
        let vs = self.shader(draw.vertex_shader.as_ref().ok_or("no vertex shader")?, true)?;
        let ps = self.shader(draw.pixel_shader.as_ref().ok_or("no pixel shader")?, false)?;
        let decl = self.declaration(&draw.declaration)?;
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
            let tex = self.texture(b)?;
            com!(d, SET_TEXTURE, (u32, Obj), b.sampler, tex);
            for (k, &v) in b.states.iter().enumerate().skip(1) {
                com!(d, SET_SAMPLER_STATE, (u32, u32, u32), b.sampler, k as u32, v);
            }
        }
        // vertices, indices, the draw
        let hr = match (&draw.vertices, &draw.indices) {
            (Vertices::Inline { bytes, stride }, Indices::None) => {
                com!(d, DRAW_PRIMITIVE_UP, (u32, u32, *const u8, u32), draw.primitive, draw.primitives, bytes.as_ptr(), *stride)
            }
            (Vertices::Inline { bytes, stride }, Indices::Inline { bytes: ib, format }) => {
                com!(d, DRAW_INDEXED_PRIMITIVE_UP, (u32, u32, u32, u32, *const u8, u32, *const u8, u32),
                     draw.primitive, draw.min_index, draw.vertex_count, draw.primitives, ib.as_ptr(), *format, bytes.as_ptr(), *stride)
            }
            (Vertices::Streams(streams), indices) => {
                for &(n, res, off, stride, freq) in streams {
                    let vb = self.vertex_buffer(res)?;
                    com!(d, SET_STREAM_SOURCE, (u32, Obj, u32, u32), n, vb, off, stride);
                    com!(d, SET_STREAM_SOURCE_FREQ, (u32, u32), n, freq.max(1));
                }
                match indices {
                    Indices::None => com!(d, DRAW_PRIMITIVE, (u32, u32, u32), draw.primitive, draw.start, draw.primitives),
                    Indices::Buffer(res, format) => {
                        let ib = self.index_buffer(*res, *format)?;
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
}

/// A managed vertex or index buffer holding `data`.
unsafe fn buffer(d: Obj, slot: usize, data: &[u8], index_format: u32) -> Result<Obj, String> {
    let mut b: Obj = std::ptr::null_mut();
    let len = (data.len() as u32).max(4);
    if slot == CREATE_VERTEX_BUFFER {
        check(com!(d, slot, (u32, u32, u32, u32, *mut Obj, Obj), len, 0, 0, POOL_MANAGED, &mut b, std::ptr::null_mut()), "CreateVertexBuffer")?;
    } else {
        check(com!(d, slot, (u32, u32, u32, u32, *mut Obj, Obj), len, 0, index_format, POOL_MANAGED, &mut b, std::ptr::null_mut()), "CreateIndexBuffer")?;
    }
    // Lock(offset, size, void**, flags) slot 11, Unlock 12
    let mut p: *mut u8 = std::ptr::null_mut();
    check(com!(b, 11, (u32, u32, *mut *mut u8, u32), 0, 0, &mut p, 0), "Lock")?;
    std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
    com!(b, 12, ());
    Ok(b)
}

/// A render-target or depth-stencil texture (D3DPOOL_DEFAULT).
unsafe fn target_texture(d: Obj, t: &TargetTexture, format: u32) -> Result<Obj, String> {
    let usage = t.usage & (USAGE_RENDERTARGET | USAGE_DEPTHSTENCIL);
    let mut tex: Obj = std::ptr::null_mut();
    match t.kind {
        Kind::Flat => check(com!(d, CREATE_TEXTURE, (u32, u32, u32, u32, u32, u32, *mut Obj, Obj),
                                 t.width, t.height, t.levels, usage, format, POOL_DEFAULT, &mut tex, std::ptr::null_mut()), "CreateTexture (render target)")?,
        Kind::Cube => check(com!(d, CREATE_CUBE_TEXTURE, (u32, u32, u32, u32, u32, *mut Obj, Obj),
                                 t.width, t.levels, usage, format, POOL_DEFAULT, &mut tex, std::ptr::null_mut()), "CreateCubeTexture (render target)")?,
        Kind::Volume => return Err("a render-target volume texture".into()),
    }
    Ok(tex)
}

/// A managed texture with every image of `images`.
unsafe fn managed_texture<'a>(d: Obj, b: &TextureBinding, images: impl Iterator<Item = &'a Upload>) -> Result<Obj, String> {
    let mut tex: Obj = std::ptr::null_mut();
    match b.kind {
        Kind::Flat => check(com!(d, CREATE_TEXTURE, (u32, u32, u32, u32, u32, u32, *mut Obj, Obj),
                                 b.width, b.height, b.levels, 0, b.format, POOL_MANAGED, &mut tex, std::ptr::null_mut()), "CreateTexture")?,
        Kind::Cube => check(com!(d, CREATE_CUBE_TEXTURE, (u32, u32, u32, u32, u32, *mut Obj, Obj),
                                 b.width, b.levels, 0, b.format, POOL_MANAGED, &mut tex, std::ptr::null_mut()), "CreateCubeTexture")?,
        Kind::Volume => check(com!(d, CREATE_VOLUME_TEXTURE, (u32, u32, u32, u32, u32, u32, u32, *mut Obj, Obj),
                                   b.width, b.height, b.depth, b.levels, 0, b.format, POOL_MANAGED, &mut tex, std::ptr::null_mut()), "CreateVolumeTexture")?,
    }
    for u in images {
        let Upload::Image { index, height, pitch, slice, bytes, depth, .. } = u else { continue };
        let block_rows = if *pitch > 0 { slice / pitch } else { *height };
        let r = (|| -> Result<(), String> {
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
            Ok(())
        })();
        if let Err(e) = r {
            release(tex);
            return Err(e);
        }
    }
    Ok(tex)
}

/// A target's contents as stored: GetRenderTargetData into a system-memory
/// surface of the same format, rows tightly packed.
unsafe fn read_raw(d: Obj, rt: Obj, t: &Target) -> Result<Vec<u8>, String> {
    let bpp = bytes_per_pixel(t.format).ok_or_else(|| format!("reading format {} back", t.format))?;
    let mut sys: Obj = std::ptr::null_mut();
    check(com!(d, CREATE_OFFSCREEN_PLAIN_SURFACE, (u32, u32, u32, u32, *mut Obj, Obj), t.width, t.height, t.format, POOL_SYSTEMMEM, &mut sys, std::ptr::null_mut()), "CreateOffscreenPlainSurface")?;
    let r = (|| -> Result<Vec<u8>, String> {
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
    })();
    release(sys);
    r
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

/// The host's D3D9 driver as a graphics backend: every command executed
/// on its device as it arrives, the device objects kept across commands
/// (a [`Session`]). Failures are collected in `errors`, as the wgpu
/// backend collects them.
pub struct Driver {
    oracle: Oracle,
    session: Session,
    pub errors: Vec<String>,
    /// the back buffer of the last Present, read back as stored
    pub last_frame: Option<(Target, Vec<u8>)>,
    /// Presents so far
    pub presents: u32,
    /// the last SetGammaRamp (not applied: the frames are the back
    /// buffer's contents, as `read_back` gives them)
    pub gamma: Option<Box<GammaRamp>>,
}

// The device is created without D3DCREATE_MULTITHREADED and used by one
// thread at a time: the backend moves between threads (as the runtime's
// backend slot requires), it is never shared.
unsafe impl Send for Driver {}

impl Driver {
    /// A backend on the host's D3D9 HAL device, or None (no D3D9, no
    /// adapter, or the device is lost to a full-screen application).
    pub fn new() -> Option<Driver> {
        let oracle = Oracle::new()?;
        let session = Session::new(oracle.device);
        Some(Driver { oracle, session, errors: Vec::new(), last_frame: None, presents: 0, gamma: None })
    }

    fn note(&mut self, what: &str, r: Result<(), String>) {
        if let Err(e) = r {
            self.errors.push(format!("{what}: {e}"));
        }
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        unsafe { self.session.finish(self.oracle.targets[0]) };
    }
}

impl GraphicsBackend for Driver {
    fn upload(&mut self, upload: Upload) {
        let r = unsafe { self.session.upload(upload) };
        self.note("upload", r);
    }

    fn draw(&mut self, draw: &DrawCall) {
        let r = unsafe { self.session.draw(draw) };
        self.note("draw", r);
    }

    fn clear(&mut self, clear: &ClearCall) {
        let r = unsafe { self.session.clear(clear) };
        self.note("clear", r);
    }

    fn present(&mut self, back_buffer: &Target) {
        self.presents += 1;
        match unsafe { self.session.read(back_buffer) } {
            Ok(raw) => self.last_frame = Some((back_buffer.clone(), raw)),
            Err(e) => self.errors.push(format!("present: {e}")),
        }
    }

    fn copy(&mut self, copy: &CopyCall) {
        let r = unsafe { self.session.copy(copy) };
        self.note("copy", r);
    }

    fn fill(&mut self, fill: &FillCall) {
        let r = unsafe { self.session.fill(fill) };
        self.note("fill", r);
    }

    fn read_back(&mut self, target: &Target) -> Option<Vec<u8>> {
        match unsafe { self.session.read(target) } {
            Ok(raw) => Some(raw),
            Err(e) => {
                self.errors.push(format!("read back: {e}"));
                None
            }
        }
    }

    fn query_begin(&mut self, query: u32) {
        let r = unsafe { self.session.query_begin(query) };
        self.note("query begin", r);
    }

    fn query_end(&mut self, query: u32) {
        let r = unsafe { self.session.query_end(query) };
        self.note("query end", r);
    }

    fn query_result(&mut self, query: u32) -> Option<u64> {
        match unsafe { self.session.query_result(query) } {
            Ok(n) => Some(n),
            Err(e) => {
                self.errors.push(format!("query result: {e}"));
                None
            }
        }
    }

    fn gamma(&mut self, ramp: Option<&GammaRamp>) {
        self.gamma = ramp.map(|r| Box::new(*r));
    }

    fn release(&mut self, resource: u32) {
        unsafe { self.session.release(resource) };
    }
}

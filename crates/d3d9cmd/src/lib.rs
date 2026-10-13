//! The commands the runtime's Direct3D 9 device sends to a graphics
//! backend (docs/RUNTIME.md, "Graphics backend design"): resource uploads,
//! draws with their complete state, clears and presents. A separate crate so
//! that backends (crates/ddgpu, wgpu) do not depend on the runtime.

mod deferred;
pub use deferred::DeferredBackend;

/// A texture's dimensionality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TexKind {
    Flat,
    Cube,
    Volume,
}

/// Shader constants: float registers (raw bits), integer and boolean ones.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Constants {
    pub f: Vec<[u32; 4]>,
    pub i: [[i32; 4]; 16],
    pub b: [u32; 16],
}

/// Resource contents for the backend.
#[derive(Clone, Debug, PartialEq)]
pub enum Upload {
    /// One image: a texture level (`index` = face * levels + level, or the
    /// volume level) or a standalone surface (`index` 0).
    Image { resource: u32, index: u32, format: u32, width: u32, height: u32, depth: u32, pitch: u32, slice: u32, bytes: Vec<u8> },
    Buffer { resource: u32, bytes: Vec<u8> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextureBinding {
    /// sampler register (0..15 pixel, 257..260 vertex)
    pub sampler: u32,
    pub resource: u32,
    pub kind: TexKind,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub levels: u32,
    /// D3DUSAGE bits the texture was created with (D3DUSAGE_RENDERTARGET 1,
    /// D3DUSAGE_DEPTHSTENCIL 2: its levels are targets as well)
    pub usage: u32,
    /// D3DSAMPLERSTATETYPE 1..13 values
    pub states: [u32; 14],
}

/// D3DUSAGE_RENDERTARGET.
pub const USAGE_RENDERTARGET: u32 = 0x1;
/// D3DUSAGE_DEPTHSTENCIL.
pub const USAGE_DEPTHSTENCIL: u32 = 0x2;

/// The texture a target surface is a level of: enough to create the
/// texture when a draw or clear renders into it before it is sampled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TargetTexture {
    pub resource: u32,
    pub kind: TexKind,
    /// the top level's size
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub levels: u32,
    /// D3DUSAGE bits (D3DUSAGE_RENDERTARGET or D3DUSAGE_DEPTHSTENCIL)
    pub usage: u32,
}

/// A render target or depth-stencil: the surface, its format and size, and
/// the texture level it belongs to (if any).
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub surface: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub texture: Option<TargetTexture>,
    /// for a texture's surface: its index among the texture's level
    /// surfaces (face * levels + level)
    pub level: u32,
    /// D3DMULTISAMPLE_TYPE (0: none; 2..16 samples; 1: non-maskable)
    pub samples: u32,
    /// the multisample quality level
    pub quality: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Vertices {
    /// stream number, vertex buffer, offset, stride, frequency setting
    Streams(std::sync::Arc<[(u32, u32, u32, u32, u32)]>),
    /// DrawPrimitiveUP data for stream 0, with its stride
    Inline { bytes: Vec<u8>, stride: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Indices {
    None,
    /// index buffer, format (D3DFMT_INDEX16/32)
    Buffer(u32, u32),
    Inline { bytes: Vec<u8>, format: u32 },
}

/// (state, value) pairs sorted by state, shared: the device and every draw
/// recorded since its last SetRenderState hold the same list, and a change
/// copies it first (L97: each draw copied the ~200 states, 1.6 KB and an
/// allocation). Reads and writes go through the Vec it derefs to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderStateList(std::sync::Arc<Vec<(u32, u32)>>);

impl RenderStateList {
    /// The same shared list (not merely equal states).
    pub fn ptr_eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
    /// Where the shared list lives, for a cache slot.
    pub fn address(&self) -> u64 {
        std::sync::Arc::as_ptr(&self.0) as usize as u64
    }
}

impl std::ops::Deref for RenderStateList {
    type Target = Vec<(u32, u32)>;
    fn deref(&self) -> &Vec<(u32, u32)> {
        &self.0
    }
}

impl std::ops::DerefMut for RenderStateList {
    fn deref_mut(&mut self) -> &mut Vec<(u32, u32)> {
        std::sync::Arc::make_mut(&mut self.0)
    }
}

impl From<Vec<(u32, u32)>> for RenderStateList {
    fn from(v: Vec<(u32, u32)>) -> Self {
        RenderStateList(std::sync::Arc::new(v))
    }
}

impl<'a> IntoIterator for &'a RenderStateList {
    type Item = &'a (u32, u32);
    type IntoIter = std::slice::Iter<'a, (u32, u32)>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawCall {
    /// D3DPRIMITIVETYPE
    pub primitive: u32,
    pub primitives: u32,
    /// first vertex (non-indexed) or first index (indexed)
    pub start: u32,
    pub base_vertex: i32,
    pub min_index: u32,
    pub vertex_count: u32,
    pub vertices: Vertices,
    pub indices: Indices,
    pub vertex_shader: Option<std::sync::Arc<[u32]>>,
    pub pixel_shader: Option<std::sync::Arc<[u32]>>,
    /// D3DVERTEXELEMENT9 bytes (END excluded); empty when FVF drives it
    pub declaration: std::sync::Arc<[[u8; 8]]>,
    pub fvf: u32,
    /// Immutable snapshot; setters detach a shared block only when bits change.
    pub vs_constants: std::sync::Arc<Constants>,
    pub ps_constants: std::sync::Arc<Constants>,
    pub textures: std::sync::Arc<[TextureBinding]>,
    /// every render state, sorted by state
    pub render_states: RenderStateList,
    pub viewport: [u32; 6],
    pub scissor: [i32; 4],
    pub targets: [Option<Target>; 4],
    pub depth: Option<Target>,
}

/// Clear: D3D9 clears the viewport, each rectangle of `rects` within it
/// (none: the whole viewport), within the scissor rectangle when the
/// scissor test is on; the colour goes through sRGB encoding when
/// SRGBWRITEENABLE is on; colour write masks do not apply (measured on the
/// driver, ddgpu tests/targets.rs).
#[derive(Clone, Debug, PartialEq)]
pub struct ClearCall {
    pub rects: Vec<[i32; 4]>,
    pub flags: u32,
    pub colour: u32,
    pub z: f32,
    pub stencil: u32,
    pub targets: [Option<Target>; 4],
    pub depth: Option<Target>,
    pub viewport: [u32; 6],
    /// the scissor rectangle when SCISSORTESTENABLE is on
    pub scissor: Option<[i32; 4]>,
    /// SRGBWRITEENABLE
    pub srgb: bool,
}

/// StretchRect: a rectangle (left, top, right, bottom) of one target
/// copied, scaled with `filter` (D3DTEXTUREFILTERTYPE), to a rectangle of
/// another.
#[derive(Clone, Debug, PartialEq)]
pub struct CopyCall {
    pub src: Target,
    pub src_rect: [i32; 4],
    pub dst: Target,
    pub dst_rect: [i32; 4],
    pub filter: u32,
}

/// ColorFill: a rectangle of a target set to a D3DCOLOR.
#[derive(Clone, Debug, PartialEq)]
pub struct FillCall {
    pub dst: Target,
    pub rect: [i32; 4],
    pub colour: u32,
}

/// What draws the frames.
pub trait GraphicsBackend: Send + std::any::Any {
    /// Wait until previously recorded host commands have executed. Immediate
    /// backends have nothing queued; read_back/query_result retain their own
    /// GPU synchronization.
    fn fence(&mut self) {}
    /// A mark after every command recorded so far (L102); `passed` tells
    /// whether the commands up to it have executed. Immediate backends
    /// execute as they record: every mark has passed.
    fn mark(&mut self) -> u64 { 0 }
    fn passed(&self, _mark: u64) -> bool { true }
    fn upload(&mut self, upload: Upload);
    fn draw(&mut self, draw: &DrawCall);
    fn clear(&mut self, clear: &ClearCall);
    /// Show the back buffer surface.
    fn present(&mut self, back_buffer: &Target);
    fn copy(&mut self, copy: &CopyCall);
    fn fill(&mut self, fill: &FillCall);
    /// A target's contents in its D3D9 format, rows tightly packed
    /// (GetRenderTargetData); None when the backend cannot read targets.
    fn read_back(&mut self, target: &Target) -> Option<Vec<u8>>;
    /// An occlusion query (D3DQUERYTYPE_OCCLUSION) issued with
    /// D3DISSUE_BEGIN: count the samples the following draws pass the
    /// depth and stencil tests with (clears and copies do not count). A
    /// second begin restarts the count.
    fn query_begin(&mut self, query: u32);
    /// The query issued with D3DISSUE_END: stop counting.
    fn query_end(&mut self, query: u32);
    /// An ended query's sample count; None when the backend does not count
    /// samples (it draws nothing).
    fn query_result(&mut self, query: u32) -> Option<u64>;
    /// The gamma ramp the frames are shown through (SetGammaRamp on a
    /// full-screen swap chain); None is the identity.
    fn gamma(&mut self, ramp: Option<&GammaRamp>);
    /// A surface, texture, buffer or query the runtime freed: drop what the
    /// backend keeps for it (the guest may reuse the address).
    fn release(&mut self, resource: u32);
}

/// D3DGAMMARAMP: 256 red, then green, then blue entries.
pub type GammaRamp = [[u16; 256]; 3];


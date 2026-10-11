//! The commands the runtime's Direct3D 9 device sends to a graphics
//! backend (docs/RUNTIME.md, "Graphics backend design"): resource uploads,
//! draws with their complete state, clears and presents. A separate crate so
//! that backends (crates/ddgpu, wgpu) do not depend on the runtime.

/// A texture's dimensionality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// D3DSAMPLERSTATETYPE 1..13 values
    pub states: [u32; 14],
}

/// A render target or depth-stencil: the surface, its format and size, and
/// the texture level it belongs to (if any).
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub surface: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub texture: Option<u32>,
    /// for a texture's surface: its index among the texture's level
    /// surfaces (face * levels + level)
    pub level: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Vertices {
    /// stream number, vertex buffer, offset, stride, frequency setting
    Streams(Vec<(u32, u32, u32, u32, u32)>),
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
    pub vertex_shader: Option<Vec<u32>>,
    pub pixel_shader: Option<Vec<u32>>,
    /// D3DVERTEXELEMENT9 bytes (END excluded); empty when FVF drives it
    pub declaration: Vec<[u8; 8]>,
    pub fvf: u32,
    pub vs_constants: Constants,
    pub ps_constants: Constants,
    pub textures: Vec<TextureBinding>,
    /// every render state, sorted by state
    pub render_states: Vec<(u32, u32)>,
    pub viewport: [u32; 6],
    pub scissor: [i32; 4],
    pub targets: [Option<Target>; 4],
    pub depth: Option<Target>,
}

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
}

/// What draws the frames.
pub trait GraphicsBackend: Send + std::any::Any {
    fn upload(&mut self, upload: Upload);
    fn draw(&mut self, draw: &DrawCall);
    fn clear(&mut self, clear: &ClearCall);
    /// Show the back buffer surface.
    fn present(&mut self, back_buffer: &Target);
}


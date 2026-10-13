//! Owned nDraw-to-worker transport. These are effective D3D9 values after
//! mtstate decoding, not wgpu objects or guest pointers to mutable memory.
use crate::{Target, TextureBinding};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NativeObjectIdentity {
    pub address: u32,
    /// nDraw immutable object's hash at +0x0c; address reuse changes identity.
    pub hash: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NativeStateIdentity {
    pub blend: NativeObjectIdentity,
    pub depth: NativeObjectIdentity,
    pub raster: NativeObjectIdentity,
    pub raster_flip: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeFixedState {
    pub identity: NativeStateIdentity,
    /// Effective D3DRS/value pairs, sorted with no duplicates. Includes dynamic
    /// stencil reference/blend factor; those do not identify a pipeline.
    pub render_states: Arc<[(u32, u32)]>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeFloatWrite {
    pub first: u32,
    pub registers: Arc<[[u32; 4]]>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeIntWrite {
    pub first: u32,
    pub registers: Arc<[[i32; 4]]>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeBoolWrite {
    pub first: u32,
    pub registers: Arc<[u32]>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeConstants {
    /// This draw's writes must cover every shader-read register. They are
    /// complete binding updates, not deltas against a preceding native job.
    /// Overlapping ranges execute in order; all values are retained bits.
    pub floats: Vec<NativeFloatWrite>,
    pub ints: Vec<NativeIntWrite>,
    pub bools: Vec<NativeBoolWrite>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NativeStream {
    pub stream: u32,
    pub resource: u32,
    pub offset: u32,
    pub stride: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NativeIndices {
    pub resource: u32,
    /// D3DFMT_INDEX16 (101) or INDEX32 (102).
    pub format: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct NativeDraw {
    pub state: Arc<NativeFixedState>,
    pub vertex_shader: Arc<[u32]>,
    pub pixel_shader: Arc<[u32]>,
    /// Resolved D3DVERTEXELEMENT9 array, excluding END; no FVF shorthand.
    pub declaration: Arc<[[u8; 8]]>,
    pub textures: Arc<[TextureBinding]>,
    pub streams: Arc<[NativeStream]>,
    pub indices: Option<NativeIndices>,
    pub vs_constants: NativeConstants,
    pub ps_constants: NativeConstants,
    /// x, y, width, height, minimum/maximum depth IEEE f32 bits.
    pub viewport: [u32; 6],
    /// D3D rectangle: left, top, right, bottom (not width/height).
    pub scissor: Option<[i32; 4]>,
    pub primitive: u32,
    pub primitives: u32,
    pub start: u32,
    pub base_vertex: i32,
    pub min_index: u32,
    pub vertex_count: u32,
    /// Initial adapter supports exactly one; unsupported forms reject the job.
    pub instances: u32,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativePass {
    pub colours: [Option<Target>; 4],
    pub depth: Option<Target>,
    /// Full-attachment clears only. Colour is D3DCOLOR; depth is f32 bits.
    /// None preserves the pre-pass attachment contents.
    pub clear_colour: Option<u32>,
    pub clear_depth: Option<u32>,
    pub clear_stencil: Option<u32>,
    pub draws: Vec<NativeDraw>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeJob {
    pub passes: Vec<NativePass>,
}

/// L127 snapshots the load state before the oracle's first write. The same
/// FIFO carries begin, legacy oracle draws, and native comparison completion.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeCompareBegin {
    pub id: u64,
    pub colours: [Option<Target>; 4],
    pub depth: Option<Target>,
}

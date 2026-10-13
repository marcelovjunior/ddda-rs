//! What the backend keeps between draws (brief L25, "backend costs"): the
//! wgpu objects a draw builds from its state are cached by that state, and
//! per-draw data goes into ring buffers uploaded once per submission. None
//! of this changes what a draw draws: every key holds everything the cached
//! object was built from.

/// A buffer that per-draw data is appended to. The bytes are staged on the
/// CPU and written in one `write_buffer` before the submission that uses
/// them (writes take effect at the start of the next submission, after
/// everything submitted before, so a region can be reused after a flush).
pub(crate) struct Ring {
    pub buffer: wgpu::Buffer,
    cpu: Vec<u8>,
    at: u64,
    /// the first byte not yet written to the GPU buffer
    dirty_from: u64,
    /// counts restarts: an offset allocated in this generation still holds
    /// what was written there
    pub generation: u64,
}

impl Ring {
    pub fn new(device: &wgpu::Device, size: u64, usage: wgpu::BufferUsages, label: &str) -> Ring {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Ring { buffer, cpu: vec![0; size as usize], at: 0, dirty_from: 0, generation: 0 }
    }

    pub fn size(&self) -> u64 {
        self.cpu.len() as u64
    }

    pub fn fits(&self, len: u64, align: u64) -> bool {
        self.at.next_multiple_of(align) + len <= self.size()
    }

    /// Room for `len` bytes at a multiple of `align`, or None when the ring
    /// is full (the caller flushes and calls [`restart`](Self::restart)).
    pub fn alloc(&mut self, len: u64, align: u64) -> Option<(u64, &mut [u8])> {
        let off = self.at.next_multiple_of(align);
        if off + len > self.size() {
            return None;
        }
        self.at = off + len;
        Some((off, &mut self.cpu[off as usize..(off + len) as usize]))
    }

    /// Write the staged bytes to the GPU buffer.
    pub fn upload(&mut self, queue: &wgpu::Queue) {
        let end = self.at.next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT);
        if end > self.dirty_from {
            queue.write_buffer(&self.buffer, self.dirty_from, &self.cpu[self.dirty_from as usize..end as usize]);
        }
        self.dirty_from = end;
    }

    /// Start again at the beginning (after a flush: earlier users are
    /// submitted).
    pub fn restart(&mut self) {
        self.at = 0;
        self.dirty_from = 0;
        self.generation += 1;
    }
}

/// A fast hasher for the backend's maps (L78; rustc's FxHash): keys are
/// resource addresses, states and small structures from the game, and
/// SipHash cost more than the lookups.
#[derive(Default, Clone, Copy)]
pub(crate) struct FxHasher(u64);

impl std::hash::Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.write_u64(u64::from_le_bytes(c.try_into().unwrap()));
        }
        for &b in chunks.remainder() {
            self.write_u64(b as u64);
        }
    }
    #[inline]
    fn write_u8(&mut self, v: u8) {
        self.write_u64(v as u64);
    }
    #[inline]
    fn write_u16(&mut self, v: u16) {
        self.write_u64(v as u64);
    }
    #[inline]
    fn write_u32(&mut self, v: u32) {
        self.write_u64(v as u64);
    }
    #[inline]
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    #[inline]
    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }
}

pub(crate) type FxMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<FxHasher>>;

/// One vertex buffer of a pipeline: stride, step mode, attributes
/// (format, offset, shader location).
pub(crate) type BufferLayout = (u64, wgpu::VertexStepMode, Vec<(wgpu::VertexFormat, u64, u32)>);

/// The texture and sampler layout of a stage: (sampler number, view
/// dimension 0 flat, 1 cube, 2 volume) per sampler.
pub(crate) type SamplerKinds = Vec<(u32, u8)>;

/// Everything a render pipeline is built from.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct PipelineKey {
    /// the vertex and pixel shader module keys
    pub vs: ModuleKey,
    pub ps: ModuleKey,
    pub buffers: Vec<BufferLayout>,
    pub topology: wgpu::PrimitiveTopology,
    /// the index format of an indexed strip draw (wgpu needs it on the
    /// pipeline; None for lists and non-indexed strips)
    pub strip_index: Option<wgpu::IndexFormat>,
    pub cull: Option<wgpu::Face>,
    pub polygon_mode: wgpu::PolygonMode,
    pub targets: Vec<Option<wgpu::ColorTargetState>>,
    pub depth: Option<wgpu::DepthStencilState>,
    /// samples per pixel of the attachments (multisampled targets, L31)
    pub samples: u32,
    /// D3DRS_MULTISAMPLEMASK for multisampled attachments
    pub sample_mask: u32,
}

/// A texture bind group: the stage and, per sampler, its number, view
/// dimension, texture, sRGB read and sampler states.
pub(crate) type TexGroupKey = (bool, Vec<(u32, u8, u32, bool, [u32; 14])>);

/// A target view: surface, texture, level, D3D format, sRGB view.
/// A translated shader: (token hash, linked vertex shader hash, depth bias,
/// channels sampled as 1 per sampler).
pub(crate) type ModuleKey = (u64, u64, bool, u64);

pub(crate) type TargetKey = (u32, Option<u32>, u32, u32, bool);

/// Bytes of the vertex and pixel constant blocks: 256 (vertex) or 224
/// (pixel) float4, 16 int4, 16 bool (one per vec4), then the extra vec4s
/// (vertex: the half-pixel offset; pixel: alpha test, 4 of sampler LOD
/// bias, and the depth bias when the shader takes it).
pub(crate) const VS_BLOCK: u64 = 256 * 16 + 16 * 16 + 16 * 16 + 16;
pub(crate) const PS_BLOCK: u64 = 224 * 16 + 16 * 16 + 16 * 16 + 6 * 16;
/// Uniform offsets (wgpu's default min_uniform_buffer_offset_alignment).
pub(crate) const UNIFORM_ALIGN: u64 = 256;
pub(crate) const UNIFORM_RING: u64 = 8 << 20;
pub(crate) const INLINE_RING: u64 = 4 << 20;
/// Draws recorded before they are submitted anyway (the GPU starts on a
/// frame while the guest still records it). L72: each submission ends the
/// open render pass, so this is high (it was 128): the uniform ring fills
/// first (about 500 draws), or Present comes.
pub(crate) const DRAWS_PER_SUBMIT: u32 = 1024;

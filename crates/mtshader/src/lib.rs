//! Reader for the MT Framework shader package index, `BbsShaderPackage.mfx`
//! (`nativePC\usershader`), as used by Dragon's Dogma: Dark Arisen on PC.
//!
//! The file names every *shader object* the engine knows: input layouts
//! (`IANonSkinTBN`), texture slots (`tAlbedoMap`), samplers, constant
//! buffers (`CBMaterial`) and shader feature switches (`FAlbedo`). Other
//! files refer to them by a 32-bit id:
//!
//! ```text
//! id = (JAMCRC(name) & 0xFFFFF) << 12 | entry index
//! ```
//!
//! Checked on the Steam data: all 500,929 shader-object ids in the 3,972
//! distinct MRL files, and all 24 MOD vertex formats, resolve this way.
//!
//! Layout (field names follow Albam's `mfx.ksy`, MIT; checked here):
//!
//! ```text
//! 0x00 "MFX\0", u16, u16, u32
//! 0x0C u32 entry count, u32 string-table offset, u32
//! 0x18 u32 entry offsets[count]
//! entry: u32 name, u32 kind name (string-table offsets), u32 bits (kind = low 6),
//!        u16, u16 index, u32, u32, u8 element count, u8, u16, ...
//! kind 9 (input layout): at +0x1C u32, then elements of 8 bytes:
//!        u32 name, u32 bits: type = (b >> 6) & 0x1F, count = (b >> 11) & 0x7F,
//!        semantic index = (b >> 18) & 0xF, offset = (b >> 22) & 0x1FF,
//!        per-instance = b >> 31
//! ```

use std::collections::HashMap;
use std::fmt;

pub const MAGIC: [u8; 4] = *b"MFX\0";

/// Kind numbers seen in the entry bits (meaning from the kind names and
/// usage: 0 constant buffer, 1 texture, 2 feature switch, 3 sampler state,
/// 9 input layout).
pub const KIND_CONSTANT_BUFFER: u8 = 0;
pub const KIND_TEXTURE: u8 = 1;
pub const KIND_FEATURE: u8 = 2;
pub const KIND_SAMPLER: u8 = 3;
/// 4 blend state (`BSSolid`, `BSBlendAlpha`, ...), 5 depth-stencil state
/// (`DSZTestWrite`, ...), 6 rasterizer state (`RSMesh`, ...): from the
/// entry names, and the ids in MRL material headers resolve to them.
pub const KIND_BLEND_STATE: u8 = 4;
pub const KIND_DEPTH_STENCIL_STATE: u8 = 5;
pub const KIND_RASTERIZER_STATE: u8 = 6;
pub const KIND_INPUT_LAYOUT: u8 = 9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub index: u16,
    pub kind: u8,
    pub name: String,
    pub kind_name: String,
}

/// Element component types as stored in the MFX. Sizes are fixed by the
/// layouts (offsets of consecutive elements); decoding rules are verified
/// on the vertex data where marked, the rest are labelled unresolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementType {
    /// 1: f32. Verified (positions lie inside the model bounds).
    F32,
    /// 2: IEEE half float (UVs, some weights).
    F16,
    /// 3: 16-bit integer (joint index in 1-weight skins). Signedness unresolved.
    U16,
    /// 4: 16-bit, used for sprite/particle fields. Unresolved.
    Unk4,
    /// 5: signed 16-bit normalised (/32767): quantised skinned positions and
    /// the first weight. Verified against bounds.
    S16N,
    /// 7: 8-bit, particle fields. Unresolved.
    Unk7,
    /// 8: u8 integer (joint indices).
    U8,
    /// 9: u8 biased to [-1, 1] as `x / 255 * 2 - 1` (normals, tangents).
    /// Verified: normals come out unit length within quantisation (0.008).
    U8Biased,
    /// 10: u8 normalised to [0, 1] (weights, occlusion, vertex alpha).
    U8N,
    /// 11: 4 x u8 biased, one packed element (normals of the `B` layouts).
    /// Verified like type 9.
    U8x4Biased,
    /// 13: u8 normalised colour channel.
    U8Color,
    /// 14: packed 32-bit colour (D3DCOLOR-like). Byte order unresolved.
    Color32,
    Other(u8),
}

impl ElementType {
    pub fn from_code(c: u8) -> Self {
        match c {
            1 => Self::F32,
            2 => Self::F16,
            3 => Self::U16,
            4 => Self::Unk4,
            5 => Self::S16N,
            7 => Self::Unk7,
            8 => Self::U8,
            9 => Self::U8Biased,
            10 => Self::U8N,
            11 => Self::U8x4Biased,
            13 => Self::U8Color,
            14 => Self::Color32,
            c => Self::Other(c),
        }
    }

    /// Bytes per component (packed types count as one component).
    pub fn size(self) -> Option<usize> {
        Some(match self {
            Self::F32 | Self::U8x4Biased | Self::Color32 => 4,
            Self::F16 | Self::U16 | Self::Unk4 | Self::S16N => 2,
            Self::Unk7 | Self::U8 | Self::U8Biased | Self::U8N | Self::U8Color => 1,
            Self::Other(_) => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub ty: ElementType,
    pub type_code: u8,
    pub count: u8,
    pub semantic_index: u8,
    pub offset: u16,
    pub per_instance: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputLayout {
    pub name: String,
    pub index: u16,
    pub elements: Vec<Element>,
}

impl InputLayout {
    /// First element with this name and semantic index.
    pub fn element(&self, name: &str, semantic_index: u8) -> Option<&Element> {
        self.elements
            .iter()
            .find(|e| e.name == name && e.semantic_index == semantic_index)
    }
}

/// Base type of a constant-buffer parameter (low 4 bits of its type word).
/// Meanings from the parameter names: `f...` are floats, `b...` bools,
/// `i...` ints; the struct codes (`sLightParam`, `sShadowReceiveParam`,
/// `sOutlineDetector`) have no member list in the MFX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    /// 1: float.
    Float,
    /// 3: bool (`bShadowUseMulti`, ...).
    Bool,
    /// 4: int (`iDebugView`, ...).
    Int,
    /// 0xB, 0xC, 0xD: structures; layout not described in the MFX.
    Struct(u8),
    Other(u8),
}

impl ParamType {
    pub fn from_code(c: u8) -> Self {
        match c {
            1 => Self::Float,
            3 => Self::Bool,
            4 => Self::Int,
            0xB..=0xD => Self::Struct(c),
            c => Self::Other(c),
        }
    }
}

/// One parameter of a constant buffer (MFX kind 0 entry).
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: ParamType,
    /// Components per row (1..4); 0 for structures.
    pub columns: u8,
    /// Rows (a `float4x2` such as `fUVTransform` has 2).
    pub rows: u16,
    /// Array length (1 for a single value).
    pub array: u16,
    /// Offset in floats (4-byte words) from the buffer start.
    pub offset: u16,
    /// Raw type word: `array << 20 | rows << 8 | columns << 4 | type`.
    pub type_word: u32,
    /// Raw second word. *Unresolved* (likely editor/annotation flags).
    pub bits: u32,
    /// Top byte of the offset word (0..5 in the data). *Unresolved.*
    pub class: u8,
}

impl Param {
    /// Floats this parameter occupies, for scalar/vector/matrix types
    /// (rows padded to 4 except the last one), or `None` for structures.
    pub fn float_count(&self) -> Option<usize> {
        if self.ty == ParamType::Float || self.ty == ParamType::Bool || self.ty == ParamType::Int {
            let (c, r, a) = (self.columns as usize, self.rows as usize, self.array as usize);
            Some(4 * r * a.saturating_sub(1) + 4 * r.saturating_sub(1) + c)
        } else {
            None
        }
    }
}

/// A constant buffer (MFX kind 0): its parameters and default values.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstantBuffer {
    pub name: String,
    pub index: u16,
    /// Size in floats.
    pub size: u16,
    pub params: Vec<Param>,
    /// `size` floats of default values (empty if the entry has none).
    pub defaults: Vec<f32>,
}

impl ConstantBuffer {
    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Floats of parameter `name` read from `data` (a buffer of this
    /// layout as little-endian f32 bytes, e.g. from an MRL). `None` if the
    /// parameter is missing, a structure, or outside the data.
    pub fn read(&self, name: &str, data: &[u8]) -> Option<Vec<f32>> {
        let p = self.param(name)?;
        let n = p.float_count()?;
        let o = p.offset as usize * 4;
        let bytes = data.get(o..o + n * 4)?;
        Some(bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
    }

    /// Size in bytes of the buffer's data.
    pub fn byte_size(&self) -> usize {
        self.size as usize * 4
    }
}

/// Blend factor codes of a blend state. Codes 0-5 are confirmed by the
/// named states (`BSBlendAlpha` = SrcAlpha/InvSrcAlpha, `BSAdd` = One/One,
/// `BSMul` = Zero/SrcColor, `BSComposite` = One/InvSrcAlpha); 6-10 follow
/// the D3D order (*assumed*); 11/12 are the factors of `BSBlendFactor` and
/// `BSBlendFactorAlpha` (*assumed* BlendFactor / InvBlendFactor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendFactor {
    Zero,
    One,
    SrcColor,
    InvSrcColor,
    SrcAlpha,
    InvSrcAlpha,
    DestAlpha,
    InvDestAlpha,
    DestColor,
    InvDestColor,
    SrcAlphaSat,
    BlendFactor,
    InvBlendFactor,
    Other(u8),
}

impl BlendFactor {
    pub fn from_code(c: u8) -> Self {
        use self::BlendFactor as F;
        [
            F::Zero,
            F::One,
            F::SrcColor,
            F::InvSrcColor,
            F::SrcAlpha,
            F::InvSrcAlpha,
            F::DestAlpha,
            F::InvDestAlpha,
            F::DestColor,
            F::InvDestColor,
            F::SrcAlphaSat,
            F::BlendFactor,
            F::InvBlendFactor,
        ]
        .get(c as usize)
        .copied()
        .unwrap_or(F::Other(c))
    }
}

/// Blend operation: 2 (`BSRevSub*`) and 4 (`BSMax`) are confirmed by
/// name; the rest follow the D3D order (*assumed*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendOp {
    Add,
    Subtract,
    RevSubtract,
    Min,
    Max,
    Other(u8),
}

/// A blend state (MFX kind 4), colour channel of render target 0.
/// Word 0 at entry +0x18: bit 1 enable, bits 9-13 source factor, bits
/// 17-21 destination factor, bits 25-27 operation. Byte 0 of word 2
/// (+0x20) is the RGBA write mask of target 0 (`BSRGBWrite` = 7,
/// `BSRWrite` = 1, `BSNoWrite` = 0). The alpha-channel word (+0x1C) is
/// *unresolved*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlendState {
    pub enable: bool,
    pub src: BlendFactor,
    pub dst: BlendFactor,
    pub op: BlendOp,
    pub write_mask: u8,
    pub raw: [u32; 4],
}

impl BlendState {
    pub fn from_words(w: [u32; 4]) -> Self {
        BlendState {
            enable: w[0] & 2 != 0,
            src: BlendFactor::from_code(((w[0] >> 9) & 0x1F) as u8),
            dst: BlendFactor::from_code(((w[0] >> 17) & 0x1F) as u8),
            op: match (w[0] >> 25) & 7 {
                0 => BlendOp::Add,
                1 => BlendOp::Subtract,
                2 => BlendOp::RevSubtract,
                3 => BlendOp::Min,
                4 => BlendOp::Max,
                c => BlendOp::Other(c as u8),
            },
            write_mask: (w[2] & 0xFF) as u8,
            raw: w,
        }
    }
}

/// Depth comparison, D3D order from 0 (NEVER). Confirmed by name:
/// `DSZTestWriteGT` = 4 (Greater), `DSZWrite` / `DSDefault` = 7 (Always,
/// with the test bit off for `DSDefault`); `DSZTest*` use 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareFunc {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}

/// A depth-stencil state (MFX kind 5). Word 0 at entry +0x18: bit 0
/// depth test, bit 1 depth write, bits 2-4 comparison. Stencil fields
/// (bits 6+ and word 1) are *unresolved*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepthStencilState {
    pub depth_test: bool,
    pub depth_write: bool,
    pub func: CompareFunc,
    pub raw: [u32; 2],
}

impl DepthStencilState {
    pub fn from_words(w: [u32; 2]) -> Self {
        use CompareFunc::*;
        DepthStencilState {
            depth_test: w[0] & 1 != 0,
            depth_write: w[0] & 2 != 0,
            func: [Never, Less, Equal, LessEqual, Greater, NotEqual, GreaterEqual, Always][((w[0] >> 2) & 7) as usize],
            raw: w,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cull {
    None,
    Front,
    Back,
    Other(u8),
}

/// A rasterizer state (MFX kind 6). Word 0 at entry +0x18: bit 0 solid
/// fill (`RSWireFrame` clears it), bits 3-4 cull (`RSMeshCN` 0 none,
/// `RSMeshCF` 1 front, `RSMesh` 2 back), bit 8 scissor (`RSScissor*`).
/// Word 1 is a signed depth bias (`RSMeshBias1` = -32, steps of -32).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RasterizerState {
    pub solid: bool,
    pub cull: Cull,
    pub scissor: bool,
    pub depth_bias: i32,
    pub raw: [u32; 2],
}

impl RasterizerState {
    pub fn from_words(w: [u32; 2]) -> Self {
        RasterizerState {
            solid: w[0] & 1 != 0,
            cull: match (w[0] >> 3) & 3 {
                0 => Cull::None,
                1 => Cull::Front,
                2 => Cull::Back,
                c => Cull::Other(c as u8),
            },
            scissor: w[0] & 0x100 != 0,
            depth_bias: w[1] as i32,
            raw: w,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    BadMagic,
    Truncated(usize),
    BadString(u32),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadMagic => write!(f, "not an MFX file"),
            Error::Truncated(o) => write!(f, "truncated at {o:#x}"),
            Error::BadString(o) => write!(f, "bad string offset {o:#x}"),
        }
    }
}

impl std::error::Error for Error {}

/// JAMCRC of a shader-object name, as used in the high 20 bits of an id.
pub fn name_hash(name: &str) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in name.as_bytes() {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// Id that other files use for this entry.
pub fn object_id(name: &str, index: u16) -> u32 {
    (name_hash(name) & 0xF_FFFF) << 12 | u32::from(index) & 0xFFF
}

#[derive(Debug, Clone, Default)]
pub struct ShaderPackage {
    pub entries: HashMap<u16, Entry>,
    pub layouts: HashMap<u16, InputLayout>,
    pub constant_buffers: HashMap<u16, ConstantBuffer>,
    pub blend_states: HashMap<u16, BlendState>,
    pub depth_stencil_states: HashMap<u16, DepthStencilState>,
    pub rasterizer_states: HashMap<u16, RasterizerState>,
}

fn u32_at(b: &[u8], o: usize) -> Result<u32, Error> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or(Error::Truncated(o))
}

/// Kind 0 entry body (offsets are absolute file offsets):
///
/// ```text
/// +0x18 u32: size in floats (low 16), parameter count (high 16)
/// +0x20 u32 parameter table, +0x24 u32 default values (size floats)
/// parameter (28 bytes): u32 name, u32 bits, u32 type word,
///   u32, u32 offset word (offset in floats = (w >> 8) & 0xFFFF,
///   top byte unresolved), u32 annotation table, u32
/// ```
///
/// Checked on all 207 constant buffers of the Steam package: in every
/// buffer without structure parameters (all but 21), the last parameter
/// ends in the buffer's last 4-float register.
fn parse_constant_buffer(
    b: &[u8],
    p: usize,
    string: &dyn Fn(u32) -> Result<String, Error>,
    name: String,
    index: u16,
) -> Result<ConstantBuffer, Error> {
    let w = u32_at(b, p + 0x18)?;
    let size = (w & 0xFFFF) as u16;
    let count = (w >> 16) as usize;
    let params_at = u32_at(b, p + 0x20)? as usize;
    let defaults_at = u32_at(b, p + 0x24)? as usize;
    let mut params = Vec::with_capacity(count);
    for k in 0..count {
        let q = params_at + k * 28;
        let type_word = u32_at(b, q + 8)?;
        let off = u32_at(b, q + 16)?;
        params.push(Param {
            name: string(u32_at(b, q)?)?,
            ty: ParamType::from_code((type_word & 0xF) as u8),
            columns: ((type_word >> 4) & 0xF) as u8,
            rows: ((type_word >> 8) & 0xFFF) as u16,
            array: (type_word >> 20) as u16,
            offset: ((off >> 8) & 0xFFFF) as u16,
            type_word,
            bits: u32_at(b, q + 4)?,
            class: (off >> 24) as u8,
        });
    }
    let defaults = if defaults_at == 0 {
        Vec::new()
    } else {
        (0..size as usize)
            .map(|i| u32_at(b, defaults_at + 4 * i).map(f32::from_bits))
            .collect::<Result<_, _>>()?
    };
    Ok(ConstantBuffer {
        name,
        index,
        size,
        params,
        defaults,
    })
}

impl ShaderPackage {
    pub fn parse(b: &[u8]) -> Result<Self, Error> {
        if b.get(0..4) != Some(&MAGIC[..]) {
            return Err(Error::BadMagic);
        }
        let count = u32_at(b, 0x0C)? as usize;
        let strings = u32_at(b, 0x10)? as usize;
        let string = |o: u32| -> Result<String, Error> {
            let start = strings + o as usize;
            let rest = b.get(start..).ok_or(Error::BadString(o))?;
            let end = rest.iter().position(|&c| c == 0).ok_or(Error::BadString(o))?;
            Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
        };
        let mut pkg = ShaderPackage::default();
        for i in 0..count {
            let p = u32_at(b, 0x18 + i * 4)? as usize;
            if p == 0 {
                continue;
            }
            let bits = u32_at(b, p + 8)?;
            let index = u16::from_le_bytes(
                b.get(p + 14..p + 16)
                    .ok_or(Error::Truncated(p))?
                    .try_into()
                    .unwrap(),
            );
            // One entry in the Steam file has a name offset outside the string
            // table; skip unnamed entries rather than fail the package.
            let Ok(name) = string(u32_at(b, p)?) else { continue };
            let kind_name = string(u32_at(b, p + 4)?).unwrap_or_default();
            let kind = (bits & 0x3F) as u8;
            if kind == KIND_INPUT_LAYOUT {
                let n = *b.get(p + 24).ok_or(Error::Truncated(p))? as usize;
                let mut elements = Vec::with_capacity(n);
                for k in 0..n {
                    let eo = p + 32 + k * 8;
                    let eb = u32_at(b, eo + 4)?;
                    let code = ((eb >> 6) & 0x1F) as u8;
                    elements.push(Element {
                        name: string(u32_at(b, eo)?)?,
                        ty: ElementType::from_code(code),
                        type_code: code,
                        count: ((eb >> 11) & 0x7F) as u8,
                        semantic_index: ((eb >> 18) & 0xF) as u8,
                        offset: ((eb >> 22) & 0x1FF) as u16,
                        per_instance: eb >> 31 != 0,
                    });
                }
                pkg.layouts.insert(
                    index,
                    InputLayout {
                        name: name.clone(),
                        index,
                        elements,
                    },
                );
            }
            if kind == KIND_CONSTANT_BUFFER {
                pkg.constant_buffers
                    .insert(index, parse_constant_buffer(b, p, &string, name.clone(), index)?);
            }
            let words = |n: usize| -> Result<Vec<u32>, Error> {
                (0..n).map(|k| u32_at(b, p + 0x18 + 4 * k)).collect()
            };
            match kind {
                KIND_BLEND_STATE => {
                    let w = words(4)?;
                    pkg.blend_states
                        .insert(index, BlendState::from_words([w[0], w[1], w[2], w[3]]));
                }
                KIND_DEPTH_STENCIL_STATE => {
                    let w = words(2)?;
                    pkg.depth_stencil_states
                        .insert(index, DepthStencilState::from_words([w[0], w[1]]));
                }
                KIND_RASTERIZER_STATE => {
                    let w = words(2)?;
                    pkg.rasterizer_states
                        .insert(index, RasterizerState::from_words([w[0], w[1]]));
                }
                _ => {}
            }
            pkg.entries.insert(
                index,
                Entry {
                    index,
                    kind,
                    name,
                    kind_name,
                },
            );
        }
        Ok(pkg)
    }

    /// Entry named by a 32-bit shader-object id, if the hash matches.
    pub fn object(&self, id: u32) -> Option<&Entry> {
        let e = self.entries.get(&((id & 0xFFF) as u16))?;
        (name_hash(&e.name) & 0xF_FFFF == id >> 12).then_some(e)
    }

    /// Constant-buffer layout for a 32-bit shader-object id (as in an MRL
    /// `ConstantBuffer` command).
    pub fn constant_buffer(&self, id: u32) -> Option<&ConstantBuffer> {
        let e = self.object(id)?;
        self.constant_buffers.get(&e.index)
    }

    /// Blend state for an MRL material's blend-state id.
    pub fn blend_state(&self, id: u32) -> Option<&BlendState> {
        self.blend_states.get(&self.object(id)?.index)
    }

    /// Depth-stencil state for an MRL material's depth-stencil id.
    pub fn depth_stencil_state(&self, id: u32) -> Option<&DepthStencilState> {
        self.depth_stencil_states.get(&self.object(id)?.index)
    }

    /// Rasterizer state for an MRL material's rasterizer id.
    pub fn rasterizer_state(&self, id: u32) -> Option<&RasterizerState> {
        self.rasterizer_states.get(&self.object(id)?.index)
    }

    /// Constant-buffer layout by name.
    pub fn constant_buffer_named(&self, name: &str) -> Option<&ConstantBuffer> {
        self.constant_buffers.values().find(|c| c.name == name)
    }

    /// Input layout for a MOD vertex-format id.
    pub fn layout(&self, vertex_format: u32) -> Option<&InputLayout> {
        let e = self.object(vertex_format)?;
        self.layouts.get(&e.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_ids_match_known_vertex_formats() {
        // Ids as stored in DDDA MOD files, names and indices from the MFX.
        assert_eq!(object_id("IANonSkinTBN", 44), 0x5E7F_202C);
        assert_eq!(object_id("IASkinTB4wt", 32), 0x14D4_0020);
        assert_eq!(object_id("IANonSkinB", 54), 0xA7D7_D036);
    }

    #[test]
    fn parses_synthetic_layout() {
        let mut b = Vec::new();
        b.extend(MAGIC);
        b.extend([0u8; 8]);
        b.extend(1u32.to_le_bytes()); // count
        let strings_at = 0x18 + 4 + 48;
        b.extend((strings_at as u32).to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(0x1Cu32.to_le_bytes()); // entry offset
                                         // entry at 0x1C
        b.extend(0u32.to_le_bytes()); // name "IAX"
        b.extend(4u32.to_le_bytes()); // kind name
        b.extend(9u32.to_le_bytes()); // kind 9
        b.extend(0u16.to_le_bytes());
        b.extend(7u16.to_le_bytes()); // index
        b.extend([0u8; 8]);
        b.extend([2u8, 0, 0, 0]); // 2 elements
        b.extend(0u32.to_le_bytes());
        b.extend(18u32.to_le_bytes()); // "Position"
        b.extend((1u32 << 6 | 3 << 11).to_le_bytes());
        b.extend(27u32.to_le_bytes()); // "TexCoord"
        b.extend((2u32 << 6 | 2 << 11 | 1 << 18 | 12 << 22).to_le_bytes());
        assert_eq!(b.len(), strings_at);
        b.extend(b"IAX\0__InputLayout\0Position\0TexCoord\0");
        let pkg = ShaderPackage::parse(&b).unwrap();
        let l = &pkg.layouts[&7];
        assert_eq!(l.name, "IAX");
        assert_eq!(l.elements[1].ty, ElementType::F16);
        assert_eq!((l.elements[1].semantic_index, l.elements[1].offset), (1, 12));
        assert_eq!(pkg.object(object_id("IAX", 7)).unwrap().name, "IAX");
        assert!(pkg.object(object_id("IAY", 7)).is_none());
    }

    #[test]
    fn decodes_state_words() {
        // Words of BSBlendAlpha, DSZTest and RSMeshBias1 in the Steam MFX.
        let bs = BlendState::from_words([0x000A_0802, 1, 0x0F0F_0F0F, 0x0F0F_0F0F]);
        assert!(bs.enable);
        assert_eq!((bs.src, bs.dst, bs.op), (BlendFactor::SrcAlpha, BlendFactor::InvSrcAlpha, BlendOp::Add));
        assert_eq!(bs.write_mask, 0xF);
        let solid = BlendState::from_words([0x200, 1, 0x0F0F_0F0F, 0x0F0F_0F0F]);
        assert!(!solid.enable);
        let ds = DepthStencilState::from_words([0x007F_FF8D, 0x0070_0070]);
        assert!(ds.depth_test && !ds.depth_write);
        assert_eq!(ds.func, CompareFunc::LessEqual);
        let rs = RasterizerState::from_words([0x291, 0xFFFF_FFE0]);
        assert_eq!((rs.solid, rs.cull, rs.depth_bias), (true, Cull::Back, -32));
    }

    #[test]
    fn parses_synthetic_constant_buffer() {
        let mut b = Vec::new();
        b.extend(MAGIC);
        b.extend([0u8; 8]);
        b.extend(1u32.to_le_bytes()); // count
        let entry = 0x1C;
        let params = entry + 0x28;
        let defaults = params + 2 * 28;
        let strings = defaults + 8 * 4;
        b.extend((strings as u32).to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend((entry as u32).to_le_bytes());
        // entry: name "CBX", kind name "", kind 0, index 5
        b.extend(0u32.to_le_bytes());
        b.extend(4u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(5u16.to_le_bytes());
        b.extend([0u8; 8]);
        b.extend((2u32 << 16 | 8).to_le_bytes()); // 2 params, 8 floats
        b.extend(0u32.to_le_bytes());
        b.extend((params as u32).to_le_bytes());
        b.extend((defaults as u32).to_le_bytes());
        assert_eq!(b.len(), params);
        // fColor: float3 at 0; fUV: float4x1 at 4
        for (name, ty, off) in [(5u32, 0x0010_0131u32, 0u32), (12, 0x0010_0141, 4)] {
            b.extend(name.to_le_bytes());
            b.extend(0u32.to_le_bytes());
            b.extend(ty.to_le_bytes());
            b.extend(0u32.to_le_bytes());
            b.extend((off << 8 | 2 << 24).to_le_bytes());
            b.extend([0u8; 8]);
        }
        for f in [0.5f32, 0.25, 1.0, 9.0, 1.0, 2.0, 3.0, 4.0] {
            b.extend(f.to_le_bytes());
        }
        assert_eq!(b.len(), strings);
        b.extend(b"CBX\0\0fColor\0fUV\0");
        let pkg = ShaderPackage::parse(&b).unwrap();
        let cb = pkg.constant_buffer(object_id("CBX", 5)).unwrap();
        assert_eq!((cb.size, cb.params.len()), (8, 2));
        let p = cb.param("fColor").unwrap();
        assert_eq!((p.ty, p.columns, p.rows, p.array, p.offset, p.class), (ParamType::Float, 3, 1, 1, 0, 2));
        let data: Vec<u8> = cb.defaults.iter().flat_map(|f| f.to_le_bytes()).collect();
        assert_eq!(cb.read("fColor", &data).unwrap(), vec![0.5, 0.25, 1.0]);
        assert_eq!(cb.read("fUV", &data).unwrap(), vec![1.0, 2.0, 3.0, 4.0]);
        assert!(cb.read("fNone", &data).is_none());
    }

    /// Checks the layouts this crate's users depend on against the real
    /// package. Runs only when `DDDA_NATIVE_PC` points at a nativePC folder.
    #[test]
    fn real_package_material_layouts() {
        let Ok(dir) = std::env::var("DDDA_NATIVE_PC") else { return };
        let b = std::fs::read(std::path::Path::new(&dir).join("usershader/BbsShaderPackage.mfx")).unwrap();
        let pkg = ShaderPackage::parse(&b).unwrap();
        assert_eq!(pkg.constant_buffers.len(), 207);
        let cb = pkg.constant_buffer_named("CBMaterial").unwrap();
        let names: Vec<(&str, u16)> = cb.params.iter().map(|p| (p.name.as_str(), p.offset)).collect();
        assert_eq!(
            names,
            [
                ("fDiffuseColor", 0),
                ("fTransparency", 3),
                ("fReflectiveColor", 4),
                ("fTransparencyVolume", 7),
                ("fUVTransform", 8),
                ("fUVTransform2", 16),
                ("fUVTransform3", 24)
            ]
        );
        assert_eq!(cb.defaults[..8], [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 10.0]);
        let g = pkg.constant_buffer_named("$Globals").unwrap();
        assert_eq!(g.param("fAlbedoColor").map(|p| (p.offset, p.columns)), Some((0, 3)));
        for (name, enable) in [("BSSolid", false), ("BSBlendAlpha", true)] {
            let e = pkg.entries.values().find(|e| e.name == name).unwrap();
            assert_eq!(pkg.blend_states[&e.index].enable, enable);
        }
    }
}

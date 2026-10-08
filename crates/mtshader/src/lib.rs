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
}

fn u32_at(b: &[u8], o: usize) -> Result<u32, Error> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or(Error::Truncated(o))
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
}

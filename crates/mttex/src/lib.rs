//! Reader for MT Framework `rTexture` files (`.tex`, version 0x99) as used
//! by Dragon's Dogma: Dark Arisen on PC.
//!
//! Layout (checked against all 73,706 `rTexture` entries of the Steam
//! install with `ddtool tex-scan`; every offset and size accounted for):
//!
//! ```text
//! 0x00  "TEX\0"
//! 0x04  u32  version:12 (0x99) | zero:16 | shape:4   (2 flat, 6 cube, 3 volume)
//! 0x08  u32  mip_count:6 | width:13 | height:13
//! 0x0C  u32  faces:8 (6 for cube, else 1) | format:8 | depth:16 (1 unless volume)
//! 0x10  cube only: 108 bytes = 27 f32 (spherical-harmonics block, meaning not
//!       yet traced)
//! ....  u32 offsets[faces * mip_count], absolute; faces outermost, i.e. all
//!       mips of face 0, then face 1 (cube order verified by sizes)
//! ....  surface data, largest mip first, tightly packed
//! ```
//!
//! The format byte indexes a 48-entry table of D3D9 `D3DFORMAT` values in
//! DDDA.exe at `0x01440698`, read by `mov edx, [eax*4 + 0x01440698]` at
//! `0x011051a2` (nDraw::Texture code). [`FORMAT_TABLE`] is that table.

use std::fmt;

pub const MAGIC: [u8; 4] = *b"TEX\0";
pub const VERSION: u32 = 0x99;
pub const HEADER_SIZE: usize = 16;
pub const SH_SIZE: usize = 108;

const FOURCC_DXT1: u32 = u32::from_le_bytes(*b"DXT1");
const FOURCC_DXT3: u32 = u32::from_le_bytes(*b"DXT3");
const FOURCC_DXT5: u32 = u32::from_le_bytes(*b"DXT5");

/// TEX format byte -> D3D9 `D3DFORMAT`, copied from DDDA.exe `0x01440698`
/// (Steam build, see `docs/METHODOLOGY.md`). Index = format byte.
#[rustfmt::skip]
pub const FORMAT_TABLE: [u32; 48] = [
    0, 116, 113, 36, 110, 115, 31, 32, // 0x00..0x07
    63, 32, 26, 112, 34, 64, 114, 75, // 0x08..0x0f
    111, 81, 28, FOURCC_DXT1, FOURCC_DXT1, FOURCC_DXT3, FOURCC_DXT3, FOURCC_DXT5, // 0x10..0x17
    FOURCC_DXT5, FOURCC_DXT1, FOURCC_DXT1, FOURCC_DXT5, 23, 25, FOURCC_DXT1, FOURCC_DXT5, // 0x18..0x1f
    FOURCC_DXT5, FOURCC_DXT5, 22, FOURCC_DXT5, FOURCC_DXT5, FOURCC_DXT5, 113, 21, // 0x20..0x27
    21, FOURCC_DXT1, FOURCC_DXT5, FOURCC_DXT5, 50, 32, 31, FOURCC_DXT5, // 0x28..0x2f
];

/// The D3D9 surface formats the TEX format table can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum D3dFormat {
    Dxt1,
    Dxt3,
    Dxt5,
    A8R8G8B8,
    X8R8G8B8,
    R5G6B5,
    A1R5G5B5,
    A4R4G4B4,
    A8,
    L8,
    L16,
    A2B10G10R10,
    A8B8G8R8,
    G16R16,
    A16B16G16R16,
    Q8W8V8U8,
    V16U16,
    Q16W16V16U16,
    D24S8,
    R16F,
    G16R16F,
    A16B16G16R16F,
    R32F,
    G32R32F,
    A32B32G32R32F,
}

impl D3dFormat {
    /// Maps a `D3DFORMAT` value (d3d9types.h) to the enum.
    pub fn from_d3d(v: u32) -> Option<Self> {
        Some(match v {
            FOURCC_DXT1 => Self::Dxt1,
            FOURCC_DXT3 => Self::Dxt3,
            FOURCC_DXT5 => Self::Dxt5,
            21 => Self::A8R8G8B8,
            22 => Self::X8R8G8B8,
            23 => Self::R5G6B5,
            25 => Self::A1R5G5B5,
            26 => Self::A4R4G4B4,
            28 => Self::A8,
            31 => Self::A2B10G10R10,
            32 => Self::A8B8G8R8,
            34 => Self::G16R16,
            36 => Self::A16B16G16R16,
            50 => Self::L8,
            63 => Self::Q8W8V8U8,
            64 => Self::V16U16,
            75 => Self::D24S8,
            110 => Self::Q16W16V16U16,
            81 => Self::L16,
            111 => Self::R16F,
            112 => Self::G16R16F,
            113 => Self::A16B16G16R16F,
            114 => Self::R32F,
            115 => Self::G32R32F,
            116 => Self::A32B32G32R32F,
            _ => return None,
        })
    }

    /// `(block edge in pixels, bytes per block)`.
    pub fn block(self) -> (u32, u32) {
        match self {
            Self::Dxt1 => (4, 8),
            Self::Dxt3 | Self::Dxt5 => (4, 16),
            Self::A8 | Self::L8 => (1, 1),
            Self::R5G6B5 | Self::A1R5G5B5 | Self::A4R4G4B4 | Self::L16 | Self::R16F => (1, 2),
            Self::A8R8G8B8
            | Self::X8R8G8B8
            | Self::A2B10G10R10
            | Self::A8B8G8R8
            | Self::G16R16
            | Self::Q8W8V8U8
            | Self::V16U16
            | Self::D24S8
            | Self::G16R16F
            | Self::R32F => (1, 4),
            Self::A16B16G16R16 | Self::Q16W16V16U16 | Self::A16B16G16R16F | Self::G32R32F => (1, 8),
            Self::A32B32G32R32F => (1, 16),
        }
    }

    pub fn is_block_compressed(self) -> bool {
        self.block().0 == 4
    }

    /// Bytes of one 2D surface of the given size.
    pub fn surface_size(self, width: u32, height: u32) -> usize {
        let (edge, bytes) = self.block();
        let bw = width.div_ceil(edge).max(1) as usize;
        let bh = height.div_ceil(edge).max(1) as usize;
        bw * bh * bytes as usize
    }
}

/// D3D9 format for a TEX format byte, from [`FORMAT_TABLE`].
pub fn d3d_format(code: u8) -> Option<D3dFormat> {
    FORMAT_TABLE
        .get(code as usize)
        .and_then(|&v| D3dFormat::from_d3d(v))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Flat,
    Cube,
    Volume,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub version: u32,
    pub shape: Shape,
    pub mip_count: u32,
    pub width: u32,
    pub height: u32,
    pub faces: u32,
    /// Raw TEX format byte (index into [`FORMAT_TABLE`]).
    pub format_code: u8,
    pub format: D3dFormat,
    pub depth: u32,
}

/// One stored surface: a mip level of one cube face (or of the flat or
/// volume texture). Volume surfaces hold `depth` slices back to back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface<'a> {
    pub face: u32,
    pub mip: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub data: &'a [u8],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Texture<'a> {
    pub header: Header,
    /// Cube maps only: the 27 floats after the header (spherical harmonics
    /// per Arrowgene/Riftstone naming; use not yet traced in the exe).
    pub sh: Option<[f32; 27]>,
    /// Surfaces in file order: face-major, then mip (largest first).
    pub surfaces: Vec<Surface<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    BadMagic([u8; 4]),
    BadVersion(u32),
    BadShape(u32),
    UnknownFormat(u8),
    Truncated {
        needed: usize,
        have: usize,
    },
    BadLayout {
        surface: usize,
        expected_offset: usize,
        offset: usize,
    },
    TrailingBytes {
        expected_len: usize,
        len: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadMagic(m) => write!(f, "not a TEX file (magic {m:02x?})"),
            Error::BadVersion(v) => write!(f, "unsupported TEX version {v:#x} (expected {VERSION:#x})"),
            Error::BadShape(s) => write!(f, "unknown TEX shape {s}"),
            Error::UnknownFormat(c) => write!(f, "TEX format byte {c:#04x} not in the exe's table"),
            Error::Truncated { needed, have } => write!(f, "truncated: need {needed} bytes, have {have}"),
            Error::BadLayout {
                surface,
                expected_offset,
                offset,
            } => write!(
                f,
                "surface {surface} at offset {offset:#x}, expected {expected_offset:#x} (tight packing)"
            ),
            Error::TrailingBytes { expected_len, len } => {
                write!(f, "file is {len} bytes, surfaces end at {expected_len}")
            }
        }
    }
}

impl std::error::Error for Error {}

fn u32_at(b: &[u8], o: usize) -> Result<u32, Error> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or(Error::Truncated {
            needed: o + 4,
            have: b.len(),
        })
}

/// Parses the 16-byte header only.
pub fn parse_header(b: &[u8]) -> Result<Header, Error> {
    if b.len() < HEADER_SIZE {
        return Err(Error::Truncated {
            needed: HEADER_SIZE,
            have: b.len(),
        });
    }
    let magic: [u8; 4] = b[0..4].try_into().unwrap();
    if magic != MAGIC {
        return Err(Error::BadMagic(magic));
    }
    let w1 = u32_at(b, 4)?;
    let w2 = u32_at(b, 8)?;
    let w3 = u32_at(b, 12)?;
    let version = w1 & 0xFFF;
    if version != VERSION {
        return Err(Error::BadVersion(version));
    }
    let shape = match w1 >> 28 {
        2 => Shape::Flat,
        6 => Shape::Cube,
        3 => Shape::Volume,
        s => return Err(Error::BadShape(s)),
    };
    let format_code = ((w3 >> 8) & 0xFF) as u8;
    Ok(Header {
        version,
        shape,
        mip_count: w2 & 0x3F,
        width: (w2 >> 6) & 0x1FFF,
        height: w2 >> 19,
        faces: w3 & 0xFF,
        format_code,
        format: d3d_format(format_code).ok_or(Error::UnknownFormat(format_code))?,
        depth: w3 >> 16,
    })
}

/// Parses a whole TEX file and checks that the surfaces tile the file
/// exactly (no gaps, no overlap, no trailing bytes).
pub fn parse(b: &[u8]) -> Result<Texture<'_>, Error> {
    let header = parse_header(b)?;
    let mut pos = HEADER_SIZE;
    let sh = if header.shape == Shape::Cube {
        let raw = b.get(pos..pos + SH_SIZE).ok_or(Error::Truncated {
            needed: pos + SH_SIZE,
            have: b.len(),
        })?;
        pos += SH_SIZE;
        let mut v = [0f32; 27];
        for (i, x) in v.iter_mut().enumerate() {
            *x = f32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap());
        }
        Some(v)
    } else {
        None
    };
    let faces = header.faces.max(1);
    let count = (faces * header.mip_count) as usize;
    let table = pos;
    pos += count * 4;
    if pos > b.len() {
        return Err(Error::Truncated {
            needed: pos,
            have: b.len(),
        });
    }
    let mut surfaces = Vec::with_capacity(count);
    for face in 0..faces {
        for mip in 0..header.mip_count {
            let idx = surfaces.len();
            let offset = u32_at(b, table + idx * 4)? as usize;
            if offset != pos {
                return Err(Error::BadLayout {
                    surface: idx,
                    expected_offset: pos,
                    offset,
                });
            }
            let w = (header.width >> mip).max(1);
            let h = (header.height >> mip).max(1);
            let d = (header.depth >> mip).max(1);
            let len = header.format.surface_size(w, h) * d as usize;
            let data = b.get(offset..offset + len).ok_or(Error::Truncated {
                needed: offset + len,
                have: b.len(),
            })?;
            surfaces.push(Surface {
                face,
                mip,
                width: w,
                height: h,
                depth: d,
                data,
            });
            pos += len;
        }
    }
    if pos != b.len() {
        return Err(Error::TrailingBytes {
            expected_len: pos,
            len: b.len(),
        });
    }
    Ok(Texture { header, sh, surfaces })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(shape: u32, mips: u32, w: u32, h: u32, faces: u32, code: u8, depth: u32) -> Vec<u8> {
        let mut b = MAGIC.to_vec();
        b.extend((VERSION | shape << 28).to_le_bytes());
        b.extend((mips | w << 6 | h << 19).to_le_bytes());
        b.extend((faces | u32::from(code) << 8 | depth << 16).to_le_bytes());
        b
    }

    fn with_surfaces(mut b: Vec<u8>, sizes: &[usize]) -> Vec<u8> {
        let mut off = b.len() + sizes.len() * 4;
        for s in sizes {
            b.extend((off as u32).to_le_bytes());
            off += s;
        }
        for (i, s) in sizes.iter().enumerate() {
            b.extend(std::iter::repeat_n(i as u8, *s));
        }
        b
    }

    #[test]
    fn exe_table_matches_measured_block_sizes() {
        // Bytes per pixel of mip 0 measured on the Steam data for each
        // format byte that occurs: 0x13/0x14/0x19 = 0.5, 0x18/0x1F/0x25/0x2F
        // = 1, 0x28 = 4.
        for (code, bpp_x2) in [
            (0x13, 1),
            (0x14, 1),
            (0x19, 1),
            (0x18, 2),
            (0x1F, 2),
            (0x25, 2),
            (0x2F, 2),
            (0x28, 8),
        ] {
            let f = d3d_format(code).unwrap();
            assert_eq!(f.surface_size(64, 64) * 2, 64 * 64 * bpp_x2, "code {code:#x}");
        }
        assert_eq!(d3d_format(0x19), Some(D3dFormat::Dxt1));
        assert_eq!(d3d_format(0x1F), Some(D3dFormat::Dxt5));
        assert_eq!(d3d_format(0x27), Some(D3dFormat::A8R8G8B8));
        assert_eq!(d3d_format(0x00), None);
        assert_eq!(d3d_format(0x30), None);
    }

    #[test]
    fn flat_dxt1_with_mips() {
        let b = with_surfaces(header(2, 3, 16, 8, 1, 0x13, 1), &[64, 16, 8]);
        let t = parse(&b).unwrap();
        assert_eq!(t.header.shape, Shape::Flat);
        assert_eq!((t.header.width, t.header.height, t.header.mip_count), (16, 8, 3));
        let dims: Vec<_> = t
            .surfaces
            .iter()
            .map(|s| (s.width, s.height, s.data.len()))
            .collect();
        assert_eq!(dims, [(16, 8, 64), (8, 4, 16), (4, 2, 8)]);
        assert!(t.surfaces[2].data.iter().all(|&x| x == 2));
    }

    #[test]
    fn cube_is_face_major_after_sh_block() {
        let mut b = header(6, 2, 4, 4, 6, 0x28, 1);
        b.extend([0u8; SH_SIZE]);
        let sizes: Vec<usize> = (0..6).flat_map(|_| [64, 16]).collect();
        let b = with_surfaces(b, &sizes);
        let t = parse(&b).unwrap();
        assert!(t.sh.is_some());
        assert_eq!(t.surfaces.len(), 12);
        assert_eq!((t.surfaces[1].face, t.surfaces[1].mip), (0, 1));
        assert_eq!((t.surfaces[2].face, t.surfaces[2].mip), (1, 0));
    }

    #[test]
    fn volume_mips_halve_depth() {
        let b = with_surfaces(header(3, 2, 4, 4, 1, 0x27, 4), &[4 * 4 * 4 * 4, 2 * 2 * 4 * 2]);
        let t = parse(&b).unwrap();
        assert_eq!(t.header.shape, Shape::Volume);
        assert_eq!(t.surfaces[1].depth, 2);
    }

    #[test]
    fn rejects_gaps_and_trailing_bytes() {
        let mut b = with_surfaces(header(2, 1, 4, 4, 1, 0x13, 1), &[8]);
        b.push(0);
        assert!(matches!(parse(&b), Err(Error::TrailingBytes { .. })));
        let mut b = header(2, 1, 4, 4, 1, 0x13, 1);
        b.extend(100u32.to_le_bytes());
        b.extend([0u8; 100]);
        assert!(matches!(parse(&b), Err(Error::BadLayout { .. })));
    }
}

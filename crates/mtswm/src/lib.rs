//! Reader for `rSwingModel` (`SWM`), the per-vertex swing data of foliage
//! models in Dragon's Dogma: Dark Arisen (PC).
//!
//! A scenery model `...\model\omXXXX` has its swing data at
//! `...\etc\omXXXX_s00`; the distant cell models (`fmfore_*`) keep it at
//! the model's own path. One node per mesh of the model, in mesh order.
//!
//! Layout (loader `rSwingModel::vf10`, 0x0113AB00, checks the magic and
//! the version; the rest is checked on the data):
//!
//! ```text
//! 0x00 "SWM\0", u16 version 0x10CA, u16 0x40 (node size)
//! 0x08 u32 node count N
//! 0x0C N nodes of 0x40 bytes:
//!      +0x00 f32[3] quantisation scale, u32
//!      +0x10 u32 byte offset of the node's vertex data
//!      +0x20 f32[3] quantisation offset, u32
//!      +0x30 u32 flags << 16 | vertex count
//!      (+0x14 and +0x34: 12 unused bytes each)
//! u32 data size, then the vertex data (a vertex buffer in the engine)
//! u32, f32, u8[4]  (stored at rSwingModel +0x6C, +0x70, +0x68..+0x6B; unresolved)
//! ```
//!
//! Each vertex record of a 4-byte node is `u8 x, y, z, w`: the swing
//! pivot `q / 255 * scale + offset` in model space and a wind weight
//! `w / 255`. The stage swing vertex shaders read exactly this
//! (`v3 * fSwingQuantScale + fSwingQuantOffset`, weight in `v3.w`).
//! Flags low byte is the record size (4 or 8). Flag bit 0x200 marks the
//! billboard nodes: in 133 of the 137 4-byte nodes with it (all 252 SWM
//! resources of the install), every vertex lies in its pivot's local XY
//! plane, i.e. the node is cards facing +Z. Nodes without it (0xC100)
//! have all-zero pivots. For the 59 scenery models whose meshes could be
//! paired, node count, vertex counts and data offsets match the model's
//! meshes exactly. *Unresolved:* the other flag bits (0x800, 0x100, ...)
//! and the 8-byte records (0xCA08, distant `fmfore` models).

use std::fmt;

pub const MAGIC: [u8; 4] = *b"SWM\0";
pub const VERSION: u16 = 0x10CA;
pub const NODE_SIZE: usize = 0x40;
/// Flag of billboard nodes (see the crate docs).
pub const FLAG_BILLBOARD: u16 = 0x200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    BadMagic,
    BadVersion(u16),
    Truncated { needed: usize, have: usize },
    BadNode(usize),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadMagic => write!(f, "not an SWM file"),
            Error::BadVersion(v) => write!(f, "unsupported SWM version {v:#x}"),
            Error::Truncated { needed, have } => write!(f, "needs {needed} bytes, file has {have}"),
            Error::BadNode(i) => write!(f, "node {i}: vertex data out of range"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub scale: [f32; 3],
    pub offset: [f32; 3],
    /// High 16 bits of the +0x30 word.
    pub flags: u16,
    pub vertex_count: u16,
    /// Byte offset of this node's records in [`SwingModel::data`].
    pub data_offset: u32,
}

impl Node {
    /// Bytes per vertex record (flags low byte: 4 or 8).
    pub fn record_size(&self) -> usize {
        usize::from(self.flags & 0xFF)
    }

    pub fn is_billboard(&self) -> bool {
        self.flags & FLAG_BILLBOARD != 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwingModel<'a> {
    pub nodes: Vec<Node>,
    pub data: &'a [u8],
    /// The 12 bytes after the data (unresolved).
    pub trailer: [u8; 12],
}

impl SwingModel<'_> {
    /// Pivot (model space) and wind weight of each vertex of a 4-byte node;
    /// `None` for other record sizes (8-byte records are not decoded).
    pub fn pivots(&self, node: usize) -> Option<Vec<([f32; 3], f32)>> {
        let n = self.nodes.get(node)?;
        if n.record_size() != 4 {
            return None;
        }
        let start = n.data_offset as usize;
        let bytes = self.data.get(start..start + 4 * usize::from(n.vertex_count))?;
        Some(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|q| {
                    let p = std::array::from_fn(|c| f32::from(q[c]) / 255.0 * n.scale[c] + n.offset[c]);
                    (p, f32::from(q[3]) / 255.0)
                })
                .collect(),
        )
    }
}

pub fn parse(b: &[u8]) -> Result<SwingModel<'_>, Error> {
    let get = |o: usize, n: usize| {
        b.get(o..o + n).ok_or(Error::Truncated {
            needed: o + n,
            have: b.len(),
        })
    };
    let u32_at = |o: usize| get(o, 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
    let f32_at = |o: usize| u32_at(o).map(f32::from_bits);
    if get(0, 4)? != MAGIC {
        return Err(Error::BadMagic);
    }
    let version = u16::from_le_bytes(get(4, 2)?.try_into().unwrap());
    if version != VERSION {
        return Err(Error::BadVersion(version));
    }
    let count = u32_at(8)? as usize;
    let nodes_end = 0xC + count.checked_mul(NODE_SIZE).ok_or(Error::BadNode(0))?;
    get(0xC, nodes_end - 0xC)?;
    let mut nodes = Vec::with_capacity(count);
    for i in 0..count {
        let o = 0xC + i * NODE_SIZE;
        let w = u32_at(o + 0x30)?;
        nodes.push(Node {
            scale: [f32_at(o)?, f32_at(o + 4)?, f32_at(o + 8)?],
            offset: [f32_at(o + 0x20)?, f32_at(o + 0x24)?, f32_at(o + 0x28)?],
            flags: (w >> 16) as u16,
            vertex_count: (w & 0xFFFF) as u16,
            data_offset: u32_at(o + 0x10)?,
        });
    }
    let size = u32_at(nodes_end)? as usize;
    let data = get(nodes_end + 4, size)?;
    let trailer: [u8; 12] = get(nodes_end + 4 + size, 12)?.try_into().unwrap();
    for (i, n) in nodes.iter().enumerate() {
        let end = n.data_offset as usize + n.record_size() * usize::from(n.vertex_count);
        if end > data.len() {
            return Err(Error::BadNode(i));
        }
    }
    Ok(SwingModel { nodes, data, trailer })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(nodes: &[(u16, u16, u32)], data: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(MAGIC);
        b.extend(VERSION.to_le_bytes());
        b.extend(0x40u16.to_le_bytes());
        b.extend((nodes.len() as u32).to_le_bytes());
        for &(flags, count, off) in nodes {
            for f in [255.0f32, 510.0, 255.0] {
                b.extend(f.to_le_bytes());
            }
            b.extend(0u32.to_le_bytes());
            b.extend(off.to_le_bytes());
            b.extend([0xCD; 12]);
            for f in [-10.0f32, 0.0, 5.0] {
                b.extend(f.to_le_bytes());
            }
            b.extend(0u32.to_le_bytes());
            b.extend((u32::from(flags) << 16 | u32::from(count)).to_le_bytes());
            b.extend([0xCD; 12]);
        }
        b.extend((data.len() as u32).to_le_bytes());
        b.extend(data);
        b.extend(3u32.to_le_bytes());
        b.extend(1.0f32.to_le_bytes());
        b.extend([0, 1, 0, 0]);
        b
    }

    #[test]
    fn decodes_pivots() {
        let data = [0, 0, 0, 0, 10, 20, 30, 255, 1, 1, 1, 1];
        let b = file(&[(0xC100, 1, 0), (0xC204, 2, 4)], &data);
        let m = parse(&b).unwrap();
        assert_eq!(m.nodes.len(), 2);
        assert!(!m.nodes[0].is_billboard() && m.nodes[1].is_billboard());
        let p = m.pivots(1).unwrap();
        assert_eq!(p[0], ([0.0, 40.0, 35.0], 1.0));
        assert_eq!(p[1].0, [-9.0, 2.0, 6.0]);
        assert_eq!(m.trailer[8..], [0, 1, 0, 0]);
    }

    #[test]
    fn rejects_out_of_range_nodes() {
        let b = file(&[(0xC204, 4, 0)], &[0; 8]);
        assert_eq!(parse(&b), Err(Error::BadNode(0)));
        assert_eq!(parse(b"SWX\0"), Err(Error::BadMagic));
    }
}

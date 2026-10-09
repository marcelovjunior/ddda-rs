//! Reader for `rCollision` (`.sbc`): stage and object collision meshes.
//!
//! Layout from Riftstone (`sbc.py`, `docs/formats.md`; MIT, see
//! `third_party/riftstone-LICENSE.txt`), read from DDDA.exe's loaders. The
//! main loader at 0x010A30D0 is a real function in our Ghidra project and
//! compares the version word against 0x77DF2114 (at 0x010A3129).
//! `ddtool sbc-scan` checks the layout on every file in the install.
//!
//! ```text
//! 0x00  header 0x50: "SBC\xff", u32 version 0x77DF2114, u32 part division,
//!       u32 triangle division (0 = BVH), u16 parts, u16 materials, u32 leaves,
//!       u32 triangles, u32 vertices, u32 modify ids, MtAABB at 0x30
//! 0x50  u32 tree node memory
//! 0x54  parts[] 0x50: MtAABB, 3 runtime words, (start, count) of leaves,
//!       triangles and vertices at +0x2C, id at +0x44
//!       trees: one per part, then one over the parts ("BVHC", 0x77B17B24,
//!       kind 1 = 0x70-byte 4-wide nodes / 2 = 0x50-byte binary nodes, node
//!       count at +0x30; 0x40-byte header then the nodes)
//!       triangles[] 0x20: f32 normal[3], u16 vertex[3] (from the part's first
//!       vertex), u16 material, u32 attribute, u8 adjust[3], u8, u32
//!       vertices[] 0x10: f32 position[3], u32
//!       materials[] 0x20; leaves[] 10 bytes
//! ```

use std::fmt;

pub const MAGIC: [u8; 4] = *b"SBC\xff";
pub const VERSION: u32 = 0x77DF_2114;
const TREE_TAG: [u8; 4] = *b"BVHC";
const TREE_VERSION: u32 = 0x77B1_7B24;

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SBC at 0x{:x}: {}", self.offset, self.message)
    }
}

impl std::error::Error for Error {}

fn err<T>(offset: usize, message: impl Into<String>) -> Result<T, Error> {
    Err(Error {
        offset,
        message: message.into(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub bounds: Aabb,
    pub triangle_start: u32,
    pub triangle_count: u32,
    pub vertex_start: u32,
    pub vertex_count: u32,
    pub id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    pub normal: [f32; 3],
    /// Vertex numbers counted from the owning part's first vertex.
    pub vertices: [u16; 3],
    pub material: u16,
    pub attribute: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Collision {
    pub bounds: Aabb,
    pub parts: Vec<Part>,
    pub triangles: Vec<Triangle>,
    pub vertices: Vec<[f32; 3]>,
    /// Raw material records (8 words each); meanings not decoded yet.
    pub materials: Vec<[u32; 8]>,
}

fn u16_at(d: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([d[i], d[i + 1]])
}
fn u32_at(d: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(d[i..i + 4].try_into().unwrap())
}
fn f32_at(d: &[u8], i: usize) -> f32 {
    f32::from_le_bytes(d[i..i + 4].try_into().unwrap())
}
fn v3(d: &[u8], i: usize) -> [f32; 3] {
    [f32_at(d, i), f32_at(d, i + 4), f32_at(d, i + 8)]
}

impl Collision {
    /// Every triangle with its corner positions, in the file's frame.
    pub fn world_triangles(&self) -> impl Iterator<Item = ([[f32; 3]; 3], &Triangle)> + '_ {
        self.parts.iter().flat_map(move |p| {
            let tris =
                &self.triangles[p.triangle_start as usize..(p.triangle_start + p.triangle_count) as usize];
            tris.iter().map(move |t| {
                let c = t
                    .vertices
                    .map(|v| self.vertices[(p.vertex_start + u32::from(v)) as usize]);
                (c, t)
            })
        })
    }
}

/// Parses a complete collision file, checking that its sections add up to
/// the file's size and that every part and triangle stays in range.
pub fn parse(d: &[u8]) -> Result<Collision, Error> {
    if d.len() < 0x54 || d[0..4] != MAGIC {
        return err(0, "not a collision file");
    }
    let version = u32_at(d, 4);
    if version != VERSION {
        return err(4, format!("version 0x{version:08x}, expected 0x{VERSION:08x}"));
    }
    if u32_at(d, 8) > 1 || u32_at(d, 12) > 1 {
        return err(8, "grid-divided collision (none ships) is not supported");
    }
    let nparts = usize::from(u16_at(d, 16));
    let nmats = usize::from(u16_at(d, 18));
    let nleaves = u32_at(d, 20) as usize;
    let ntris = u32_at(d, 24) as usize;
    let nverts = u32_at(d, 28) as usize;
    let bounds = Aabb {
        min: v3(d, 0x30),
        max: v3(d, 0x40),
    };
    let mut o = 0x54 + nparts * 0x50;
    if o > d.len() {
        return err(0x54, "parts run past the end");
    }
    for _ in 0..=nparts {
        if o + 0x40 > d.len() {
            return err(o, "a tree runs past the end");
        }
        let kind = u32_at(d, o + 8);
        if d[o..o + 4] != TREE_TAG || u32_at(d, o + 4) != TREE_VERSION {
            return err(o, "no BVH tree where one belongs");
        }
        let node = match kind {
            1 => 0x70,
            2 => 0x50,
            _ => return err(o + 8, format!("tree kind {kind}")),
        };
        o += 0x40 + u32_at(d, o + 0x30) as usize * node;
    }
    let tri_at = o;
    let vert_at = tri_at + ntris * 0x20;
    let mat_at = vert_at + nverts * 0x10;
    let leaf_at = mat_at + nmats * 0x20;
    let end = leaf_at + nleaves * 10;
    if end != d.len() {
        return err(
            o,
            format!("sections end at 0x{end:x}, file is 0x{:x} bytes", d.len()),
        );
    }
    let mut parts = Vec::with_capacity(nparts);
    for i in 0..nparts {
        let p = 0x54 + i * 0x50;
        let part = Part {
            bounds: Aabb {
                min: v3(d, p),
                max: v3(d, p + 0x10),
            },
            triangle_start: u32_at(d, p + 0x34),
            triangle_count: u32_at(d, p + 0x38),
            vertex_start: u32_at(d, p + 0x3C),
            vertex_count: u32_at(d, p + 0x40),
            id: u32_at(d, p + 0x44),
        };
        if (part.triangle_start + part.triangle_count) as usize > ntris
            || (part.vertex_start + part.vertex_count) as usize > nverts
        {
            return err(p, format!("part {i} names triangles or vertices past the file's"));
        }
        parts.push(part);
    }
    let triangles: Vec<Triangle> = (0..ntris)
        .map(|k| {
            let t = tri_at + k * 0x20;
            Triangle {
                normal: v3(d, t),
                vertices: [u16_at(d, t + 12), u16_at(d, t + 14), u16_at(d, t + 16)],
                material: u16_at(d, t + 18),
                attribute: u32_at(d, t + 20),
            }
        })
        .collect();
    for (i, p) in parts.iter().enumerate() {
        for t in &triangles[p.triangle_start as usize..(p.triangle_start + p.triangle_count) as usize] {
            if t.vertices.iter().any(|&v| u32::from(v) >= p.vertex_count) {
                return err(
                    tri_at,
                    format!("a triangle of part {i} names a vertex past the part's"),
                );
            }
        }
    }
    let vertices = (0..nverts).map(|k| v3(d, vert_at + k * 0x10)).collect();
    let materials = (0..nmats)
        .map(|k| std::array::from_fn(|w| u32_at(d, mat_at + k * 0x20 + 4 * w)))
        .collect();
    Ok(Collision {
        bounds,
        parts,
        triangles,
        vertices,
        materials,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One part, one triangle, three vertices, empty trees.
    fn sample() -> Vec<u8> {
        let mut d = vec![0u8; 0x54];
        d[0..4].copy_from_slice(&MAGIC);
        d[4..8].copy_from_slice(&VERSION.to_le_bytes());
        d[16..18].copy_from_slice(&1u16.to_le_bytes()); // parts
        d[24..28].copy_from_slice(&1u32.to_le_bytes()); // triangles
        d[28..32].copy_from_slice(&3u32.to_le_bytes()); // vertices
        let mut part = vec![0u8; 0x50];
        part[0x38..0x3C].copy_from_slice(&1u32.to_le_bytes()); // triangle count
        part[0x40..0x44].copy_from_slice(&3u32.to_le_bytes()); // vertex count
        d.extend_from_slice(&part);
        for _ in 0..2 {
            let mut tree = vec![0u8; 0x40];
            tree[0..4].copy_from_slice(&TREE_TAG);
            tree[4..8].copy_from_slice(&TREE_VERSION.to_le_bytes());
            tree[8..12].copy_from_slice(&1u32.to_le_bytes());
            d.extend_from_slice(&tree);
        }
        let mut tri = vec![0u8; 0x20];
        tri[4..8].copy_from_slice(&1.0f32.to_le_bytes()); // normal +Y
        tri[12..14].copy_from_slice(&0u16.to_le_bytes());
        tri[14..16].copy_from_slice(&1u16.to_le_bytes());
        tri[16..18].copy_from_slice(&2u16.to_le_bytes());
        d.extend_from_slice(&tri);
        for p in [[0.0f32, 5.0, 0.0], [0.0, 5.0, 10.0], [10.0, 5.0, 0.0]] {
            for c in p {
                d.extend_from_slice(&c.to_le_bytes());
            }
            d.extend_from_slice(&0u32.to_le_bytes());
        }
        d
    }

    #[test]
    fn parses_and_resolves_triangles() {
        let c = parse(&sample()).unwrap();
        let tris: Vec<_> = c.world_triangles().collect();
        assert_eq!(tris.len(), 1);
        assert_eq!(tris[0].0[1], [0.0, 5.0, 10.0]);
    }

    #[test]
    fn rejects_size_mismatch() {
        let mut d = sample();
        d.push(0);
        assert!(parse(&d).is_err());
    }
}

//! Reader for MT Framework `rModel` files (`.mod`, version 0xD4 = 212) as
//! used by Dragon's Dogma: Dark Arisen on PC.
//!
//! Field names follow Albam's `mod-21.ksy` (MIT). Every structural claim
//! below was checked on all 3,940 distinct models of the Steam install
//! (`ddtool mod-scan`):
//!
//! ```text
//! 0x00  "MOD\0", u8 version 0xD4, u8 revision 0
//!       u16 bones, u16 meshes, u16 materials
//!       u32 vertices, u32 index count ("faces"), u32 edges, u32 vertex-buffer bytes
//!       u32 reserved, u32 groups
//!       u32 offsets: bones, groups, material names, meshes, vertex buffer,
//!       index buffer; u32 "file size" (= real size - 4)
//! 0x40  vec4 bounding sphere, vec4 bbox min, vec4 bbox max
//! 0x70  model info: i32 middist, i32 lowdist, u32 light group, u8 strip type,
//!       u8 memory, u16
//! 0x80  u32 weight-bound count                      (header ends at 0x84)
//! bones:   24 bytes each, then local and inverse-bind 4x4 matrices, then a
//!          256-byte joint remap table
//! groups:  32 bytes each
//! names:   char[128] per material (matched to MRL entries by JAMCRC)
//! meshes:  48 bytes each, then weight bounds of 144 bytes each, ending
//!          exactly at the vertex buffer
//! indices: u16 triangle strips, 4 zero bytes after the last one
//! ```
//!
//! Vertex layouts are not hard-coded: a mesh's vertex-format id names an
//! input layout in the shader package (see `mtshader`).

use std::fmt;

use mtshader::{ElementType, InputLayout};

pub const MAGIC: [u8; 4] = *b"MOD\0";
pub const VERSION: u8 = 0xD4;
pub const HEADER_SIZE: usize = 0x84;
pub const BONE_SIZE: usize = 24;
pub const GROUP_SIZE: usize = 32;
pub const MATERIAL_NAME_SIZE: usize = 128;
pub const MESH_SIZE: usize = 48;
pub const WEIGHT_BOUND_SIZE: usize = 144;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    BadMagic,
    BadVersion(u8),
    Truncated {
        what: &'static str,
        needed: usize,
        have: usize,
    },
    Layout(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadMagic => write!(f, "not a MOD file"),
            Error::BadVersion(v) => write!(f, "unsupported MOD version {v:#x}"),
            Error::Truncated { what, needed, have } => {
                write!(f, "{what}: needs {needed} bytes, file has {have}")
            }
            Error::Layout(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn get(&self, what: &'static str, o: usize, n: usize) -> Result<&'a [u8]> {
        self.0.get(o..o + n).ok_or(Error::Truncated {
            what,
            needed: o + n,
            have: self.0.len(),
        })
    }
    fn u8(&self, w: &'static str, o: usize) -> Result<u8> {
        Ok(self.get(w, o, 1)?[0])
    }
    fn u16(&self, w: &'static str, o: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.get(w, o, 2)?.try_into().unwrap()))
    }
    fn u32(&self, w: &'static str, o: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.get(w, o, 4)?.try_into().unwrap()))
    }
    fn f32(&self, w: &'static str, o: usize) -> Result<f32> {
        Ok(f32::from_le_bytes(self.get(w, o, 4)?.try_into().unwrap()))
    }
    fn vec<const N: usize>(&self, w: &'static str, o: usize) -> Result<[f32; N]> {
        let mut v = [0f32; N];
        for (i, x) in v.iter_mut().enumerate() {
            *x = self.f32(w, o + 4 * i)?;
        }
        Ok(v)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub anim_map_index: u8,
    pub parent: u8,
    pub mirror: u8,
    pub mapping: u8,
    pub length: f32,
    pub parent_distance: f32,
    pub location: [f32; 3],
    /// Row-major 4x4 as stored.
    pub local: [f32; 16],
    pub inverse_bind: [f32; 16],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub index: u32,
    pub sphere: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mesh {
    pub draw_mode: u16,
    pub vertex_count: u16,
    pub group: u16,
    pub material: u16,
    pub lod: u8,
    /// Bits 0-2: disp, shape, sort; bits 3-7: max weights per vertex.
    pub flags: u8,
    pub alpha_priority: u8,
    pub vertex_stride: u8,
    /// Low 6 bits: topology (always 4 = triangle strip in DDDA).
    pub topology: u8,
    pub vertex_start: u32,
    pub vertex_byte_offset: u32,
    pub vertex_format: u32,
    pub index_start: u32,
    pub index_count: u32,
    pub base_vertex: u32,
    pub bone_start: u8,
    pub weight_bound_count: u8,
    pub connect_id: u16,
    pub min_index: u16,
    pub max_index: u16,
}

impl Mesh {
    pub fn max_weights(&self) -> u8 {
        self.flags >> 3
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model<'a> {
    pub revision: u8,
    pub vertex_total: u32,
    pub index_total: u32,
    pub sphere: [f32; 4],
    pub bbox_min: [f32; 4],
    pub bbox_max: [f32; 4],
    pub mid_dist: i32,
    pub low_dist: i32,
    pub light_group: u32,
    pub bones: Vec<Bone>,
    /// 256-byte joint remap table (present when there are bones).
    pub bone_map: Option<&'a [u8]>,
    pub groups: Vec<Group>,
    pub material_names: Vec<String>,
    pub meshes: Vec<Mesh>,
    pub weight_bounds: &'a [u8],
    pub vertex_buffer: &'a [u8],
    pub index_buffer: &'a [u8],
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

pub fn parse(b: &[u8]) -> Result<Model<'_>> {
    let r = Reader(b);
    if r.get("magic", 0, 4)? != MAGIC {
        return Err(Error::BadMagic);
    }
    let version = r.u8("header", 4)?;
    if version != VERSION {
        return Err(Error::BadVersion(version));
    }
    let n_bones = r.u16("header", 6)? as usize;
    let n_meshes = r.u16("header", 8)? as usize;
    let n_materials = r.u16("header", 10)? as usize;
    let vertex_total = r.u32("header", 12)?;
    let index_total = r.u32("header", 16)?;
    let vb_size = r.u32("header", 24)? as usize;
    let n_groups = r.u32("header", 32)? as usize;
    let off = |i: usize| r.u32("header", 36 + 4 * i).map(|v| v as usize);
    let (o_bones, o_groups, o_names, o_meshes, o_vb, o_ib) =
        (off(0)?, off(1)?, off(2)?, off(3)?, off(4)?, off(5)?);
    let n_weight_bounds = r.u32("header", 0x80)? as usize;

    let mut bones = Vec::with_capacity(n_bones);
    let mut bone_map = None;
    if n_bones > 0 {
        let mats = o_bones + n_bones * BONE_SIZE;
        let inv = mats + n_bones * 64;
        for i in 0..n_bones {
            let o = o_bones + i * BONE_SIZE;
            bones.push(Bone {
                anim_map_index: r.u8("bones", o)?,
                parent: r.u8("bones", o + 1)?,
                mirror: r.u8("bones", o + 2)?,
                mapping: r.u8("bones", o + 3)?,
                length: r.f32("bones", o + 4)?,
                parent_distance: r.f32("bones", o + 8)?,
                location: r.vec("bones", o + 12)?,
                local: r.vec("bone matrices", mats + i * 64)?,
                inverse_bind: r.vec("bone matrices", inv + i * 64)?,
            });
        }
        bone_map = Some(r.get("bone map", inv + n_bones * 64, 256)?);
    }
    let mut groups = Vec::with_capacity(n_groups);
    for i in 0..n_groups {
        let o = o_groups + i * GROUP_SIZE;
        groups.push(Group {
            index: r.u32("groups", o)?,
            sphere: r.vec("groups", o + 16)?,
        });
    }
    let mut material_names = Vec::with_capacity(n_materials);
    for i in 0..n_materials {
        material_names.push(cstr(r.get(
            "material names",
            o_names + i * MATERIAL_NAME_SIZE,
            MATERIAL_NAME_SIZE,
        )?));
    }
    let mut meshes = Vec::with_capacity(n_meshes);
    for i in 0..n_meshes {
        let o = o_meshes + i * MESH_SIZE;
        let bits = r.u32("meshes", o + 4)?;
        meshes.push(Mesh {
            draw_mode: r.u16("meshes", o)?,
            vertex_count: r.u16("meshes", o + 2)?,
            group: (bits & 0xFFF) as u16,
            material: ((bits >> 12) & 0xFFF) as u16,
            lod: (bits >> 24) as u8,
            flags: r.u8("meshes", o + 8)?,
            alpha_priority: r.u8("meshes", o + 9)?,
            vertex_stride: r.u8("meshes", o + 10)?,
            topology: r.u8("meshes", o + 11)?,
            vertex_start: r.u32("meshes", o + 12)?,
            vertex_byte_offset: r.u32("meshes", o + 16)?,
            vertex_format: r.u32("meshes", o + 20)?,
            index_start: r.u32("meshes", o + 24)?,
            index_count: r.u32("meshes", o + 28)?,
            base_vertex: r.u32("meshes", o + 32)?,
            bone_start: r.u8("meshes", o + 36)?,
            weight_bound_count: r.u8("meshes", o + 37)?,
            connect_id: r.u16("meshes", o + 38)?,
            min_index: r.u16("meshes", o + 40)?,
            max_index: r.u16("meshes", o + 42)?,
        });
    }
    let wb_at = o_meshes + n_meshes * MESH_SIZE;
    let weight_bounds = r.get("weight bounds", wb_at, n_weight_bounds * WEIGHT_BOUND_SIZE)?;
    if n_meshes > 0 && wb_at + weight_bounds.len() != o_vb {
        return Err(Error::Layout(format!(
            "mesh table ends at {:#x}, vertex buffer starts at {o_vb:#x}",
            wb_at + weight_bounds.len()
        )));
    }
    let vertex_buffer = r.get("vertex buffer", o_vb, vb_size)?;
    let index_buffer = r.get("index buffer", o_ib, index_total as usize * 2)?;
    if o_ib + index_buffer.len() + 4 != b.len() {
        return Err(Error::Layout(format!(
            "index buffer ends at {:#x}, file is {:#x} (expected 4 trailing bytes)",
            o_ib + index_buffer.len(),
            b.len()
        )));
    }
    let model = Model {
        revision: r.u8("header", 5)?,
        vertex_total,
        index_total,
        sphere: r.vec("bounds", 0x40)?,
        bbox_min: r.vec("bounds", 0x50)?,
        bbox_max: r.vec("bounds", 0x60)?,
        mid_dist: r.u32("model info", 0x70)? as i32,
        low_dist: r.u32("model info", 0x74)? as i32,
        light_group: r.u32("model info", 0x78)?,
        bones,
        bone_map,
        groups,
        material_names,
        meshes,
        weight_bounds,
        vertex_buffer,
        index_buffer,
    };
    for (i, m) in model.meshes.iter().enumerate() {
        model
            .mesh_vertices(m)
            .map_err(|e| Error::Layout(format!("mesh {i}: {e}")))?;
        model
            .mesh_indices(m)
            .map_err(|e| Error::Layout(format!("mesh {i}: {e}")))?;
    }
    Ok(model)
}

/// Decoded vertex attributes of one mesh. Missing attributes are empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Vertices {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// xyz tangent, w = handedness as stored (biased byte).
    pub tangents: Vec<[f32; 4]>,
    /// TexCoord semantic 0..3 (the layout's `TexCoord` elements in order).
    pub uvs: [Vec<[f32; 2]>; 4],
    pub colors: Vec<[f32; 4]>,
    pub joints: Vec<[u16; 8]>,
    pub weights: Vec<[f32; 8]>,
}

impl<'a> Model<'a> {
    /// Raw bytes of a mesh's vertices.
    pub fn mesh_vertices(&self, m: &Mesh) -> std::result::Result<&'a [u8], String> {
        let stride = m.vertex_stride as usize;
        let start = m.vertex_byte_offset as usize + (m.base_vertex + m.vertex_start) as usize * stride;
        let len = m.vertex_count as usize * stride;
        self.vertex_buffer.get(start..start + len).ok_or_else(|| {
            format!(
                "vertices {start:#x}+{len:#x} past vertex buffer {:#x}",
                self.vertex_buffer.len()
            )
        })
    }

    /// Strip indices of a mesh, rebased so 0 is the mesh's first vertex.
    pub fn mesh_indices(&self, m: &Mesh) -> std::result::Result<Vec<u16>, String> {
        let s = m.index_start as usize * 2;
        let raw = self
            .index_buffer
            .get(s..s + m.index_count as usize * 2)
            .ok_or_else(|| "indices past index buffer".to_string())?;
        raw.as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                let i = u16::from_le_bytes([c[0], c[1]]);
                i.checked_sub(m.vertex_start as u16)
                    .filter(|&k| k < m.vertex_count)
                    .ok_or_else(|| format!("index {i} outside mesh vertices"))
            })
            .collect()
    }

    /// Triangle list from the mesh's strip. Degenerate triangles are dropped;
    /// odd triangles are flipped so all wind like the first. Winding checked
    /// on the data: face normal (b-a)x(c-a) agrees with the stored vertex
    /// normals for 98.4% of non-degenerate triangles.
    pub fn triangles(&self, m: &Mesh) -> std::result::Result<Vec<[u16; 3]>, String> {
        Ok(strip_to_list(&self.mesh_indices(m)?))
    }

    /// Decodes the mesh's vertices with its input layout.
    pub fn decode(&self, m: &Mesh, layout: &InputLayout) -> std::result::Result<Vertices, String> {
        let raw = self.mesh_vertices(m)?;
        let stride = m.vertex_stride as usize;
        let n = m.vertex_count as usize;
        let ext = [
            self.bbox_max[0] - self.bbox_min[0],
            self.bbox_max[1] - self.bbox_min[1],
            self.bbox_max[2] - self.bbox_min[2],
        ];
        // Quantised positions share one scale, the largest bbox extent
        // (checked: per-axis maxima equal extent/max_extent).
        let dim = ext[0].max(ext[1]).max(ext[2]);
        let mut v = Vertices::default();
        let read = |vi: usize, e: &mtshader::Element, comp: usize| -> std::result::Result<f32, String> {
            read_component(raw, vi * stride + e.offset as usize, e.ty, comp)
        };
        if let Some(e) = layout.element("Position", 0) {
            for i in 0..n {
                let p = [read(i, e, 0)?, read(i, e, 1)?, read(i, e, 2)?];
                v.positions.push(if e.ty == ElementType::S16N {
                    [
                        p[0] * dim + self.bbox_min[0],
                        p[1] * dim + self.bbox_min[1],
                        p[2] * dim + self.bbox_min[2],
                    ]
                } else {
                    p
                });
            }
        }
        if let Some(e) = layout.element("Normal", 0) {
            for i in 0..n {
                v.normals.push([read(i, e, 0)?, read(i, e, 1)?, read(i, e, 2)?]);
            }
        }
        if let Some(e) = layout.element("Tangent", 0) {
            for i in 0..n {
                v.tangents
                    .push([read(i, e, 0)?, read(i, e, 1)?, read(i, e, 2)?, read(i, e, 3)?]);
            }
        }
        let texcoords: Vec<_> = layout.elements.iter().filter(|e| e.name == "TexCoord").collect();
        for (slot, e) in texcoords.iter().take(4).enumerate() {
            for i in 0..n {
                v.uvs[slot].push([read(i, e, 0)?, read(i, e, 1)?]);
            }
        }
        if let Some(e) = layout.element("VertexColor", 0) {
            let alpha = layout.element("VertexAlpha", 0);
            for i in 0..n {
                let a = match alpha {
                    Some(a) => read(i, a, 0)?,
                    None => 1.0,
                };
                v.colors.push([read(i, e, 0)?, read(i, e, 1)?, read(i, e, 2)?, a]);
            }
        }
        if layout.element("Joint", 0).is_some() {
            for i in 0..n {
                let (j, w) = skin(raw, i * stride, layout)?;
                v.joints.push(j);
                v.weights.push(w);
            }
        }
        Ok(v)
    }
}

/// Reads component `comp` of an element as f32 (integers as their value).
pub fn read_component(
    raw: &[u8],
    at: usize,
    ty: ElementType,
    comp: usize,
) -> std::result::Result<f32, String> {
    let size = ty
        .size()
        .ok_or_else(|| format!("element type {ty:?} not supported"))?;
    let o = match ty {
        ElementType::U8x4Biased | ElementType::Color32 => at + comp,
        _ => at + comp * size,
    };
    let b = |n: usize| {
        raw.get(o..o + n)
            .ok_or_else(|| format!("vertex read past data at {o:#x}"))
    };
    Ok(match ty {
        ElementType::F32 => f32::from_le_bytes(b(4)?.try_into().unwrap()),
        ElementType::F16 => f16_to_f32(u16::from_le_bytes(b(2)?.try_into().unwrap())),
        ElementType::U16 | ElementType::Unk4 => f32::from(u16::from_le_bytes(b(2)?.try_into().unwrap())),
        ElementType::S16N => f32::from(i16::from_le_bytes(b(2)?.try_into().unwrap())) / 32767.0,
        ElementType::U8 | ElementType::Unk7 => f32::from(b(1)?[0]),
        ElementType::U8Biased | ElementType::U8x4Biased => f32::from(b(1)?[0]) / 255.0 * 2.0 - 1.0,
        ElementType::U8N | ElementType::U8Color | ElementType::Color32 => f32::from(b(1)?[0]) / 255.0,
        ElementType::Other(c) => return Err(format!("element type {c} not supported")),
    })
}

/// Joints and weights of one vertex. Weight sources by layout, from the MFX
/// element lists: `Weight` semantic 0 (one value, in the position's w),
/// semantic 1.. (more values), the remainder up to 1.0 goes to the last
/// joint. Layouts without weights (1 joint) get weight 1.
fn skin(raw: &[u8], at: usize, layout: &InputLayout) -> std::result::Result<([u16; 8], [f32; 8]), String> {
    let mut joints = [0u16; 8];
    let mut nj = 0;
    for e in layout.elements.iter().filter(|e| e.name == "Joint") {
        for c in 0..e.count as usize {
            if nj < 8 {
                let v = read_component(raw, at + e.offset as usize, e.ty, c)? as u16;
                // Element type 4 (only in IASkinBridge2wt, 0xDB7DA014) holds a
                // joint index in the low byte of each 16-bit component; the
                // high byte is not an index (0x80 in the data). Albam (MIT,
                // engines/mtfw/mesh.py) reads bytes 0 and 2 for this format.
                joints[nj] = if e.ty == ElementType::Unk4 { v & 0xFF } else { v };
                nj += 1;
            }
        }
    }
    let mut weights = [0f32; 8];
    let mut nw = 0;
    let mut ws: Vec<_> = layout.elements.iter().filter(|e| e.name == "Weight").collect();
    ws.sort_by_key(|e| e.semantic_index);
    for e in ws {
        for c in 0..e.count as usize {
            if nw < 8 {
                weights[nw] = read_component(raw, at + e.offset as usize, e.ty, c)?;
                nw += 1;
            }
        }
    }
    if nw < nj {
        let used: f32 = weights[..nw].iter().sum();
        weights[nw] = (1.0 - used).max(0.0);
    }
    Ok((joints, weights))
}

pub fn strip_to_list(strip: &[u16]) -> Vec<[u16; 3]> {
    let mut out = Vec::with_capacity(strip.len().saturating_sub(2));
    for (i, w) in strip.windows(3).enumerate() {
        let (a, b, c) = (w[0], w[1], w[2]);
        if a == b || b == c || a == c {
            continue;
        }
        out.push(if i % 2 == 0 { [a, b, c] } else { [b, a, c] });
    }
    out
}

pub fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = (h >> 10) & 0x1F;
    let frac = f32::from(h & 0x3FF);
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 => {
            if frac == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        e => (1.0 + frac / 1024.0) * 2f32.powi(i32::from(e) - 15),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_drop_degenerates_and_alternate() {
        assert_eq!(
            strip_to_list(&[0, 1, 2, 3, 3, 4, 4, 5, 6]),
            [[0, 1, 2], [2, 1, 3], [4, 5, 6]]
        );
    }

    #[test]
    fn half_floats() {
        assert_eq!(f16_to_f32(0x3C00), 1.0);
        assert_eq!(f16_to_f32(0xC000), -2.0);
        assert_eq!(f16_to_f32(0x3800), 0.5);
        assert_eq!(f16_to_f32(0x0001), 2f32.powi(-24));
    }

    #[test]
    fn biased_bytes() {
        assert_eq!(read_component(&[255], 0, ElementType::U8Biased, 0).unwrap(), 1.0);
        assert_eq!(read_component(&[0], 0, ElementType::U8Biased, 0).unwrap(), -1.0);
        assert_eq!(
            read_component(&[0, 0, 0xFF, 0x7F], 2, ElementType::S16N, 0).unwrap(),
            1.0
        );
    }
}

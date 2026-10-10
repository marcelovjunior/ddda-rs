use crate::{Bounds, Error, Reader, SpatialCells};

#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub enabled: bool,
    pub label: u16,
}
#[derive(Debug, Clone, PartialEq)]
pub struct MeshLink {
    pub target: u32,
    /// Native nodeData+0x44 entries retain these two edge references.
    pub edges: [u32; 2],
    pub values: [f32; 3],
}
#[derive(Debug, Clone, PartialEq)]
pub struct MeshNode {
    pub index: i32,
    pub attributes: Vec<u32>,
    pub flag: bool,
    /// Retained native fields; not assumed to be the geometric normal.
    pub vector: [f32; 3],
    pub scalar: f32,
    pub references: Vec<u32>,
    pub vertices: Vec<u32>,
    pub links: Vec<MeshLink>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum RegionShape {
    Box(Bounds),
    OrientedBox { center: [f32; 3], matrix: [f32; 16] },
    Sphere { radius: f32, center: [f32; 3] },
}
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub index: u16,
    pub name: String,
    pub value: u32,
    pub shapes: Vec<RegionShape>,
    pub range: [u16; 2],
    pub parent: u16,
    pub flags: Vec<u8>,
    pub more_flags: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct NodeInfo {
    pub node_offset: u32,
    pub bounds: Bounds,
    pub padding: [f32; 2],
}
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub kind: u32,
    pub name: String,
    pub vertices: Vec<Vertex>,
    pub nodes: Vec<MeshNode>,
    pub regions: Vec<Region>,
    pub region_footer: [u16; 2],
    pub totals: [u32; 3],
    pub node_info: Vec<NodeInfo>,
    pub spatial: SpatialCells,
}

impl Mesh {
    /// Translated from 0x01099E00 -> 0x01099100, 0x011FB840 and
    /// 0x0114C140 (DDDA.exe 2017-12-13 Steam). Checked lengths and finite
    /// values are reader validation, not a claim about native error handling.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        let kind = r.header(b"NAV\0", 0x21)?;
        let word_total = r.u32()?;
        let name = r.string()?;
        let vertex_count = r.u32()?;
        let node_count = r.u32()?;
        let info_count = r.u32()?;
        let vertex_flags = r.u8()?;
        let n = r.count(vertex_count, if vertex_flags == 1 { 15 } else { 12 })?;
        let mut vertices = Vec::with_capacity(n);
        for _ in 0..n {
            let position = r.floats()?;
            let (enabled, label) = if vertex_flags == 1 {
                (r.u8()? == 1, r.u16()?)
            } else {
                (false, 0)
            };
            vertices.push(Vertex {
                position,
                enabled,
                label,
            });
        }
        let n = r.count(node_count, 34)?;
        let mut nodes = Vec::with_capacity(n);
        let mut actual_words = 0usize;
        for _ in 0..n {
            let index = r.i32()?;
            let count = r.u32()?;
            let attributes = r.words(count)?;
            let flag = r.u8()? == 1;
            let vector = r.floats()?;
            let scalar = r.float()?;
            let count = u32::from(r.u8()?);
            let references = r.words(count)?;
            let count = r.u32()?;
            let node_vertices = r.words(count)?;
            let count = r.u32()?;
            let count = r.count(count, 24)?;
            let mut links = Vec::with_capacity(count);
            for _ in 0..count {
                links.push(MeshLink {
                    target: r.u32()?,
                    edges: [r.u32()?, r.u32()?],
                    values: r.floats()?,
                });
            }
            actual_words += attributes.len() + references.len() + node_vertices.len() + links.len() * 6;
            nodes.push(MeshNode {
                index,
                attributes,
                flag,
                vector,
                scalar,
                references,
                vertices: node_vertices,
                links,
            });
        }
        if actual_words != word_total as usize {
            return Err(r.error("mesh word total disagrees with records"));
        }
        let count = u32::from(r.u16()?);
        let count = r.count(count, 23)?;
        let mut regions = Vec::with_capacity(count);
        for _ in 0..count {
            let index = r.u16()?;
            let name = r.string()?;
            let value = r.u32()?;
            let count = r.u32()?;
            let count = r.count(count, 1)?;
            let mut shapes = Vec::with_capacity(count);
            for _ in 0..count {
                shapes.push(match r.u8()? {
                    0 => RegionShape::Box(Bounds {
                        min: r.floats()?,
                        max: r.floats()?,
                    }),
                    1 => RegionShape::OrientedBox {
                        center: r.floats()?,
                        matrix: r.floats()?,
                    },
                    2 => RegionShape::Sphere {
                        radius: r.float()?,
                        center: r.floats()?,
                    },
                    _ => return Err(r.error("unsupported region shape")),
                });
            }
            let range = [r.u16()?, r.u16()?];
            let parent = r.u16()?;
            let count = r.u8()? as usize;
            let flags = r.take(count)?.to_vec();
            let count = r.u8()? as usize;
            let more_flags = r.take(count)?.to_vec();
            regions.push(Region {
                index,
                name,
                value,
                shapes,
                range,
                parent,
                flags,
                more_flags,
            });
        }
        let region_footer = [r.u16()?, r.u16()?];
        let totals = [r.u32()?, r.u32()?, r.u32()?];
        let count = r.count(info_count, 36)?;
        let mut node_info = Vec::with_capacity(count);
        for _ in 0..count {
            let node_offset = r.u32()?;
            let low: [f32; 4] = r.floats()?;
            let high: [f32; 4] = r.floats()?;
            node_info.push(NodeInfo {
                node_offset,
                bounds: Bounds {
                    min: [low[0], low[1], low[2]],
                    max: [high[0], high[1], high[2]],
                },
                padding: [low[3], high[3]],
            });
        }
        let levels = r.u8()?;
        let count = r.u32()?;
        let low: [f32; 4] = r.floats()?;
        let high: [f32; 4] = r.floats()?;
        let bounds = Bounds {
            min: [low[0], low[1], low[2]],
            max: [high[0], high[1], high[2]],
        };
        let spatial = r.cells(levels, bounds, count, false)?;
        r.finish()?;
        for node in &nodes {
            if node.vertices.iter().any(|&v| v as usize >= vertices.len())
                || node.links.iter().any(|l| l.target as usize >= nodes.len())
            {
                return Err(r.error("mesh node references outside resource"));
            }
        }
        if spatial
            .cells
            .iter()
            .flatten()
            .any(|c| c.node as usize >= nodes.len())
        {
            return Err(r.error("mesh cell references outside resource"));
        }
        Ok(Self {
            kind,
            name,
            vertices,
            nodes,
            regions,
            region_footer,
            totals,
            node_info,
            spatial,
        })
    }
}

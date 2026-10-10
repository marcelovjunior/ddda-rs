use crate::{Bounds, Error, Reader, SpatialCells};

#[derive(Debug, Clone, PartialEq)]
pub struct WaypointEdge {
    pub target: u16,
    pub attributes: Vec<u32>,
    pub values: [f32; 3],
}
#[derive(Debug, Clone, PartialEq)]
pub enum WaypointShape {
    None,
    Sphere([f32; 4]),
    OrientedBox([f32; 20]),
}
#[derive(Debug, Clone, PartialEq)]
pub struct Waypoint {
    pub index: u16,
    pub position: [f32; 3],
    pub attributes: Vec<u32>,
    pub edges: Vec<WaypointEdge>,
    pub shape: WaypointShape,
    pub shape_attributes: Vec<u32>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct WaypointSet {
    pub kind: u32,
    pub totals: [u32; 4],
    pub nodes: Vec<Waypoint>,
    pub spatial: SpatialCells,
}
impl WaypointSet {
    /// Translated from 0x011FD5C0 and 0x0114D5D0 (DDDA.exe 2017-12-13 Steam).
    /// Field cells contain 16-bit references; the mesh's cells contain 32-bit references.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        let kind = r.header(b"WAY\0", 0x26)?;
        let word_total = r.u32()?;
        let count = r.u32()?;
        let totals = [r.u32()?, r.u32()?, r.u32()?, r.u32()?];
        let count = r.count(count, 20)?;
        let mut nodes = Vec::with_capacity(count);
        let mut actual_words = 0usize;
        let mut actual_edges = 0usize;
        for _ in 0..count {
            let index = r.u16()?;
            let position = r.floats()?;
            let count = u32::from(r.u16()?);
            let edge_count = u32::from(r.u16()?);
            let attributes = r.words(count)?;
            let count = r.count(edge_count, 15)?;
            let mut edges = Vec::with_capacity(count);
            for _ in 0..count {
                let target = r.u16()?;
                let count = u32::from(r.u8()?);
                let attributes = r.words(count)?;
                edges.push(WaypointEdge {
                    target,
                    attributes,
                    values: r.floats()?,
                });
            }
            let shape = match r.u8()? {
                0 => WaypointShape::None,
                1 => WaypointShape::Sphere(r.floats()?),
                2 => WaypointShape::OrientedBox(r.floats()?),
                _ => return Err(r.error("unsupported waypoint shape")),
            };
            let count = u32::from(r.u8()?);
            let shape_attributes = r.words(count)?;
            actual_words += attributes.len()
                + shape_attributes.len()
                + edges.iter().map(|e| e.attributes.len()).sum::<usize>();
            actual_edges += edges.len();
            nodes.push(Waypoint {
                index,
                position,
                attributes,
                edges,
                shape,
                shape_attributes,
            });
        }
        if actual_words != word_total as usize || actual_edges != totals[0] as usize {
            return Err(r.error("waypoint totals disagree with records"));
        }
        let levels = r.u8()?;
        let max = r.floats()?;
        let min = r.floats()?;
        // Native 0x0114D5D0 sums 4^level, with base double4 at 0x01443318.
        // Integer checked arithmetic avoids native allocation wrap on malformed files.
        let mut cells = 0u32;
        let mut width = 1u32;
        for _ in 0..levels {
            cells = cells
                .checked_add(width)
                .ok_or_else(|| r.error("waypoint cell count overflow"))?;
            width = width
                .checked_mul(4)
                .ok_or_else(|| r.error("waypoint cell count overflow"))?;
        }
        let spatial = r.cells(levels, Bounds { min, max }, cells, true)?;
        r.finish()?;
        if nodes
            .iter()
            .flat_map(|n| &n.edges)
            .any(|e| e.target as usize >= nodes.len())
            || spatial
                .cells
                .iter()
                .flatten()
                .any(|c| c.node as usize >= nodes.len())
        {
            return Err(r.error("waypoint references outside resource"));
        }
        Ok(Self {
            kind,
            totals,
            nodes,
            spatial,
        })
    }
}

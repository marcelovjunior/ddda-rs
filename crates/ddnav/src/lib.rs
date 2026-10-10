//! Installed stage navigation resources, independent of OS and presentation.
//! Mesh and field waypoints are distinct native services, never interchangeable.
mod mesh;
mod projection;
mod spatial;
mod waypoint;
pub use mesh::{Mesh, MeshLink, MeshNode, NodeInfo, Region, RegionShape, Vertex};
pub use projection::Projection;
pub use waypoint::{Waypoint, WaypointEdge, WaypointSet, WaypointShape};

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "navigation at 0x{:x}: {}", self.offset, self.message)
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// Native spatial cell entries: an authored node reference and a sorting word.
/// Interpretation of the sorting word belongs to the native query port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellEntry {
    pub node: u32,
    pub key: u32,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialCells {
    pub levels: u8,
    pub bounds: Bounds,
    pub cells: Vec<Vec<CellEntry>>,
}

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }
    fn error(&self, message: impl Into<String>) -> Error {
        Error {
            offset: self.offset,
            message: message.into(),
        }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let end = self
            .offset
            .checked_add(count)
            .filter(|&end| end <= self.data.len())
            .ok_or_else(|| self.error("truncated resource"))?;
        let bytes = &self.data[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }
    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, Error> {
        Ok(self.u32()? as i32)
    }
    fn float(&mut self) -> Result<f32, Error> {
        let f = f32::from_bits(self.u32()?);
        if !f.is_finite() {
            return Err(self.error("nonfinite scalar"));
        }
        Ok(f)
    }
    fn floats<const N: usize>(&mut self) -> Result<[f32; N], Error> {
        let mut out = [0.; N];
        for value in &mut out {
            *value = self.float()?;
        }
        Ok(out)
    }
    fn count(&self, count: u32, minimum: usize) -> Result<usize, Error> {
        let count = count as usize;
        if count > (self.data.len() - self.offset) / minimum.max(1) {
            return Err(self.error("record count exceeds remaining payload"));
        }
        Ok(count)
    }
    fn words(&mut self, count: u32) -> Result<Vec<u32>, Error> {
        let count = self.count(count, 4)?;
        (0..count).map(|_| self.u32()).collect()
    }
    fn string(&mut self) -> Result<String, Error> {
        let count = self.u32()? as usize;
        let bytes = self.take(
            count
                .checked_add(1)
                .ok_or_else(|| self.error("string length overflow"))?,
        )?;
        if bytes[count] != 0 || bytes[..count].contains(&0) {
            return Err(self.error("invalid counted string"));
        }
        // Resource names use bytes, as ARC paths do; do not assume UTF-8.
        Ok(bytes[..count].iter().map(|&c| char::from(c)).collect())
    }
    fn header(&mut self, magic: &[u8; 4], version: u32) -> Result<u32, Error> {
        if self.take(4)? != magic {
            return Err(self.error("wrong resource magic"));
        }
        if self.u32()? != version {
            return Err(self.error("unsupported resource version"));
        }
        self.u32()
    }
    fn finish(&self) -> Result<(), Error> {
        if self.offset != self.data.len() {
            return Err(self.error("trailing resource bytes"));
        }
        Ok(())
    }
    fn cells(
        &mut self,
        levels: u8,
        bounds: Bounds,
        count: u32,
        short_index: bool,
    ) -> Result<SpatialCells, Error> {
        let count = self.count(count, 4)?;
        let mut cells = Vec::with_capacity(count);
        for _ in 0..count {
            let n = self.u32()?;
            let n = self.count(n, if short_index { 6 } else { 8 })?;
            let mut cell = Vec::with_capacity(n);
            for _ in 0..n {
                let node = if short_index {
                    u32::from(self.u16()?)
                } else {
                    self.u32()?
                };
                cell.push(CellEntry {
                    node,
                    key: self.u32()?,
                });
            }
            cells.push(cell);
        }
        Ok(SpatialCells {
            levels,
            bounds,
            cells,
        })
    }
}

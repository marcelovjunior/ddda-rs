use crate::{Error, Mesh};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    pub node: usize,
    pub distance: f32,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    // UNRESOLVED: verify the native plane dot's addition order on slopes
    // before accepting this draft. Horizontal native probes cannot prove it.
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

// Translated from 0x00D665C0..0x00D66A64 (DDDA.exe 2017-12-13 Steam),
// specialized to the down ray (0,-1,0) used by 0x01093500. Preserve the
// normalized plane and edge tolerances. Plane dot operation order remains
// unresolved above; this query draft has not passed its Rust package gate.
fn down_ray(origin: [f32; 3], triangle: [[f32; 3]; 3]) -> Option<f32> {
    let [a, b, c] = triangle;
    let first = sub(b, a);
    let second = sub(c, a);
    let mut normal = cross(first, second);
    // Native length sums Y*Y + X*X + Z*Z; 0x01407124 is f32 epsilon.
    let length = ((normal[1] * normal[1] + normal[0] * normal[0]) + normal[2] * normal[2]).sqrt();
    if length >= f32::EPSILON {
        let reciprocal = 1.0 / length;
        normal = normal.map(|v| v * reciprocal);
    }
    let denominator = -normal[1];
    // Embedded 0x0161CF84 / 0x0161C3C0, used at 0x00D666D9 / D666F7.
    if denominator > -0.00001 && denominator < 0.00001 {
        return None;
    }
    let plane = -dot(normal, a);
    let side = dot(normal, origin) + plane;
    let distance = -(side / denominator);
    if distance < 0.0 || !distance.is_finite() {
        return None;
    }
    let point = [
        origin[0] + 0.0 * distance,
        origin[1] + -1.0 * distance,
        origin[2] + 0.0 * distance,
    ];
    // Native 0x0161CF7C, read at 0x00D66910/D669B8/D66A4E.
    for (start, end) in [(a, b), (b, c), (c, a)] {
        let edge_normal = cross(sub(start, point), sub(end, start));
        // The native inside tests sum Y first, then X, then Z.
        let inside = (edge_normal[1] * normal[1] + edge_normal[0] * normal[0]) + edge_normal[2] * normal[2];
        if inside < -0.01 {
            return None;
        }
    }
    Some(distance)
}

impl Mesh {
    /// Vertical selection portion of 0x010975E0 -> 0x01093500
    /// (DDDA.exe 2017-12-13 Steam). This deliberately does not implement
    /// the native nearest-node fallback at 0x01094FA0; None means this
    /// portion found no down-ray candidate, not that a complete query failed.
    /// Query positions must already be in the mesh/service frame. Transformed
    /// services need their native matrix before calling this helper.
    /// A nonzero exclusion mask requires the requested attribute word on
    /// every visited node; unsupported attribute layouts are errors.
    pub fn vertical_candidate(
        &self,
        position: [f32; 3],
        y_offset: f32,
        max_distance: f32,
        exclude_attribute: usize,
        exclude_mask: u32,
    ) -> Result<Option<Projection>, Error> {
        let error = |message: &str| Error {
            offset: 0,
            message: message.into(),
        };
        if !y_offset.is_finite() || !max_distance.is_finite() || max_distance < 0.0 {
            return Err(error("invalid vertical navigation query"));
        }
        let mut origin = position;
        origin[1] += y_offset;
        if !origin.iter().all(|v| v.is_finite()) {
            return Err(error("nonfinite navigation query origin"));
        }
        let Some(level) = self.spatial.levels.checked_sub(1) else {
            return Err(error("navigation grid has no levels"));
        };
        let Some(cell) = self.spatial.cell_index(origin, level)? else {
            return Ok(None);
        };
        let mut closest = max_distance;
        let mut selected = None;
        for entry in &self.spatial.cells[cell] {
            let node = self
                .nodes
                .get(entry.node as usize)
                .ok_or_else(|| error("navigation cell node out of range"))?;
            if exclude_mask != 0 {
                let attribute = node
                    .attributes
                    .get(exclude_attribute)
                    .ok_or_else(|| error("navigation exclusion attribute out of range"))?;
                if attribute & exclude_mask != 0 {
                    continue;
                }
            }
            if node.vertices.len() != 3 {
                return Err(error("vertical candidate requires native triangle node"));
            }
            let mut triangle = [[0.; 3]; 3];
            for (corner, &index) in triangle.iter_mut().zip(&node.vertices) {
                *corner = self
                    .vertices
                    .get(index as usize)
                    .ok_or_else(|| error("triangle vertex out of range"))?
                    .position;
            }
            if let Some(distance) = down_ray(origin, triangle) {
                // 0x01093635 rejects equal distances, preserving cell order ties.
                if distance < closest {
                    closest = distance;
                    selected = Some(Projection {
                        node: entry.node as usize,
                        distance,
                    });
                }
            }
        }
        Ok(selected)
    }
}

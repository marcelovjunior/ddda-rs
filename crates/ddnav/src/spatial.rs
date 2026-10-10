use crate::{Error, SpatialCells};

fn error(message: &str) -> Error {
    Error {
        offset: 0,
        message: message.into(),
    }
}

// Translated bit expansion from 0x0114D8A2..0x0114D90D
// (DDDA.exe 2017-12-13 Steam). X occupies even bits; Z occupies odd bits.
fn spread(mut coordinate: u32) -> u32 {
    coordinate = (coordinate | coordinate << 8) & 0x00ff_00ff;
    coordinate = (coordinate | coordinate << 4) & 0x0f0f_0f0f;
    coordinate = (coordinate | coordinate << 2) & 0x3333_3333;
    (coordinate | coordinate << 1) & 0x5555_5555
}

impl SpatialCells {
    /// Native cell selection, 0x0114D7B0 / 0x0114D990
    /// (DDDA.exe 2017-12-13 Steam). Position is in the resource/service
    /// frame; converting body/world-offset coordinates belongs to Trace.
    /// This selects candidates, and does not answer a path query.
    pub fn cell_index(&self, position: [f32; 3], level: u8) -> Result<Option<usize>, Error> {
        if !position.iter().all(|x| x.is_finite()) {
            return Err(error("nonfinite navigation query position"));
        }
        if level >= self.levels {
            return Ok(None);
        }
        // Native grid width is a byte. Reject wrapped-width resources rather
        // than guessing a new grid for them; installed beach/field use 7/4.
        if self.levels == 0 || self.levels > 8 {
            return Err(error("unsupported navigation grid width"));
        }
        let width = 1u32 << (self.levels - 1);
        let reciprocal_width = 1.0f32 / width as f32;
        let mut coordinates = [0u32; 2];
        for (output, axis) in coordinates.iter_mut().zip([0, 2]) {
            let extent = self.bounds.max[axis] - self.bounds.min[axis];
            let cell_width = extent * reciprocal_width;
            if !extent.is_finite() || extent <= 0.0 || !cell_width.is_finite() || cell_width <= 0.0 {
                return Err(error("invalid navigation grid extent"));
            }
            let relative = position[axis] - self.bounds.min[axis];
            let clamped = relative.min(extent).max(0.0);
            // Original FLD/FDIV keeps the two f32 operands in x87 precision
            // before integer truncation. Avoid a rounded f32 quotient here.
            let mut coordinate = (f64::from(clamped) / f64::from(cell_width)).trunc() as u32;
            if coordinate >= width {
                coordinate -= 1;
            }
            *output = coordinate;
        }
        let morton = spread(coordinates[0]) | spread(coordinates[1]) << 1;
        let shift = 2 * u32::from(self.levels - level - 1);
        let first = ((1u32 << (2 * u32::from(level))) - 1) / 3;
        let index = (first + (morton >> shift)) as usize;
        Ok((index < self.cells.len()).then_some(index))
    }
}

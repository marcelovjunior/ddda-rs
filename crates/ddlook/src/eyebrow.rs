//! Eyebrow material controls from the runtime edit record.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Eyebrow {
    /// The value assigned to fAlbedoColor, already multiplied by edit alpha.
    pub color: [f32; 3],
    pub texture: u32,
    pub uv_scale: [f32; 2],
    pub uv_offset: [f32; 2],
}

/// Translated from 0x00780fa0 (DDDA.exe 2017-12-13 Steam).
pub fn save_offset(preset: u32, x: u32, y: u32) -> [f32; 2] {
    const SLIDER: [f32; 5] = [-0.05, -0.025, 0.0, 0.025, 0.05];
    // 0x00781b13..0x00781b93 builds/selects these five stack pairs:
    // .02 at 0x0161c398, .07 at 0x01502774, .04 at 0x015176d8,
    // -.02 at 0x0161cf80 and -.03 at 0x0161d384; remaining lanes zero.
    const FIXED: [[f32; 2]; 5] = [[0.0, 0.02], [0.02, 0.07], [0.04, -0.02], [0.0, 0.0], [-0.03, 0.02]];
    if (200..205).contains(&preset) { FIXED[(preset - 200) as usize] }
    else if preset < 200 { [SLIDER.get(x as usize).copied().unwrap_or(0.0), SLIDER.get(y as usize).copied().unwrap_or(0.0)] }
    else { [0.0; 2] }
}

impl Eyebrow {
    // Translated from 0x00843940 (DDDA.exe 2017-12-13 Steam): actor
    // +0x1400 RGBA, +0x1428 offset, +0x1430 scale, +0x1438 texture.
    pub fn from_edit(color: [f32; 4], offset: [f32; 2], scale: [f32; 2], texture: u32) -> Self {
        let (uv_offset, uv_scale) = if texture == 4 {
            ([2.0; 2], [0.01; 2])
        } else {
            ([offset[0] - 0.5, offset[1] - 0.5], [scale[0] + 1.0, scale[1] + 1.0])
        };
        Self { color: [color[0] * color[3], color[1] * color[3], color[2] * color[3]], texture, uv_scale, uv_offset }
    }

    /// 0 keeps the authored texture; 1..3 replace it; 4 uses the UV-hide path.
    // Translated from 0x00843940 (DDDA.exe 2017-12-13 Steam).
    pub fn albedo_path(&self) -> Option<String> {
        (1..=3).contains(&self.texture).then(|| format!(r"model\pl\textures\pl_eyebrow{:02}_BM", self.texture))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn color_uv_and_hide_follow_the_runtime_material_setter() {
        assert_eq!(save_offset(0, 0, 4), [-0.05, 0.05]);
        assert_eq!(save_offset(1, 255, 2), [0.0, 0.0]);
        assert_eq!(save_offset(202, 0, 4), [0.04, -0.02]);
        assert_eq!(save_offset(205, 0, 4), [0.0; 2]);
        let b = Eyebrow::from_edit([0.2, 0.4, 0.8, 0.5], [0.05, -0.05], [0.25, 0.5], 2);
        assert_eq!(b.color, [0.1, 0.2, 0.4]);
        assert_eq!(b.uv_scale, [1.25, 1.5]);
        assert_eq!(b.uv_offset, [-0.45, -0.55]);
        assert_eq!(b.albedo_path().unwrap(), r"model\pl\textures\pl_eyebrow02_BM");
        let hidden = Eyebrow::from_edit([1.0; 4], [0.0; 2], [0.0; 2], 4);
        assert_eq!((hidden.uv_scale, hidden.uv_offset), ([0.01; 2], [2.0; 2]));
        assert!(hidden.albedo_path().is_none());
    }
}

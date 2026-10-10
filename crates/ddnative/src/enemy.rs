//! `uEnemy` members.

/// One blend source of `uEnemy::vf195` (`+0x5E84` array, 0x18 bytes each):
/// the pointed pair of 2D values and the blend factor at +0x14.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendSource {
    /// `[source+4]`: a0, a1 (from) and b0, b1 (to) at +0x0, +0x4, +0x8, +0xC.
    pub values: [f32; 4],
    pub t: f32,
}

/// One target of the blend (`+0x22B8` array, 0x230 bytes each): it gets
/// state 4 at +0x8 and the blended pair at +0x114 / +0x118.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendTarget {
    pub state_08: u32,
    pub value_114: f32,
    pub value_118: f32,
}

/// `uEnemy::vf195`: for each of `+0x22BC` targets, blends its source pair
/// as `(1 - t) * a + b * t` in single precision, in the original's order
/// (`b*t` first, then `(1-t)*a`, then the sum). `sources` is None when
/// `+0x5E84` is null; `targets` None when `+0x22B8` is null.
// Translated from 0x00AAEF50 (DDDA.exe 2017-12-13 Steam), uEnemy::vf195.
pub fn blend(sources: Option<&[BlendSource]>, targets: Option<&mut [BlendTarget]>) {
    let (Some(sources), Some(targets)) = (sources, targets) else { return };
    for (s, t) in sources.iter().zip(targets.iter_mut()) {
        t.state_08 = 4;
        let one_minus = 1.0f32 - s.t;
        t.value_114 = one_minus * s.values[0] + s.values[2] * s.t;
        t.value_118 = one_minus * s.values[1] + s.values[3] * s.t;
    }
}

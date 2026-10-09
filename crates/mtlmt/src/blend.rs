//! Interpolation between motions, as the original's `uModel` does it.
//!
//! When a model layer is given a new motion, the engine freezes the pose
//! the model shows at that moment (every joint's local rotation,
//! translation and scale) and fades that frozen pose out over the new
//! motion: each update it lowers a counter by the model's frame step and
//! sets the frozen pose's weight to an ease curve of the remaining
//! fraction. The new motion plays from its start frame from the first
//! update on; the old motion does not keep playing.
//!
//! Evidence (DDDA.exe 2017-12-13 Steam):
//! - `uModel::vf30` (0x0105B480), the layer's motion request
//!   `(layer, motion, interpolation frames, start frame, rate, flags)`:
//!   stores the interpolation frames twice (`uModel::Motion::mInterFrame`
//!   +0x40, the total, and `mInterCount` +0x44, the remaining) and the
//!   frozen pose's weight 1.0 (+0x60, not reflected). On a layer that had
//!   no motion yet, remaining and weight are 0 (no blend).
//! - `0x01061DD0`, run when a layer's motion changed: copies each joint's
//!   current local pose (joint +0x60 rotation, +0x80 translation, +0x70
//!   scale) into the per-joint interpolation buffer (uModel +0xDD0), and
//!   only when the total is above `f32::EPSILON`.
//! - `0x01060730`, the per-update layer step: `remaining -= step`
//!   (`cUnit::mDeltaTime` +0x14, *not* multiplied by the layer's
//!   `mSpeed`), then the weight is
//!   `ease(curve, remaining / total)`, or 0 and the blend ends once
//!   remaining reaches 0.
//! - `0x00D4EBF0`: the ease curve, a cubic Bezier from 0 to 1 with inner
//!   control values (p0, p1) (`ease`).
//! - `0x01058A70`: the per-joint mix (`mix_rotation`, `mix_vector`).
//!
//! Where the interpolation frames come from is in `docs/ANIMATION.md`
//! (`rAdjustParam` twin of each motion list).

/// Inner control values of the layer's ease curve (`mInterCurve`,
/// +0x6C). `uModel::Motion`'s
/// constructor (0x01056EC0) copies them from the global at 0x018D06A0,
/// which lies in zero-initialised data and has no writer in the exe (its
/// four references all read it), so both are 0.0 and the curve is t³.
/// *Static analysis only*; not checked against the running game.
pub const LAYER_CURVE: [f32; 2] = [0.0, 0.0];

/// Two named curves of the same form used elsewhere in the exe (0x0162ACC0
/// and 0x0162ACC8): smoothstep and linear. For the tests.
pub const SMOOTHSTEP_CURVE: [f32; 2] = [0.0, 1.0];
pub const LINEAR_CURVE: [f32; 2] = [1.0 / 3.0, 2.0 / 3.0];

/// Cubic Bezier ease from 0 (t = 0) to 1 (t = 1) with inner control
/// values `curve`:
/// `3·p0·t + (3·p1 − 6·p0)·t² + (3·p0 − 3·p1 + 1)·t³`.
// Translated from 0x00D4EBF0 (DDDA.exe 2017-12-13 Steam)
pub fn ease(curve: [f32; 2], t: f32) -> f32 {
    let [p0, p1] = curve;
    let t2 = t * t;
    (3.0 * p1 - 6.0 * p0) * t2 + t2 * t * ((3.0 * p0 - 3.0 * p1) + 1.0) + 3.0 * p0 * t
}

/// The fade of a frozen pose over a newly started motion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interpolation {
    /// Interpolation length in motion frames (`mInterFrame`).
    pub total: f32,
    /// Frames left (`mInterCount`).
    pub remaining: f32,
    /// Weight of the frozen pose (layer +0x60): 1 = only the frozen pose.
    pub weight: f32,
    pub curve: [f32; 2],
}

impl Interpolation {
    /// A blend of `frames` motion frames, or `None` when the request
    /// snaps (`frames` at or below `f32::EPSILON`: 0x01061DD0 freezes no
    /// pose then).
    // Translated from 0x0105B480 and 0x01061DD0 (DDDA.exe 2017-12-13 Steam)
    pub fn start(frames: f32) -> Option<Self> {
        (frames > f32::EPSILON).then_some(Self {
            total: frames,
            remaining: frames,
            weight: 1.0,
            curve: LAYER_CURVE,
        })
    }

    /// Advances by `step` motion frames (the model's frame step; the
    /// layer's playback rate does not apply). Returns `false` once the
    /// blend has ended (weight 0).
    // Translated from 0x01060730 (DDDA.exe 2017-12-13 Steam)
    pub fn advance(&mut self, step: f32) -> bool {
        self.remaining -= step;
        if self.remaining > 0.0 {
            self.weight = if self.total == 0.0 {
                0.0
            } else {
                ease(self.curve, self.remaining / self.total)
            };
            true
        } else {
            self.remaining = 0.0;
            self.weight = 0.0;
            false
        }
    }
}

/// A joint rotation (x, y, z, w) moved toward the frozen one by `weight`:
/// a normalised lerp on the shorter arc.
// Translated from 0x01058A70 (DDDA.exe 2017-12-13 Steam)
pub fn mix_rotation(current: [f32; 4], frozen: [f32; 4], weight: f32) -> [f32; 4] {
    let dot: f32 = (0..4).map(|i| current[i] * frozen[i]).sum();
    let target = if dot >= 0.0 { frozen } else { frozen.map(|x| -x) };
    let v: [f32; 4] = std::array::from_fn(|i| (target[i] - current[i]) * weight + current[i]);
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.map(|x| x / n)
}

/// A joint translation or scale moved toward the frozen one by `weight`.
// Translated from 0x01058A70 (DDDA.exe 2017-12-13 Steam)
pub fn mix_vector(current: [f32; 3], frozen: [f32; 3], weight: f32) -> [f32; 3] {
    std::array::from_fn(|i| (frozen[i] - current[i]) * weight + current[i])
}

/// Default interpolation frames of `motion` from its bank's `rAdjustParam`
/// table (256 floats, one per slot): `table[motion & 0xFF]`. Used when an
/// action requests a negative interpolation length, as the player actions
/// do (`uPlayerBase::vf36`, 0x00B800C0, through the table pointer at
/// uPlayer +0x3398 + 8 × bank).
// Translated from 0x00B800C0 (DDDA.exe 2017-12-13 Steam)
pub fn default_frames(table: &[f32], motion: u16) -> Option<f32> {
    table.get(usize::from(motion & 0xFF)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn ease_endpoints_and_named_curves() {
        for c in [LAYER_CURVE, SMOOTHSTEP_CURVE, LINEAR_CURVE] {
            assert!(close(ease(c, 0.0), 0.0));
            assert!(close(ease(c, 1.0), 1.0));
        }
        for t in [0.1f32, 0.25, 0.5, 0.9] {
            assert!(close(ease(LAYER_CURVE, t), t * t * t));
            assert!(close(ease(SMOOTHSTEP_CURVE, t), t * t * (3.0 - 2.0 * t)));
            assert!(close(ease(LINEAR_CURVE, t), t));
        }
    }

    #[test]
    fn weight_over_time() {
        // 5 frames (walk and run in m0000_co), stepped one frame at a time
        let mut b = Interpolation::start(5.0).unwrap();
        assert_eq!(b.weight, 1.0);
        let mut w = Vec::new();
        while b.advance(1.0) {
            w.push(b.weight);
        }
        let expect = [0.8f32, 0.6, 0.4, 0.2].map(|t| t * t * t);
        assert_eq!(w.len(), 4);
        for (a, e) in w.iter().zip(expect) {
            assert!(close(*a, e), "{a} vs {e}");
        }
        assert_eq!(b.weight, 0.0);
        assert_eq!(b.remaining, 0.0);
        // a quarter-frame step (the simulation's tick) takes 4x the steps
        let mut b = Interpolation::start(2.0).unwrap();
        let mut n = 0;
        while b.advance(0.25) {
            n += 1;
        }
        assert_eq!(n, 7);
    }

    #[test]
    fn zero_frames_snaps() {
        assert!(Interpolation::start(0.0).is_none());
        assert!(Interpolation::start(f32::EPSILON).is_none());
        assert!(Interpolation::start(1.0).is_some());
    }

    #[test]
    fn rotation_mix() {
        let id = [0.0, 0.0, 0.0, 1.0];
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let y90 = [0.0, h, 0.0, h];
        // weight 0: the motion's rotation; weight 1: the frozen one
        let a = mix_rotation(id, y90, 0.0);
        assert!((0..4).all(|i| close(a[i], id[i])));
        let b = mix_rotation(id, y90, 1.0);
        assert!((0..4).all(|i| close(b[i], y90[i])));
        // halfway: 45 degrees about y, unit length
        let m = mix_rotation(id, y90, 0.5);
        let half = (std::f32::consts::PI / 8.0).sin();
        assert!(close(m[1], half) && close(m[3], (std::f32::consts::PI / 8.0).cos()));
        // the frozen quaternion's sign does not matter (shorter arc)
        let n = mix_rotation(id, y90.map(|x| -x), 0.5);
        assert!((0..4).all(|i| close(n[i], m[i])));
    }

    #[test]
    fn vector_mix_and_default_frames() {
        assert_eq!(mix_vector([0.0, 2.0, 4.0], [10.0, 2.0, 0.0], 0.25), [2.5, 2.0, 3.0]);
        let table: Vec<f32> = (0..256).map(|i| i as f32).collect();
        assert_eq!(default_frames(&table, 0x0003), Some(3.0));
        assert_eq!(default_frames(&table, 0x0611), Some(17.0));
        assert_eq!(default_frames(&table[..4], 0x0010), None);
    }
}

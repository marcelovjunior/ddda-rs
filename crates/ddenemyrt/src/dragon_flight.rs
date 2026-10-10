//! TargetFly's quaternion steering. All angles are radians.
//! Translated from 0x00a60b70, 0x00a61590, 0x007af5b0 and
//! 0x007ae860 (DDDA.exe 2017-12-13 Steam).
#![allow(clippy::approx_constant)]

use super::{yaw_difference, TargetFlyTravel};

fn length(v: [f32; 3]) -> f32 {
    (v[1] * v[1] + v[0] * v[0] + v[2] * v[2]).sqrt()
}
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let n = length(v);
    if n >= f32::EPSILON {
        v.map(|x| x / n)
    } else {
        v
    }
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Native look-at basis: +Z points at the target, +Y is the supplied up.
/// The native degenerate fallback uses initialized globals; this subset
/// preserves orientation instead of guessing them for a vertical/zero aim.
fn target_rotation(delta: [f32; 3]) -> Option<[f32; 4]> {
    let right = cross([0.0, 1.0, 0.0], delta);
    if right.iter().map(|x| x * x).sum::<f32>() < f32::EPSILON {
        return None;
    }
    let right = normalize(right);
    let forward = normalize(delta);
    let up = normalize(cross(forward, right));
    let row = [right, up, forward];
    let trace = row[0][0] + row[1][1] + row[2][2];
    let mut q = [0.0; 4];
    if trace > 0.0 {
        let s = (trace + 1.0).sqrt();
        q[3] = s * 0.5;
        let r = 0.5 / s;
        q[0] = (row[1][2] - row[2][1]) * r;
        q[1] = (row[2][0] - row[0][2]) * r;
        q[2] = (row[0][1] - row[1][0]) * r;
    } else {
        let mut i = usize::from(row[0][0] < row[1][1]);
        if row[i][i] < row[2][2] {
            i = 2;
        }
        let j = [1, 2, 0][i];
        let k = [1, 2, 0][j];
        let s = ((row[i][i] - row[j][j]) - row[k][k] + 1.0).sqrt();
        q[i] = s * 0.5;
        let r = 0.5 / s;
        q[3] = (row[j][k] - row[k][j]) * r;
        q[j] = (row[i][j] + row[j][i]) * r;
        q[k] = (row[i][k] + row[k][i]) * r;
    }
    Some(q)
}

fn product(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0] * b[3] + a[3] * b[0] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] + a[1] * b[3] + a[2] * b[0] - a[0] * b[2],
        a[2] * b[3] + a[3] * b[2] + a[0] * b[1] - a[1] * b[0],
        ((a[3] * b[3] - a[0] * b[0]) - a[1] * b[1]) - a[2] * b[2],
    ]
}

/// Quaternion angular limiter, 0x007ae860. Negative limits become zero;
/// the relative quaternion is flipped to select the shorter rotation.
pub fn rotate_towards(current: [f32; 4], target: [f32; 4], limit: f32) -> [f32; 4] {
    let n = current.iter().map(|x| x * x).sum::<f32>();
    let inverse = if n > 0.0 {
        [-current[0] / n, -current[1] / n, -current[2] / n, current[3] / n]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    let mut relative = product(target, inverse);
    if relative[3] < 0.0 {
        relative = relative.map(|x| -x);
    }
    let angle = relative[3].clamp(-1.0, 1.0).acos() * 2.0;
    let limit = limit.max(0.0);
    if angle < limit {
        return target;
    }
    let half = angle.min(limit) * 0.5;
    let axis_sq = relative[..3].iter().map(|x| x * x).sum::<f32>();
    let v = if axis_sq <= f32::EPSILON {
        [0.0; 3]
    } else {
        std::array::from_fn(|i| relative[i] * half.sin() / axis_sq.sqrt())
    };
    let mut step = [v[0], v[1], v[2], half.cos()];
    let n = step.iter().map(|x| x * x).sum::<f32>().sqrt();
    step = step.map(|x| x / n);
    product(step, current)
}

pub fn forward(rotation: [f32; 4]) -> [f32; 3] {
    let [x, y, z, w] = rotation;
    [
        2.0 * x * z + 2.0 * y * w,
        2.0 * y * z - 2.0 * x * w,
        1.0 - (2.0 * y * y + 2.0 * x * x),
    ]
}

#[derive(Debug, Clone, PartialEq)]
pub struct TargetFly {
    pub travel: TargetFlyTravel,
    pub rotation: [f32; 4],
    pub angular_speed: f32,
    pub lagging_turn: bool,
    /// Layer 2, native motions 122/123 and weight +0xD8.
    pub turn_motion: Option<u16>,
    pub turn_weight: f32,
    /// +0xDC mode: 0 left, 1 right, 2 aligned.
    turn_mode: u8,
}

impl TargetFly {
    pub fn enter(
        pos: [f32; 3],
        rotation: [f32; 4],
        target: [f32; 3],
        speed: f32,
        angular_degrees: f32,
        arrival_radius: f32,
        turn_weight: f32,
    ) -> Self {
        let delta = std::array::from_fn(|i| target[i] - pos[i]);
        let angular_speed = angular_degrees * 0.017453292;
        let aim = target_rotation(delta).unwrap_or(rotation);
        let dot = rotation
            .iter()
            .zip(aim)
            .map(|(a, b)| a * b)
            .sum::<f32>()
            .abs()
            .clamp(-1.0, 1.0);
        let required =
            (2.0 * dot.acos() * 57.295776) / (length(delta) / if speed == 0.0 { 1.0 } else { speed });
        let direction = forward(rotation);
        let difference = yaw_difference(direction[0].atan2(direction[2]), delta[0].atan2(delta[2]));
        let turn_mode = if difference.abs() * 57.295776 < 30.0 {
            2
        } else if difference > 0.0 {
            0
        } else {
            1
        };
        Self {
            travel: TargetFlyTravel::enter(target, speed, arrival_radius),
            rotation,
            angular_speed: if angular_speed == 0.0 {
                3.1415927
            } else {
                angular_speed
            },
            // Entry compares the authored (possibly zero) speed before replacement.
            lagging_turn: angular_speed < required,
            turn_motion: match turn_mode {
                0 => Some(123),
                1 => Some(122),
                _ => None,
            },
            turn_weight,
            turn_mode,
        }
    }

    /// Phase 97: orientation changes first; movement still consumes the
    /// model matrix cached before this phase, not this new quaternion.
    pub fn step(&mut self, pos: &mut [f32; 3], cached_forward: [f32; 3], step: f32) {
        if self.travel.done {
            return;
        }
        let delta = std::array::from_fn(|i| self.travel.target[i] - pos[i]);
        if let Some(aim) = target_rotation(delta) {
            self.rotation = rotate_towards(self.rotation, aim, self.angular_speed * step);
        }
        let heading = forward(self.rotation);
        let difference = yaw_difference(heading[0].atan2(heading[2]), delta[0].atan2(delta[2]));
        if self.turn_mode != 2 {
            self.turn_mode = if difference.abs() <= f32::EPSILON {
                2
            } else if difference >= 0.0 {
                0
            } else {
                1
            };
        }
        let wanted = match self.turn_mode {
            0 => Some(123),
            1 => Some(122),
            _ => None,
        };
        if wanted == self.turn_motion && wanted.is_some() {
            self.turn_weight = (self.turn_weight + step * 0.025).min(0.5);
        } else {
            self.turn_weight -= step * 0.025;
            if self.turn_weight < 0.0 {
                self.turn_weight = if wanted.is_some() { -self.turn_weight } else { 0.0 };
                self.turn_motion = wanted;
            }
        }
        if self.lagging_turn && difference.abs() <= f32::EPSILON {
            self.lagging_turn = false;
        }
        self.travel.step(pos, cached_forward, self.lagging_turn, step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn angular_limiter_and_shortest_quaternion_arc() {
        let identity = [0.0, 0.0, 0.0, 1.0];
        let target = [0.0, (0.5f32).sin(), 0.0, (0.5f32).cos()];
        let q = rotate_towards(identity, target, 0.25);
        assert!((forward(q)[0] - 0.25f32.sin()).abs() < 1e-6);
        assert_eq!(rotate_towards(identity, target, 2.0), target);
        assert_eq!(rotate_towards(identity, target, -1.0), identity);
        let flipped = rotate_towards(identity, target.map(|v| -v), 0.25);
        for (a, b) in q.into_iter().zip(flipped) {
            assert!((a - b).abs() < 1e-6);
        }
    }
    #[test]
    fn steering_retains_cached_translation_and_strict_arrival() {
        let mut flight = TargetFly::enter(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            [100.0, 0.0, 0.0],
            10.0,
            90.0,
            1.0,
            0.0,
        );
        let mut pos = [0.0; 3];
        flight.step(&mut pos, [0.0, 0.0, 1.0], 1.0);
        assert_eq!(pos, [0.0, 0.0, 10.0]);
        assert!((forward(flight.rotation)[0] - 1.0).abs() < 1e-6);
        assert!(!flight.travel.done);
        let mut pos = [100.0, 0.0, 1.0];
        flight.step(&mut pos, [0.0, 0.0, -1.0], 0.0);
        assert!(!flight.travel.done);
        pos[2] = 0.5;
        flight.step(&mut pos, [0.0, 0.0, -1.0], 0.0);
        assert!(flight.travel.done);
        assert_eq!(pos, flight.travel.target);
    }
}

//! Wolf and harpy runtime data and sourced action phases.
use crate::body::{load, Body, MotionId};
use ddai::thinktbl::families::{Action, Family};
use ddai::thinktbl::Tables;
use ddcombat::damage::{EnemyAttacker, EnemyDefender, RegionPool};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct BeastData {
    pub family: Family,
    pub body: Body,
    pub tables: Arc<Tables>,
    pub actions: Vec<Action>,
    pub attacker: EnemyAttacker,
    pub defender: EnemyDefender,
    pub regions: Vec<RegionPool>,
    pub obj_size: u32,
}

impl BeastData {
    pub fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>, family: Family) -> Result<Self, String> {
        let (id, group, model) = match family {
            Family::Wolf => ("em0200", "e02", "e0200"),
            Family::Harpy => ("em0600", "e06", "e0600"),
        };
        // Unit resource lists registered by 0x008f7b80 / 0x0093cbd0.
        let skeleton = load::skeleton(vfs, &format!(r"model\em\{group}\{model}\{model}"))?;
        let mut banks = Vec::new();
        for suffix in ["co", "at", "dm"] {
            banks.push(Some(load::bank(
                vfs,
                &format!(r"motion\em\{group}\{model}_{suffix}\{model}_{suffix}"),
            )?));
        }
        let collision_path = format!(r"collision\em\{group}\{model}\{model}");
        // Both resource lists register one rObjCollision containing the
        // body and attacks, unlike em0100's separate _00 attack resource.
        let collision = vec![load::collision(vfs, &collision_path, None)?];
        let stats = crate::enemy_stats::load(vfs, id)?;
        Ok(Self {
            family,
            // Standard em0200/em0600 actor scale is not yet compared live.
            body: Body {
                skeleton,
                banks,
                collision,
                scale: 1.0,
            },
            tables: Arc::new(family.tables(exe).map_err(|e| e.to_string())?),
            actions: family.actions(exe).map_err(|e| e.to_string())?,
            attacker: stats.attacker,
            defender: stats.defender,
            regions: stats.regions,
            obj_size: stats.obj_size,
        })
    }
}

/// The three sequential bite motions; transitions occur on motion end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WolfBite {
    #[default]
    Windup,
    Strike,
    Recovery,
    Finished,
}

impl WolfBite {
    // Translated from cEm0200Bite::vf05 0x008fe340 / vf06 0x008fe390
    // (DDDA.exe 2017-12-13 Steam).
    pub fn motion(self) -> MotionId {
        MotionId::new(
            1,
            match self {
                Self::Windup => 0,
                Self::Strike => 1,
                _ => 2,
            },
        )
    }

    pub fn motion_end(&mut self) {
        *self = match self {
            Self::Windup => Self::Strike,
            Self::Strike => Self::Recovery,
            Self::Recovery | Self::Finished => Self::Finished,
        };
    }

    /// During strike frames below 7 the unit enables its attack tracking
    /// and writes 0.1 to steering coefficient +0x6074.
    pub fn tracks_target(self, frame: f32) -> bool {
        self == Self::Strike && frame < 7.0
    }
}

/// Vertical target tracking of airborne actions, after both ground-ray
/// distances have been supplied by the world. State 1 uses a different
/// helper (0x00937f80) and is deliberately not handled here.
// Translated from 0x00937e10 (DDDA.exe 2017-12-13 Steam).
pub fn flight_height_step(current_y: f32, goal_y: f32, speed: f32, frames: f32) -> f32 {
    let delta = goal_y - current_y;
    let distance = delta.abs();
    if distance < f32::EPSILON {
        return current_y;
    }
    let step = (speed * speed * (-1.0 / distance) * frames + speed).max(0.0) * frames;
    current_y + if delta >= 0.0 { step } else { -step }
}

/// Harpy target-angle interpolation. The cap is per call, whereas the
/// proportional term scales with the simulation's 30-Hz frame step.
// Translated from 0x0093ca10 (DDDA.exe 2017-12-13 Steam).
pub fn harpy_turn(yaw: f32, goal: f32, cap: f32, divisor: f32, frames: f32) -> f32 {
    let delta = goal - yaw;
    let mut wrapped = delta - (delta * 0.159_154_94) as i32 as f32 * 6.283_185_5;
    if delta < 0.0 {
        wrapped += 6.283_185_5;
    }
    if wrapped >= 3.141_592_7 {
        wrapped -= 6.283_185_5;
    }
    let step = (wrapped / divisor * frames).clamp(-cap, cap);
    if wrapped.abs() - step.abs() <= 0.0 {
        goal
    } else {
        yaw + step
    }
}

/// AirAttack's approach state. The caller supplies the unit's target and
/// ground-ray result, and advances the selected motion separately.
#[derive(Debug, Clone, Copy)]
pub struct HarpyAirAttack {
    pub elapsed: f32,
    pub initial_distance: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AirAttackStep {
    pub yaw: f32,
    pub position: [f32; 3],
    /// New motion and starting frame; absent means keep the current one.
    pub motion: Option<(MotionId, f32)>,
    pub done: bool,
}

impl HarpyAirAttack {
    // Translated from cEm0600ActAirAttack::vf05 0x009453c0
    // (DDDA.exe 2017-12-13 Steam).
    pub fn new(position: [f32; 3], target: [f32; 3]) -> Self {
        Self {
            elapsed: 0.0,
            initial_distance: distance(position, target),
        }
    }

    pub fn initial_motion() -> MotionId {
        MotionId::new(1, 1)
    }

    // Translated from cEm0600ActAirAttack::vf06 0x00945490, phase byte
    // +0x70 == 0 (DDDA.exe 2017-12-13 Steam). Other phase values perform
    // no approach update; the owning action dispatcher guards this call.
    pub fn approach(
        &mut self,
        position: [f32; 3],
        yaw: f32,
        target: [f32; 3],
        height_goal: f32,
        motion_ended: bool,
        grounded: bool,
        frames: f32,
    ) -> AirAttackStep {
        let range = distance(position, target);
        self.elapsed += frames;
        let mut heading = yaw;
        if self.elapsed < 30.0 && range > 400.0 {
            heading = harpy_turn(
                yaw,
                (target[0] - position[0]).atan2(target[2] - position[2]),
                std::f32::consts::FRAC_PI_4,
                4.0,
                frames,
            );
        }
        // The original constructs a point 300 cm ahead, then applies
        // speed-30 distance damping toward that point.
        let (sin, cos) = heading.sin_cos();
        let dx = (position[0] + sin * 300.0) - position[0];
        let dz = (position[2] + cos * 300.0) - position[2];
        let ahead = (dx * dx + dz * dz).sqrt();
        let rate = if ahead >= f32::EPSILON {
            (1.0 / ahead) * ((-1.0 / ahead) * 900.0 * frames + 30.0).max(0.0)
        } else {
            0.0
        };
        AirAttackStep {
            yaw: heading,
            position: [
                position[0] + rate * dx * frames,
                flight_height_step(position[1], height_goal, 8.0, frames),
                position[2] + dz * rate * frames,
            ],
            motion: if range < 400.0 {
                Some((MotionId::new(1, 2), 8.0))
            } else if motion_ended {
                Some((MotionId::new(0, 0x2c), 0.0))
            } else {
                None
            },
            done: (self.elapsed > 60.0 && range > self.initial_distance) || self.elapsed > 120.0 || grounded,
        }
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harpy_dive_boundaries_and_turn_window() {
        let target = [0.0, 0.0, 400.0];
        let mut dive = HarpyAirAttack::new([0.0; 3], target);
        let out = dive.approach([0.0; 3], 0.0, target, 64.0, false, false, 1.0);
        for (actual, expected) in out.position.into_iter().zip([0.0, 7.0, 27.0]) {
            assert!((actual - expected).abs() < 0.00001);
        }
        assert_eq!(out.motion, None, "400 cm is outside the strike threshold");
        let out = dive.approach([0.0, 0.0, 0.01], 0.0, target, 0.0, false, false, 1.0);
        assert_eq!(out.motion, Some((MotionId::new(1, 2), 8.0)));
        dive.elapsed = 59.0;
        assert!(
            !dive
                .approach([0.0; 3], 0.0, [0.0, 0.0, 401.0], 0.0, false, false, 1.0)
                .done
        );
        assert!(
            dive.approach([0.0; 3], 0.0, [0.0, 0.0, 401.0], 0.0, false, false, 1.0)
                .done
        );
        dive.elapsed = 29.0;
        assert_eq!(
            dive.approach([0.0; 3], 0.0, [500.0, 0.0, 0.0], 0.0, false, false, 1.0)
                .yaw,
            0.0
        );
        assert!(dive.approach([0.0; 3], 0.0, target, 0.0, false, true, 1.0).done);
    }

    #[test]
    fn harpy_turn_wraps_and_caps_without_overshoot() {
        let pi = std::f32::consts::PI;
        let yaw = harpy_turn(pi - 0.1, -pi + 0.1, 1.0, 4.0, 1.0);
        assert!((yaw - (pi - 0.05)).abs() < 0.00001);
        assert_eq!(harpy_turn(0.0, 2.0, 0.1, 1.0, 0.5), 0.1);
        assert_eq!(harpy_turn(0.0, 0.01, 1.0, 1.0, 2.0), 0.01);
    }

    #[test]
    fn bite_waits_for_each_motion_end_and_limits_tracking() {
        let mut bite = WolfBite::default();
        assert_eq!(bite.motion(), MotionId::new(1, 0));
        assert!(!bite.tracks_target(0.0));
        bite.motion_end();
        assert_eq!(bite.motion(), MotionId::new(1, 1));
        assert!(bite.tracks_target(6.99));
        assert!(!bite.tracks_target(7.0));
        bite.motion_end();
        assert_eq!(bite.motion(), MotionId::new(1, 2));
        bite.motion_end();
        assert_eq!(bite, WolfBite::Finished);
        bite.motion_end();
        assert_eq!(bite, WolfBite::Finished);
    }

    #[test]
    fn flight_height_uses_distance_damping_in_both_directions() {
        assert_eq!(flight_height_step(0.0, 64.0, 8.0, 1.0), 7.0);
        assert_eq!(flight_height_step(64.0, 0.0, 8.0, 1.0), 57.0);
        assert_eq!(flight_height_step(0.0, 4.0, 8.0, 1.0), 0.0);
        assert_eq!(flight_height_step(32.0, 32.0, 8.0, 1.0), 32.0);
        assert_eq!(flight_height_step(0.0, 64.0, 8.0, 0.5), 3.75);
    }
}

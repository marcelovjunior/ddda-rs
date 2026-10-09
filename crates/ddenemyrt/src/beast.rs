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

#[cfg(test)]
mod tests {
    use super::*;

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

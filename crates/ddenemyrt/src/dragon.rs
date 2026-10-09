//! Dragon AI definitions read from the player's executable.
//!
//! This is the sequence-4 data boundary, not yet a dragon action simulator.
//! The entrance FSM is interpreted by ddai; movement and attack action
//! implementations must supply its completion and motion feedback.

use crate::combat::{Attacker, Fighter, Victim};
use ddai::{Error, thinktbl::Tables};
use ddcombat::damage::{CollisionRates, EnemyAttacker, EnemyDefender, RegionPool};

/// Phase-93 projection of character condition flags into collision flags.
/// `controller_flag_c` is the observed cAICtrl +0x0C byte; `actor_flags`
/// is the word returned by vf180 (actor +0x2738, 0x004c7480).
///
/// Translated from uEnemy::vf93 0x00aa9bf0 (0x00aa9d08..0x00aa9dbc),
/// reached by uEmDragonBase::vf93 0x00a40c20, DDDA.exe 2017-12-13 Steam.
/// No defaults are supplied: a host must provide the actual controller and
/// condition state before invoking this phase.
pub fn sync_collision_flags(rates: &mut CollisionRates, controller_flag_c: bool, actor_flags: u32) {
    if controller_flag_c {
        return;
    }
    let projected = ((actor_flags >> 27) & 1)
        | ((actor_flags >> 25) & 2)
        | ((actor_flags >> 26) & 4)
        | ((actor_flags >> 22) & 8);
    rates.flags = (rates.flags & !0xf) | projected;
}

/// The two defined HoverUp height modes. The actor's current think target
/// is read on every update; an explicit height is captured on entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HoverHeight {
    ThinkTarget,
    Explicit(f32),
}

/// Native HoverUp movement/completion state. This is the action's control
/// portion, not the actor's frame scheduler: motion binding, attached-actor
/// messages (0x00aad0c0), air flags and integration remain host work.
#[derive(Debug, Clone, PartialEq)]
pub struct HoverUp {
    pub height: HoverHeight,
    pub velocity: [f32; 3],
    pub done: bool,
}

impl HoverUp {
    /// Motion 100, blend 5, speed 1: 0x00a6ba90, DDDA.exe 2017-12-13 Steam.
    pub const MOTION: u16 = 100;

    /// Translated from cEm5800HoverUp::vf05 0x00a6ba90.
    pub fn enter(scale_y: f32, height: HoverHeight) -> Self {
        Self {
            height,
            velocity: [0.0, scale_y * 35.0, 0.0],
            done: false,
        }
    }

    /// Translated from vf06 0x00a6bb40. Called before velocity integration.
    /// Completion stops velocity; it does not snap the actor to the height.
    pub fn update(&mut self, actor_y: f32, think_target_y: f32) {
        let height = match self.height {
            HoverHeight::ThinkTarget => think_target_y,
            HoverHeight::Explicit(height) => height,
        };
        if height <= actor_y {
            self.velocity = [0.0; 3];
            self.done = true;
        }
    }

    /// Translated from vf08 0x00a6bc80: cancellation also stops velocity.
    pub fn exit(&mut self) {
        self.velocity = [0.0; 3];
    }
}

/// Shared hover-transition completion test, not an action implementation.
/// Both HoverKakku and KakkuHover use vf06 0x00a6be00 (Steam 2017-12-13).
/// Disassembly confirms both calls are the same primary-layer end getter;
/// model +0x494 bit31 is the additional completion path.
pub fn hover_transition_complete(motion_ended: bool, motion_flags: u32) -> bool {
    motion_ended || motion_flags & 0x8000_0000 != 0
}

/// Loaded opening-dragon body and combat parameters. Action stepping is
/// separate; this type does not manufacture action completion or volumes.
#[derive(Debug, Clone)]
pub struct DragonData {
    pub ai: DragonAiData,
    pub body: crate::body::Body,
    pub attacker: EnemyAttacker,
    pub defender: EnemyDefender,
    pub regions: Vec<RegionPool>,
    pub obj_size: u32,
}

impl DragonData {
    pub fn opening(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>, scale: f32) -> Result<Self, String> {
        let ai = DragonAiData::opening(exe).map_err(|e| e.to_string())?;
        let body = ai.load_body(vfs, scale)?;
        // 0x00a49f2d clears EAX before 0x00770910 selects region kind 0.
        // 0x00a49f3d passes charparam\em\em5800_cmn to 0x00aafdb0.
        // DDDA.exe 2017-12-13 Steam; shared reader keeps bulk values in the install.
        let stats = crate::enemy_stats::load(vfs, "em5800")?;
        Ok(Self {
            ai,
            body,
            attacker: stats.attacker,
            defender: stats.defender,
            regions: stats.regions,
            obj_size: stats.obj_size,
        })
    }

    /// Constructs the actual fight component with independent owner and pools.
    /// The sequence-4 initialization branch of 0x00a49f00 sets collision
    /// +0xCFC bit 0: HP damage is disabled initially. The later phase-93
    /// projection is [`sync_collision_flags`]; callers must supply actual
    /// controller/condition state before assuming the entrance is damageable.
    pub fn fighter(&self, owner: u32) -> Fighter {
        Fighter {
            owner,
            attacker: Attacker::Enemy(self.attacker),
            victim: Victim::Enemy {
                defender: self.defender,
                regions: self.regions.clone(),
                rates: CollisionRates {
                    flags: 1,
                    ..Default::default()
                },
            },
            attacks: Vec::new(),
            hurts: Vec::new(),
            history: Default::default(),
            statuses: Default::default(),
            dots: Default::default(),
            pools: Default::default(),
        }
    }
}

/// uEm5800 initialization 0x00a49f00, sequence-4 branch: assigns this root
/// to the think manager's current, default and base table pointers.
pub const OPENING_THINK_ROOT: u32 = 0x0169_9518;
/// Action descriptors in the supported executable (0x44 bytes each, DTI
/// at +0x28), shared by uEm5800 action dispatch. 0x00a3f89f passes the DTI
/// column 0x0185bd20 to 0x007f19c0, which advances by 0x44 per record.
pub const ACTION_TABLE: u32 = 0x0185_bcf8;
/// Embedded count passed in EDI at 0x00a3f8a4 (DDDA.exe 2017-12-13 Steam).
pub const ACTION_COUNT: u32 = 93;

/// uEm5800::vf05 0x00a49ed0 passes this table and count to 0x00889de0
/// (DDDA.exe 2017-12-13 Steam). Each descriptor occupies three words.
pub const RESOURCE_TABLE: u32 = 0x0185_d5d8;
pub const RESOURCE_COUNT: u32 = 57;

/// A resource registration, preserving executable order without assuming
/// that registration indices are motion-bank or collision-set indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragonResource {
    pub path: String,
    pub class: String,
    /// Flags after the executable's static initializer has run.
    pub flags: u32,
}

// Translated from 0x01363d50..0x0136412c (DDDA.exe 2017-12-13 Steam).
// Startup enables binding for the first 49 descriptors; the remaining
// effect/sound resources are preload-only. Disk flag words are zero.
fn initialized_resource_flags(index: u32, disk_flags: u32) -> u32 {
    if index < 49 {
        (disk_flags & !4) | 3
    } else {
        (disk_flags & !5) | 2
    }
}

#[derive(Debug, Clone)]
pub struct DragonAiData {
    pub tables: Tables,
    pub action_classes: Vec<String>,
    pub resources: Vec<DragonResource>,
}

impl DragonAiData {
    /// Binds the model, motion lists and collision resources selected by
    /// the initialized descriptor table. Scale belongs to the actor's
    /// placement; it is deliberately supplied by the caller.
    ///
    /// Translated from 0x008892b0 / 0x00889510 (DDDA.exe 2017-12-13 Steam):
    /// bit 0 enables binding, motion banks advance in descriptor order.
    /// Collision activations select the loaded file by its resource ID.
    pub fn load_body(&self, vfs: &ddvfs::Vfs, scale: f32) -> Result<crate::body::Body, String> {
        use crate::body::{Body, load};
        let bound: Vec<_> = self.resources.iter().filter(|r| r.flags & 1 != 0).collect();
        let models: Vec<_> = bound.iter().filter(|r| r.class == "rModel").collect();
        if models.len() != 1 {
            return Err(format!("dragon: expected one bound model, got {}", models.len()));
        }
        let skeleton = load::skeleton(vfs, &models[0].path)?;
        let banks = bound
            .iter()
            .filter(|r| r.class == "rMotionList")
            .map(|r| load::bank(vfs, &r.path).map(Some))
            .collect::<Result<_, _>>()?;
        let collision = bound
            .iter()
            .filter(|r| r.class == "rObjCollision")
            .map(|r| load::collision(vfs, &r.path, None))
            .collect::<Result<_, _>>()?;
        Ok(Body {
            skeleton,
            banks,
            collision,
            scale,
        })
    }

    /// Reads the Cassardis sequence's reachable think records and dragon
    /// action class names. No bulk game records are compiled into the port.
    pub fn opening(exe: &ddexe::Exe<'_>) -> Result<Self, Error> {
        let tables = Tables::read_roots(exe, vec![OPENING_THINK_ROOT], &[])?;
        let names = exe.class_names();
        let mut action_classes = Vec::new();
        // Translated from 0x00a3f89f / 0x007f19c0 (DDDA.exe 2017-12-13 Steam).
        for index in 0..ACTION_COUNT {
            let address = ACTION_TABLE + index * 0x44;
            let dti = exe.u32(address + 0x28).map_err(|e| Error::Data(e.0))?;
            action_classes.push(names.get(&dti).cloned().ok_or_else(|| {
                Error::Data(format!(
                    "dragon action {}: unknown DTI {dti:#x}",
                    action_classes.len()
                ))
            })?);
        }
        let mut resources = Vec::new();
        // Translated from 0x00889de0 (DDDA.exe 2017-12-13 Steam): null
        // path entries are skipped, and surviving entries retain order.
        for index in 0..RESOURCE_COUNT {
            let address = RESOURCE_TABLE + index * 12;
            let path = exe.u32(address).map_err(|e| Error::Data(e.0))?;
            if path == 0 {
                continue;
            }
            let dti = exe.u32(address + 4).map_err(|e| Error::Data(e.0))?;
            resources.push(DragonResource {
                path: exe.cstr(path).map_err(|e| Error::Data(e.0))?.to_owned(),
                class: names
                    .get(&dti)
                    .cloned()
                    .ok_or_else(|| Error::Data(format!("dragon resource {index}: unknown DTI {dti:#x}")))?,
                flags: initialized_resource_flags(index, exe.u32(address + 8).map_err(|e| Error::Data(e.0))?),
            });
        }
        Ok(Self {
            tables,
            action_classes,
            resources,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::initialized_resource_flags;
    use super::{CollisionRates, sync_collision_flags};
    use super::{HoverHeight, HoverUp, hover_transition_complete};

    #[test]
    fn hover_up_tracks_live_target_and_stops_before_integrating_again() {
        let mut rise = HoverUp::enter(2.0, HoverHeight::ThinkTarget);
        assert_eq!(HoverUp::MOTION, 100);
        assert_eq!(rise.velocity, [0.0, 70.0, 0.0]);
        rise.update(100.0, 120.0);
        assert!(!rise.done);
        // A moved think target is read on this frame; the controller does
        // not retain the old 120 height or snap the actor down to 90.
        rise.update(100.0, 90.0);
        assert!(rise.done);
        assert_eq!(rise.velocity, [0.0; 3]);
        let mut fixed = HoverUp::enter(1.0, HoverHeight::Explicit(130.0));
        fixed.update(129.0, 0.0);
        assert!(!fixed.done);
        fixed.update(130.0, 1000.0);
        assert!(fixed.done);
        let mut cancelled = HoverUp::enter(1.0, HoverHeight::ThinkTarget);
        cancelled.update(100.0, f32::NAN);
        assert!(!cancelled.done);
        cancelled.exit();
        assert_eq!(cancelled.velocity, [0.0; 3]);
        assert!(!cancelled.done, "exit is not action completion");
    }

    #[test]
    fn hover_transition_has_only_the_two_sourced_completion_paths() {
        assert!(!hover_transition_complete(false, 0x7fff_ffff));
        assert!(hover_transition_complete(true, 0));
        assert!(hover_transition_complete(false, 0x8000_0000));
    }

    #[test]
    fn phase93_keeps_controller_guard_and_projects_only_four_collision_bits() {
        let mut rates = CollisionRates {
            flags: 0x8a01,
            ..Default::default()
        };
        sync_collision_flags(&mut rates, true, 0);
        assert_eq!(
            rates.flags, 0x8a01,
            "completed/killed controller preserves initialization"
        );
        sync_collision_flags(&mut rates, false, 0);
        assert_eq!(
            rates.flags, 0x8a00,
            "running controller clears the initial no-HP bit"
        );
        for (actor, collision) in [
            (0x0800_0000, 1),
            (0x0400_0000, 2),
            (0x1000_0000, 4),
            (0x0200_0000, 8),
        ] {
            sync_collision_flags(&mut rates, false, actor);
            assert_eq!(rates.flags, 0x8a00 | collision);
        }
        sync_collision_flags(&mut rates, false, 0xe1ff_ffff);
        assert_eq!(rates.flags, 0x8a00, "unrelated actor flags must not project");
    }

    #[test]
    fn static_initialization_preserves_unrelated_flag_bits() {
        assert_eq!(initialized_resource_flags(48, 0), 3);
        assert_eq!(initialized_resource_flags(49, 0), 2);
        assert_eq!(initialized_resource_flags(48, 0xffff_ffff), 0xffff_fffb);
        assert_eq!(initialized_resource_flags(49, 0xffff_ffff), 0xffff_fffa);
    }
}

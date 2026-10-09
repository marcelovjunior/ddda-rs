//! Dragon AI definitions read from the player's executable.
//!
//! This is the sequence-4 data boundary, not yet a dragon action simulator.
//! The entrance FSM is interpreted by ddai; movement and attack action
//! implementations must supply its completion and motion feedback.

use crate::combat::{Attacker, Fighter, Victim};
use ddai::{Error, thinktbl::Tables};
use ddcombat::damage::{CollisionRates, EnemyAttacker, EnemyDefender, RegionPool};

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
    /// +0xCFC bit 0: HP damage is disabled initially. Its clearing transition
    /// remains untraced; callers must not assume the entrance is damageable.
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

    #[test]
    fn static_initialization_preserves_unrelated_flag_bits() {
        assert_eq!(initialized_resource_flags(48, 0), 3);
        assert_eq!(initialized_resource_flags(49, 0), 2);
        assert_eq!(initialized_resource_flags(48, 0xffff_ffff), 0xffff_fffb);
        assert_eq!(initialized_resource_flags(49, 0xffff_ffff), 0xffff_fffa);
    }
}

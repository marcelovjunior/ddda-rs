//! Ordinary `uEm5000` cyclops data and think initialization.
//! This is not yet an action driver: weapon children, armor and clinging
//! callbacks require their own runtime observations before combat is enabled.

/// Enemy kind written by constructor 0x009780d0 (2017-12-13 Steam).
pub const ENEMY_KIND: u8 = 0x3f;

/// The small callback subset whose motion request and completion conditions
/// are traced. Base-enter effects and the action manager remain separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimpleAction {
    Wait,
    Common,
    Attack,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionRequest {
    pub motion: crate::body::MotionId,
    pub force_restart: bool,
    pub blend_frames: f32,
    pub speed: f32,
}

impl SimpleAction {
    /// Wait emits this on update (0x0097ff30); Common/Attack emit it on
    /// enter (0x00980fb0). Body vf36 (0x00886e70) interprets the second
    /// argument as force-restart, not a bank. Packed motion IDs are decoded
    /// by uModel::vf30 (0x0105b480). The other arguments request zero flags,
    /// five blend frames, starting frame zero and playback speed one.
    pub fn motion_request(self, argument: u32) -> MotionRequest {
        MotionRequest {
            motion: crate::body::MotionId::new(((argument >> 8) & 0xf) as u8, (argument & 0xff) as u16),
            force_restart: self != Self::Wait,
            blend_frames: 5.0,
            speed: 1.0,
        }
    }

    /// Caller supplies the actual active motion's completion bit and flags.
    /// No duration or timer stands in for a missing motion observation.
    // Translated from 0x0097ff30, 0x00996f20 and 0x00981000
    // (DDDA.exe 2017-12-13 Steam). 0x010570f0 reads active motion +6 bit2;
    // both apparent queries in the latter callbacks read the same object.
    pub fn update(self, motion_end: bool, motion_flags: u32) -> SimpleUpdate {
        SimpleUpdate {
            done: self == Self::Wait || motion_end || motion_flags & 0x8000_0000 != 0,
            body_flags_or: if self == Self::Attack { 0x101 } else { 1 },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimpleUpdate {
    pub done: bool,
    /// OR into both actor +0x2360 and +0x2370, preserving other bits.
    pub body_flags_or: u32,
}

#[derive(Debug, Clone)]
pub struct Catalogue {
    pub actions: Vec<crate::chimera::ActionRecord>,
    pub resources: Vec<crate::chimera::ResourceRequest>,
    /// Hit-region to life-pool identity, not an array index.
    pub hit_to_pool: Vec<u32>,
}

impl Catalogue {
    /// Bind only the actor's resources (flag bit zero). Prepared child models
    /// and the interaction motion lists do not become additional body banks.
    // Translated from 0x008892b0 / 0x00889510, called by 0x00978820
    // (DDDA.exe 2017-12-13 Steam).
    pub fn load_body(&self, vfs: &ddvfs::Vfs, scale: f32) -> Result<crate::body::Body, String> {
        use crate::body::{Body, load};
        let bound = || self.resources.iter().filter(|resource| resource.flags & 1 != 0);
        let mut models = bound().filter(|resource| resource.class == "rModel");
        let model = models.next().ok_or("cyclops: missing body model")?;
        if models.next().is_some() {
            return Err("cyclops: ambiguous body model".into());
        }
        Ok(Body {
            skeleton: load::skeleton(vfs, &model.path)?,
            banks: bound()
                .filter(|resource| resource.class == "rMotionList")
                .map(|resource| load::bank(vfs, &resource.path).map(Some))
                .collect::<Result<_, _>>()?,
            collision: bound()
                .filter(|resource| resource.class == "rObjCollision")
                .map(|resource| load::collision(vfs, &resource.path, None))
                .collect::<Result<_, _>>()?,
            scale,
        })
    }

    /// Read the installed executable's bulk descriptors. Initializer
    /// 0x00978820 registers actions at 0x01847d38 and 44 resource slots at
    /// 0x01849cf0; the null resource slot is skipped by 0x00889de0.
    pub fn read(exe: &ddexe::Exe<'_>) -> Result<Self, String> {
        let names = exe.class_names();
        let class = |dti: u32| {
            names
                .get(&dti)
                .cloned()
                .ok_or_else(|| format!("cyclops: unknown DTI {dti:#x}"))
        };
        let mut actions = Vec::new();
        // 0x007f19c0 reads 0x44-byte descriptors until a null DTI.
        for index in 0..115 {
            let address = 0x0184_7d38 + index * 0x44;
            actions.push(crate::chimera::ActionRecord {
                class: class(exe.u32(address)?)?,
                argument: exe.u32(address + 4)?,
            });
        }
        if exe.u32(0x0184_9bc4)? != 0 {
            return Err("cyclops action terminator is missing".into());
        }
        let mut resources = Vec::new();
        for index in 0..44 {
            let address = 0x0184_9cf0 + index * 12;
            let path = exe.u32(address)?;
            if path != 0 {
                resources.push(crate::chimera::ResourceRequest {
                    path: exe.cstr(path)?.to_owned(),
                    class: class(exe.u32(address + 4)?)?,
                    flags: initialized_resource_flags(index, exe.u32(address + 8)?),
                });
            }
        }
        Ok(Self {
            actions,
            resources,
            hit_to_pool: exe.u32s(0x014f_f680, 13)?,
        })
    }
}

// Translated from 0x01357a00..0x01357cfa (DDDA.exe 2017-12-13 Steam).
fn initialized_resource_flags(index: u32, flags: u32) -> u32 {
    if index <= 20 || index == 37 || index >= 39 {
        (flags & !4) | 3
    } else {
        (flags & !5) | 2
    }
}

#[derive(Debug, Clone)]
pub struct ThinkInitialization {
    pub tables: ddai::thinktbl::Tables,
    pub current: u32,
    pub default: u32,
    pub base: u32,
    pub lock: bool,
}

impl ThinkInitialization {
    /// `controller` is present only when actor +0x5c14 enables the index
    /// at +0x5c18. No target, motion or family flags are fabricated here.
    // Translated from 0x00978820 (DDDA.exe 2017-12-13 Steam).
    pub fn read(exe: &ddexe::Exe<'_>, controller: Option<u32>) -> Result<Self, String> {
        let default = 0x0173_4b48;
        let current = match controller {
            Some(index) => {
                let address = index
                    .checked_mul(4)
                    .and_then(|offset| 0x0173_6ee8u32.checked_add(offset))
                    .ok_or("cyclops controller index overflow")?;
                exe.u32(address)?
            }
            None => default,
        };
        if exe.section_of(current) != Some(".data") {
            return Err(format!(
                "cyclops controller root {current:#x} is not executable data"
            ));
        }
        let tables = ddai::thinktbl::Tables::read_roots(exe, vec![current, default], &[])
            .map_err(|error| error.to_string())?;
        // +0x789c is overwritten with fallback after the enabled branch;
        // +0x78a0 remains the selected base, unlike the chimera initializer.
        Ok(Self {
            tables,
            current,
            default,
            base: current,
            lock: controller.is_some(),
        })
    }

    pub fn manager(&self, scale: f32) -> ddai::thinktbl::ThinkMgr {
        let mut manager = ddai::thinktbl::ThinkMgr::new();
        manager.table = self.current;
        manager.root = self.current;
        manager.default_table = self.default;
        manager.base_table = self.base;
        manager.lock = self.lock;
        manager.scale_on = scale > 0.0 && scale != 1.0;
        manager.scale = if manager.scale_on { scale } else { 1.0 };
        manager
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_rethinks_immediately_while_common_and_attack_wait_for_motion() {
        assert!(SimpleAction::Wait.update(false, 0).done);
        assert!(!SimpleAction::Wait.motion_request(2).force_restart);
        for action in [SimpleAction::Common, SimpleAction::Attack] {
            assert!(!action.update(false, 0).done);
            assert!(action.update(true, 0).done);
            assert!(action.update(false, 0x8000_0000).done);
            assert!(!action.update(false, 0x7fff_ffff).done);
            assert_eq!(
                action.motion_request(0x164).motion,
                crate::body::MotionId::new(1, 0x64)
            );
            assert!(action.motion_request(0x164).force_restart);
        }
        assert_eq!(SimpleAction::Attack.update(false, 0).body_flags_or, 0x101);
        assert_eq!(SimpleAction::Common.update(false, 0).body_flags_or, 1);
    }

    #[test]
    fn resource_fixups_preserve_high_bits_and_separate_child_requests() {
        for index in 0..44 {
            let bound = index <= 20 || index == 37 || index >= 39;
            assert_eq!(
                initialized_resource_flags(index, 0xffff_ffff),
                if bound { 0xffff_fffb } else { 0xffff_fffa }
            );
            assert_eq!(initialized_resource_flags(index, 0), if bound { 3 } else { 2 });
        }
    }
}

/// Mutable base-action fields. Scratch names describe their enter operation;
/// their later gameplay meanings have not yet been traced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActionEnterState {
    /// Action +0x50, reset by cActionBase::vf05.
    pub completion: u8,
    /// Action +0x54: owner link admitted by owner +0x206c bit 7.
    pub has_enemy_owner: bool,
    /// Action +0x58/+0x60/+0x70, cleared independently.
    pub scratch_words: [u32; 3],
    /// Action +0x64, cleared float.
    pub scratch_float: f32,
    /// Action +0x6c, set on entry.
    pub entered: bool,
    /// Action +0x74: successful uEm5000 owner cast.
    pub has_cyclops_owner: bool,
}

/// State of owner +0x2e74 consumed by 0x0081f790. Slot objects are optional;
/// the six cached suppression flags are updated even for absent objects.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollisionSwitch {
    pub bound: bool,                          // +4 != 0
    pub mode: u8,                             // +8
    pub suppress_object_writes: bool,         // +0x7d
    pub object_suppressed: [Option<bool>; 6], // +0xc pointers, object +0x61
    pub cached_suppressed: [bool; 6],         // +0x2c
}

/// Mutable owner fields touched by base enter, separate from immutable Body
/// assets. Offsets document unresolved semantics without inventing behaviour.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EnterBodyState {
    pub owner_flags: u8,              // +0x206c
    pub is_cyclops: bool,             // result of original DTI ancestry check
    pub action_flags: [u32; 2],       // +0x2370/+0x2374
    pub enemy_enter_scratch: u32,     // +0x273c
    pub current_priority: u32,        // +0x2e34
    pub pending_priority: u32,        // +0x2e38 (read only)
    pub enemy_action_sentinel: u32,   // +0x2ba0
    pub reset_vectors: [[f32; 4]; 2], // +0x2530/+0x2540
    pub cyclops_enter_scratch: u8,    // +0x5c7c
    /// Fallback used by motion/frame selector 0x0097e6a0 (+0x623c).
    pub motion_frame_fallback: u32,
    pub collision: CollisionSwitch,
}

/// Enter one action. `reset_vector` is runtime global +0x018cfe80..88:
/// callers supply it rather than assuming the BSS image is its runtime value.
/// The passed id is owner +0x2dd4. Out-of-range ids select row zero.
// Translated from 0x0097fd30, base 0x007f1890, helper 0x0081f790
// (DDDA.exe 2017-12-13 Steam).
pub fn enter_action(
    exe: &ddexe::Exe<'_>,
    action_id: u32,
    action: &mut ActionEnterState,
    body: &mut EnterBodyState,
    reset_vector: [f32; 3],
) -> Result<(), String> {
    // Read first so a failed resource read does not partly mutate state.
    // Disassembly 0x0097fe56/75 confirms BOTH fields have 16-byte stride;
    // the first load is a byte, despite the decompiler's array type.
    let row = if action_id < 115 { action_id } else { 0 };
    let table = if body.is_cyclops {
        Some((exe.u8(0x014f_ee64 + row * 16)?, exe.u32(0x014f_ee68 + row * 16)?))
    } else {
        None
    };
    let enemy = body.owner_flags & 2 != 0;
    if enemy {
        body.enemy_enter_scratch = 0;
    }
    action.completion = 0;
    body.action_flags = [0; 2];
    if enemy && body.pending_priority != 0 {
        body.current_priority = body.pending_priority;
        if body.pending_priority & 0x40 != 0 {
            body.current_priority = (body.current_priority & !0x40) | 0x10_0000;
        }
        if body.pending_priority & 0x400 != 0 {
            body.current_priority = (body.current_priority & !0x400) | 0x400_0000;
        }
    }
    action.has_enemy_owner = body.owner_flags & 0x80 != 0;
    action.scratch_words = [0; 3];
    action.scratch_float = 0.0;
    action.entered = true;
    if action.has_enemy_owner {
        body.enemy_action_sentinel = u32::MAX;
    }
    action.has_cyclops_owner = body.is_cyclops;
    if let Some((mode, fallback)) = table {
        body.reset_vectors = [[reset_vector[0], reset_vector[1], reset_vector[2], 0.0]; 2];
        body.cyclops_enter_scratch = 0;
        let collision = &mut body.collision;
        if collision.bound && collision.mode != mode {
            let suppressed = mode != 1;
            if !collision.suppress_object_writes {
                for slot in collision.object_suppressed.iter_mut().flatten() {
                    *slot = suppressed;
                }
            }
            collision.cached_suppressed = [suppressed; 6];
            collision.mode = mode;
        }
        body.motion_frame_fallback = fallback;
    }
    Ok(())
}

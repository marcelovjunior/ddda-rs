//! Ordinary `uEm5000` cyclops data and think initialization.
//! The table driver takes canonical sensor/world observations explicitly.
//! Weapon children, armor and clinging still need runtime adapters.

/// Enemy kind written by constructor 0x009780d0 (2017-12-13 Steam).
pub const ENEMY_KIND: u8 = 0x3f;

/// Placement fields copied to actor +0x6095/+0x609c/+0x60a0/+0x60a4.
/// Values come from cSetInfoEnemy5000, not constructor defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacementValues {
    pub arisen_proof: bool,
    pub remaining_hp_ratio: f32,
    pub leg_stagger: f32,
    pub leg_knockdown: f32,
}

/// Apply the placement-specific health and leg writes to this actor's pools.
/// Pools must already have their ordinary initialization and any common HP
/// multiplier applied (0x00889040). This translates the health/leg portion
/// only; equipment, experience, think and rendering setup remain separate.
// Translated from 0x00978820, specifically 0x009790ca..0x0097916d
// (DDDA.exe 2017-12-13 Steam).
pub fn initialize_placement(
    exe: &ddexe::Exe<'_>,
    values: PlacementValues,
    regions: &mut [ddcombat::damage::RegionPool],
) -> Result<(), String> {
    if !values.arisen_proof {
        return Ok(());
    }
    // Validate both installed table reads before changing caller state.
    let leg_ids = [exe.u32(0x014f_f680 + 5 * 4)?, exe.u32(0x014f_f680 + 6 * 4)?];
    // 0x004c6af0 (owner vf82) reads the first array slot's maximum,
    // not a sum of region HP or a search by pool identity.
    if let Some(main) = regions.first_mut() {
        let remaining = main.hp_max * values.remaining_hp_ratio;
        // The source clamps only above the maximum; negative authored values
        // are not silently clamped to zero.
        main.hp = if remaining > main.hp_max {
            main.hp_max
        } else {
            remaining
        };
    }
    // 0x00770f00 resolves hurt-region indices through the installed mapping
    // and searches by pool +4 identity. These are current DP/BP (+14/+20),
    // leaving their maxima (+18/+24) and recovery speeds unchanged.
    for pool_id in leg_ids {
        if let Some(leg) = regions.iter_mut().find(|pool| pool.id == pool_id) {
            leg.dp = values.leg_stagger;
            leg.bp = values.leg_knockdown;
        }
    }
    Ok(())
}

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

/// A measured object supplied by the host, in the actor's coordinate space.
/// IDs identify actors, never a global player or a party index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThinkTarget {
    /// Nonzero object token (the original stores a pointer); the host maps
    /// its canonical actor IDs to these tokens, including an actor with ID0.
    pub id: u32,
    pub position: [f32; 3],
    pub radius: f32,
    pub flags: [u32; 2],
    /// Actor +0x2370, copied to measured slot +0x50 by 0x00aadcf0.
    pub action_flags: u32,
}

/// One sensor slot from actor +0x2f94 (0xa3 dwords per slot). Score
/// components are present only when their source enable byte is nonzero.
/// How sensors calculate these scores is outside this table driver.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorCandidate {
    pub target: ThinkTarget,
    pub enabled: bool,
    pub scores: [Option<f32>; 32],
}

/// Highest strictly positive enabled score wins; ties retain the earlier
/// sensor slot. Only the original's 16 slots are examined. With `false`,
/// targets with actor +0x2360 bits 16/17 are rejected; +0x2364 bit5 is
/// always rejected. No nearest-player fallback is present in this function.
// Translated from 0x00ab52e0 (DDDA.exe 2017-12-13 Steam), called for slot0
// by 0x0097a6a0 with ignore_protection=false.
pub fn sensor_target(candidates: &[SensorCandidate], ignore_protection: bool) -> Option<ThinkTarget> {
    let mut best = None;
    let mut best_score = 0.0;
    for candidate in candidates.iter().take(16) {
        if candidate.target.id == 0
            || !candidate.enabled
            || (!ignore_protection && candidate.target.flags[0] & 0x30000 != 0)
            || candidate.target.flags[1] & 0x20 != 0
        {
            continue;
        }
        let score: f32 = candidate.scores.iter().flatten().copied().sum();
        if score > best_score {
            best_score = score;
            best = Some(candidate.target);
        }
    }
    best
}

/// The unit fields read by local checks. Their owning systems supply the
/// observations; a missing attachment or secondary target is not fabricated.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocalChecks {
    /// Actor +0x5c64: includes attachment entries whose owner is absent.
    pub attachment_count: u32,
    /// Present owners' vocation at owner +0x3c20 -> +0x64.
    pub attached_vocations: Vec<u32>,
    /// Actor +0x6238, accumulated in 0x0097e1a0 and reset by set callback1.
    pub elapsed_frames: f32,
    /// Actor +0x62bc; its producing system is not translated here.
    pub counter: u32,
    /// Actor +0x614c (not the sensor target at slot0).
    pub secondary_target: Option<[f32; 3]>,
    pub position: [f32; 3],
}

impl LocalChecks {
    // Translated from 0x00987e90 / 0x00987f10 / 0x00987fb0
    // (DDDA.exe 2017-12-13 Steam).
    pub fn check(&self, id: u32, argument: u32) -> bool {
        match id {
            0 => argument <= self.attachment_count,
            1 => self.attached_vocations.iter().any(|vocation| match vocation {
                4 => argument == 3,
                13 => argument == 4,
                14 => argument == 4 || argument == 1,
                15..=17 => argument == 1,
                18 => argument == 4 || argument == 2,
                19..=21 => argument == 2,
                _ => argument == 0,
            }),
            2 => argument as f32 <= self.elapsed_frames,
            3 => argument <= self.counter,
            4 => self.secondary_target.is_some_and(|position| {
                let dx = position[0] - self.position[0];
                let dz = position[2] - self.position[2];
                (dx * dx + dz * dz).sqrt() <= argument as f32
            }),
            _ => false,
        }
    }
}

/// Family flags at +0x6a24..37 prepared in 0x0097a6a0. The host supplies
/// bits3..19 from the canonical cyclops systems. Bits0..2 come from vf108
/// (actor +0x2d70); bit7 is cleared then derived from the weapon's +0x206d
/// bit4. Bits20..31 are untouched by this flag-preparation block.
// Translated from 0x0097a6a0 (DDDA.exe 2017-12-13 Steam).
pub fn think_flags(mode: u32, existing_flags: u32, weapon_flag: bool) -> u32 {
    let mode_bits = match mode {
        1 => 2,
        2 => 4,
        _ => 1,
    };
    (existing_flags & !(7 | (1 << 7))) | mode_bits | (u32::from(weapon_flag) << 7)
}

/// World observations for one table run. Seven local slots are authored in
/// 0x0097a6a0; extra object slots9..22 may be prepared by common callbacks.
/// Slot0 prefers the supplied rage target, otherwise the sensor selector.
/// Point targets (escape position etc.) can be installed directly into mgr
/// slots; this adapter only replaces slots whose inputs are listed here.
#[derive(Debug, Clone, Default)]
pub struct ThinkInput {
    pub position: [f32; 3],
    pub yaw: f32,
    pub mode: u32,
    pub family_flags: u32,
    pub weapon_flag: bool,
    pub sensors: Vec<SensorCandidate>,
    pub rage_target: Option<ThinkTarget>,
    /// Slots1..6, already measured by the host. Missing inputs clear their
    /// slot as the original does, including the point targets3 and6.
    pub local_slots: [Option<ddai::thinktbl::Slot>; 6],
}

/// One actor's table state. Action execution and world providers are separate
/// from immutable installed table data; errors never substitute callback
/// answers. Forced-action arbitration is supplied before this table phase.
#[derive(Debug, Clone)]
pub struct ThinkDriver {
    pub manager: ddai::thinktbl::ThinkMgr,
    pub local: LocalChecks,
    pub action: i32,
    pub target: Option<u32>,
}

impl ThinkDriver {
    pub fn new(initialization: &ThinkInitialization, scale: f32, action: i32) -> Self {
        Self {
            manager: initialization.manager(scale),
            local: LocalChecks::default(),
            action,
            target: None,
        }
    }

    /// Timer phase called by uEm5000 vf08, independent of whether the
    /// current action allowed a table run. The host keeps the phase order.
    // Translated from 0x00979420 -> 0x008b7930
    // (DDDA.exe 2017-12-13 Steam).
    pub fn tick_timers(&mut self, frame_step: f32) {
        self.manager.tick(frame_step);
    }

    /// Tail of the unit's think (after 0x0097a6a0), not a real-time seconds
    /// timer. `reset_elapsed` is actor +0x6148 != 0; `weapon_type` is +0x608c.
    // Translated from 0x0097e1a0 (DDDA.exe 2017-12-13 Steam).
    pub fn advance_elapsed(&mut self, frame_step: f32, reset_elapsed: bool, weapon_type: u32) {
        if reset_elapsed || weapon_type == 2 {
            self.local.elapsed_frames = 0.0;
        } else {
            self.local.elapsed_frames += frame_step;
        }
    }

    /// The ordinary table phase of 0x0097a6a0: only runs when the action's
    /// +0x50 completion byte or actor +0x7c30 permits it. The caller handles
    /// forced actions, FSM activity, death and disabled-AI before this phase.
    // Translated from 0x0097a6a0 (DDDA.exe 2017-12-13 Steam).
    pub fn think(
        &mut self,
        tables: &ddai::thinktbl::Tables,
        input: &ThinkInput,
        action_complete: bool,
        allow_think: bool,
        world: &mut dyn ddai::thinktbl::Host,
    ) -> Result<Option<i32>, ddai::Error> {
        if !action_complete && !allow_think {
            return Ok(None);
        }
        self.local.position = input.position;
        self.manager.flags = think_flags(input.mode, input.family_flags, input.weapon_flag);
        let target = input.rage_target.or_else(|| sensor_target(&input.sensors, false));
        self.manager.slots[0] = target.map_or_else(ddai::thinktbl::Slot::default, |target| {
            let mut slot = ddai::thinktbl::Slot::measure(
                input.position,
                input.yaw,
                target.position,
                target.radius,
                target.id,
            );
            slot.mask48 = target.flags[0];
            slot.mask50 = target.action_flags;
            slot.category = 0x400;
            slot.categorized = true;
            slot
        });
        for (destination, source) in self.manager.slots[1..7].iter_mut().zip(&input.local_slots) {
            *destination = source.unwrap_or_default();
        }
        // Slot category flags are supplied by this family, not the world.
        // 0x0097a6a0 cases1..6; slot0 is marked above.
        for (slot, category) in self.manager.slots[1..7]
            .iter_mut()
            .zip([0x800, 0x1000, 0x800, 0x2000, 0x4000, 4])
        {
            slot.category = category;
            slot.categorized = true;
        }
        self.manager.reload_default();
        let mut adapter = ThinkCallbacks {
            local: &mut self.local,
            world,
        };
        let result = self.manager.think(tables, &mut adapter)?;
        self.target = self
            .manager
            .slots
            .get(self.manager.cur as usize)
            .filter(|slot| slot.valid && slot.obj != 0)
            .map(|slot| slot.obj);
        if result != -1 && (result != self.action || self.manager.ea5) {
            self.action = result;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }
}

struct ThinkCallbacks<'a> {
    local: &'a mut LocalChecks,
    world: &'a mut dyn ddai::thinktbl::Host,
}

impl ddai::thinktbl::Host for ThinkCallbacks<'_> {
    // Translated from 0x00987e60 (DDDA.exe 2017-12-13 Steam).
    fn check(&mut self, id: u32, argument: u32) -> Result<bool, ddai::Error> {
        if id < 1000 {
            Ok(self.local.check(id, argument))
        } else {
            self.world.check(id, argument)
        }
    }

    // Translated from 0x00987e20 (DDDA.exe 2017-12-13 Steam).
    fn set(&mut self, id: u32, argument: u32) -> Result<u32, ddai::Error> {
        if id >= 1000 {
            return self.world.set(id, argument);
        }
        if id == 1 {
            self.local.elapsed_frames = 0.0;
        }
        Ok(0)
    }

    fn row_target(&mut self, slot: usize) -> bool {
        self.world.row_target(slot)
    }
    fn rand(&mut self) -> u32 {
        self.world.rand()
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

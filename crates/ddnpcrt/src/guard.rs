//! Authored NPC guard dialogue resources. Actor eligibility, reservation,
//! activation and cleanup remain separate from message presentation.

use crate::{schedule::Guard, TalkSet};
use std::{collections::BTreeMap, sync::Arc};

/// Resource keys used by 0x00bb47d0 / 0x00baddf0. Quest identifiers are
/// runtime data; the two negative keys are embedded selector constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourceKey {
    Quest(u32),
    Base,
    Override,
}

/// Loaded resources for one NPC, preserving explicit absence. No fallback
/// is invented when a requested quest resource has not been loaded.
#[derive(Default)]
pub struct Resources {
    sets: BTreeMap<ResourceKey, Arc<TalkSet>>,
}

impl Resources {
    pub fn insert(&mut self, key: ResourceKey, set: Arc<TalkSet>) {
        self.sets.insert(key, set);
    }

    pub fn remove(&mut self, key: ResourceKey) {
        self.sets.remove(&key);
    }

    /// Translated from 0x00bb47d0 and 0x00baddf0
    /// (DDDA.exe 2017-12-13 Steam). Kind 4 checks key -1 for the
    /// authored conversation, then uses key -2; kind 5 uses key -2.
    pub fn select(&self, guard: &Guard) -> Option<Arc<TalkSet>> {
        let key = match guard.message_kind {
            2 => ResourceKey::Quest(guard.quest),
            4 if self.sets.get(&ResourceKey::Override).is_some_and(|set|
                guard.message >= 0 && set.set.by_number(guard.message).is_some()) => ResourceKey::Override,
            4 | 5 => ResourceKey::Base,
            _ => return None,
        };
        self.sets.get(&key).cloned()
    }
}

/// Quest text preload list for a resolved NPC id. Translated from
/// 0x00bace50 (DDDA.exe 2017-12-13 Steam): pointer table 0x0163cdf0,
/// u16 entries until 0xffff, only identifiers below 128 are loaded.
/// The actor adapter supplies a valid NPC id; this does not establish
/// placement existence or execute the special actor branches of vf32.
pub fn quest_preloads(exe: &ddexe::Exe<'_>, npc: u32) -> Result<Vec<u16>, ddexe::Error> {
    let entry = npc.checked_mul(4).and_then(|v| v.checked_add(0x0163_cdf0))
        .ok_or_else(|| ddexe::Error("NPC preload table address overflow".into()))?;
    let mut address = exe.u32(entry)?;
    if address == 0 { return Ok(vec![]); }
    let mut quests = Vec::new();
    loop {
        let quest = exe.u16(address)?;
        if quest == 0xffff { return Ok(quests); }
        if quest < 128 { quests.push(quest); }
        address = address.checked_add(2)
            .ok_or_else(|| ddexe::Error("NPC preload list address overflow".into()))?;
    }
}

impl crate::InstallData {
    /// Load exactly one authored message resource from the player's install.
    /// Preload membership and actor-specific vf32 branches are the caller's
    /// responsibility; loading a resource alone does not start a conversation.
    pub fn authored_talk_set(&self, path: &str) -> Result<Arc<TalkSet>, ddai::Error> {
        let bytes = self.vfs.read(path, mtarc::type_hash("rMsgSet")).map_err(ddai::Error::Data)?;
        let set = ddnpc::MessageSet::parse(&bytes).map_err(|e| ddai::Error::Data(e.0))?;
        let bytes = self.vfs.read(&format!("{path}_{}", self.language), mtarc::type_hash("rGUIMessage"))
            .map_err(ddai::Error::Data)?;
        let text = mtgmd::MessageFile::parse(&bytes).map_err(|e| ddai::Error::Data(e.0))?
            .messages.iter().map(ToString::to_string).collect();
        Ok(Arc::new(TalkSet { set, text }))
    }
}

/// Path authored by 0x00bace50 for a quest text resource.
pub fn quest_path(npc: u32, quest: u16) -> String {
    format!(r"id\npc_wind\npc_quest_talk\q{quest:04}_n{npc:03}")
}

/// A resolved actor lifetime and interacting seat. `actor` must change on
/// replacement even when the NPC id is reused. Seat ownership is the co-op
/// extension of the original's single actor pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogueOwner {
    pub actor: u64,
    pub player: crate::PlayerId,
}

/// The original shared candidate/active dialogue slots, with seat ownership.
/// Callers arbitrate eligible seats in stable order; eligibility and resource
/// availability are separate observations, not inferred here.
#[derive(Debug, Default)]
pub struct DialogueSlot {
    reserved: Option<DialogueOwner>,
    active: Option<DialogueOwner>,
}

impl DialogueSlot {
    pub fn reserved(&self) -> Option<DialogueOwner> { self.reserved }
    pub fn active(&self) -> Option<DialogueOwner> { self.active }

    /// Translated from 0x0043dd80 (DDDA.exe 2017-12-13 Steam).
    /// Success does not imply this owner acquired the candidate: an existing
    /// reservation is retained. Promotion must separately check ownership.
    pub fn reserve(&mut self, owner: DialogueOwner, actor_status: Option<u32>) -> bool {
        if self.active.is_some() || owner.actor == 0
            || !actor_status.is_some_and(|status| matches!(status & 7, 1 | 2)) {
            return false;
        }
        self.reserved.get_or_insert(owner);
        true
    }

    /// Matching-owner candidate removal from 0x0043de30. Guard dispatch
    /// 0x00bb0470 uses this when the actor loses eligibility. Clearing a
    /// candidate does not clear an active dialogue or report TalkEnd.
    /// Ancillary GUI/global side effects remain outside this slot model.
    pub fn clear_reserved(&mut self, owner: DialogueOwner) -> bool {
        if self.reserved != Some(owner) { return false; }
        self.reserved = None;
        true
    }

    /// Translated from 0x0043ded0 (DDDA.exe 2017-12-13 Steam).
    /// The seat comparison prevents another co-op seat taking a reservation
    /// for the same actor. No reservation is required when both slots are free.
    pub fn activate(&mut self, owner: DialogueOwner, actor_status: Option<u32>) -> bool {
        if owner.actor == 0 || !actor_status.is_some_and(|s| matches!(s & 7, 1 | 2))
            || self.reserved.is_some_and(|reserved| reserved != owner) || self.active.is_some() {
            return false;
        }
        self.active = Some(owner);
        self.reserved = None;
        true
    }

    /// Matching-owner portion of 0x0043dfd0. The actor cleanup adapter calls
    /// this after real exit; a text/UI End event does not call it implicitly.
    /// Audio/global side effects of the original helper are not implemented.
    pub fn clear_active(&mut self, owner: DialogueOwner) -> bool {
        if self.active != Some(owner) { return false; }
        self.active = None;
        true
    }

    /// Actor destruction/replacement invalidates only that lifetime. This
    /// host lifecycle hook is independent of successful dialogue completion.
    pub fn retire(&mut self, actor: u64) {
        if self.reserved.is_some_and(|owner| owner.actor == actor) { self.reserved = None; }
        if self.active.is_some_and(|owner| owner.actor == actor) { self.active = None; }
    }
}

/// Real actor observations read by uNpc::vf111. Unknown observations cannot
/// satisfy a branch. This carries no UI completion event.
#[derive(Debug, Default, Clone, Copy)]
pub struct TalkEndObservation {
    /// Actor+0x586a, written by the actual talk-exit helper 0x00bb41c0.
    pub pending_exit: u8,
    /// None covers absent controller or an unavailable observation.
    pub controller_enabled: Option<bool>,
    pub game_flags: Option<u32>,
    pub current_action: Option<u32>,
    pub active_dialogue_empty: Option<bool>,
}

impl TalkEndObservation {
    /// Translated from 0x00bb3290 (DDDA.exe 2017-12-13 Steam).
    /// The pending branch consumes only value1. A second independent branch
    /// observes talk action0x03000041 with an empty active dialogue slot.
    /// Caller forwards this frame's result, not a persistent latched pulse.
    pub fn take_frame(&mut self) -> bool {
        let pending = self.pending_exit == 1 && self.controller_enabled == Some(true)
            && self.game_flags.is_some_and(|flags| flags & 0x10_0000 == 0);
        if pending { self.pending_exit = 0; }
        pending || (self.current_action == Some(0x0300_0041) && self.active_dialogue_empty == Some(true))
    }
}

/// Motion-state observations shared by Guard launch and real talk exit.
/// Unknown state never authorizes an actor transition.
#[derive(Debug, Default, Clone, Copy)]
pub struct TalkMotionObservation {
    /// Actor+0x57e4.
    pub mode: Option<u32>,
    /// Actor+0x57e0, nonzero means active.
    pub active: Option<bool>,
    /// Actor+0x56d0.
    pub flags: Option<u32>,
}

impl TalkMotionObservation {
    /// Translated from 0x00bb4f90; the same predicate gates cleanup in
    /// 0x00bb41c0 (DDDA.exe 2017-12-13 Steam). Mode zero needs no motion
    /// observation; nonzero mode requires an active motion and bit2.
    /// Guard0x00bb0db0 uses true for automatic launch, false for manual
    /// candidate reservation when its authored guard flag permits it. False
    /// therefore does not reject all Guard interactions. Talk exit0x00bb41c0
    /// does require true. This is not full actor eligibility.
    pub fn ready(&self) -> bool {
        match self.mode {
            Some(0) => true,
            Some(_) => self.active == Some(true) && self.flags.is_some_and(|flags| flags & 4 != 0),
            None => false,
        }
    }
}

/// Translated from 0x0049ace0 (DDDA.exe 2017-12-13 Steam), consumed as
/// a byte by Guard0x00bb0db0. This reads cUnit+4, not NPC intel+0x5c0.
/// A known allocated unit (status1) alone is insufficient for this check.
pub fn guard_unit_ready(status: Option<u32>) -> bool {
    status.is_some_and(|status| status & 7 == 2 && status & 0x4800 == 0x4800)
}

/// The range trigger stored at intel+0x4d4. Changing Guard configuration
/// resets it (0x00bbf580). An actor crossing the sphere can trigger once;
/// remaining inside does not retrigger the authored conversation.
#[derive(Debug, Default)]
pub struct Sensor {
    triggered: bool,
}

impl Sensor {
    /// Spatial portion of 0x00bb0db0 (DDDA.exe 2017-12-13 Steam).
    /// Stage-frame positions have already had any world offset resolved.
    /// Angular guards need the original direction helper; unsupported
    /// kinds/ranges fail explicitly rather than ignoring that restriction.
    pub fn hit(&mut self, config: &Guard, actor: [f32; 3], yaw: f32,
        player: [f32; 3]) -> Result<bool, ddai::Error> {
        if !matches!(config.kind, 1 | 2) || config.range != 0.0 {
            return Err(ddai::Error::Unimplemented("NPC Guard angular or nonspatial sensor".into()));
        }
        let distance = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
        if distance(actor, player) >= 5000.0 { return Ok(false); }
        let center = if config.kind == 1 {
            if (actor[1] - player[1]).abs() > config.height { return Ok(false); }
            let (sin, cos) = yaw.sin_cos();
            [actor[0] + cos * config.center[0] + sin * config.center[2],
             actor[1] + config.center[1],
             actor[2] - sin * config.center[0] + cos * config.center[2]]
        } else { config.center };
        if distance(center, player) >= config.radius {
            self.triggered = false;
            return Ok(false);
        }
        Ok(!self.triggered)
    }

    /// Set only after launch succeeds; unavailable text cannot consume it.
    pub fn launched(&mut self) { self.triggered = true; }
}

/// The actor side of one real Guard conversation, separate from the text
/// cursor. Motion readiness and controller state are explicit observations.
#[derive(Debug, Default)]
pub struct ActorTalk {
    pub motion: TalkMotionObservation,
    pub controller_enabled: bool,
    restore_controller: bool,
    active: bool,
    text_finished: bool,
    pending_exit: u8,
    dialogue_released: bool,
}

impl ActorTalk {
    pub fn new(motion: TalkMotionObservation, controller_enabled: bool) -> Self {
        Self { motion, controller_enabled, ..Default::default() }
    }
    /// Entering the promoted talk action (0x00bb4060 / 0x00bba820).
    pub fn enter(&mut self) {
        self.restore_controller = self.controller_enabled;
        self.active = true;
        self.text_finished = false;
        self.dialogue_released = false;
        self.controller_enabled = false;
    }
    pub fn text_finished(&mut self) { self.text_finished = true; }
    /// Normal (non-jump) cNpcActTalk::vf06 state0x1b, 0x00b9a430,
    /// clears the active slot before motion cleanup. The stock resource
    /// release checks 0x0044a140/0x00449f40 must actually have completed.
    /// Jump dialogue types8/9 also need counterpart actor observations and
    /// are outside this narrow path. Text End alone never releases a slot.
    pub fn release_dialogue(&mut self, resources_released: Option<bool>, jump_kind: u32) -> bool {
        if !self.active || !self.text_finished || self.dialogue_released
            || resources_released != Some(true) || jump_kind != 0 { return false; }
        self.dialogue_released = true;
        true
    }
    /// Actor cleanup from 0x00bb41c0, gated by actual motion readiness.
    /// A text End alone is insufficient. True permits clearing the slot.
    pub fn cleanup(&mut self) -> bool {
        if !self.active || !self.text_finished || !self.motion.ready() { return false; }
        self.active = false;
        self.controller_enabled = self.restore_controller;
        self.pending_exit = 1;
        true
    }
    /// Pending-exit branch of uNpc::vf111 (0x00bb3290).
    pub fn take_frame(&mut self, game_flags: Option<u32>, active_dialogue_empty: Option<bool>) -> bool {
        let mut observation = TalkEndObservation {
            pending_exit: self.pending_exit,
            controller_enabled: Some(self.controller_enabled),
            game_flags,
            current_action: self.active.then_some(0x0300_0041),
            active_dialogue_empty: if self.dialogue_released { active_dialogue_empty } else { Some(false) },
            ..Default::default()
        };
        let result = observation.take_frame();
        self.pending_exit = observation.pending_exit;
        result
    }
}

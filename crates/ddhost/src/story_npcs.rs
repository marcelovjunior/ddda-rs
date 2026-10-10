//! Explicit placement bindings for applied NPC action graphs. Actor movement,
//! guard eligibility and dialogue remain observations from their real systems.

use crate::{Host, HostError, StoryWorld};
use ddnpcrt::schedule::{Clock, Npc, Request};
use ddstory::{data::Data, Target};
use std::collections::{BTreeMap, HashMap};

/// Host identity for one materialized intel lifetime. This deterministic
/// co-op token replaces pointer identity; it is not an original game value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NpcLifetime {
    pub npc: u32,
    pub generation: u64,
}

struct Applied {
    stage: u32,
    target: Target,
    path: String,
    action: Npc,
    normal: Option<Npc>,
}

struct GuardAction {
    config: ddnpcrt::schedule::Guard,
    sensor: ddnpcrt::guard::Sensor,
}

struct GuardTalk {
    owner: ddnpcrt::guard::DialogueOwner,
    actor: ddnpcrt::guard::ActorTalk,
    jump_kind: u32,
}

#[derive(Default)]
pub struct StoryNpcs {
    /// Intels allocated by the real placement adapter, distinct from
    /// externally supplied fixture bindings/observations.
    intels: HashMap<(u32, Target), crate::npc_placements::Placement>,
    bindings: HashMap<(u32, Target), u32>,
    positions: HashMap<u32, [f32; 3]>,
    intel_flags: HashMap<u32, u32>,
    gauge_visible: HashMap<u32, bool>,
    lifetimes: HashMap<u32, u64>,
    pub(crate) lifetime_counter: u64,
    pending: HashMap<(u32, Target), String>,
    normal_controllers: HashMap<(u32, Target), bool>,
    applied: BTreeMap<u32, Applied>,
    guards: BTreeMap<u32, GuardAction>,
    dialogue: ddnpcrt::guard::DialogueSlot,
    talks: BTreeMap<u32, GuardTalk>,
    motion_bodies: HashMap<u32, std::sync::Arc<crate::npc_motion::Banks>>,
    motion_requests: HashMap<u32, (crate::npc_motion::Token, ddnpcrt::schedule::Motion)>,
    motions: BTreeMap<u32, crate::npc_motion::Actor>,
    motion_counter: u64,
}

impl<D: Data, W: StoryWorld> Host<D, W> {
    /// The actor adapter supplies actual body eligibility and talk-motion
    /// observations. Distance alone cannot authorize dialogue. Arbitration
    /// is host-owned and names the interacting seat, not a global player.
    pub fn talk_story_guard(&mut self, player: crate::PlayerId, owner: NpcLifetime,
        actor_ready: bool, yaw: f32, motion: ddnpcrt::guard::TalkMotionObservation)
        -> Result<Option<crate::npcs::TalkOutcome>, HostError>
    {
        self.roster.seat(player).ok_or(HostError::UnknownPlayer(player))?;
        if !actor_ready || self.story_npc_lifetime(owner.npc) != Some(owner) { return Ok(None); }
        let Some(position) = self.story_npc_position(owner.npc) else { return Ok(None); };
        let Some(player_position) = self.sim.player(player).map(|p| p.position) else { return Ok(None); };
        let Some(guard) = self.story_npcs.guards.get_mut(&owner.npc) else { return Ok(None); };
        // Motion-not-ready is the manual candidate branch of0x00bb0db0;
        // authored guard=true rejects that branch. Non-jump path only.
        if (!motion.ready() && guard.config.guard) || guard.config.jump_kind != 0 { return Ok(None); }
        if !guard.sensor.hit(&guard.config, position, yaw, player_position)
            .map_err(|e| HostError::Npc(e.to_string()))? { return Ok(None); }
        let config = guard.config.clone();
        let data = self.data.npc.clone().ok_or(HostError::NoData("npc"))?;
        let Some(set) = data.guard_talk_set(owner.npc, &config)
            .map_err(|e| HostError::Npc(e.to_string()))? else { return Ok(None); };
        if set.set.by_number(config.message).is_none() { return Ok(None); }
        let dialogue_owner = ddnpcrt::guard::DialogueOwner { actor: owner.generation, player };
        if !self.story_npcs.dialogue.reserve(dialogue_owner, Some(2))
            || !self.story_npcs.dialogue.activate(dialogue_owner, Some(2)) { return Ok(None); }
        let controller_enabled = self.story_npcs.applied.get(&owner.npc)
            .is_some_and(|applied| !applied.action.applied_finished());
        let mut actor = ddnpcrt::guard::ActorTalk::new(motion, controller_enabled);
        actor.enter();
        self.story_npcs.talks.insert(owner.npc, GuardTalk {
            owner: dialogue_owner, actor, jump_kind: config.jump_kind,
        });
        if let Some(story) = self.story.as_mut() { story.stage.game_flags.s54 = 1; }
        match self.talk_authored(player, owner.npc, set, config.message) {
            Ok(outcome) => {
                // 0x00bb0db0 leaves +0x4d4 clear in the manual branch;
                // only its motion-ready automatic launch consumes the latch.
                if motion.ready() { self.story_npcs.guards.get_mut(&owner.npc).unwrap().sensor.launched(); }
                Ok(Some(outcome))
            }
            Err(error) => {
                self.story_npcs.talks.remove(&owner.npc);
                self.story_npcs.dialogue.clear_active(dialogue_owner);
                if let Some(story) = self.story.as_mut() { story.stage.game_flags.s54 = 0; }
                Err(error)
            }
        }
    }

    pub fn story_npc_guard(&self, npc: u32) -> Option<&ddnpcrt::schedule::Guard> {
        self.story_npcs.guards.get(&npc).map(|action| &action.config)
    }

    pub(crate) fn story_guard_text_ended(&mut self, player: crate::PlayerId, npc: u32) -> bool {
        if let Some(talk) = self.story_npcs.talks.get_mut(&npc)
            .filter(|talk| talk.owner.player == player
                && self.story_npcs.dialogue.active() == Some(talk.owner)) {
            talk.actor.text_finished();
            return true;
        }
        false
    }
    /// Reconcile the adapter's complete materialized NPC set for a stage.
    /// The caller resolves loaded cells, group gates, name-family selection,
    /// intel-pool admission and archive readiness first. Raw Candidates are
    /// not sufficient. Omitted placements are retired; unchanged ones retain
    /// their graph state. This is host lifecycle composition, not a claim
    /// that the original's streaming/admission policy has been reproduced.
    pub fn reconcile_story_npc_intels(
        &mut self, stage: u32, mut placements: Vec<crate::npc_placements::Placement>,
    ) -> Result<Vec<(Target, String)>, HostError> {
        let mut targets = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        for placement in &placements {
            if placement.stage != stage || placement.target.kind != 5
                || !(0..900).contains(&placement.npc)
                || !targets.insert(placement.target) || !ids.insert(placement.npc as u32) {
                return Err(HostError::Npc("invalid or duplicate materialized NPC placement".into()));
            }
        }
        // Validate external owner conflicts before retiring this stage's set.
        for (&key, &npc) in &self.story_npcs.bindings {
            if ids.contains(&npc) && (key.0 != stage
                || (!self.story_npcs.intels.contains_key(&key)
                    && !placements.iter().any(|p| p.npc as u32 == npc && p.target == key.1))) {
                return Err(HostError::Npc("materialized NPC has a conflicting external owner".into()));
            }
        }
        for &npc in &ids {
            if self.npcs.npcs.contains_key(&npc)
                && !self.story_npcs.bindings.values().any(|&bound| bound == npc) {
                return Err(HostError::Npc("NPC graph exists without a matching placement binding".into()));
            }
        }
        let mut retired: Vec<_> = self.story_npcs.intels.iter()
            .filter(|((owner_stage, _), old)| *owner_stage == stage
                && !placements.iter().any(|new| new == *old))
            .map(|(&(stage, target), _)| (stage, target)).collect();
        retired.sort_by_key(|(stage, target)| (*stage, target.kind, target.group, target.index));
        for (stage, target) in retired { self.unbind_story_npc(stage, target); }
        placements.sort_by_key(|p| (p.npc, p.target.kind, p.target.group, p.target.index));
        let mut diagnostics = Vec::new();
        for placement in placements {
            let target = placement.target;
            if let Some(message) = self.load_story_npc_intel(placement)? {
                diagnostics.push((target, message));
            }
        }
        Ok(diagnostics)
    }

    /// Actual placement owners, independent of normal or applied graph state.
    /// The presentation adapter can retain/draw an actor while its normal
    /// graph is suspended; graph-map membership is not actor existence.
    pub fn story_npc_intels(&self) -> impl Iterator<Item = &crate::npc_placements::Placement> {
        let mut entries: Vec<_> = self.story_npcs.intels.values().collect();
        entries.sort_by_key(|p| (p.stage, p.target.kind, p.target.group, p.target.index));
        entries.into_iter()
    }

    /// Create an intel after the placement adapter has resolved the
    /// 0x00762000 materialization prerequisites (name family, intel-pool
    /// capacity and archive readiness). Reading Candidates alone is not enough.
    /// 0x00bbd930 allocates normal controller state before daily graph load.
    /// A graph load failure is returned as a diagnostic while retaining
    /// that controller; it cannot be mistaken for an absent intel.
    pub fn load_story_npc_intel(&mut self, placement: crate::npc_placements::Placement)
        -> Result<Option<String>, HostError>
    {
        if !(0..900).contains(&placement.npc) {
            return Err(HostError::Npc("nonstandard NPC intel id is not implemented".into()));
        }
        let npc = placement.npc as u32;
        let key = (placement.stage, placement.target);
        if self.story_npcs.bindings.iter().any(|(other, &id)| *other != key && id == npc) {
            return Err(HostError::Npc("NPC already bound to another placement".into()));
        }
        if self.story_npcs.intels.get(&key) == Some(&placement) {
            return Ok(None);
        }
        let generation = self.story_npcs.lifetime_counter.checked_add(1)
            .ok_or_else(|| HostError::Npc("NPC lifetime token space exhausted".into()))?;
        if self.story_npcs.bindings.contains_key(&key) {
            self.unbind_story_npc(key.0, key.1);
        } else if self.npcs.npcs.contains_key(&npc) {
            return Err(HostError::Npc("NPC graph exists without a matching placement binding".into()));
        }
        let mut normal = Npc::intel(npc);
        let scene = self.story.as_ref().map_or(0, |s| s.world.sce_no);
        let mut diagnostic = None;
        if let Some(path) = ddnpc::schedule_path(key.0 as i32, scene as i32, placement.npc, placement.use_24_schedule) {
            if let Some(data) = &self.data.npc {
                match Npc::scheduled(npc, &**data, &path) {
                    Ok(loaded) => normal = loaded,
                    Err(error) => diagnostic = Some(error.to_string()),
                }
            } else {
                diagnostic = Some("NPC graph data unavailable".into());
            }
        }
        self.npcs.npcs.insert(npc, normal);
        self.bind_story_npc(key.0, key.1, npc);
        // 0x00bbdaa0 starts the placement FSM through 0x0059e260 before
        // the independent daily graph runs. A queued explicit ApplyFSM is
        // a later command and supersedes this initial placement default.
        if !placement.authored_action.is_empty() {
            self.story_npcs.pending.entry(key).or_insert_with(|| placement.authored_action.clone());
        }
        self.story_npcs.positions.insert(npc, placement.transform.position);
        // uNpcIntel constructor0x00bbbfa0 initializes +0x5c0 to zero.
        self.story_npcs.intel_flags.insert(npc, 0);
        // uNpcIntel constructor0x00bbbfa0 clears +0x547.
        self.story_npcs.gauge_visible.insert(npc, false);
        self.story_npcs.lifetime_counter = generation;
        self.story_npcs.lifetimes.insert(npc, generation);
        self.story_npcs.intels.insert(key, placement);
        self.observe_story_npc_normal_controller(key.0, key.1, Some(true));
        self.world.observe_npc_intel(key.0, key.1, true);
        Ok(diagnostic)
    }

    /// The layout adapter supplies identity; no target/NPC-number heuristic.
    pub fn bind_story_npc(&mut self, stage: u32, target: Target, npc: u32) {
        assert_eq!(target.kind, 5, "NPC binding requires an NPC target");
        let key = (stage, target);
        if self.story_npcs.bindings.get(&key).is_some_and(|old| *old != npc) {
            // A replacement actor cannot inherit the old actor's running
            // graph, pending request or allocation observation.
            self.unbind_story_npc(stage, target);
        }
        self.story_npcs.bindings.insert(key, npc);
    }

    pub fn unbind_story_npc(&mut self, stage: u32, target: Target) {
        if let Some(intel) = self.story_npcs.intels.remove(&(stage, target)) {
            if let Some(lifetime) = self.story_npcs.lifetimes.get(&(intel.npc as u32)) {
                if self.story_npcs.dialogue.active().is_some_and(|owner| owner.actor == *lifetime) {
                    if let Some(story) = self.story.as_mut() { story.stage.game_flags.s54 = 0; }
                }
                self.story_npcs.dialogue.retire(*lifetime);
            }
            if let Some(talk) = self.story_npcs.talks.remove(&(intel.npc as u32)) {
                if self.npcs.talking_to(intel.npc as u32) == Some(talk.owner.player) {
                    self.npcs.talk_abort(talk.owner.player);
                }
                let _ = self.locks.release(intel.npc as u32, talk.owner.player);
            }
            self.story_npcs.guards.remove(&(intel.npc as u32));
            self.npcs.npcs.remove(&(intel.npc as u32));
            self.story_npcs.positions.remove(&(intel.npc as u32));
            self.story_npcs.intel_flags.remove(&(intel.npc as u32));
            self.story_npcs.gauge_visible.remove(&(intel.npc as u32));
            self.story_npcs.motion_bodies.remove(&(intel.npc as u32));
            self.story_npcs.motion_requests.remove(&(intel.npc as u32));
            self.story_npcs.motions.remove(&(intel.npc as u32));
            self.story_npcs.lifetimes.remove(&(intel.npc as u32));
            self.world.observe_npc_intel(stage, target, false);
        }
        if let Some(npc) = self.story_npcs.bindings.remove(&(stage, target)) {
            self.story_npcs.applied.remove(&npc);
        }
        self.story_npcs.pending.remove(&(stage, target));
        self.story_npcs.normal_controllers.remove(&(stage, target));
    }

    /// Queue an authored graph until its actual placement is bound.
    pub fn apply_story_npc(&mut self, stage: u32, target: Target, path: &str) {
        if target.kind == 5 {
            self.story_npcs.pending.insert((stage, target), path.into());
        }
    }

    /// A real dialogue system supplies this pulse, after its own eligibility
    /// checks. This does not mean every talk ending satisfies TalkEnd.
    pub fn observe_story_npc_talk_end(&mut self, npc: u32) {
        if let Some(applied) = self.story_npcs.applied.get_mut(&npc) {
            applied.action.observe_talk_end(true);
        }
    }

    pub fn story_npc_lifetime(&self, npc: u32) -> Option<NpcLifetime> {
        self.story_npcs.lifetimes.get(&npc).map(|&generation| NpcLifetime { npc, generation })
    }

    /// Reject a delayed observation from a retired actor, even if its NPC id
    /// and placement target have been reused. Dialogue/action eligibility is
    /// still the caller's responsibility, as in the synchronous fixture API.
    pub fn observe_story_npc_talk_end_for(&mut self, owner: NpcLifetime) -> bool {
        if self.story_npc_lifetime(owner.npc) != Some(owner)
            || !self.story_npcs.applied.get(&owner.npc).is_some_and(|a| !a.action.applied_finished()) { return false; }
        self.observe_story_npc_talk_end(owner.npc);
        true
    }

    /// Actor-lifetime checked arrival. A future asynchronous mover must ALSO
    /// validate the current interface/command when an action changes within
    /// the same actor lifetime; this token alone does not identify a Goto.
    pub fn story_npc_arrived_for(&mut self, owner: NpcLifetime, position: [f32; 3]) -> bool {
        if self.story_npc_lifetime(owner.npc) != Some(owner) { return false; }
        self.story_npc_arrived(owner.npc, position);
        true
    }

    /// Position of the materialized actor, independent of graph suspension
    /// and presentation interpolation. Only real placement/actor requests
    /// or an explicit host mover arrival update it.
    pub fn story_npc_position(&self, npc: u32) -> Option<[f32; 3]> {
        self.story_npcs.positions.get(&npc).copied()
    }

    /// Canonical intel flags, including Set Disable Flag writes. This does
    /// not claim every flag consumer is implemented. uNpc::vf180 at
    /// 0x00bb4730 returns this same intel+0x5c0 storage for a linked body.
    pub fn story_npc_intel_flags(&self, npc: u32) -> Option<u32> {
        self.story_npcs.intel_flags.get(&npc).copied()
    }

    /// Authored gauge desire (intel+0x547), not actual GUI allocation or HP.
    pub fn story_npc_gauge_visible(&self, npc: u32) -> Option<bool> {
        self.story_npcs.gauge_visible.get(&npc).copied()
    }

    fn story_npc_motion_command(&self, npc: u32) -> Option<u64> {
        if let Some(applied) = self.story_npcs.applied.get(&npc).filter(|a| !a.action.applied_finished()) {
            applied.action.motion_command()
        } else { self.npcs.npcs.get(&npc).and_then(Npc::motion_command) }
    }

    /// Called only after the actor adapter materializes this lifetime's
    /// body and resolves its real resource banks. An intel alone cannot
    /// play or finish motions. Rebinding the same body is idempotent.
    pub fn bind_story_npc_motion_body(&mut self, owner: NpcLifetime,
        banks: std::sync::Arc<crate::npc_motion::Banks>) -> Result<bool, HostError> {
        if self.story_npc_lifetime(owner.npc) != Some(owner) { return Ok(false); }
        if !self.story_npcs.motion_bodies.contains_key(&owner.npc) {
            if let Some((token, _)) = self.story_npcs.motion_requests.get_mut(&owner.npc) {
                self.story_npcs.motion_counter = self.story_npcs.motion_counter.checked_add(1)
                    .ok_or_else(|| HostError::Npc("NPC motion launch space exhausted".into()))?;
                token.launch = self.story_npcs.motion_counter;
            }
        }
        self.story_npcs.motion_bodies.insert(owner.npc, banks);
        self.start_story_npc_motion(owner.npc)?;
        Ok(true)
    }

    pub fn unbind_story_npc_motion_body(&mut self, owner: NpcLifetime) -> bool {
        if self.story_npc_lifetime(owner.npc) != Some(owner) { return false; }
        self.story_npcs.motion_bodies.remove(&owner.npc);
        self.story_npcs.motions.remove(&owner.npc);
        true
    }

    pub fn story_npc_motion(&self, npc: u32) -> Option<&crate::npc_motion::Actor> {
        self.story_npcs.motions.get(&npc)
    }

    pub fn story_npc_motion_resource(&self, npc: u32) -> Option<&mtlmt::Motion> {
        let actor = self.story_npcs.motions.get(&npc)?;
        self.story_npcs.motion_bodies.get(&npc)?.resolve(actor.authored.number).ok().map(|(_, r)| r)
    }

    fn start_story_npc_motion(&mut self, npc: u32) -> Result<(), HostError> {
        let Some((token, motion)) = self.story_npcs.motion_requests.get(&npc).cloned() else { return Ok(()) };
        if self.story_npc_lifetime(npc) != Some(token.owner)
            || self.story_npc_motion_command(npc) != Some(token.command)
            || self.story_npcs.motions.get(&npc).is_some_and(|a| a.token == token) { return Ok(()); }
        let Some(banks) = self.story_npcs.motion_bodies.get(&npc) else { return Ok(()) };
        let (number, resource) = banks.resolve(motion.number).map_err(HostError::Npc)?;
        let actor = crate::npc_motion::Actor::new(token, motion, number, resource).map_err(HostError::Npc)?;
        if let Some(flags) = self.story_npcs.intel_flags.get_mut(&npc) {
            // 0x00576e10 writes vf180 bit0x20 only on an actual NPC body.
            if actor.authored.talk { *flags |= 0x20; } else { *flags &= !0x20; }
        }
        self.story_npcs.motions.insert(npc, actor);
        Ok(())
    }

    /// Consume only the current lifetime and launch's real action terminal.
    /// Does not expose the unrestricted graph Done shortcut to the adapter.
    pub fn observe_story_npc_motion(&mut self, observation: crate::npc_motion::Observation) -> bool {
        let npc = observation.token.owner.npc;
        if self.story_npc_lifetime(npc) != Some(observation.token.owner)
            || self.story_npcs.talks.contains_key(&npc)
            || self.story_npc_motion_command(npc) != Some(observation.token.command) { return false; }
        let Some(actor) = self.story_npcs.motions.get_mut(&npc) else { return false };
        if !actor.observe(observation) { return false; }
        if let Some(applied) = self.story_npcs.applied.get_mut(&npc).filter(|a| !a.action.applied_finished()) {
            applied.action.motion_done(observation.token.command)
        } else { self.npcs.npcs.get_mut(&npc).is_some_and(|n| n.motion_done(observation.token.command)) }
    }

    fn step_story_npc_motions(&mut self) -> Result<(), HostError> {
        let mut observations = Vec::new();
        // Type1 is the original sPlayerManager+0x99c, represented by the
        // host-owned party slot0 in co-op, never the viewing seat.
        let first = match self.party.slots[0] {
            Some(crate::Member::Arisen(id)) => self.sim.player(id).map(|p| p.position), _ => None,
        };
        for (&npc, actor) in &mut self.story_npcs.motions {
            if self.story_npcs.talks.contains_key(&npc) { continue; }
            if let Some(placement) = self.story_npcs.intels.values().find(|p| p.npc as u32 == npc) {
                if let Some(position) = self.story_npcs.positions.get(&npc) {
                    let target = match actor.authored.target_type {
                        0 => Some(actor.authored.target_position), 1 => first,
                        _ => None, // Party/layout target types require their real actor adapters.
                    };
                    actor.target(*position, placement.transform.angle[1], target);
                }
            }
            actor.advance(self.clock.delta).map_err(HostError::Npc)?;
            observations.push(actor.observation());
        }
        for observation in observations { self.observe_story_npc_motion(observation); }
        Ok(())
    }

    pub(crate) fn record_story_npc_requests(&mut self, requests: &[Request]) {
        for request in requests {
            match request {
                Request::Motion { npc, command, motion: Some(motion), .. } => {
                    if let Some(owner) = self.story_npc_lifetime(*npc) {
                        if self.story_npc_motion_command(*npc) != Some(*command) { continue; }
                        let Some(launch) = self.story_npcs.motion_counter.checked_add(1) else {
                            self.npc_errors.push((*npc, "NPC motion launch space exhausted".into())); continue;
                        };
                        self.story_npcs.motion_counter = launch;
                        self.story_npcs.motion_requests.insert(*npc, (crate::npc_motion::Token {
                            owner, command: *command, launch,
                        }, motion.clone()));
                        if let Err(error) = self.start_story_npc_motion(*npc) {
                            self.npc_errors.push((*npc, error.to_string()));
                        }
                    }
                }
                Request::Gauge { npc, visible } => {
                    if let Some(gauge) = self.story_npcs.gauge_visible.get_mut(npc) { *gauge = *visible; }
                }
                Request::Place { npc, pos, .. } => {
                    if let Some(position) = self.story_npcs.positions.get_mut(npc) { *position = *pos; }
                }
                Request::ActorFlag { npc, flag, value } => {
                    // Intel storage also backs linked-body vf180 (0x00bb4730).
                    // Flag consumers and FixedPosition body ground mode remain separate.
                    if let Some(flags) = self.story_npcs.intel_flags.get_mut(npc) {
                        if *value { *flags |= flag.mask(); } else { *flags &= !flag.mask(); }
                    }
                }
                Request::DisableFlags { npc, mask } => {
                    if let Some(flags) = self.story_npcs.intel_flags.get_mut(npc) { *flags |= mask; }
                }
                Request::Guard { npc, config } => {
                    if config.kind == 0 {
                        self.story_npcs.guards.remove(npc);
                    } else if self.story_npcs.lifetimes.contains_key(npc) {
                        self.story_npcs.guards.insert(*npc, GuardAction {
                            config: config.clone(), sensor: Default::default(),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    /// Synchronous arrival from the current owner only. An asynchronous mover
    /// must validate its placement/lifetime token before calling this method;
    /// an NPC id alone cannot distinguish a retired actor from its replacement.
    pub fn story_npc_arrived(&mut self, npc: u32, position: [f32; 3]) {
        if let Some(applied) = self.story_npcs.applied.get_mut(&npc).filter(|a| !a.action.applied_finished()) {
            applied.action.done(Some(position));
        } else if let Some(normal) = self.npcs.npcs.get_mut(&npc) {
            normal.done(Some(position));
        }
        if let Some(current) = self.story_npcs.positions.get_mut(&npc) { *current = position; }
    }

    /// Report actual normal-controller creation/destruction for a bound NPC.
    /// 0x00bbd930 allocates intel+0x15c before loading its daily graph; body
    /// setup 0x00bbe545 links the same pointer through cAICtrl+0x4c.
    /// Missing graph data and applied Finish cannot establish this fact.
    /// None withdraws an observation; unbinding also removes it.
    pub fn observe_story_npc_normal_controller(
        &mut self, stage: u32, target: Target, present: Option<bool>,
    ) {
        let key = (stage, target);
        if !self.story_npcs.bindings.contains_key(&key) {
            return;
        }
        if let Some(present) = present {
            self.story_npcs.normal_controllers.insert(key, present);
        } else {
            self.story_npcs.normal_controllers.remove(&key);
        }
    }

    /// Explicit NPC ApplyFSM observation, independent of graph completion.
    /// The actor adapter still reports target resolution separately.
    pub fn story_npc_apply_fsm_ready(&self, stage: u32, target: Target) -> Option<bool> {
        self.story_npcs.normal_controllers.get(&(stage, target)).copied()
    }

    /// Loaded normal graph availability, kept distinct from both controller
    /// allocation and applied graph Finish. This is diagnostic only.
    pub fn story_npc_normal_graph_present(&self, npc: u32) -> Option<bool> {
        self.story_npcs.applied.get(&npc).map(|a| {
            a.normal
                .as_ref()
                .or_else(|| self.npcs.npcs.get(&npc))
                .is_some_and(|n| n.controller.is_some())
        })
    }

    /// Applied graph status for the actor adapter and diagnostics. Its
    /// finished bit must not be substituted for the parent ApplyFSM result.
    pub fn story_npc_action(&self, npc: u32) -> Option<(u32, Target, &str, bool)> {
        self.story_npcs
            .applied
            .get(&npc)
            .map(|a| (a.stage, a.target, a.path.as_str(), a.action.applied_finished()))
    }

    pub(crate) fn prepare_story_npcs(&mut self) -> Result<(), HostError> {
        let Some(story) = self.story.as_ref() else {
            return Ok(());
        };
        let stage = story.stage.stage_no;
        let unloaded: Vec<_> = self.story_npcs.intels.keys().copied()
            .filter(|(s, _)| *s != stage).collect();
        for (old_stage, target) in unloaded {
            self.unbind_story_npc(old_stage, target);
        }
        self.story_npcs.applied.retain(|_, a| a.stage == stage);
        self.story_npcs.bindings.retain(|(s, _), _| *s == stage);
        self.story_npcs.pending.retain(|(s, _), _| *s == stage);
        self.story_npcs.normal_controllers.retain(|(s, _), _| *s == stage);
        let Some(data) = self.data.npc.clone() else {
            return Ok(());
        };
        let mut ready: Vec<_> = self
            .story_npcs
            .pending
            .iter()
            .filter_map(|(key, path)| {
                self.story_npcs
                    .bindings
                    .get(key)
                    .map(|&npc| (*key, npc, path.clone()))
            })
            .collect();
        // Stable host ordering for co-op, matching the NPC runtime's id
        // ordering; never depend on randomized hash-table iteration.
        ready.sort_by_key(|((stage, target), npc, _)|
            (*npc, *stage, target.kind, target.group, target.index));
        for ((stage, target), npc, path) in ready {
            // Applied graph replacement cannot inherit an old motion launch
            // whose local command number happens to equal its first command.
            self.story_npcs.motion_requests.remove(&npc);
            self.story_npcs.motions.remove(&npc);
            let action = Npc::applied(npc, &*data, &path).map_err(|e| HostError::Npc(e.to_string()))?;
            let normal = self
                .story_npcs
                .applied
                .remove(&npc)
                .and_then(|a| a.normal)
                .or_else(|| self.npcs.npcs.remove(&npc));
            self.story_npcs.applied.insert(
                npc,
                Applied {
                    stage,
                    target,
                    path,
                    action,
                    normal,
                },
            );
            self.story_npcs.pending.remove(&(stage, target));
        }
        Ok(())
    }

    pub(crate) fn step_story_npcs(&mut self) -> Result<Vec<Request>, HostError> {
        self.step_story_npc_motions()?;
        // A departed co-op seat cannot leave a shared dialogue stop behind.
        // Retirement is cancellation, never the original actor's TalkEnd.
        let departed: Vec<_> = self.story_npcs.talks.iter()
            .filter(|(_, talk)| self.roster.seat(talk.owner.player).is_none())
            .map(|(&npc, _)| npc).collect();
        for npc in departed {
            let talk = self.story_npcs.talks.remove(&npc).unwrap();
            self.story_npcs.dialogue.retire(talk.owner.actor);
            if self.story_npcs.dialogue.active().is_none() {
                if let Some(story) = self.story.as_mut() { story.stage.game_flags.s54 = 0; }
            }
        }
        // Normal Guard talks hold no appearance stock resource transactions:
        // loading their text is synchronous, and no exit stock was requested.
        // Empty stock release lists return true at0x00788bb0. Jump/replacement
        // talks are explicitly rejected by talk_story_guard until implemented.
        let mut ended = Vec::new();
        for (&npc, talk) in &mut self.story_npcs.talks {
            if talk.actor.release_dialogue(Some(true), talk.jump_kind) {
                self.story_npcs.dialogue.clear_active(talk.owner);
                if let Some(story) = self.story.as_mut() { story.stage.game_flags.s54 = 0; }
                let _ = self.locks.release(npc, talk.owner.player);
            }
            if talk.actor.take_frame(None, Some(self.story_npcs.dialogue.active().is_none())) { ended.push(npc); }
        }
        for npc in ended { self.observe_story_npc_talk_end(npc); }
        let Some(data) = self.data.npc.clone() else {
            return Ok(vec![]);
        };
        let Some(story) = self.story.as_ref() else {
            return Ok(vec![]);
        };
        let clock = Clock {
            hour: story.world.hour,
            minute: story.world.minute,
            dt: self.clock.seconds(),
        };
        // 0x0059e320 frees an inactive applied wrapper on the following
        // frame and returns. This runs after normal NPC stepping, so the
        // retained normal controller next runs on the subsequent frame.
        for (&npc, applied) in &mut self.story_npcs.applied {
            if applied.action.applied_finished() {
                if let Some(normal) = applied.normal.take() {
                    self.story_npcs.motion_requests.remove(&npc);
                    self.story_npcs.motions.remove(&npc);
                    self.npcs.npcs.insert(npc, normal);
                }
            }
        }
        let mut out = vec![];
        for (&npc, applied) in &mut self.story_npcs.applied {
            applied.action.observe_scene(Some(story.world.sce_no));
            match applied.action.step_observed(&*data, clock, Some(&story.world)) {
                Ok(requests) => out.extend(requests),
                Err(error) => {
                    if let Some(position) = applied.action.position() {
                        self.npc_parked.insert(npc, position);
                    }
                    // Port failure is neither actor destruction nor Finish.
                    // Keep the normal owner suspended and report the gap once.
                    applied.action.halt_graphs();
                    self.npc_errors.push((npc, format!("applied {}: {error}", applied.path)));
                }
            }
        }
        Ok(out)
    }
}

//! Bind an actual NPC intel lifetime to its headless combat body.

use crate::{Host, HostError, StoryWorld};
use crate::story_npcs::NpcLifetime;
use ddstory::{data::Data, Target};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Facts { pub npc: u32, pub hp: f32, pub max_hp: f32 }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeathState {
    pub phase: ddenemyrt::npc_death::DeathPhase,
    pub wait: f32,
    pub alpha: f32,
}

#[derive(Debug, Clone, Copy)]
struct Binding { lifetime: NpcLifetime, owner: u32, stage: u32, target: Target }

#[derive(Default)]
pub struct NpcCombat {
    bindings: BTreeMap<u32, Binding>,
    deaths: BTreeMap<u32, DeadActor>,
    region: ddenemyrt::npc_combat::Region,
}

struct DeadActor {
    lifetime: NpcLifetime,
    layer: crate::npc_motion::Actor,
    body: Arc<ddenemyrt::npc_body::NpcBody>,
    action: ddenemyrt::npc_death::DeathAction,
    intel: ddenemyrt::npc_death::CorpseIntel,
    max_hp: f32,
}

impl NpcCombat {
    pub(crate) fn owns_death(&self, npc: u32) -> bool { self.deaths.contains_key(&npc) }
}

/// Typed death-action observations. These are separate from HP positivity:
/// callers may report them only when the corresponding actor stage happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathObservation {
    /// cNpcActDead phase1 observed the actual death layer ending.
    MotionEnded,
    /// cNpcActDead phase3 completed the real wait/fade and removed the body.
    FadeFinished,
}

impl<D: Data, W: StoryWorld> Host<D, W> {
    /// Explicit session-wide region policy, default Other. A client never
    /// selects a per-player civilian protection policy for shared NPCs.
    pub fn set_npc_region(&mut self, region: ddenemyrt::npc_combat::Region) {
        self.npc_combat.region = region;
    }

    pub fn npc_region(&self) -> ddenemyrt::npc_combat::Region { self.npc_combat.region }

    /// Materialization supplies real resources, pool and current motion.
    /// Rebinding the same lifetime preserves its existing HP and owner.
    pub fn bind_story_npc_combat(&mut self, lifetime: NpcLifetime,
        body: Arc<ddenemyrt::npc_body::NpcBody>, mut region: ddenemyrt::npc_combat::RegionPool,
        motion: ddenemyrt::body::MotionId) -> Result<Option<u32>, HostError> {
        if self.story_npc_lifetime(lifetime.npc) != Some(lifetime) { return Ok(None); }
        let placement = self.story_npc_intels().find(|p| p.npc as u32 == lifetime.npc)
            .cloned().ok_or_else(|| HostError::Npc("NPC combat needs a materialized placement".into()))?;
        if let Some(binding) = self.npc_combat.bindings.get(&lifetime.npc) {
            if binding.lifetime == lifetime {
                return Ok(self.fight.as_ref().and_then(|f| f.npcs.get(&binding.owner)).map(|_| binding.owner));
            }
        }
        // New ordinary intel lifetime only. Same-lifetime rebinding above
        // retains damage and never reapplies the placement multiplier.
        // cSetInfoNpc vf08 0x007A32F0 stores enable+450/rate+430;
        // intel vf05 0x00BBC4E0 initializes HP/max before uNpc vf05
        // 0x00BABEE0 copies the linked values into its collision pool.
        let rate = match placement.record.get("HP倍率設定の有無") {
            Some(mtlot::Value::Int(0)) => None,
            Some(mtlot::Value::Int(_)) => match placement.record.get("HPの倍率") {
                Some(mtlot::Value::F32(rate)) if rate.is_finite() => Some(*rate),
                _ => return Err(HostError::Npc("NPC placement HP rate absent or nonfinite".into())),
            },
            _ => return Err(HostError::Npc("NPC placement HP rate enable absent".into())),
        };
        let mut hp = ddenemyrt::npc_combat::IntelHp { current: region.hp, maximum: region.hp_max };
        hp.initialize(0, false, rate);
        region.hp = hp.current;
        region.hp_max = hp.maximum;
        let fight = self.fight.as_mut().ok_or(HostError::NoData("fight"))?;
        if let Some(old) = self.npc_combat.bindings.remove(&lifetime.npc) {
            fight.unbind_npc(old.owner);
            self.npc_combat.deaths.remove(&lifetime.npc);
        }
        let owner = fight.bind_npc(lifetime.npc, lifetime.generation, body, region, motion);
        if let Some(npc) = fight.npcs.get_mut(&owner) {
            npc.position = placement.transform.position;
            npc.yaw = placement.transform.angle[1];
            npc.scale = placement.transform.scale;
        }
        self.npc_combat.bindings.insert(lifetime.npc, Binding { lifetime, owner,
            stage: placement.stage, target: placement.target });
        Ok(Some(owner))
    }

    pub fn story_npc_combat_owner(&self, lifetime: NpcLifetime) -> Option<u32> {
        self.npc_combat.bindings.get(&lifetime.npc).filter(|b| b.lifetime == lifetime).map(|b| b.owner)
    }

    pub fn story_npc_combat_facts(&self, lifetime: NpcLifetime) -> Option<Facts> {
        if self.story_npc_lifetime(lifetime.npc) != Some(lifetime) { return None; }
        if let Some(dead) = self.npc_combat.deaths.get(&lifetime.npc).filter(|dead|
            dead.lifetime == lifetime && dead.action.phase() == ddenemyrt::npc_death::DeathPhase::Finished) {
            return Some(Facts { npc: lifetime.npc, hp: 0.0, max_hp: dead.max_hp });
        }
        let owner = self.story_npc_combat_owner(lifetime)?;
        let npc = self.fight.as_ref()?.npcs.get(&owner)?;
        let ddenemyrt::combat::Victim::Npc { region, .. } = &npc.fighter.victim else { return None };
        Some(Facts { npc: lifetime.npc, hp: region.hp, max_hp: region.hp_max })
    }

    pub(crate) fn npc_death_motion(&self, npc: u32) -> Option<&crate::npc_motion::Actor> {
        let dead = self.npc_combat.deaths.get(&npc)?;
        (self.story_npc_lifetime(npc) == Some(dead.lifetime)).then_some(&dead.layer)
    }

    pub(crate) fn npc_death_resource(&self, npc: u32) -> Option<&mtlmt::Motion> {
        let dead = self.npc_combat.deaths.get(&npc)?;
        if self.story_npc_lifetime(npc) != Some(dead.lifetime) { return None; }
        dead.body.body.motion(ddenemyrt::body::MotionId::new((dead.layer.motion >> 8) as u8, dead.layer.motion & 255))
    }

    /// A permanent body removal leaves its intel/death banks alive. Native
    /// 0x00BB57E0 removes the ordinary body, not its shared lifetime record.
    pub fn story_npc_body_retired(&self, npc: u32) -> bool {
        self.npc_combat.deaths.get(&npc).is_some_and(|dead|
            self.story_npc_lifetime(npc) == Some(dead.lifetime)
            && dead.action.phase() == ddenemyrt::npc_death::DeathPhase::Finished)
    }

    pub fn story_npc_corpse_alpha(&self, lifetime: NpcLifetime) -> Option<f32> {
        let dead = self.npc_combat.deaths.get(&lifetime.npc)?;
        (self.story_npc_lifetime(lifetime.npc) == Some(lifetime) && dead.lifetime == lifetime)
            .then_some(dead.intel.alpha)
    }

    pub fn story_npc_death_state(&self, lifetime: NpcLifetime) -> Option<DeathState> {
        let dead = self.npc_combat.deaths.get(&lifetime.npc)?;
        (self.story_npc_lifetime(lifetime.npc) == Some(lifetime) && dead.lifetime == lifetime)
            .then_some(DeathState { phase: dead.action.phase(), wait: dead.intel.wait, alpha: dead.intel.alpha })
    }

    /// Dispatch the materialized ordinary body's actual zero HP into a
    /// per-lifetime action. Ordinary goblin damage follows NPC vf72
    /// 0x00BB7410 -> uPlayerBase vf72 0x00B7C470 (action kind4).
    /// Civilian human-attacker rescue/escape and special type14/15 overrides
    /// remain outside this opening body adapter.
    pub(crate) fn start_npc_deaths(&mut self) -> Result<(), HostError> {
        let bindings: Vec<_> = self.npc_combat.bindings.values().copied().collect();
        for binding in bindings {
            let npc = binding.lifetime.npc;
            if self.story_npc_lifetime(npc) != Some(binding.lifetime)
                || self.npc_combat.deaths.contains_key(&npc)
                || !self.story_npc_combat_facts(binding.lifetime).is_some_and(|f| f.hp <= 0.0) { continue; }
            let Some(body) = self.fight.as_ref().and_then(|f| f.npcs.get(&binding.owner)).map(|n| n.body.clone()) else { continue };
            let Some(prior) = self.story_npc_motion(npc).cloned() else {
                return Err(HostError::Npc("NPC death needs its current real motion body".into()));
            };
            // Translated ordinary branch of cNpcActDead vf05 0x00B988B0:
            // packed CC6/CC7, blend15, frame0, speed1, non-loop attributes0.
            let number = 0x0cc6 + u16::from(body.look.sex != 0);
            let resolved = body.resolve(number as i32).map_err(HostError::Npc)?;
            let resource = body.body.motion(resolved).ok_or_else(|| HostError::Npc("NPC death LMT absent".into()))?;
            let mut motion = prior.authored;
            motion.number = number as i32;
            motion.blend_frames = 15.0;
            motion.start_frame = 0.0;
            motion.speed = 1.0;
            motion.looping = false;
            // These are unused interface adapter fields for the death action,
            // which never consumes graph targeting or publishes MotionDone.
            motion.target = false;
            let token = self.npc_death_token(binding.lifetime)?;
            let layer = crate::npc_motion::Actor::new(token, motion,
                (u16::from(resolved.bank) << 8) | resolved.slot, resource).map_err(HostError::Npc)?;
            let apparent = self.story.as_ref().is_some_and(|s| s.world.npc_app_die.get(npc as usize));
            let max_hp = self.story_npc_combat_facts(binding.lifetime).unwrap().max_hp;
            self.npc_combat.deaths.insert(npc, DeadActor { lifetime: binding.lifetime,
                layer, body, action: ddenemyrt::npc_death::DeathAction::new(apparent),
                intel: ddenemyrt::npc_death::CorpseIntel::new(crate::clock::BASE_FPS).map_err(HostError::Npc)?, max_hp });
            // Death owns the actor without completing its previous graph.
            self.npcs.npcs.remove(&npc);
        }
        Ok(())
    }

    /// Native uNpcIntel is on line12; its clock precedes the actor on line15.
    pub(crate) fn step_npc_death_intels(&mut self) -> Result<(), HostError> {
        let reports: Vec<_> = self.npc_combat.deaths.keys().copied().filter_map(|npc| {
            if self.story_npc_lifetime(npc) != Some(self.npc_combat.deaths[&npc].lifetime) { return None; }
            let apparent = self.story.as_ref().is_some_and(|s| s.world.npc_app_die.get(npc as usize));
            let permanent = self.story.as_ref().is_some_and(|s| s.world.npc_die.get(npc as usize));
            Some((npc, apparent, permanent, self.story_npc_intel_flags(npc).unwrap_or(0)))
        }).collect();
        for (npc, apparent, permanent, flags) in reports {
            self.npc_combat.deaths.get_mut(&npc).unwrap().intel.advance(apparent, permanent,
                flags, self.clock.delta).map_err(HostError::Npc)?;
        }
        Ok(())
    }

    pub(crate) fn step_npc_death_actors(&mut self) -> Result<(), HostError> {
        let flags: Vec<_> = self.npc_combat.deaths.keys().copied().filter_map(|npc|
            (self.story_npc_lifetime(npc) == Some(self.npc_combat.deaths[&npc].lifetime))
                .then(|| (npc, self.story_npc_intel_flags(npc).unwrap_or(0)))).collect();
        for (npc, flags) in flags {
            let dead = self.npc_combat.deaths.get_mut(&npc).unwrap();
            if dead.action.phase() == ddenemyrt::npc_death::DeathPhase::Finished { continue; }
            dead.layer.advance(self.clock.delta).map_err(HostError::Npc)?;
            // Positive non-loop layer uses terminal flag4 (0x01060730).
            // Native 0x01057100 also accepts disabled flag1; unsupported
            // missing/replaced layers are not fabricated as completion here.
            let effects = dead.action.advance(dead.layer.observation().action_terminal,
                flags, Some(&mut dead.intel), self.clock.delta).map_err(HostError::Npc)?;
            if let Some(story) = self.story.as_mut() {
                if effects.apparent { story.world.npc_app_die.set(npc as usize, true); }
                if effects.permanent { story.world.npc_app_die.set(npc as usize, true); story.world.npc_die.set(npc as usize, true); }
            }
            if effects.permanent {
                if let Some(binding) = self.npc_combat.bindings.get(&npc) {
                    if let Some(fight) = self.fight.as_mut() { fight.unbind_npc(binding.owner); }
                }
            }
        }
        Ok(())
    }

    // Translated from cNpcActDead::vf06 0x00B98AE0, apparent setter
    // 0x00BB8B10 -> 0x00437670, and final 0x00BB57E0 -> 0x00446460
    // (DDDA.exe 2017-12-13 Steam).
    pub fn observe_story_npc_death(&mut self, lifetime: NpcLifetime,
        observation: DeathObservation) -> bool {
        if !self.story_npc_combat_facts(lifetime).is_some_and(|f| f.hp <= 0.0) { return false; }
        let holds_corpse = self.story_npc_intel_flags(lifetime.npc).is_some_and(|f| f & 0x0040_0000 != 0);
        let Some(story) = self.story.as_mut() else { return false };
        match observation {
            DeathObservation::MotionEnded => { story.world.npc_app_die.set(lifetime.npc as usize, true); }
            DeathObservation::FadeFinished => {
                // The ordinary NPC action does not reach this path while
                // canonical bit0x400000 holds the corpse in phase2.
                if holds_corpse { return false; }
                story.world.npc_app_die.set(lifetime.npc as usize, true);
                story.world.npc_die.set(lifetime.npc as usize, true);
            }
        }
        true
    }

    pub(crate) fn observe_npc_combat(&mut self) {
        let reports: Vec<_> = self.npc_combat.bindings.values().filter_map(|b|
            self.story_npc_combat_facts(b.lifetime).map(|facts| (b.stage, b.target, facts))).collect();
        for (stage, target, facts) in reports { self.world.observe_npc_combat(stage, target, facts); }
    }

    /// Sync after S32 action/motion advancement, before the next hit pass.
    /// The current placement yaw is retained; motion root travel and target
    /// yaw/ground integration remain explicit S32 actor-system limitations.
    pub(crate) fn sync_npc_combat(&mut self) {
        let bindings: Vec<_> = self.npc_combat.bindings.values().copied().collect();
        for binding in bindings {
            let lifetime = self.story_npc_lifetime(binding.lifetime.npc);
            if lifetime != Some(binding.lifetime) {
                if let Some(fight) = self.fight.as_mut() { fight.unbind_npc(binding.owner); }
                self.npc_combat.bindings.remove(&binding.lifetime.npc);
                self.npc_combat.deaths.remove(&binding.lifetime.npc);
                continue;
            }
            let position = self.story_npc_position(binding.lifetime.npc);
            let flags = self.story_npc_intel_flags(binding.lifetime.npc);
            let actor = self.story_npc_motion(binding.lifetime.npc)
                .filter(|a| a.token.owner == binding.lifetime)
                .map(|a| (a.motion, a.frame));
            let Some(fight) = self.fight.as_mut() else { continue };
            let Some(npc) = fight.npcs.get_mut(&binding.owner) else { continue };
            if let Some(position) = position { npc.position = position; }
            if let Some(flags) = flags { npc.set_flags(flags); }
            if let Some((motion, frame)) = actor {
                npc.motion = ddenemyrt::body::MotionId::new((motion >> 8) as u8, motion & 255);
                npc.frame = frame;
                match npc.volumes() {
                    Ok(()) => { fight.npc_errors.remove(&binding.owner); }
                    Err(error) => { fight.npc_errors.insert(binding.owner, error); }
                }
            } else {
                npc.fighter.hurts.clear();
                fight.npc_errors.insert(binding.owner, "NPC canonical motion body/action is not bound".into());
            }
            fight.positions.insert(binding.owner, npc.position);
        }
    }
}

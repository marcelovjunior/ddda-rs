//! Bind an actual NPC intel lifetime to its headless combat body.

use crate::{Host, HostError, StoryWorld};
use crate::story_npcs::NpcLifetime;
use ddstory::{data::Data, Target};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Facts { pub npc: u32, pub hp: f32, pub max_hp: f32 }

#[derive(Debug, Clone, Copy)]
struct Binding { lifetime: NpcLifetime, owner: u32, stage: u32, target: Target }

#[derive(Default)]
pub struct NpcCombat { bindings: BTreeMap<u32, Binding> }

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
    /// Materialization supplies real resources, pool and current motion.
    /// Rebinding the same lifetime preserves its existing HP and owner.
    pub fn bind_story_npc_combat(&mut self, lifetime: NpcLifetime,
        body: Arc<ddenemyrt::npc_body::NpcBody>, region: ddenemyrt::npc_combat::RegionPool,
        motion: ddenemyrt::body::MotionId) -> Result<Option<u32>, HostError> {
        if self.story_npc_lifetime(lifetime.npc) != Some(lifetime) { return Ok(None); }
        let placement = self.story_npc_intels().find(|p| p.npc as u32 == lifetime.npc)
            .cloned().ok_or_else(|| HostError::Npc("NPC combat needs a materialized placement".into()))?;
        if let Some(binding) = self.npc_combat.bindings.get(&lifetime.npc) {
            if binding.lifetime == lifetime {
                return Ok(self.fight.as_ref().and_then(|f| f.npcs.get(&binding.owner)).map(|_| binding.owner));
            }
        }
        let fight = self.fight.as_mut().ok_or(HostError::NoData("fight"))?;
        if let Some(old) = self.npc_combat.bindings.remove(&lifetime.npc) { fight.unbind_npc(old.owner); }
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
        let owner = self.story_npc_combat_owner(lifetime)?;
        let npc = self.fight.as_ref()?.npcs.get(&owner)?;
        let ddenemyrt::combat::Victim::Npc { region, .. } = &npc.fighter.victim else { return None };
        Some(Facts { npc: lifetime.npc, hp: region.hp, max_hp: region.hp_max })
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

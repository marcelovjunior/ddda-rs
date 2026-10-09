//! Explicit host-owned facts for quest and stage scripts.
//!
//! Unlike `ScriptedHost`, this adapter never advances combat, completes
//! applied scripts, or assumes a layout has loaded. Simulation and
//! streaming adapters report facts with their stage and placement keys.
//! Unreported facts are absent. Group completion is a separate report:
//! it cannot be inferred from an empty set of currently loaded enemies.

use crate::{Host, NpcIntel, Target};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UnitFacts {
    pub layout_loaded: bool,
    /// A live unit resolves through sSetManager, even if its HP is zero.
    pub present: bool,
    /// Persistent placement death bit (0x004a6510), independent of loading.
    pub killed: bool,
    pub hp: Option<f32>,
    pub max_hp: Option<f32>,
    pub npc: Option<NpcIntel>,
    /// The host finished the ApplyFSM task, not a guessed duration.
    pub applied_script_finished: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GroupFacts {
    pub loaded: bool,
    /// sSetManager active group membership (0x004a52b0).
    pub active: bool,
    /// cLayoutSet group completion byte +0x160 (0x004a52e0).
    pub done: bool,
}

#[derive(Debug, Default, Clone)]
pub struct EncounterFacts {
    units: HashMap<(u32, Target), UnitFacts>,
    groups: HashMap<(u32, u32, u32), GroupFacts>,
    /// NPC ids of active units, supplied by the NPC runtime.
    pub active_npcs: HashSet<u32>,
    pub tutorials: bool,
    /// The original Host trait's Arisen observations. The session adapter
    /// selects/aggregates seats; this registry never owns a global player.
    pub player_checks: HashMap<u32, i32>,
    pub damaged_this_frame: bool,
    pub lost_pawn: bool,
    pub items: HashMap<u32, u32>,
}

impl EncounterFacts {
    pub fn report_unit(&mut self, stage: u32, target: Target, facts: UnitFacts) {
        self.units.insert((stage, target), facts);
    }
    pub fn unit(&self, stage: u32, target: &Target) -> UnitFacts {
        self.units.get(&(stage, *target)).copied().unwrap_or_default()
    }
    pub fn report_group(&mut self, stage: u32, kind: u32, group: u32, facts: GroupFacts) {
        self.groups.insert((stage, kind, group), facts);
    }
    pub fn group(&self, stage: u32, kind: u32, group: u32) -> GroupFacts {
        self.groups
            .get(&(stage, kind, group))
            .copied()
            .unwrap_or_default()
    }
}

impl Host for EncounterFacts {
    fn layout_loaded(&self, stage: u32, target: &Target) -> bool {
        self.unit(stage, target).layout_loaded
    }
    fn unit_exists(&self, stage: u32, target: &Target) -> bool {
        self.unit(stage, target).present
    }
    fn group_loaded(&self, stage: u32, kind: u32, group: u32) -> bool {
        self.group(stage, kind, group).loaded
    }
    fn enemy_group_done(&self, stage: u32, group: u32) -> bool {
        self.group(stage, 2, group).done
    }
    fn enemy_group_active(&self, stage: u32, group: u32) -> bool {
        self.group(stage, 2, group).active
    }
    // Translated from 0x005bf3d0 (DDDA.exe 2017-12-13 Steam).
    fn enemy_check(&self, stage: u32, target: &Target, flag: u32) -> bool {
        let u = self.unit(stage, target);
        match flag {
            0x1000 => true,
            0 => u.present,
            1 => u.killed,
            2 => u.present && u.hp.is_some_and(|hp| hp > 0.0),
            3 => {
                u.killed
                    || (u.present && u.max_hp.is_some_and(|hp| hp > 0.0) && u.hp.is_some_and(|hp| hp <= 0.0))
            }
            _ => false,
        }
    }
    fn npc_intel(&self, stage: u32, target: &Target) -> Option<NpcIntel> {
        self.unit(stage, target).npc
    }
    fn npc_unit(&self, npc: u32) -> bool {
        self.active_npcs.contains(&npc)
    }
    // Translated from 0x005c8460 (DDDA.exe 2017-12-13 Steam).
    // An unloaded NPC/object is not completion; missing party members
    // and enemies with persistent death records are explicit exceptions.
    fn applied_script_finished(&self, stage: u32, target: &Target) -> bool {
        let u = self.unit(stage, target);
        (u.present && u.applied_script_finished)
            || (target.kind == 1 && !u.present)
            || (target.kind == 2 && u.killed)
    }
    fn unit_hp(&self, stage: u32, target: &Target, percent: bool) -> Option<f32> {
        let u = self.unit(stage, target);
        if !u.present {
            return None;
        }
        let hp = u.hp?;
        if percent {
            let max = u.max_hp?;
            (max > 0.0).then_some(hp / max * 100.0)
        } else {
            Some(hp)
        }
    }
    fn tutorials_enabled(&self) -> bool {
        self.tutorials
    }
    fn pl_check(&self, kind: u32) -> i32 {
        self.player_checks.get(&kind).copied().unwrap_or(0)
    }
    fn player_damaged(&self) -> bool {
        self.damaged_this_frame
    }
    fn pawn_lost(&self) -> bool {
        self.lost_pawn
    }
    fn has_item(&self, item: u32, count: u32) -> bool {
        self.items.get(&item).copied().unwrap_or(0) >= count
    }
}

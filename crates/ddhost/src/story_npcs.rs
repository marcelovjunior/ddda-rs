//! Explicit placement bindings for applied NPC action graphs. Actor movement,
//! guard eligibility and dialogue remain observations from their real systems.

use crate::{Host, HostError, StoryWorld};
use ddnpcrt::schedule::{Clock, Npc, Request};
use ddstory::{data::Data, Target};
use std::collections::{BTreeMap, HashMap};

struct Applied {
    stage: u32,
    target: Target,
    path: String,
    action: Npc,
    normal: Option<Npc>,
}

#[derive(Default)]
pub struct StoryNpcs {
    bindings: HashMap<(u32, Target), u32>,
    pending: HashMap<(u32, Target), String>,
    normal_controllers: HashMap<(u32, Target), bool>,
    applied: BTreeMap<u32, Applied>,
}

impl<D: Data, W: StoryWorld> Host<D, W> {
    /// The layout adapter supplies identity; no target/NPC-number heuristic.
    pub fn bind_story_npc(&mut self, stage: u32, target: Target, npc: u32) {
        assert_eq!(target.kind, 5, "NPC binding requires an NPC target");
        if self.story_npcs.bindings.insert((stage, target), npc) != Some(npc) {
            self.story_npcs.normal_controllers.remove(&(stage, target));
        }
    }

    pub fn unbind_story_npc(&mut self, stage: u32, target: Target) {
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

    pub fn story_npc_arrived(&mut self, npc: u32, position: [f32; 3]) {
        if let Some(applied) = self.story_npcs.applied.get_mut(&npc) {
            applied.action.done(Some(position));
        }
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
                    self.npcs.npcs.insert(npc, normal);
                }
            }
        }
        let mut out = vec![];
        for applied in self.story_npcs.applied.values_mut() {
            out.extend(
                applied
                    .action
                    .step(&*data, clock)
                    .map_err(|e| HostError::Npc(e.to_string()))?,
            );
        }
        Ok(out)
    }
}

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
    /// Intels allocated by the real placement adapter, distinct from
    /// externally supplied fixture bindings/observations.
    intels: HashMap<(u32, Target), crate::npc_placements::Placement>,
    bindings: HashMap<(u32, Target), u32>,
    pending: HashMap<(u32, Target), String>,
    normal_controllers: HashMap<(u32, Target), bool>,
    applied: BTreeMap<u32, Applied>,
}

impl<D: Data, W: StoryWorld> Host<D, W> {
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
            self.npcs.npcs.remove(&(intel.npc as u32));
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
        for (&npc, applied) in &mut self.story_npcs.applied {
            match applied.action.step(&*data, clock) {
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

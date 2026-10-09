//! A driver for the NPC FSM graphs (`cAIConsiderFSM_24Schedule` daily
//! controllers and `cAIActionFSMNpc` / `cAIActionFSMDemo` action graphs).
//!
//! It follows the AI core step `0x00e092a0` as ddstory translates it for
//! cFSMOrder scripts (`ddstory::script`): per level, transitions are
//! checked first (`0x00e06710`: from-all entries in state order, skipping
//! the current state, then the current state's enabled links; a link to a
//! missing state stops the search), a taken transition enters the new
//! state (its sub-machine restarts at its initial state) and runs the
//! processes' enter callbacks, then every process's state callback runs.
//! Not modelled here: state `mSetting` bits (check-after, re-run, once
//! states) and the level timer; NPC graphs were not seen to need them, but
//! this is **unverified**.

use ddstory::cond::{self, Vars};
use ddstory::Error;
use mtfsm::{Cluster, Graph, Process, State};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Runner {
    pub path: String,
    graph: Arc<Graph>,
    /// Current state id per level, root first.
    levels: Vec<u32>,
    started: bool,
}

/// What a graph's processes do on entry and per frame.
pub trait Processes {
    fn enter(&mut self, p: &Process) -> Result<(), Error>;
    fn state(&mut self, p: &Process) -> Result<(), Error>;
    /// Called when any state is entered (before its processes).
    fn state_changed(&mut self);
}

fn cluster_at<'g>(g: &'g Graph, levels: &[u32], k: usize) -> Option<&'g Cluster> {
    let mut c = g.root.as_ref()?;
    for &s in &levels[..k] {
        c = c.state(s)?.subcluster.as_deref()?;
    }
    Some(c)
}

impl Runner {
    pub fn new(path: &str, graph: Arc<Graph>) -> Result<Runner, Error> {
        let root = graph.root.as_ref().ok_or_else(|| Error::Data(format!("{path}: no root machine")))?;
        let mut levels = vec![root.initial_state];
        let mut c: &Cluster = root;
        while let Some(sub) = c.state(*levels.last().unwrap()).and_then(|s| s.subcluster.as_deref()) {
            levels.push(sub.initial_state);
            c = sub;
        }
        Ok(Runner {
            path: path.to_string(),
            graph,
            levels,
            started: false,
        })
    }

    pub fn states(&self) -> &[u32] {
        &self.levels
    }

    pub fn state_names(&self) -> Vec<String> {
        (0..self.levels.len())
            .filter_map(|k| {
                cluster_at(&self.graph, &self.levels, k)
                    .and_then(|c| c.state(self.levels[k]))
                    .map(|s| String::from_utf8_lossy(&s.name).into_owned())
            })
            .collect()
    }

    fn condition(&self, id: u32, vars: &dyn Vars) -> Result<bool, Error> {
        match self.graph.condition(id).and_then(|c| c.root.as_ref()) {
            Some(e) => cond::eval(e, vars),
            None => Ok(false),
        }
    }

    fn transition(&self, k: usize, vars: &dyn Vars) -> Result<Option<u32>, Error> {
        let c = cluster_at(&self.graph, &self.levels, k).ok_or_else(|| Error::Data("level without machine".into()))?;
        let cur = self.levels[k];
        for s in &c.states {
            let Some(cid) = s.from_all else { continue };
            if s.id != cur && self.condition(cid, vars)? {
                return Ok(Some(s.id));
            }
        }
        if let Some(st) = c.state(cur) {
            for l in st.links.iter().filter(|l| l.enabled) {
                if self.condition(l.condition, vars)? {
                    return Ok(c.state(l.destination).map(|s| s.id));
                }
            }
        }
        Ok(None)
    }

    fn state_at(&self, k: usize) -> Option<&State> {
        cluster_at(&self.graph, &self.levels, k).and_then(|c| c.state(self.levels[k]))
    }

    fn enter_level(&mut self, k: usize, p: &mut dyn Processes) -> Result<(), Error> {
        p.state_changed();
        let g = self.graph.clone();
        if let Some(st) = cluster_at(&g, &self.levels, k).and_then(|c| c.state(self.levels[k])) {
            for pr in &st.processes {
                p.enter(pr)?;
            }
        }
        Ok(())
    }

    /// One frame. `vars` is read for conditions before any process runs;
    /// processes report through `p`.
    pub fn step(&mut self, vars: &dyn Vars, p: &mut dyn Processes) -> Result<(), Error> {
        let mut k = 0;
        while k < self.levels.len() {
            let entered = if !self.started {
                true
            } else if let Some(next) = self.transition(k, vars)? {
                self.levels.truncate(k + 1);
                self.levels[k] = next;
                let mut c = cluster_at(&self.graph, &self.levels, k).cloned();
                // Restart the sub-machines below the new state.
                while let Some(sub) = c.as_ref().and_then(|c| c.state(*self.levels.last().unwrap())).and_then(|s| s.subcluster.clone()) {
                    self.levels.push(sub.initial_state);
                    c = Some(*sub);
                }
                true
            } else {
                false
            };
            if entered {
                // A new state at level k also (re)enters its sub-levels.
                for j in k..self.levels.len() {
                    self.enter_level(j, p)?;
                }
                self.started = true;
                let g = self.graph.clone();
                for j in k..self.levels.len() {
                    if let Some(st) = cluster_at(&g, &self.levels, j).and_then(|c| c.state(self.levels[j])) {
                        for pr in &st.processes {
                            p.state(pr)?;
                        }
                    }
                }
                return Ok(());
            }
            let g = self.graph.clone();
            if let Some(st) = cluster_at(&g, &self.levels, k).and_then(|c| c.state(self.levels[k])) {
                for pr in &st.processes {
                    p.state(pr)?;
                }
            }
            k += 1;
        }
        let _ = self.state_at(0);
        Ok(())
    }
}

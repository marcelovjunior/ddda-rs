//! The AI core: one running `rAIFSM` graph, its levels and their states.
//! Ported from `ddstory::script` (claude-7) with the owner made generic.
//!
//! The driver translates the core step 0x00e092a0. Levels (the root
//! machine, then the sub-machine of its current state, ...) are processed in
//! order each frame; per level:
//!
//! 1. unless the state's `mSetting` bit 1 is set, check transitions first
//!    (0x00e06710: transition-from-all entries, then links, 0x00e05800);
//! 2. if a transition is pending, the level is new, or the core runs for
//!    the first time: exit the deeper levels (0x00e06800), enter the next
//!    state (starting its sub-machine, 0x00e06a50 / 0x00e06af0), call the
//!    owner's state-change hook (`cBaseFSM` vf05) and every process's
//!    "update" callback (record +0x50);
//! 3. call every process's "state" callback (+0x54);
//! 4. with `mSetting` bit 1, check transitions now; with bit 2 (or bit 4
//!    after a transition) run this level again at once.
//!
//! The runtime core attribute (+0xa8) is 3 when a core is made (0x00e05797)
//! and nothing in the core rewrites it for these owners. The cluster timer
//! (+0x18/+0x1c) that 0x00e06710 honours is never armed by the processes
//! implemented here and is not modelled. Graphs whose `mFSMAttribute` has
//! bit 2 get per-cluster work memory (0x00e09750 -> 0x00e06e00 /
//! 0x00e087e0); that only matters for sub-machines, so such graphs with a
//! sub-machine are refused as unimplemented.

use crate::cond::{self, Value, Var, Vars};
use crate::Error;
use mtfsm::{Cluster, Graph, Process};
use std::collections::HashMap;
use std::sync::Arc;

/// Per-process scratch memory for owners that keep state between frames
/// (cleared on every entry of the state).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProcMem {
    pub step: u32,
    pub value: i64,
    pub time: f32,
}

/// Where a process sits, for messages.
#[derive(Debug, Clone, Copy)]
pub struct ProcAt<'a> {
    pub graph: &'a str,
    pub state_name: &'a [u8],
    pub index: usize,
}

/// The object that owns a core (a `cBaseFSM` subclass).
pub trait Owner {
    /// Whatever the owner reads and writes outside itself (inputs,
    /// requests, world state).
    type Env<'e>;
    /// `cBaseFSM` vf05, run on every state change.
    fn state_changed(&mut self, env: &mut Self::Env<'_>);
    /// A reflected property read by a condition.
    fn var(&self, env: &Self::Env<'_>, v: &Var) -> Result<Value, Error>;
    /// A process's "update" callback (state entry).
    fn update(&mut self, env: &mut Self::Env<'_>, p: &Process, mem: &mut ProcMem, at: ProcAt) -> Result<(), Error>;
    /// A process's "state" callback (every frame).
    fn state(&mut self, env: &mut Self::Env<'_>, p: &Process, mem: &mut ProcMem, at: ProcAt) -> Result<(), Error>;
}

#[derive(Debug, Clone)]
struct Level {
    current: u32,
    next: Option<u32>,
    go: bool,
    came_by_entry: bool,
    /// +0x16: the first transition check after the level starts is skipped.
    skip_check: bool,
    /// +0x17: the enter step is due.
    pending_enter: bool,
    /// Unique ids of from-all states with `mSetting` bit 8 already taken.
    once: Vec<u32>,
}

/// Bound on reruns of one level in one frame (see `Core::step`).
pub const MAX_ROUNDS: u32 = 1000;

/// One running graph.
pub struct Core {
    pub path: String,
    graph: Arc<Graph>,
    levels: Vec<Level>,
    first: bool,
    mem: HashMap<(u32, usize), ProcMem>,
    /// Runtime core attribute (+0xa8).
    attribute: u32,
}

struct VarCtx<'a, 'e, O: Owner> {
    owner: &'a O,
    env: &'a O::Env<'e>,
}

impl<O: Owner> Vars for VarCtx<'_, '_, O> {
    fn var(&self, v: &Var) -> Result<Value, Error> {
        self.owner.var(self.env, v)
    }
}

fn cluster_of<'g>(g: &'g Graph, levels: &[Level], k: usize) -> Option<&'g Cluster> {
    let mut c = g.root.as_ref()?;
    for l in &levels[..k] {
        c = c.state(l.current)?.subcluster.as_deref()?;
    }
    Some(c)
}

fn has_sub(c: &Cluster) -> bool {
    c.states.iter().any(|s| s.subcluster.is_some())
}

impl Core {
    pub fn new(path: &str, graph: Arc<Graph>) -> Result<Core, Error> {
        let root = graph.root.as_ref().ok_or_else(|| Error::Data(format!("{path}: no root machine")))?;
        if graph.attribute & 2 != 0 && has_sub(root) {
            return Err(Error::Unimplemented(format!("{path}: per-cluster work (mFSMAttribute bit 2) with sub-machines")));
        }
        let mut s = Core {
            path: path.to_string(),
            graph: graph.clone(),
            levels: vec![],
            first: true,
            mem: HashMap::new(),
            attribute: 3,
        };
        s.start_level(0, root.initial_state, root)?;
        Ok(s)
    }

    pub fn graph(&self) -> &Arc<Graph> {
        &self.graph
    }

    /// 0x00e06a50 + 0x00e06af0: start level `k` at `state`, recursively
    /// starting the sub-machines of initial states.
    fn start_level(&mut self, k: usize, state: u32, cluster: &Cluster) -> Result<(), Error> {
        self.levels.truncate(k);
        let st = cluster
            .state(state)
            .ok_or_else(|| Error::Data(format!("{}: initial state {state} absent", self.path)))?;
        self.levels.push(Level {
            current: state,
            next: None,
            go: false,
            came_by_entry: false,
            skip_check: true,
            pending_enter: true,
            once: vec![],
        });
        if let Some(sub) = &st.subcluster {
            self.start_level(k + 1, sub.initial_state, sub)?;
        }
        Ok(())
    }

    /// Current state ids, root first.
    pub fn current_states(&self) -> Vec<u32> {
        self.levels.iter().map(|l| l.current).collect()
    }

    /// Names of the current states, root first.
    pub fn current_names(&self) -> Vec<String> {
        let g = self.graph.clone();
        (0..self.levels.len())
            .filter_map(|k| {
                let c = cluster_of(&g, &self.levels, k)?;
                c.state(self.levels[k].current).map(|s| String::from_utf8_lossy(&s.name).into_owned())
            })
            .collect()
    }

    fn condition<O: Owner>(&self, owner: &O, env: &O::Env<'_>, id: u32) -> Result<bool, Error> {
        // 0x0117aaa0: first tree with the id; an absent id is false.
        match self.graph.condition(id).and_then(|c| c.root.as_ref()) {
            Some(e) => cond::eval(e, &VarCtx { owner, env }),
            None => Ok(false),
        }
    }

    // Translated from 0x00e06710 and 0x00e05800 (DDDA.exe 2017-12-13 Steam).
    fn check_transitions<O: Owner>(&mut self, owner: &O, env: &O::Env<'_>, k: usize) -> Result<(), Error> {
        let g = self.graph.clone();
        let c = cluster_of(&g, &self.levels, k).ok_or_else(|| Error::Data("level without machine".into()))?;
        self.levels[k].came_by_entry = false;
        let mut found: Option<u32> = None;
        if self.attribute & 2 != 0 {
            for s in &c.states {
                let Some(cid) = s.from_all else { continue };
                if s.id == self.levels[k].current {
                    continue;
                }
                if s.setting & 8 != 0 && self.levels[k].once.contains(&s.unique_id) {
                    continue;
                }
                if self.condition(owner, env, cid)? {
                    self.levels[k].came_by_entry = true;
                    found = Some(s.id);
                    break;
                }
            }
        }
        if found.is_none() && self.attribute & 1 != 0 {
            if let Some(cur) = c.state(self.levels[k].current) {
                for l in &cur.links {
                    if !l.enabled {
                        continue;
                    }
                    if self.condition(owner, env, l.condition)? {
                        // 0x010c9290: the first state with that id; none =
                        // no transition, and later links are not tried.
                        found = c.state(l.destination).map(|s| s.id);
                        break;
                    }
                }
            }
        }
        let lv = &mut self.levels[k];
        if !lv.skip_check {
            if let Some(n) = found {
                lv.next = Some(n);
                lv.go = true;
            } else {
                lv.go = false;
            }
        }
        lv.skip_check = false;
        Ok(())
    }

    fn run_callbacks<O: Owner>(&mut self, owner: &mut O, env: &mut O::Env<'_>, k: usize, enter: bool) -> Result<(), Error> {
        let g = self.graph.clone();
        let c = cluster_of(&g, &self.levels, k).ok_or_else(|| Error::Data("level without machine".into()))?;
        let st = c.state(self.levels[k].current).ok_or_else(|| Error::Data("no current state".into()))?;
        for (i, p) in st.processes.iter().enumerate() {
            let mem = self.mem.entry((st.unique_id, i)).or_default();
            if enter {
                *mem = ProcMem::default();
            }
            let at = ProcAt { graph: &self.path, state_name: &st.name, index: i };
            if enter {
                owner.update(env, p, mem, at)?;
            } else {
                owner.state(env, p, mem, at)?;
            }
        }
        Ok(())
    }

    /// One frame of the core (0x00e092a0).
    pub fn step<O: Owner>(&mut self, owner: &mut O, env: &mut O::Env<'_>) -> Result<(), Error> {
        let g = self.graph.clone();
        let mut k = 0;
        while k < self.levels.len() {
            let mut rounds = 0u32;
            loop {
                // The original loops here until a transition fires (a state
                // with mSetting bit 2 and no true link would hang the game);
                // a bound turns that into an error instead.
                rounds += 1;
                if rounds > MAX_ROUNDS {
                    return Err(Error::Data(format!(
                        "{}: level {k} state {} reran {MAX_ROUNDS} times without a transition",
                        self.path, self.levels[k].current
                    )));
                }
                let c = cluster_of(&g, &self.levels, k).ok_or_else(|| Error::Data("level without machine".into()))?;
                let setting = c.state(self.levels[k].current).map(|s| s.setting).unwrap_or(0);
                if setting & 1 == 0 {
                    self.check_transitions(owner, env, k)?;
                }
                let went = self.levels[k].go;
                if went || self.levels[k].pending_enter || self.first {
                    if let Some(next) = self.levels[k].next.take() {
                        // 0x00e06800 runs "exit" callbacks (none registered
                        // by these owners) of this level and deeper ones.
                        self.levels.truncate(k + 1);
                        self.levels[k].current = next;
                        let c = cluster_of(&g, &self.levels, k).unwrap();
                        if let Some(sub) = c.state(next).and_then(|s| s.subcluster.as_deref()) {
                            self.start_level(k + 1, sub.initial_state, sub)?;
                        }
                    }
                    owner.state_changed(env);
                    self.run_callbacks(owner, env, k, true)?;
                    self.levels[k].pending_enter = false;
                    if self.levels[k].came_by_entry {
                        let c = cluster_of(&g, &self.levels, k).unwrap();
                        if let Some(s) = c.state(self.levels[k].current) {
                            if s.setting & 8 != 0 {
                                let uid = s.unique_id;
                                self.levels[k].once.push(uid);
                            }
                        }
                    }
                }
                self.levels[k].go = false;
                self.run_callbacks(owner, env, k, false)?;
                let c = cluster_of(&g, &self.levels, k).ok_or_else(|| Error::Data("level without machine".into()))?;
                let setting = c.state(self.levels[k].current).map(|s| s.setting).unwrap_or(0);
                if setting & 1 != 0 {
                    self.check_transitions(owner, env, k)?;
                }
                let dl = self.levels[k].go || went;
                let again = setting & 2 != 0 || (setting & 4 != 0 && dl);
                if !again {
                    break;
                }
            }
            k += 1;
        }
        self.first = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtfsm::{Condition, Expression, Link, Record, State};

    #[derive(Default)]
    struct Log {
        entered: Vec<String>,
        changes: u32,
    }

    impl Owner for Log {
        type Env<'e> = ();
        fn state_changed(&mut self, _: &mut ()) {
            self.changes += 1;
        }
        fn var(&self, _: &(), v: &Var) -> Result<Value, Error> {
            Err(Error::Unimplemented(v.property.to_string()))
        }
        fn update(&mut self, _: &mut (), p: &Process, _: &mut ProcMem, _: ProcAt) -> Result<(), Error> {
            self.entered.push(String::from_utf8_lossy(&p.container).into_owned());
            Ok(())
        }
        fn state(&mut self, _: &mut (), _: &Process, _: &mut ProcMem, _: ProcAt) -> Result<(), Error> {
            Ok(())
        }
    }

    fn state(id: u32, setting: u32, links: &[(u32, u32)], from_all: Option<u32>) -> State {
        State {
            id,
            unique_id: 100 + id,
            owner_id: 0,
            name: format!("s{id}").into_bytes(),
            setting,
            user_attribute: 0,
            from_all,
            links: links
                .iter()
                .map(|&(d, c)| Link { name: vec![], destination: d, enabled: true, condition: c })
                .collect(),
            processes: vec![Process { container: format!("enter{id}").into_bytes(), category: vec![], parameter: None }],
            subcluster: None,
        }
    }

    fn graph(states: Vec<State>) -> Arc<Graph> {
        // Condition 0: an operation without operands (true, 0x0117c040).
        let empty = Expression {
            record: Record { class_hash: 0, class_name: Some("rAIConditionTree::OperationNode"), fields: vec![] },
            operator: Some(0),
            children: vec![],
        };
        Arc::new(Graph {
            owner: b"test".to_vec(),
            attribute: 0,
            root: Some(Cluster { id: 0, owner_node_unique_id: 0, initial_state: 0, states }),
            conditions: vec![Condition { id: 0, name: vec![], root: Some(empty) }],
        })
    }

    #[test]
    fn transitions_wait_one_frame_and_absent_conditions_are_false() {
        // 0 -> 1 through condition 0 (true); 1 -> 2 through condition 9 (absent).
        let g = graph(vec![state(0, 0, &[(1, 0)], None), state(1, 0, &[(2, 9)], None), state(2, 0, &[], None)]);
        let mut core = Core::new("t", g).unwrap();
        let mut o = Log::default();
        core.step(&mut o, &mut ()).unwrap();
        // The first check after a level starts is skipped.
        assert_eq!((core.current_states(), o.entered.clone()), (vec![0], vec!["enter0".to_string()]));
        core.step(&mut o, &mut ()).unwrap();
        assert_eq!(core.current_states(), [1]);
        for _ in 0..3 {
            core.step(&mut o, &mut ()).unwrap();
        }
        assert_eq!(core.current_states(), [1]);
        assert_eq!(o.changes, 2);
    }

    #[test]
    fn from_all_wins_and_rerun_states_are_bounded() {
        // State 2 can be entered from anywhere (condition 0); state 1 has
        // mSetting 2 (rerun in the same frame) and no true link.
        let g = graph(vec![state(0, 0, &[(1, 0)], None), state(1, 2, &[(0, 9)], None), state(2, 0, &[], Some(0))]);
        let mut core = Core::new("t", g).unwrap();
        let mut o = Log::default();
        core.step(&mut o, &mut ()).unwrap();
        core.step(&mut o, &mut ()).unwrap();
        // The from-all entry (core attribute 3, bit 2) is checked before links.
        assert_eq!(core.current_states(), [2]);
        let g = graph(vec![state(0, 2, &[(1, 9)], None)]);
        let mut core = Core::new("t", g).unwrap();
        let e = core.step(&mut o, &mut ()).unwrap_err();
        assert!(matches!(e, Error::Data(ref s) if s.contains("without a transition")), "{e}");
    }
}

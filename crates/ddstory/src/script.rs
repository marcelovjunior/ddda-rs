//! One running cFSMOrder script: the state machine driver and the owner's
//! registers.
//!
//! The driver translates the AI core step 0x00e092a0 for one core:
//! levels (root machine, then the sub-machine of its current state, ...)
//! are processed in order each frame; per level:
//!
//! 1. unless the state's `mSetting` bit 1 is set, check transitions first
//!    (0x00e06710: from-all entries, then links, 0x00e05800);
//! 2. if a transition is pending, the level is new, or the core runs for
//!    the first time: exit the deeper levels (0x00e06800), enter the next
//!    state (starting its sub-machine, 0x00e06a50 / 0x00e06af0), call the
//!    owner's state-change hook (cFSMOrder vf05 0x005bacd0: FreeFlags and
//!    mCntState reset, registered at 0x00749820) and every process's
//!    "update" callback (record +0x50);
//! 3. call every process's "state" callback (+0x54);
//! 4. with `mSetting` bit 1, check transitions now; with bit 2 (or bit 4
//!    after a transition) run this level again at once.
//!
//! Callback return values are ignored by the driver. The level timer
//! (+0x18/+0x1c) is never armed by cFSMOrder scripts and is not modelled.

use crate::cond::{self, Value, Var, Vars};
use crate::{Env, Error};
use mtfsm::{Cluster, Graph, Process, State};
use std::collections::HashMap;
use std::sync::Arc;

/// cFSMOrder fields the orders and conditions use (offsets from vf03
/// 0x005b8ff0 and the constructor 0x005b7a60).
#[derive(Debug, Clone, PartialEq)]
pub struct Owner {
    /// SysCondition (+0x68): bit 0 set by Finish (0x007497e0), 0x10 by Kill.
    pub sys_condition: u32,
    /// The 32 FreeFlag bits (+0x174, bit n = byte n>>3 bit n&7).
    pub free_flags: u32,
    pub free_ins: [i32; 32],
    pub free_real: [f32; 32],
    /// Seconds in the current state / since the script started.
    pub cnt_state: f32,
    pub cnt_fsm: f32,
    /// FreeReal register that runs as a timer (+0x184, -1 none).
    pub timer_index: i8,
    /// Event order step (+0x6c), reset on state change.
    pub event_step: u32,
    /// The quest this script belongs to (cQuest +0x30 through +0x1a0):
    /// `quest << 16 | branch`.
    pub quest: Option<u32>,
    /// The quest's GameHour origin (cQuest +0xf8 day / +0x100 ms), set by
    /// SetGameHour (0x005bc590).
    pub quest_stamp: Option<(u64, u32)>,
    /// The script's camera slot (cFSMOrder +0x188).
    pub camera: CameraSlot,
}

/// cFSMOrder +0x188, driven by 0x005c4340 every frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraSlot {
    pub cam_id: u32,
    /// SceNo to raise to when the camera ends (+8).
    pub sce_end: u32,
    pub set_pos_type: u32,
    pub start_pos: u32,
    /// 0 idle, 1 started, 2 running, 3 ended (+0xd).
    pub state: u8,
    pub simple: bool,
}

impl Default for Owner {
    fn default() -> Self {
        Owner {
            sys_condition: 0,
            free_flags: 0,
            free_ins: [0; 32],
            free_real: [0.0; 32],
            cnt_state: 0.0,
            cnt_fsm: 0.0,
            timer_index: -1,
            event_step: 0,
            quest: None,
            quest_stamp: None,
            camera: CameraSlot::default(),
        }
    }
}

impl Owner {
    pub fn free_flag(&self, n: u32) -> bool {
        n < 32 && self.free_flags & (1 << n) != 0
    }
    /// The result-flag writer 0x005cc030: type 0 = FreeFlag (< 32), type 1
    /// = GSF (< 0x600), anything else nothing.
    // Translated from 0x005cc030 (DDDA.exe 2017-12-13 Steam).
    pub fn write_result(&mut self, world: &mut crate::World, kind: u32, no: u32, value: bool) {
        match kind {
            0 if no < 32 => {
                if value {
                    self.free_flags |= 1 << no;
                } else {
                    self.free_flags &= !(1 << no);
                }
            }
            1 if (no as usize) < crate::world::GSF_COUNT => world.gsf.set(no as usize, value),
            _ => {}
        }
    }
    /// cFSMOrder vf05 0x005bacd0, run on every state change.
    // Translated from 0x005bacd0 (DDDA.exe 2017-12-13 Steam). Not modelled:
    // the four cQuest bytes it clears (+0x44/+0x54/+0x64/+0x74, quest
    // target counters).
    fn state_changed(&mut self) {
        self.cnt_state = 0.0;
        self.free_flags = 0;
        self.event_step = 0;
    }
}

/// Per-process scratch memory for orders that keep state between frames.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProcMem {
    pub step: u32,
    pub value: i64,
    pub time: f32,
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

pub struct Script {
    pub path: String,
    graph: Arc<Graph>,
    levels: Vec<Level>,
    first: bool,
    pub owner: Owner,
    /// +0x70 bit 0.
    pub running: bool,
    mem: HashMap<(u32, usize), ProcMem>,
    /// Runtime core attribute (+0xa8); 3 when a core is made (0x00e05797).
    attribute: u32,
}

/// Context handed to orders.
pub struct OrderCtx<'a, 'e> {
    pub script: &'a str,
    pub owner: &'a mut Owner,
    pub mem: &'a mut ProcMem,
    pub env: &'a mut Env<'e>,
    /// Process position, for messages.
    pub state_name: &'a [u8],
}

/// The order dispatcher; implemented in `orders`.
pub trait Dispatch {
    fn update(ctx: &mut OrderCtx, p: &Process) -> Result<(), Error>;
    fn state(ctx: &mut OrderCtx, p: &Process) -> Result<(), Error>;
}

struct VarCtx<'a, 'b, 'e> {
    owner: &'a Owner,
    env: &'b Env<'e>,
}

impl Vars for VarCtx<'_, '_, '_> {
    fn var(&self, v: &Var) -> Result<Value, Error> {
        crate::vars::var(self.owner, self.env, v)
    }
}

fn cluster_of<'g>(g: &'g Graph, levels: &[Level], k: usize) -> Option<&'g Cluster> {
    let mut c = g.root.as_ref()?;
    for l in &levels[..k] {
        c = c.state(l.current)?.subcluster.as_deref()?;
    }
    Some(c)
}

impl Script {
    pub fn new(path: &str, graph: Arc<Graph>) -> Result<Script, Error> {
        let root = graph.root.as_ref().ok_or_else(|| Error::Data(format!("{path}: no root machine")))?;
        let mut s = Script {
            path: path.to_string(),
            graph: graph.clone(),
            levels: vec![],
            first: true,
            owner: Owner::default(),
            running: true,
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

    /// Current state ids, root first (for tests and debugging).
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

    fn condition(&self, env: &Env, id: u32) -> Result<bool, Error> {
        // 0x0117aaa0: first tree with the id; an absent id is false.
        match self.graph.condition(id).and_then(|c| c.root.as_ref()) {
            Some(e) => cond::eval(e, &VarCtx { owner: &self.owner, env }),
            None => Ok(false),
        }
    }

    /// 0x00e06710 (+ 0x00e05800 for the links).
    // Translated from 0x00e06710 and 0x00e05800 (DDDA.exe 2017-12-13 Steam).
    fn check_transitions(&mut self, env: &Env, k: usize) -> Result<(), Error> {
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
                if self.condition(env, cid)? {
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
                    if self.condition(env, l.condition)? {
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

    fn run_callbacks<D: Dispatch>(&mut self, env: &mut Env, k: usize, enter: bool) -> Result<(), Error> {
        let g = self.graph.clone();
        let c = cluster_of(&g, &self.levels, k).ok_or_else(|| Error::Data("level without machine".into()))?;
        let st: &State = c.state(self.levels[k].current).ok_or_else(|| Error::Data("no current state".into()))?;
        for (i, p) in st.processes.iter().enumerate() {
            let mem = self.mem.entry((st.unique_id, i)).or_default();
            if enter {
                // Our per-process bookkeeping ("request sent once per state
                // entry", countdowns) starts afresh on every entry.
                *mem = ProcMem::default();
            }
            let mut ctx = OrderCtx { script: &self.path, owner: &mut self.owner, mem, env, state_name: &st.name };
            if enter {
                D::update(&mut ctx, p)?;
            } else {
                D::state(&mut ctx, p)?;
            }
        }
        Ok(())
    }

    /// One frame of the core (0x00e092a0), then the cFSMOrder timers and
    /// the Finish check (cFSMOrder vf06 0x005babb0).
    pub fn step<D: Dispatch>(&mut self, env: &mut Env) -> Result<(), Error> {
        if !self.running {
            return Ok(());
        }
        let g = self.graph.clone();
        let mut k = 0;
        while k < self.levels.len() {
            loop {
                let c = cluster_of(&g, &self.levels, k).ok_or_else(|| Error::Data("level without machine".into()))?;
                let setting = c.state(self.levels[k].current).map(|s| s.setting).unwrap_or(0);
                if setting & 1 == 0 {
                    self.check_transitions(env, k)?;
                }
                let went = self.levels[k].go;
                if went || self.levels[k].pending_enter || self.first {
                    if let Some(next) = self.levels[k].next.take() {
                        // 0x00e06800 runs "exit" callbacks (none registered
                        // by cFSMOrder) of this level and deeper ones.
                        self.levels.truncate(k + 1);
                        self.levels[k].current = next;
                        let c = cluster_of(&g, &self.levels, k).unwrap();
                        if let Some(sub) = c.state(next).and_then(|s| s.subcluster.as_deref()) {
                            self.start_level(k + 1, sub.initial_state, sub)?;
                        }
                    }
                    self.owner.state_changed();
                    self.run_callbacks::<D>(env, k, true)?;
                    self.levels[k].pending_enter = false;
                    let lv = &mut self.levels[k];
                    if lv.came_by_entry {
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
                self.run_callbacks::<D>(env, k, false)?;
                let c = cluster_of(&g, &self.levels, k).ok_or_else(|| Error::Data("level without machine".into()))?;
                let setting = c.state(self.levels[k].current).map(|s| s.setting).unwrap_or(0);
                if setting & 1 != 0 {
                    self.check_transitions(env, k)?;
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
        self.timers(env);
        if self.owner.sys_condition & 1 != 0 {
            // cFSMOrder vf09 0x005cdb60 clears the running bit.
            self.running = false;
        }
        Ok(())
    }

    /// 0x005bac30: add max(mDeltaTime, 0.01) / mFps to mCntState, mCntFSM
    /// and the timer register.
    // Translated from 0x005bac30 (DDDA.exe 2017-12-13 Steam). Includes the camera
    // slot tick 0x005c4340(this + 0x188).
    fn timers(&mut self, env: &mut Env) {
        let d = env.time.delta.max(0.01) / env.time.fps;
        self.owner.cnt_state += d;
        self.owner.cnt_fsm += d;
        if self.owner.timer_index >= 0 {
            self.owner.free_real[self.owner.timer_index as usize] += d;
        }
        crate::orders::camera_tick(&mut self.owner, env);
    }
}

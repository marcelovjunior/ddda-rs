//! The enemy owner: `cThinkFSM` -> `cAIConsiderFSM` -> `cAILevelAbstractFSM`.
//!
//! A [`Thinker`] holds one enemy's FSM core and the owner fields its
//! processes and conditions use. Each frame the host fills an [`Inputs`]
//! snapshot and calls [`Thinker::frame`] (the owner's vf06 0x007a9cb0, which
//! the enemy's `cAICtrl` at unit +0x2E64 calls every frame from the
//! character's think, docs/FRAME.md); the frame returns [`Request`]s for the
//! actor. Field offsets are those of `cThinkFSM` (reflected by vf03
//! 0x007a82d0).

use crate::cond::{Value, Var};
use crate::core::{Core, Owner, ProcAt, ProcMem};
use crate::fields::Fields;
use crate::Error;
use mtfsm::{Graph, Process};
use std::sync::Arc;

/// Frame timing as sMain reports it: `delta` is mDeltaTime (+0x70, frames)
/// and `fps` is +0x38. Timers add `delta / fps` seconds per frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Time {
    pub delta: f32,
    pub fps: f32,
}

impl Default for Time {
    fn default() -> Self {
        Time {
            delta: 1.0,
            fps: 30.0,
        }
    }
}

/// The actor the thinker controls (the unit behind owner +8).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Actor {
    /// `uCoord.mPos` (+0x40): used by target searches.
    pub pos: [f32; 3],
    /// Unit +0xe0 (**guessed** world-space position: the distance getters
    /// add sWorldOffset to their reference point before comparing with it).
    pub world_pos: [f32; 3],
    /// The current action's completion byte (action +0x50 of the actor's
    /// action controller +0x2dc8), read by the CtrlAction / CtrlMotion state
    /// callback 0x005a8990.
    pub action_done: bool,
}

/// A unit a target search may pick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    /// The host's id for the unit.
    pub id: u32,
    /// `uCoord.mPos` (+0x40).
    pub pos: [f32; 3],
    pub alive: bool,
}

/// What the thinker reads this frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Inputs {
    pub time: Time,
    /// `None` when the owner has no live actor (the getters then read 0).
    pub actor: Option<Actor>,
    /// Unit +0xe0 of each player's Arisen. The original reads "the player"
    /// as sPlayerManager+0x99c, the first party slot: index 0 here (in
    /// co-op the host decides who that is).
    pub players: Vec<[f32; 3]>,
    /// sWorldOffset +0x50 and its active flag +0x20.
    pub world_offset: [f32; 3],
    pub world_offset_on: bool,
    /// The current stage's FSM flag word (sGameSys+0xadef8 + stage index
    /// * 0x14c, read MSB-first by 0x004b2810).
    pub fsm_flags: u32,
    /// AreaStayFlag word sGameSys+0xb884c (MSB-first, 0x005bae00).
    pub area_stay: u32,
    /// SceNo (sGameSys+0x30, 0x005baf00).
    pub sce_no: u32,
    /// Candidates per search group (`対象グループ`). The search rule is
    /// traced; which units belong to a group is the host's (group 0 is the
    /// player, group 1 the party slots 1..3 in 0x005a8aa0; others untraced).
    pub groups: Vec<Vec<Candidate>>,
}

/// What the thinker asks of its actor or the world.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// CtrlAction (0x005a8950): actor vf+0x210(action number).
    Action { action: i64 },
    /// CtrlMotion (0x005a8890): actor vf+0x214 with the motion and its
    /// optional interpolation, start frame, speed and attribute.
    Motion {
        motion: u64,
        interpolation: Option<f32>,
        start_frame: Option<f32>,
        speed: Option<f32>,
        attribute: Option<u64>,
    },
    /// CtrlThinkTbl (state callback 0x005a9240): the actor's think table
    /// (vf+0x2f4 / +0x2f8 with the table number) and, when `mode` != 0,
    /// unit +0x2d70 = mode - 1. Sent every frame the state runs.
    ThinkTable { table: u64, mode: u64, class: String },
    /// SetEscapePos (0x005a9170): actor vf+0x2e8(position), the world offset
    /// added when sWorldOffset is active.
    EscapePos { pos: [f32; 3] },
    /// SetFreeFlg (0x005a9210): OR into FreeCondition (+0x70) of the FSM the
    /// actor's AI controller holds (unit +0x2e64 -> +0x4c). The host applies
    /// it, normally with [`Thinker::or_free_condition`] on this thinker.
    FreeCondition { bits: u32 },
    /// SearchSetTarget (0x005a8aa0): the nearest candidate became the target.
    Target { group: u64, id: u32 },
    /// Set Kill Owner (0x007ab200): remove the owning unit.
    KillOwner,
}

/// `cThinkFSM` fields.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ThinkFields {
    /// SysCondition (+0x68): bit 0 Finish (0x007497e0), 0x10 Kill
    /// (0x00749800), 0x20000000 request done / target found, 0x40000000
    /// no target (0x005a8aa0); 0x60000000 cleared by the request updates.
    pub sys_condition: u32,
    /// +0x6c: the request state callback's arm byte (0x005a8990).
    pub arm: u8,
    /// FreeCondition (+0x70); not cleared on state change.
    pub free_condition: u32,
    /// +0x74: which of Timer[0..3] run.
    pub timer_run: u32,
    /// Timer[0..3] (+0x78), seconds.
    pub timers: [f32; 4],
    /// RandomTimer (+0x88), counts down to 0.
    pub random_timer: f32,
    /// Set Check Pos vector (+0x90).
    pub check_pos: [f32; 3],
    /// My AreaHit No (+0xa0).
    pub my_area_hit_no: i32,
    /// Attack (flag) (+0xa4).
    pub attack: bool,
    /// +0xa5, a countdown byte (vf06).
    pub countdown_a5: i8,
    /// +0xa8, frames after a state change (2, counts down; vf05 / vf06).
    pub after_change: i32,
    /// Motion Loop Cnt (+0xac), +0xb0, Check Target Distance (+0xb4) and
    /// ActionEx Fail Time (+0xb8): cleared on state change.
    pub motion_loop_cnt: i32,
    pub field_b0: i32,
    pub check_target_distance: f32,
    pub action_ex_fail_time: f32,
}

/// Per-frame environment of the owner.
pub struct Env<'a> {
    pub inputs: &'a Inputs,
    pub requests: &'a mut Vec<Request>,
}

/// One enemy thinker.
pub struct Thinker {
    core: Core,
    pub fields: ThinkFields,
    /// `cAIFSMCtrl` +5 (cAICtrl +0x49): the FSM finished and waits for the
    /// unit to clear it ([`Thinker::controlled_frame`]).
    pub paused: bool,
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    // SSE differences, then x87 squares, sum and FSQRT, read back as f32.
    let d = [
        f64::from(a[0] - b[0]),
        f64::from(a[1] - b[1]),
        f64::from(a[2] - b[2]),
    ];
    (d[1] * d[1] + d[0] * d[0] + d[2] * d[2]).sqrt() as f32
}

impl ThinkFields {
    /// `cThinkFSM` vf05 0x007a9da0: the state-change hook.
    // Translated from 0x007a9da0 (DDDA.exe 2017-12-13 Steam).
    fn state_changed(&mut self) {
        self.sys_condition = 0;
        self.arm = 0;
        self.after_change = 2;
        self.motion_loop_cnt = 0;
        self.field_b0 = -1;
        self.check_target_distance = 0.0;
        self.action_ex_fail_time = 0.0;
    }

    /// The request state callback shared by CtrlMotion / CtrlAction /
    /// CtrlActionEx: arm on the first frame, then mark done when the
    /// actor's current action is complete.
    // Translated from 0x005a8990 (DDDA.exe 2017-12-13 Steam).
    fn request_state(&mut self, inputs: &Inputs) {
        match self.arm {
            0 => self.arm = 1,
            1 if inputs.actor.is_some_and(|a| a.action_done) => self.sys_condition |= 0x2000_0000,
            _ => {}
        }
    }
}

fn idx(v: &Var) -> Result<u32, Error> {
    v.index
        .ok_or_else(|| Error::Data(format!("{} read without an index", v.property)))
}

impl Owner for ThinkFields {
    type Env<'e> = Env<'e>;

    fn state_changed(&mut self, _env: &mut Self::Env<'_>) {
        ThinkFields::state_changed(self);
    }

    fn var(&self, env: &Self::Env<'_>, v: &Var) -> Result<Value, Error> {
        let i = env.inputs;
        Ok(match v.property {
            "SysCondition" => Value::Int(i64::from(self.sys_condition)),
            "FreeCondition" => Value::Int(i64::from(self.free_condition)),
            "Timer[0]" => Value::Float(self.timers[0]),
            "Timer[1]" => Value::Float(self.timers[1]),
            "Timer[2]" => Value::Float(self.timers[2]),
            "Timer[3]" => Value::Float(self.timers[3]),
            "RandomTimer" => Value::Float(self.random_timer),
            "Attack (flag)" => Value::Bool(self.attack),
            "Motion Loop Cnt" => Value::Int(i64::from(self.motion_loop_cnt)),
            "Check Target Distance" => Value::Float(self.check_target_distance),
            "ActionEx Fail Time" => Value::Float(self.action_ex_fail_time),
            "My AreaHit No" => Value::Int(i64::from(self.my_area_hit_no)),
            // Translated from 0x007aba80 (DDDA.exe 2017-12-13 Steam): distance
            // from the actor (+0xe0) to the check point plus the world
            // offset; 0 without an actor.
            "Check Pos Length" => Value::Float(match i.actor {
                Some(a) => {
                    let p = [
                        self.check_pos[0] + i.world_offset[0],
                        self.check_pos[1] + i.world_offset[1],
                        self.check_pos[2] + i.world_offset[2],
                    ];
                    dist(a.world_pos, p)
                }
                None => 0.0,
            }),
            // Translated from 0x007abe20 (DDDA.exe 2017-12-13 Steam): actor
            // +0xe0 to the player's +0xe0; 0 without either.
            "Check Player Distance" => Value::Float(match (i.actor, i.players.first()) {
                (Some(a), Some(&p)) => dist(a.world_pos, p),
                _ => 0.0,
            }),
            // Translated from 0x007abcd0 -> 0x004b2810: bit of the stage's
            // FSM flag word, false past 31.
            "FSMFlag" => {
                let n = idx(v)?;
                Value::Bool(n < 32 && i.fsm_flags & (0x8000_0000 >> n) != 0)
            }
            // Translated from 0x005bae00 (DDDA.exe 2017-12-13 Steam).
            "FSMAreaStayFlag" => {
                let n = idx(v)?;
                Value::Bool(n < 32 && i.area_stay & (0x8000_0000 >> n) != 0)
            }
            // 0x005baf00: sGameSys +0x30.
            "SceNo" => Value::Int(i64::from(i.sce_no)),
            other => return Err(Error::Unimplemented(format!("variable {}.{other}", v.owner))),
        })
    }

    fn update(
        &mut self,
        env: &mut Self::Env<'_>,
        p: &Process,
        _mem: &mut ProcMem,
        at: ProcAt,
    ) -> Result<(), Error> {
        let name = String::from_utf8_lossy(&p.container);
        let par = || {
            p.parameter
                .as_ref()
                .ok_or_else(|| Error::Data(format!("{} {}: {name} without parameter", at.graph, at.index)))
        };
        match name.as_ref() {
            // Translated from 0x005a8950 (DDDA.exe 2017-12-13 Steam).
            "CtrlAction" => {
                let action = par()?
                    .s("アクション番号")
                    .ok_or_else(|| Error::Data("CtrlAction number".into()))?;
                self.sys_condition &= 0x9fff_ffff;
                if env.inputs.actor.is_some() {
                    env.requests.push(Request::Action { action });
                }
            }
            // Translated from 0x005a8890 (DDDA.exe 2017-12-13 Steam).
            "CtrlMotion" => {
                let r = par()?;
                let motion = r
                    .u("モーション番号")
                    .ok_or_else(|| Error::Data("CtrlMotion number".into()))?;
                let on = |flag: &str| r.b(flag).unwrap_or(false);
                self.sys_condition &= 0x9fff_ffff;
                env.requests.push(Request::Motion {
                    motion,
                    interpolation: on("補間指定有効？").then(|| r.f("補間フレーム").unwrap_or(0.0)),
                    start_frame: on("開始指定有効？").then(|| r.f("開始フレーム").unwrap_or(0.0)),
                    speed: on("速度指定有効？").then(|| r.f("再生速度(倍率)").unwrap_or(1.0)),
                    attribute: on("属性指定有効？").then(|| r.u("属性").unwrap_or(0)),
                });
            }
            // 0x005a8a90: the update of SearchSetTarget and CtrlThinkTbl
            // only clears the request bits.
            "SearchSetTarget" | "CtrlThinkTbl" => self.sys_condition &= 0x9fff_ffff,
            // Translated from 0x005a9170 (DDDA.exe 2017-12-13 Steam).
            "SetEscapePos" => {
                let mut pos = par()?
                    .vec3("逃走ポイント座標")
                    .ok_or_else(|| Error::Data("SetEscapePos point".into()))?;
                self.sys_condition &= 0x9fff_ffff;
                if env.inputs.world_offset_on {
                    for (p, o) in pos.iter_mut().zip(env.inputs.world_offset) {
                        *p += o;
                    }
                }
                env.requests.push(Request::EscapePos { pos });
            }
            // Translated from 0x005a9210 (DDDA.exe 2017-12-13 Steam).
            "SetFreeFlg" => {
                let bits = par()?
                    .u("FreeCondition")
                    .ok_or_else(|| Error::Data("SetFreeFlg bits".into()))? as u32;
                env.requests.push(Request::FreeCondition { bits });
            }
            // Translated from 0x007a9de0 (DDDA.exe 2017-12-13 Steam): Type 0
            // start from 0, 1 stop, 2 zero, 3 resume; TimerNo or Type above 3
            // does nothing.
            "Timer" => {
                let r = par()?;
                let (kind, no) = (r.u("Type").unwrap_or(0), r.s("TimerNo").unwrap_or(0));
                if (0..=3).contains(&no) && kind <= 3 {
                    let n = no as usize;
                    match kind {
                        0 => {
                            self.timer_run |= 1 << n;
                            self.timers[n] = 0.0;
                        }
                        1 => self.timer_run &= !(1 << n),
                        2 => self.timers[n] = 0.0,
                        _ => self.timer_run |= 1 << n,
                    }
                }
            }
            // Translated from 0x007aaee0 (DDDA.exe 2017-12-13 Steam).
            "Set Check Pos" => {
                self.check_pos = par()?
                    .vec3("Vec")
                    .ok_or_else(|| Error::Data("Set Check Pos vector".into()))?;
            }
            // Translated from 0x007ab200 (DDDA.exe 2017-12-13 Steam), as a request.
            "Set Kill Owner" => env.requests.push(Request::KillOwner),
            // Registered with the empty update 0x007497f0.
            "Finish" | "Kill" => {}
            other => {
                return Err(Error::Unimplemented(format!(
                    "{}: process {other} in state {}",
                    at.graph,
                    String::from_utf8_lossy(at.state_name)
                )))
            }
        }
        Ok(())
    }

    fn state(
        &mut self,
        env: &mut Self::Env<'_>,
        p: &Process,
        _mem: &mut ProcMem,
        at: ProcAt,
    ) -> Result<(), Error> {
        let name = String::from_utf8_lossy(&p.container);
        match name.as_ref() {
            "CtrlAction" | "CtrlMotion" => self.request_state(env.inputs),
            // Translated from 0x005a9240 (DDDA.exe 2017-12-13 Steam). The
            // class name only has to name a class (0x00cf5870 looks it up);
            // an empty one is treated as absent.
            "CtrlThinkTbl" => {
                let r = p
                    .parameter
                    .as_ref()
                    .ok_or_else(|| Error::Data("CtrlThinkTbl without parameter".into()))?;
                let class = r.text("DTI名").unwrap_or_default();
                if !class.is_empty() && env.inputs.actor.is_some() {
                    env.requests.push(Request::ThinkTable {
                        table: r.u("思考テーブル番号").unwrap_or(0),
                        mode: r.u("思考モード").unwrap_or(0),
                        class,
                    });
                    self.sys_condition |= 0x2000_0000;
                }
            }
            "SearchSetTarget" => self.search(env, p)?,
            // Translated from 0x007497e0 / 0x00749800.
            "Finish" => self.sys_condition |= 1,
            "Kill" => self.sys_condition |= 0x10,
            // Registered without a state callback, or with the stub 0x007497f0.
            "SetEscapePos" | "SetFreeFlg" | "Timer" | "Set Check Pos" | "Set Kill Owner" => {}
            other => {
                return Err(Error::Unimplemented(format!(
                    "{}: process {other} in state {}",
                    at.graph,
                    String::from_utf8_lossy(at.state_name)
                )))
            }
        }
        Ok(())
    }
}

impl ThinkFields {
    /// SearchSetTarget's state callback 0x005a8aa0: the nearest live
    /// candidate strictly within the range of the centre becomes the target
    /// and SysCondition gets 0x20000000; none gives 0x40000000. The centre
    /// is the actor's `mPos`, or the parameter's point (plus the world offset
    /// when active) when `探す中心点を座標指定する` is set. The comparison
    /// (squared distance below both the best so far and range²) is the one
    /// traced for group 1; groups 0 and 2..4 are assumed to use it too.
    // Translated from 0x005a8aa0 (DDDA.exe 2017-12-13 Steam), selection part.
    fn search(&mut self, env: &mut Env, p: &Process) -> Result<(), Error> {
        let r = p
            .parameter
            .as_ref()
            .ok_or_else(|| Error::Data("SearchSetTarget without parameter".into()))?;
        if r.b("LayoutSetでの[Group][ID]の指定を使用するか？")
            .unwrap_or(false)
        {
            return Err(Error::Unimplemented("SearchSetTarget by layout group/id".into()));
        }
        let group = r.u("対象グループ").unwrap_or(0);
        if group > 4 {
            // The jump table at 0x005a90c4 has five entries; above them the
            // search finds nothing.
            self.sys_condition |= 0x4000_0000;
            return Ok(());
        }
        let range = r.f("探す範囲").unwrap_or(0.0);
        let dead_too = r.b("死んでいても探す対象に加える").unwrap_or(false);
        let centre = if r.b("探す中心点を座標指定する").unwrap_or(false) {
            let mut c = r.vec3("探す中心点の座標").unwrap_or([0.0; 3]);
            if env.inputs.world_offset_on {
                for (x, o) in c.iter_mut().zip(env.inputs.world_offset) {
                    *x += o;
                }
            }
            c
        } else {
            match env.inputs.actor {
                Some(a) => a.pos,
                None => return Ok(()),
            }
        };
        let mut best = f32::MAX;
        let mut found = None;
        for c in env.inputs.groups.get(group as usize).into_iter().flatten() {
            if !c.alive && !dead_too {
                continue;
            }
            let d = [c.pos[0] - centre[0], c.pos[1] - centre[1], c.pos[2] - centre[2]];
            let sq = d[1] * d[1] + d[0] * d[0] + d[2] * d[2];
            if sq < best && sq < range * range {
                best = sq;
                found = Some(c.id);
            }
        }
        match found {
            Some(id) => {
                env.requests.push(Request::Target { group, id });
                self.sys_condition |= 0x2000_0000;
            }
            None => self.sys_condition |= 0x4000_0000,
        }
        Ok(())
    }
}

impl Thinker {
    pub fn new(path: &str, graph: Arc<Graph>) -> Result<Self, Error> {
        Ok(Thinker {
            core: Core::new(path, graph)?,
            fields: ThinkFields::default(),
            paused: false,
        })
    }

    /// Restart the graph from its initial state (`cBaseFSM` vf08 0x00749740
    /// sets the resource and starts a new core). Owner fields such as
    /// FreeCondition and the timers are kept.
    pub fn restart(&mut self) -> Result<(), Error> {
        self.core = Core::new(&self.core.path.clone(), self.core.graph().clone())?;
        Ok(())
    }

    /// One frame as the unit runs it, with the finish/restart cycle of the
    /// FSM controller (`cAIFSMCtrl` embedded at cAICtrl +0x44):
    /// - 0x00aa78b0 (first in `uEnemy::vf90` 0x00aa96e0) clears the paused
    ///   flag once the unit has a current action (+0x2dd4 != -1) whose
    ///   record has +0x50 set (`Actor::action_done`);
    /// - 0x0059f440 then runs the FSM frame; when it ends with SysCondition
    ///   bit 0 (a Finish) it rewinds the FSM to its start state (`cAIFSM`
    ///   vf06 0x00e098a0 -> 0x00e09250) and pauses it. SysCondition stays
    ///   set until the start state is entered again (vf05 0x007a9da0 clears
    ///   it); FreeCondition and the timers carry over.
    ///
    /// So a finished enemy FSM restarts by itself from its start state as
    /// soon as the unit's current action allows it; no request is needed.
    // Translated from 0x00aa78b0 and 0x0059f440 (DDDA.exe 2017-12-13 Steam).
    pub fn controlled_frame(&mut self, inputs: &Inputs) -> Result<Vec<Request>, Error> {
        if self.paused {
            if !inputs.actor.is_some_and(|a| a.action_done) {
                return Ok(vec![]);
            }
            self.paused = false;
        }
        let requests = self.frame(inputs)?;
        if self.finished() {
            self.core = Core::new(&self.core.path.clone(), self.core.graph().clone())?;
            self.paused = true;
        }
        Ok(requests)
    }

    pub fn current_states(&self) -> Vec<u32> {
        self.core.current_states()
    }
    pub fn current_names(&self) -> Vec<String> {
        self.core.current_names()
    }
    /// SysCondition bit 0 (a Finish process ran).
    pub fn finished(&self) -> bool {
        self.fields.sys_condition & 1 != 0
    }
    /// What a FreeCondition request does to this thinker (0x005a9210's OR).
    pub fn or_free_condition(&mut self, bits: u32) {
        self.fields.free_condition |= bits;
    }

    /// One frame: `cThinkFSM` vf06 0x007a9cb0 (countdowns, the core step,
    /// then Timer[0..3] by their run bits and RandomTimer).
    // Translated from 0x007a9cb0 (DDDA.exe 2017-12-13 Steam).
    pub fn frame(&mut self, inputs: &Inputs) -> Result<Vec<Request>, Error> {
        let f = &mut self.fields;
        if f.after_change > 0 {
            f.after_change -= 1;
        }
        if f.countdown_a5 > 0 {
            f.countdown_a5 -= 1;
        }
        let mut requests = vec![];
        let mut env = Env {
            inputs,
            requests: &mut requests,
        };
        self.core.step(&mut self.fields, &mut env)?;
        let f = &mut self.fields;
        let step = inputs.time.delta / inputs.time.fps;
        for n in 0..4 {
            if f.timer_run & (1 << n) != 0 {
                f.timers[n] += step;
            }
        }
        if f.random_timer > 0.0 {
            f.random_timer -= step;
        } else {
            f.random_timer = 0.0;
        }
        Ok(requests)
    }
}

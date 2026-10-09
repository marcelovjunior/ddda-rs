//! Headless gameplay simulation. No rendering, audio or OS input: the host
//! steps it with per-player input, and any number of views present its
//! state. Every system is written for N players (docs/COOP_DESIGN.md).
//!
//! Units are the game's: centimetres, stage frame (origin cell 50m50n),
//! radians. The simulation advances in fixed motion frames (30 per second,
//! *assumed*, see `MOTION_FPS`), because the original's jump parameters are
//! per-frame values; this also keeps it deterministic for co-op.
//!
//! From DDDA.exe and the data (docs/MOVEMENT.md):
//! - action motions (bank 0 = `m0000_co`): idle 0, walk 3, run 4, fall 7,
//!   jump takeoff 10 (moving) / 13 (standing), airborne 11 / 14, landing
//!   12 / 15, sprint start 43, sprint 20, sprint-jump takeoff 22, sprint-jump
//!   airborne 23, sprint-jump landing 24, ledge grab 45 / 50 (rising 49,
//!   settling 46), hang 47, climb 48 (`cPlAct*::vf05`, `cPlCliffHang*`);
//! - walking, running and sprinting move by a velocity set in code, not
//!   by root motion: facing x a speed from `param\pl\other\m_pl000_00`
//!   (`rAdjustParam`, a float table read by id) (`cPlActWalk::vf06`,
//!   `cPlActRun::vf06`, `cPlActDash::vf06`, `cPlActDashBegin::vf06`); other
//!   ground motions (takeoff, landing) move by their root track (joint 255);
//! - jumps are physics from `param\pl\other\pljumpparam`: at takeoff the
//!   velocity is SpeedY up plus SpeedZ forward along the facing (standing
//!   jumps get no SpeedZ), Gravity is the per-frame acceleration, and each
//!   frame (`0x00AC51D0`) the horizontal speed loses DampingZ while held
//!   input moves the position by AddMoveSpeedXZ. Normal jump: sets 0-8;
//!   sprint jump: sets 9-17 (`cPlActJump::vf14`, `cPlActDashJump::vf14`);
//! - dagger vocations have a second jump (`cPlActWpnDaggerJumpSecond`,
//!   sets 1-8); here it is a per-player capability;
//! - ledges are grabbed from a fall or the sprint jump's landing, never
//!   from a jump in the air; the geometry is in `ledge`.
//!
//! Approximations are marked *placeholder* or *assumed*.

use std::collections::HashMap;

pub mod ledge;

pub use ledge::{LedgeParam, LEDGE_SHIPPED};

/// Motion frames per second. *Assumed* 30 (not traced). Supporting
/// evidence: the recorded jump peaks 0.325 s after takeoff (9.5 frames at
/// 30 fps gives 0.317 s), and non-locomotion motions advance 30 frames/s.
pub const MOTION_FPS: f32 = 30.0;

/// Playback rate of the walk, run and sprint motions: recorded at 34.5
/// frames/s (the motion frame counter, uPlayer motion +0x3CC) against 30.0
/// for other motions. *Measured*; where the rate is set is not traced.
pub const LOCOMOTION_RATE: f32 = 1.15;

/// Factor on walk and run speed (uPlayer +0xDA8, multiplied in
/// `0x00B80690` / `0x00B80710`). Recorded as 0.8471090 on the ground for a
/// male Arisen; its writer is not traced. *Measured*.
pub const GROUND_SPEED_FACTOR: f32 = 0.847_109;

/// Sprint start reaches sprint speed this many frames after it begins,
/// rising linearly (never below its entry speed, per
/// `cPlActDashBegin::vf06`). *Measured* (4-5 frames in the recording); the
/// original's ramp (`0x010571C0` with id 9) is not traced.
pub const SPRINT_RAMP_FRAMES: f32 = 4.5;

/// Indices into the `rAdjustParam` float table (`rAdjustParam::vf15` at
/// 0x00CA47D0 returns `floats[id]`) used for ground speeds. The relaxed ids
/// apply where the area query `0x0043DAC0` reports flag 0x50 (towns and
/// some quest areas).
pub const ADJUST_WALK: usize = 0xFE;
pub const ADJUST_WALK_RELAXED: usize = 0xFF;
pub const ADJUST_RUN: usize = 0x100;
pub const ADJUST_RUN_RELAXED: usize = 0x101;
pub const ADJUST_SPRINT: usize = 0xF1;

/// Ground speeds, cm per frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveParam {
    pub walk: f32,
    pub walk_relaxed: f32,
    pub run: f32,
    pub run_relaxed: f32,
    pub sprint: f32,
}

impl MoveParam {
    /// Speeds from the `rAdjustParam` float table: walk and run get
    /// `GROUND_SPEED_FACTOR`, sprint does not (`cPlActDash::vf06`).
    pub fn from_adjust(table: &[f32]) -> Option<Self> {
        let get = |i: usize| table.get(i).copied();
        Some(MoveParam {
            walk: get(ADJUST_WALK)? * GROUND_SPEED_FACTOR,
            walk_relaxed: get(ADJUST_WALK_RELAXED)? * GROUND_SPEED_FACTOR,
            run: get(ADJUST_RUN)? * GROUND_SPEED_FACTOR,
            run_relaxed: get(ADJUST_RUN_RELAXED)? * GROUND_SPEED_FACTOR,
            sprint: get(ADJUST_SPRINT)?,
        })
    }
}

/// `m_pl000_00` as shipped: walk 5.0, run 15.0, relaxed run 12.0, sprint 22.5.
pub const MOVE_SHIPPED: MoveParam = MoveParam {
    walk: 5.0 * GROUND_SPEED_FACTOR,
    walk_relaxed: 5.0 * GROUND_SPEED_FACTOR,
    run: 15.0 * GROUND_SPEED_FACTOR,
    run_relaxed: 12.0 * GROUND_SPEED_FACTOR,
    sprint: 22.5,
};

/// Simulation step in motion frames: 120 steps per second. Per-frame values
/// (velocities, gravity, damping, air control) are scaled by the step, as
/// the original scales them by its frame delta (`0x00AC51D0` multiplies by
/// the object's delta).
pub const TICK: f32 = 0.25;

pub type PlayerId = u8;

/// One row of `pljumpparam` (cm per frame, cm per frame squared).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JumpParam {
    pub speed_y: f32,
    pub speed_z: f32,
    pub gravity: f32,
    pub add_move_xz: f32,
    pub damping_z: f32,
}

/// `pljumpparam` set 0 (normal jump) and set 9 (sprint jump) as shipped.
pub const JUMP_NORMAL: JumpParam = JumpParam {
    speed_y: 19.0,
    speed_z: 13.95,
    gravity: -2.0,
    add_move_xz: 4.25,
    damping_z: 0.3,
};
pub const JUMP_SPRINT: JumpParam = JumpParam {
    speed_y: 15.25,
    speed_z: 22.9,
    gravity: -1.25,
    add_move_xz: 4.25,
    damping_z: 0.3,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    Wait,
    /// `relaxed`: the town variant (area flag 0x50), other motion and speed.
    Walk {
        relaxed: bool,
    },
    Run {
        relaxed: bool,
    },
    SprintStart,
    Sprint,
    /// Takeoff; `standing` = no forward speed.
    JumpStart {
        standing: bool,
        sprint: bool,
    },
    Air {
        standing: bool,
        sprint: bool,
    },
    /// Dagger vocations' second jump: a forward roll (`cPlActWpnDaggerJumpSecond`).
    SecondJump,
    Fall,
    Land {
        standing: bool,
    },
    /// The sprint jump's landing (`cPlActDashJumpLand`, action 0x0C).
    SprintJumpLand,
    /// Taking hold of a ledge (`cPlCliffHangBegin`, action 0x34).
    /// `braced`: the short-bodied hang of `ledge::foot_point`.
    LedgeGrab {
        braced: bool,
        phase: GrabPhase,
    },
    /// Hanging (`cPlCliffHang`, action 0x35).
    LedgeHang {
        braced: bool,
    },
    /// Climbing up (`cPlCliffHangClimb`, action 0x36).
    LedgeClimb {
        braced: bool,
    },
}

/// Phases of `cPlCliffHangBegin::vf06` (0x00ADA980).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GrabPhase {
    /// Cases 10-11: reach for the edge (motion 0x2D, braced 0x32).
    Reach,
    /// Case 2: the feet are well below the hang, rise into it (motion 0x31).
    Rise,
    /// Case 10 after rising: settle into the hang (motion 0x2E).
    Settle,
}

impl State {
    /// Motion id (`bank << 8 | slot`, bank 0) the original plays.
    pub fn motion_id(self) -> u16 {
        match self {
            State::Wait => 0,
            State::Walk { relaxed: false } => 3,
            State::Walk { relaxed: true } => 0x54,
            State::Run { relaxed: false } => 4,
            State::Run { relaxed: true } => 0x55,
            State::SprintStart => 43,
            State::Sprint => 20,
            State::JumpStart { sprint: true, .. } => 22,
            State::JumpStart { standing: true, .. } => 13,
            State::JumpStart { .. } => 10,
            State::Air { sprint: true, .. } => 23,
            State::Air { standing: true, .. } => 14,
            State::Air { .. } => 11,
            // bank 6 slot 0x11: the dagger weapon set's roll (m0004_co, docs/MOVEMENT.md)
            State::SecondJump => 0x611,
            State::Fall => 7,
            State::Land { standing: true } => 15,
            State::Land { .. } => 12,
            // cPlActDashJumpLand::vf06
            State::SprintJumpLand => 0x18,
            // cPlCliffHangBegin::vf06
            State::LedgeGrab {
                phase: GrabPhase::Rise,
                ..
            } => 0x31,
            State::LedgeGrab {
                phase: GrabPhase::Settle,
                ..
            } => 0x2E,
            State::LedgeGrab { braced: false, .. } => 0x2D,
            State::LedgeGrab { braced: true, .. } => 0x32,
            // cPlCliffHang::vf05 plays 0x2F; for a braced hang it requests
            // no motion, so the grab's 0x32 goes on
            State::LedgeHang { braced: false } => 0x2F,
            State::LedgeHang { braced: true } => 0x32,
            // cPlCliffHangClimb::vf05: 0x30 (its variant 1, 0x2C, starts
            // during the grab and is not modelled)
            State::LedgeClimb { .. } => 0x30,
        }
    }

    /// Whether the motion loops while the state lasts.
    pub fn looping(self) -> bool {
        matches!(
            self,
            State::Wait
                | State::Walk { .. }
                | State::Run { .. }
                | State::Sprint
                | State::Air { .. }
                | State::Fall
                | State::LedgeHang { .. }
        )
    }
}

/// One player's input for a step: a stick in the camera's frame plus the
/// camera yaw, so movement is camera-relative like the original.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Input {
    /// Stick, x screen-right and y forward, length 0..1.
    pub stick: [f32; 2],
    /// Hold to walk instead of run (keyboard walk modifier).
    pub walk: bool,
    /// Hold to sprint.
    pub sprint: bool,
    /// Jump is held (a press is the rising edge).
    pub jump: bool,
    /// Camera yaw (radians, 0 looks along +Z).
    pub camera_yaw: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub id: PlayerId,
    /// Feet position, cm, stage frame.
    pub position: [f32; 3],
    /// Facing (radians, 0 faces +Z).
    pub yaw: f32,
    pub state: State,
    /// Frame within the current state's motion.
    pub frame: f32,
    /// Velocity while airborne, cm per frame.
    pub velocity: [f32; 3],
    pub jump: JumpParam,
    pub jumps_left: u8,
    /// Dagger vocations can jump again in the air.
    pub double_jump: bool,
    /// In an area with the relaxed walk and run (towns; area flag 0x50 of
    /// `0x0043DAC0`). Set by the host from the player's area.
    pub relaxed: bool,
    /// Ground speed of the last frame, cm per frame.
    pub speed: f32,
    /// Ground speed when the sprint start began.
    sprint_entry: f32,
    jump_held: bool,
    /// Height the current descent started from (the original's fall
    /// distance uPlayer +0x207C is `y - drop_from`; `0x0088A7E0` resets it
    /// whenever the character is not moving down).
    pub drop_from: f32,
    /// The ledge being grabbed, hung from or climbed.
    pub hold: Option<Hold>,
}

/// A held ledge and the points the hang and climb move between.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hold {
    pub ledge: ledge::Ledge,
    pub hang: ledge::Hang,
    /// Position when the grab or the climb began.
    pub start: [f32; 3],
    /// The climb's wall point (cPlCliffHangClimb +0x90) and the height the
    /// climb rises from (uPlayer +0x3104).
    pub wall: [f32; 3],
    pub from_y: f32,
}

/// Root motion of one motion: the root track's position by frame.
#[derive(Debug, Clone, PartialEq)]
pub struct RootMotion {
    pub frame_count: u32,
    track: Option<mtlmt::Track>,
}

impl RootMotion {
    pub fn from_motion(m: &mtlmt::Motion) -> Self {
        let track = m
            .tracks
            .iter()
            .find(|t| t.joint == 255 && t.usage == mtlmt::Usage::AbsolutePosition && !t.keys.is_empty())
            .cloned();
        RootMotion {
            frame_count: m.frame_count,
            track,
        }
    }

    /// A motion of `frame_count` frames without root movement.
    pub fn none(frame_count: u32) -> Self {
        RootMotion {
            frame_count,
            track: None,
        }
    }

    pub fn at(&self, frame: f32) -> [f32; 3] {
        match &self.track {
            Some(t) => {
                let v = t.sample(frame);
                [v[0], v[1], v[2]]
            }
            None => [0.0; 3],
        }
    }

    /// Loop period in frames: the last frame repeats the first pose, so a
    /// motion of `frame_count` frames loops every `frame_count - 1`.
    pub fn period(&self) -> f32 {
        self.frame_count.saturating_sub(1).max(1) as f32
    }

    /// Model-space displacement from `from` to `from + dt_frames`, unwrapped
    /// across loops (each completed loop adds the root's end position).
    pub fn delta(&self, from: f32, dt_frames: f32) -> [f32; 3] {
        let period = self.period();
        let span = self.at(period);
        let from = from.rem_euclid(period);
        let end = from + dt_frames.max(0.0);
        let loops = (end / period).floor();
        let to = end - loops * period;
        let a = self.at(from);
        let b = self.at(to);
        std::array::from_fn(|i| b[i] - a[i] + loops * span[i])
    }

    /// Displacement over `[from, from + dt]` without looping (clamped at the end).
    pub fn delta_once(&self, from: f32, dt_frames: f32) -> [f32; 3] {
        let end = self.period();
        let a = self.at(from.min(end));
        let b = self.at((from + dt_frames).min(end));
        std::array::from_fn(|i| b[i] - a[i])
    }
}

/// The nearest hit of a segment cast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    pub point: [f32; 3],
    /// Unit surface normal, on the side the cast came from.
    pub normal: [f32; 3],
}

/// Ground under a point, from the world (collision).
pub trait Ground {
    /// Highest ground height (cm) at (x, z) at or below `below_y`.
    fn height(&self, x: f32, z: f32, below_y: f32) -> Option<f32>;

    /// A wall within `radius` of the vertical segment at `p` (x, z) from
    /// height `low` to `high`: the horizontal unit direction from the wall
    /// to the segment. The default world has no walls.
    fn wall(&self, _p: [f32; 3], _radius: f32, _low: f32, _high: f32) -> Option<[f32; 2]> {
        None
    }

    /// The first surface the segment `from`-`to` meets (the original's
    /// collision cast `0x00DEA870`). Surfaces count from both sides
    /// (*assumed*). The default world has nothing to hit, so no ledges.
    fn ray(&self, _from: [f32; 3], _to: [f32; 3]) -> Option<RayHit> {
        None
    }

    /// Whether any surface lies within `radius` of `center` (the
    /// original's sphere test `0x00FAF4B0`).
    fn sphere(&self, _center: [f32; 3], _radius: f32) -> bool {
        false
    }
}

/// The body as a vertical cylinder for wall tests, cm. *Placeholder*: the
/// original's player collision shape is not traced.
pub const BODY_RADIUS: f32 = 35.0;
pub const BODY_HEIGHT: f32 = 170.0;

/// Moves from `from` toward `to` (same height) against walls: the full
/// move if nothing blocks it or it leads away from the wall, else the move
/// slid along the wall, else no move.
fn wall_slide(ground: &dyn Ground, from: [f32; 3], to: [f32; 3]) -> [f32; 3] {
    let hit = |q: [f32; 3]| ground.wall(q, BODY_RADIUS, from[1] + STEP_UP, from[1] + BODY_HEIGHT);
    let d = [to[0] - from[0], to[2] - from[2]];
    let Some(n) = hit(to) else { return to };
    let dn = d[0] * n[0] + d[1] * n[1];
    if dn >= 0.0 {
        // already touching and moving away from it
        return to;
    }
    let slid = [from[0] + d[0] - dn * n[0], to[1], from[2] + d[1] - dn * n[1]];
    match hit(slid) {
        None => slid,
        Some(m) if (slid[0] - from[0]) * m[0] + (slid[2] - from[2]) * m[1] >= 0.0 => slid,
        Some(_) => from,
    }
}

/// Stick magnitude above which the player moves. *Placeholder*.
pub const MOVE_THRESHOLD: f32 = 0.2;
/// Highest step the feet climb while on the ground, cm. *Placeholder*.
pub const STEP_UP: f32 = 60.0;
/// Drop below the feet that starts a fall instead of following the ground, cm. *Placeholder*.
pub const STEP_DOWN: f32 = 50.0;
/// Gravity while falling (cm/frame²): the normal jump's. *Placeholder* until
/// `cPlActFall` is traced.
pub const FALL_GRAVITY: f32 = -2.0;
/// A fall may grab a ledge only after dropping more than this, cm
/// (`0x00B6FA00`: in cPlActFall a ledge is refused while uPlayer +0x207C,
/// the fall distance, is >= -100; constant 0x0161C308).
pub const LEDGE_FALL_MIN_DROP: f32 = 100.0;

pub struct Sim {
    pub players: Vec<Player>,
    pub motions: HashMap<u16, RootMotion>,
    pub jump_normal: JumpParam,
    pub jump_sprint: JumpParam,
    pub move_param: MoveParam,
    pub ledge_param: LedgeParam,
    /// Fraction of a frame not yet simulated.
    pending: f32,
}

fn len2(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

impl Sim {
    pub fn new(motions: HashMap<u16, RootMotion>) -> Self {
        Sim {
            players: Vec::new(),
            motions,
            jump_normal: JUMP_NORMAL,
            jump_sprint: JUMP_SPRINT,
            move_param: MOVE_SHIPPED,
            ledge_param: LEDGE_SHIPPED,
            pending: 0.0,
        }
    }

    pub fn add_player(&mut self, id: PlayerId, position: [f32; 3], yaw: f32) {
        self.players.push(Player {
            id,
            position,
            yaw,
            state: State::Wait,
            frame: 0.0,
            velocity: [0.0; 3],
            jump: JUMP_NORMAL,
            jumps_left: 0,
            double_jump: false,
            relaxed: false,
            speed: 0.0,
            sprint_entry: 0.0,
            jump_held: false,
            drop_from: position[1],
            hold: None,
        });
    }

    pub fn player(&self, id: PlayerId) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }

    pub fn player_mut(&mut self, id: PlayerId) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.id == id)
    }

    fn period_of(&self, motion: u16) -> f32 {
        self.motions.get(&motion).map_or(30.0, RootMotion::period)
    }

    /// Advances every player by `dt` seconds, in steps of `TICK` motion frames.
    pub fn step(&mut self, dt: f32, inputs: &HashMap<PlayerId, Input>, ground: &dyn Ground) {
        self.pending += dt * MOTION_FPS;
        while self.pending >= TICK {
            self.pending -= TICK;
            for i in 0..self.players.len() {
                let input = inputs.get(&self.players[i].id).copied().unwrap_or_default();
                self.frame(i, input, ground, TICK);
            }
        }
    }

    fn set_state(p: &mut Player, s: State) {
        if p.state != s {
            p.state = s;
            p.frame = 0.0;
        }
    }

    /// One step of `dt` motion frames for player `i`.
    fn frame(&mut self, i: usize, input: Input, ground: &dyn Ground, dt: f32) {
        let y0 = self.players[i].position[1];
        self.frame_states(i, input, ground, dt);
        // 0x0088A7E0: the fall distance (uPlayer +0x207C) counts from the
        // last frame the character did not move down
        let p = &mut self.players[i];
        if p.position[1] >= y0 {
            p.drop_from = p.position[1];
        }
    }

    fn frame_states(&mut self, i: usize, input: Input, ground: &dyn Ground, dt: f32) {
        let (jn, js) = (self.jump_normal, self.jump_sprint);
        let moving = len2(input.stick) >= MOVE_THRESHOLD;
        // Stick x is screen-right; looking along +Z with +Y up, screen-right
        // is -X, so right is a negative turn about +Y.
        let input_yaw = input.camera_yaw + (-input.stick[0]).atan2(input.stick[1]);
        let pressed = {
            let p = &mut self.players[i];
            let pr = input.jump && !p.jump_held;
            p.jump_held = input.jump;
            pr
        };
        let state = self.players[i].state;
        match state {
            State::Wait
            | State::Walk { .. }
            | State::Run { .. }
            | State::SprintStart
            | State::Sprint
            | State::Land { .. }
            | State::SprintJumpLand => {
                if pressed {
                    let p = &mut self.players[i];
                    let sprint = matches!(state, State::Sprint | State::SprintStart);
                    if moving {
                        p.yaw = input_yaw;
                    }
                    Self::set_state(
                        p,
                        State::JumpStart {
                            standing: !moving,
                            sprint,
                        },
                    );
                    p.jump = if sprint { js } else { jn };
                    p.jumps_left = u8::from(p.double_jump);
                    return;
                }
                // uPlayerBase::vf136 (0x00B6D510) runs the ledge test in
                // cPlActDashJumpLand with the longer reach; 0x00B6FD20 also
                // needs stick input (uPlayer +0x32C0 != 0)
                if state == State::SprintJumpLand
                    && moving
                    && self.try_grab(i, self.ledge_param.reach_land, ground)
                {
                    return;
                }
                // a landing plays out unless the player moves
                if matches!(state, State::Land { .. } | State::SprintJumpLand)
                    && !moving
                    && self.players[i].frame < self.period_of(state.motion_id())
                {
                    self.ground_move(i, state, ground, dt);
                    return;
                }
                let next = if !moving {
                    State::Wait
                } else if input.sprint {
                    let start_done = self.players[i].frame >= self.period_of(State::SprintStart.motion_id());
                    match state {
                        State::Sprint => State::Sprint,
                        State::SprintStart if start_done => State::Sprint,
                        _ => State::SprintStart,
                    }
                } else if input.walk {
                    State::Walk {
                        relaxed: self.players[i].relaxed,
                    }
                } else {
                    State::Run {
                        relaxed: self.players[i].relaxed,
                    }
                };
                let p = &mut self.players[i];
                if next == State::SprintStart && p.state != State::SprintStart {
                    p.sprint_entry = p.speed;
                }
                Self::set_state(p, next);
                if moving {
                    // *Placeholder*: the original turns over time.
                    p.yaw = input_yaw;
                }
                self.ground_move(i, next, ground, dt);
            }
            State::JumpStart { standing, sprint } => {
                // takeoff motion (the sprint takeoff carries root motion), then launch
                self.ground_move(i, state, ground, dt);
                let len = self.period_of(state.motion_id());
                let p = &mut self.players[i];
                if p.state == state && p.frame >= len {
                    let (s, c) = p.yaw.sin_cos();
                    let fwd = if standing { 0.0 } else { p.jump.speed_z };
                    p.velocity = [fwd * s, p.jump.speed_y, fwd * c];
                    Self::set_state(p, State::Air { standing, sprint });
                }
            }
            State::Air { .. } | State::Fall | State::SecondJump => {
                if pressed && self.players[i].jumps_left > 0 {
                    let p = &mut self.players[i];
                    p.jumps_left -= 1;
                    // second jump (dagger vocations): fresh speed from sets 1-8
                    p.jump = jn;
                    p.velocity[1] = jn.speed_y;
                    if moving {
                        p.yaw = input_yaw;
                        let (s, c) = p.yaw.sin_cos();
                        p.velocity[0] = jn.speed_z * s;
                        p.velocity[2] = jn.speed_z * c;
                    }
                    Self::set_state(p, State::SecondJump);
                }
                // the roll plays once, then the falling loop
                if self.players[i].state == State::SecondJump
                    && self.players[i].frame >= self.period_of(State::SecondJump.motion_id())
                {
                    Self::set_state(&mut self.players[i], State::Fall);
                }
                // uPlayerBase::vf136: a fall tests for ledges while the stick
                // is held (command slot 0 active), with the shorter reach.
                // Jumps (cPlActJump, cPlActDashJump) never do: 0x00B6FD20
                // accepts only actions 0x0C, 0x0D and 0x0F.
                if self.players[i].state == State::Fall
                    && moving
                    && self.try_grab(i, self.ledge_param.reach_fall, ground)
                {
                    return;
                }
                self.air_move(i, moving.then_some(input_yaw), ground, dt);
            }
            State::LedgeGrab { braced, phase } => {
                let len = self.period_of(state.motion_id());
                let rise_speed = self.ledge_param.rise_speed;
                let p = &mut self.players[i];
                let Some(h) = p.hold else {
                    Self::let_go(p, jn);
                    return;
                };
                if !ledge::holds(ground, &h.ledge) {
                    Self::let_go(p, jn);
                    return;
                }
                p.frame += dt;
                // *Placeholder* progress: the original reads the motion's
                // curve 9 (0x010571C0), not read by mtlmt yet
                let t = (p.frame / len).min(1.0);
                match phase {
                    GrabPhase::Reach => {
                        // *Placeholder* path: the original poses the body by IK
                        // (0x00850BD0) toward the body point and takes the root
                        // from it; here the feet move straight to the foot point
                        p.position = ledge::lerp(h.start, h.hang.foot, t);
                        if p.frame >= len {
                            p.position = h.hang.foot;
                            Self::set_state(p, State::LedgeHang { braced });
                        }
                    }
                    GrabPhase::Rise => {
                        let q = ledge::rise_step(
                            ground,
                            &h.ledge,
                            h.start,
                            h.hang.foot,
                            p.position[1],
                            t,
                            rise_speed * dt,
                        );
                        p.position = q;
                        if h.hang.foot[1] <= q[1] || p.frame >= len {
                            if ledge::rise_snaps(h.hang.foot, q[1]) {
                                p.position[1] = h.hang.foot[1];
                                Self::set_state(
                                    p,
                                    State::LedgeGrab {
                                        braced,
                                        phase: GrabPhase::Settle,
                                    },
                                );
                            } else {
                                // cPlCliffHangBegin::vf06 case 2: falls without a push
                                p.hold = None;
                                p.velocity = [0.0; 3];
                                p.jump = jn;
                                Self::set_state(p, State::Fall);
                            }
                        }
                    }
                    GrabPhase::Settle => {
                        if p.frame >= len {
                            Self::set_state(p, State::LedgeHang { braced });
                        }
                    }
                }
            }
            State::LedgeHang { braced } => {
                let period = self.period_of(state.motion_id());
                let p = &mut self.players[i];
                let Some(mut h) = p.hold else {
                    Self::let_go(p, jn);
                    return;
                };
                p.frame = (p.frame + dt).rem_euclid(period);
                if !ledge::holds(ground, &h.ledge) {
                    Self::let_go(p, jn);
                    return;
                }
                // cPlCliffHang::vf06 (0x00ADCDD0): uPlayer +0x32CC above 0.8
                // climbs, below -0.8 lets go. +0x32CC is *assumed* to be the
                // stick's push along the facing (its writer is not traced).
                let push = if moving {
                    len2(input.stick).min(1.0) * (input_yaw - p.yaw).cos()
                } else {
                    0.0
                };
                if push > ledge::HANG_STICK {
                    // cPlCliffHangClimb::vf05 (0x00ADD670)
                    h.start = p.position;
                    h.from_y = h.hang.foot[1];
                    h.wall = ledge::climb_wall_point(ground, &h.ledge, h.hang.body, p.position[1]);
                    p.hold = Some(h);
                    Self::set_state(p, State::LedgeClimb { braced });
                } else if push < -ledge::HANG_STICK {
                    Self::let_go(p, jn);
                }
            }
            State::LedgeClimb { .. } => {
                let len = self.period_of(state.motion_id());
                let p = &mut self.players[i];
                let Some(h) = p.hold else {
                    Self::let_go(p, jn);
                    return;
                };
                if !ledge::holds(ground, &h.ledge) {
                    Self::let_go(p, jn);
                    return;
                }
                p.frame += dt;
                // cPlCliffHangClimb::vf06 (0x00ADDC00). *Placeholder*
                // progress: the original uses the motion's curves 9 (height)
                // and 10 (position), not read by mtlmt yet
                let t = (p.frame / len).min(1.0);
                let target = ledge::climb_target(ground, &h.ledge, h.wall, h.from_y, t);
                p.position = ledge::lerp(h.start, target, t);
                if p.frame >= len {
                    p.position = ledge::climb_end(&h.ledge);
                    p.hold = None;
                    Self::set_state(p, State::Wait);
                }
            }
        }
    }

    /// Runs the ledge test (with `reach`) and, if it passes, takes hold.
    /// The caller checks the action and the stick (uPlayerBase::vf136).
    fn try_grab(&mut self, i: usize, reach: f32, ground: &dyn Ground) -> bool {
        let lp = self.ledge_param;
        let p = &self.players[i];
        let Some(l) = ledge::detect(ground, &lp, p.position, p.yaw, reach) else {
            return false;
        };
        // 0x00B6FA00: a fall must have dropped far enough
        if p.state == State::Fall && p.position[1] - p.drop_from >= -LEDGE_FALL_MIN_DROP {
            return false;
        }
        // uPlayerBase::vf136 also needs a foot point (0x00ADBEB0 != -1)
        let Some((braced, foot)) = ledge::foot_point(ground, &l, p.position) else {
            return false;
        };
        let mut hang = ledge::hang_pose(ground, &lp, &l, braced, foot);
        // cPlCliffHangBegin::vf06 case 1: a free hang whose feet are well
        // below the foot point rises into it
        let rise = !braced && p.position[1] <= hang.foot[1] - lp.rise_margin;
        if rise {
            hang.foot = ledge::rise_foot(ground, &l, hang.foot);
        }
        let p = &mut self.players[i];
        p.hold = Some(Hold {
            ledge: l,
            hang,
            start: p.position,
            wall: hang.body,
            from_y: hang.foot[1],
        });
        p.velocity = [0.0; 3];
        p.speed = 0.0;
        // *Placeholder*: the original turns toward the wall over the grab
        // (slerp to a rotation built from the body line, vf05/vf06)
        p.yaw = ledge::facing(&l);
        Self::set_state(
            p,
            State::LedgeGrab {
                braced,
                phase: if rise { GrabPhase::Rise } else { GrabPhase::Reach },
            },
        );
        true
    }

    /// Lets go of a ledge: pushed out from the wall at 2 cm per frame, falling
    /// (cPlCliffHang::vf06; the same in cPlCliffHangBegin and
    /// cPlCliffHangClimb when the ledge disappears).
    fn let_go(p: &mut Player, jn: JumpParam) {
        p.velocity = [0.0; 3];
        if let Some(h) = p.hold.take() {
            let n = h.ledge.normal;
            p.velocity = [n[0] * ledge::LET_GO_SPEED, 0.0, n[2] * ledge::LET_GO_SPEED];
        }
        p.jump = jn;
        Self::set_state(p, State::Fall);
    }

    /// Ground speed (cm per frame) of a velocity-driven state, or None for
    /// states that move by root motion.
    fn ground_speed(&self, p: &Player, state: State) -> Option<f32> {
        let mp = &self.move_param;
        Some(match state {
            State::Walk { relaxed: false } => mp.walk,
            State::Walk { relaxed: true } => mp.walk_relaxed,
            State::Run { relaxed: false } => mp.run,
            State::Run { relaxed: true } => mp.run_relaxed,
            State::Sprint => mp.sprint,
            // cPlActDashBegin::vf06: max(entry speed, ramp to sprint speed);
            // the motion frame runs at LOCOMOTION_RATE
            State::SprintStart => {
                let t = p.frame / LOCOMOTION_RATE / SPRINT_RAMP_FRAMES;
                (mp.sprint * t.min(1.0)).max(p.sprint_entry)
            }
            _ => return None,
        })
    }

    /// Movement on the ground (velocity or root motion), with step-up /
    /// step-down rules.
    fn ground_move(&mut self, i: usize, state: State, ground: &dyn Ground, dt: f32) {
        let jn = self.jump_normal;
        let speed = self.ground_speed(&self.players[i], state);
        let period = self.period_of(state.motion_id());
        let p = &mut self.players[i];
        let mut target = p.position;
        if let Some(v) = speed {
            // velocity along the facing; the motion only animates
            let (s, c) = p.yaw.sin_cos();
            target[0] += v * dt * s;
            target[2] += v * dt * c;
            p.speed = v;
            let f = p.frame + dt * LOCOMOTION_RATE;
            p.frame = if state.looping() { f.rem_euclid(period) } else { f };
        } else if let Some(m) = self.motions.get(&state.motion_id()) {
            p.speed = 0.0;
            let d = if state.looping() {
                m.delta(p.frame, dt)
            } else {
                m.delta_once(p.frame, dt)
            };
            let (s, c) = p.yaw.sin_cos();
            target[0] += d[0] * c + d[2] * s;
            target[2] += -d[0] * s + d[2] * c;
            p.frame = if state.looping() {
                (p.frame + dt).rem_euclid(m.period())
            } else {
                p.frame + dt
            };
        } else {
            p.speed = 0.0;
            p.frame += dt;
        }
        let target = wall_slide(ground, p.position, target);
        match ground.height(target[0], target[2], p.position[1] + STEP_UP) {
            Some(h) if h >= p.position[1] - STEP_DOWN => p.position = [target[0], h, target[2]],
            Some(_) if !matches!(state, State::JumpStart { .. }) => {
                // walked off an edge: fall, keeping the ground speed (per frame)
                p.velocity = [
                    (target[0] - p.position[0]) / dt,
                    0.0,
                    (target[2] - p.position[2]) / dt,
                ];
                p.position = [target[0], p.position[1], target[2]];
                p.jump = jn;
                p.jumps_left = u8::from(p.double_jump);
                Self::set_state(p, State::Fall);
            }
            // a wall or cliff face (or an edge during takeoff): stay
            _ => {}
        }
    }

    /// One airborne frame: damping, air control, gravity, walls, ledges, landing.
    fn air_move(&mut self, i: usize, steer: Option<f32>, ground: &dyn Ground, dt: f32) {
        let p = &mut self.players[i];
        let jp = p.jump;
        let gravity = if matches!(p.state, State::Fall) {
            FALL_GRAVITY
        } else {
            jp.gravity
        };
        // horizontal speed loses DampingZ per frame (0x00AC51D0)
        let h = (p.velocity[0].powi(2) + p.velocity[2].powi(2)).sqrt();
        let damp = jp.damping_z * dt;
        if h <= damp {
            p.velocity[0] = 0.0;
            p.velocity[2] = 0.0;
        } else {
            let k = (h - damp) / h;
            p.velocity[0] *= k;
            p.velocity[2] *= k;
        }
        // *Assumed* integration order: position += velocity, then velocity += gravity.
        let mut next = [
            p.position[0] + p.velocity[0] * dt,
            p.position[1] + p.velocity[1] * dt,
            p.position[2] + p.velocity[2] * dt,
        ];
        p.velocity[1] += gravity * dt;
        if let Some(y) = steer {
            next[0] += jp.add_move_xz * dt * y.sin();
            next[2] += jp.add_move_xz * dt * y.cos();
        }
        p.frame += dt;
        // a wall ahead: nothing to stand on within a step of the body, but
        // ground above it
        let wall = (ground.height(next[0], next[2], p.position[1] + STEP_UP).is_none()
            && ground.height(next[0], next[2], f32::MAX).is_some())
            || ground
                .wall(
                    next,
                    BODY_RADIUS,
                    p.position[1] + STEP_UP,
                    p.position[1] + BODY_HEIGHT,
                )
                .is_some_and(|n| (next[0] - p.position[0]) * n[0] + (next[2] - p.position[2]) * n[1] < 0.0);
        if wall {
            // slide along the wall, keeping the vertical motion
            let slid = wall_slide(ground, p.position, [next[0], p.position[1], next[2]]);
            next[0] = slid[0];
            next[2] = slid[2];
            p.velocity[0] = 0.0;
            p.velocity[2] = 0.0;
        }
        // landing: the ground under the new position, reached on the way down
        match ground.height(next[0], next[2], p.position[1].max(next[1]) + 1.0) {
            Some(g) if next[1] <= g && p.velocity[1] <= 0.0 => {
                let standing = matches!(p.state, State::Air { standing: true, .. }) || steer.is_none();
                // cPlActDashJump::vf06 (0x00ACDED0) lands into action 0x0C
                let land = if matches!(p.state, State::Air { sprint: true, .. }) {
                    State::SprintJumpLand
                } else {
                    State::Land { standing }
                };
                p.position = [next[0], g, next[2]];
                p.velocity = [0.0; 3];
                Self::set_state(p, land);
            }
            _ => p.position = next,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat(f32);
    impl Ground for Flat {
        fn height(&self, _: f32, _: f32, below_y: f32) -> Option<f32> {
            (self.0 <= below_y).then_some(self.0)
        }
    }

    /// A run-like motion: root moves +Z 100 cm over 10 frames.
    fn run() -> RootMotion {
        RootMotion {
            frame_count: 11,
            track: Some(mtlmt::Track {
                codec: 3,
                usage: mtlmt::Usage::AbsolutePosition,
                bone_type: 0,
                joint: 255,
                weight: 1.0,
                reference: [0.0; 4],
                keys: vec![
                    mtlmt::Key {
                        frame: 0,
                        value: [0.0, 0.0, 0.0, 0.0],
                    },
                    mtlmt::Key {
                        frame: 10,
                        value: [0.0, 0.0, 100.0, 0.0],
                    },
                ],
            }),
        }
    }

    fn sim() -> Sim {
        let mut m = HashMap::new();
        for id in [3u16, 4, 20, 0x54, 0x55] {
            m.insert(id, run());
        }
        m.insert(43, RootMotion::none(15));
        for id in [10u16, 13, 22] {
            m.insert(id, RootMotion::none(5));
        }
        // ledge motions: grab 0x2D / 0x32, rise 0x31, settle 0x2E, hang
        // 0x2F, climb 0x30; sprint-jump landing 0x18
        for (id, frames) in [
            (0x2D, 21),
            (0x32, 21),
            (0x31, 16),
            (0x2E, 11),
            (0x2F, 31),
            (0x30, 37),
            (0x18, 11),
        ] {
            m.insert(id, RootMotion::none(frames));
        }
        let mut s = Sim::new(m);
        s.add_player(0, [0.0; 3], 0.0);
        s.add_player(1, [500.0, 0.0, 0.0], 0.0);
        s
    }

    fn fwd() -> Input {
        Input {
            stick: [0.0, 1.0],
            ..Default::default()
        }
    }

    const RUN5: f32 = 5.0 * MOVE_SHIPPED.run;

    #[test]
    fn running_moves_forward_at_the_run_speed() {
        let mut s = sim();
        let mut inputs = HashMap::new();
        inputs.insert(0, fwd());
        s.step(5.0 / MOTION_FPS, &inputs, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert_eq!(p.state, State::Run { relaxed: false });
        assert!((p.position[2] - RUN5).abs() < 1e-3, "{:?}", p.position);
        // the motion plays faster than the simulation's frames
        assert!((p.frame - 5.0 * LOCOMOTION_RATE).abs() < 1e-3, "{}", p.frame);
        let q = s.player(1).unwrap();
        assert_eq!(q.state, State::Wait);
        assert_eq!(q.position[0], 500.0);
    }

    #[test]
    fn stick_right_is_screen_right() {
        let mut s = sim();
        let mut inputs = HashMap::new();
        inputs.insert(
            0,
            Input {
                stick: [1.0, 0.0],
                ..Default::default()
            },
        );
        s.step(5.0 / MOTION_FPS, &inputs, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert!(
            (p.position[0] + RUN5).abs() < 1e-3 && p.position[2].abs() < 1e-3,
            "{:?}",
            p.position
        );
    }

    #[test]
    fn camera_relative_direction() {
        let mut s = sim();
        let mut inputs = HashMap::new();
        inputs.insert(
            0,
            Input {
                stick: [0.0, 1.0],
                camera_yaw: std::f32::consts::FRAC_PI_2,
                ..Default::default()
            },
        );
        s.step(5.0 / MOTION_FPS, &inputs, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert!(
            (p.position[0] - RUN5).abs() < 1e-3 && p.position[2].abs() < 1e-3,
            "{:?}",
            p.position
        );
    }

    #[test]
    fn no_jump_at_the_loop_seam() {
        let mut s = sim();
        let mut inputs = HashMap::new();
        inputs.insert(0, fwd());
        let mut last = s.player(0).unwrap().position[2];
        for _ in 0..200 {
            s.step(1.0 / MOTION_FPS, &inputs, &Flat(0.0));
            let z = s.player(0).unwrap().position[2];
            assert!(
                (z - last - MOVE_SHIPPED.run).abs() < 1e-2,
                "moved {} in one frame",
                z - last
            );
            last = z;
        }
    }

    #[test]
    fn standing_jump_reaches_the_parameter_apex_and_lands() {
        let mut s = sim();
        let mut inputs = HashMap::new();
        inputs.insert(
            0,
            Input {
                jump: true,
                ..Default::default()
            },
        );
        let mut apex = 0.0f32;
        for _ in 0..60 {
            s.step(1.0 / MOTION_FPS, &inputs, &Flat(0.0));
            apex = apex.max(s.player(0).unwrap().position[1]);
        }
        let p = s.player(0).unwrap();
        // SpeedY 19, Gravity -2 per frame: 90.25 cm in continuous time, plus
        // v * dt / 2 = 2.4 cm with quarter-frame steps
        assert!((apex - 92.6).abs() < 0.5, "apex {apex}");
        assert!(
            matches!(p.state, State::Land { standing: true } | State::Wait),
            "{:?}",
            p.state
        );
        assert_eq!(p.position[1], 0.0);
        assert_eq!(p.position[2], 0.0, "a standing jump has no forward speed");
    }

    #[test]
    fn double_jump_only_with_the_capability() {
        for (cap, expect_higher) in [(false, false), (true, true)] {
            let mut s = sim();
            s.player_mut(0).unwrap().double_jump = cap;
            let mut apex = 0.0f32;
            for f in 0..80 {
                let mut inputs = HashMap::new();
                // press, release, press again near the top
                inputs.insert(
                    0,
                    Input {
                        jump: f < 2 || (12..14).contains(&f),
                        ..Default::default()
                    },
                );
                s.step(1.0 / MOTION_FPS, &inputs, &Flat(0.0));
                apex = apex.max(s.player(0).unwrap().position[1]);
            }
            assert_eq!(apex > 150.0, expect_higher, "cap {cap}: apex {apex}");
        }
    }

    /// Ground with a wall: x < 100 at y 0, x >= 100 at `top`.
    struct Wall(f32);
    impl Ground for Wall {
        fn height(&self, x: f32, _: f32, below_y: f32) -> Option<f32> {
            let h = if x < 100.0 { 0.0 } else { self.0 };
            (h <= below_y).then_some(h)
        }
    }

    #[test]
    fn walls_block() {
        let mut s = sim();
        let mut inputs = HashMap::new();
        inputs.insert(
            0,
            Input {
                stick: [0.0, 1.0],
                camera_yaw: std::f32::consts::FRAC_PI_2,
                ..Default::default()
            },
        );
        for _ in 0..60 {
            s.step(1.0 / 30.0, &inputs, &Wall(500.0));
        }
        let p = s.player(0).unwrap();
        assert!(p.position[0] < 100.0, "walked through the wall: {:?}", p.position);
        assert_eq!(p.position[1], 0.0);
    }

    #[test]
    fn walking_off_an_edge_falls_and_lands() {
        // the ground drops 300 cm at x = 100
        struct Drop;
        impl Ground for Drop {
            fn height(&self, x: f32, _: f32, below_y: f32) -> Option<f32> {
                let h = if x < 100.0 { 0.0 } else { -300.0 };
                (h <= below_y).then_some(h)
            }
        }
        let mut s = sim();
        let mut inputs = HashMap::new();
        inputs.insert(
            0,
            Input {
                stick: [0.0, 1.0],
                camera_yaw: std::f32::consts::FRAC_PI_2,
                ..Default::default()
            },
        );
        let mut fell = false;
        for _ in 0..90 {
            s.step(1.0 / 30.0, &inputs, &Drop);
            fell |= s.player(0).unwrap().state == State::Fall;
        }
        let p = s.player(0).unwrap();
        assert!(fell);
        assert_eq!(p.position[1], -300.0, "{:?}", p);
    }

    /// Axis-aligned solid boxes over a floor plane, with casts and sphere
    /// tests that count surfaces from both sides, like `ddworld::Terrain`.
    struct Boxes {
        floor: f32,
        boxes: Vec<([f32; 3], [f32; 3])>,
    }

    impl Boxes {
        /// A wall block filling x >= `x`, from far below up to `top`.
        fn wall(floor: f32, x: f32, top: f32) -> Self {
            Boxes {
                floor,
                boxes: vec![([x, -5000.0, -5000.0], [x + 2000.0, top, 5000.0])],
            }
        }
    }

    impl Ground for Boxes {
        fn height(&self, x: f32, z: f32, below_y: f32) -> Option<f32> {
            let mut best = (self.floor <= below_y).then_some(self.floor);
            for (lo, hi) in &self.boxes {
                let inside = (lo[0]..=hi[0]).contains(&x) && (lo[2]..=hi[2]).contains(&z);
                if inside && hi[1] <= below_y && best.is_none_or(|b| hi[1] > b) {
                    best = Some(hi[1]);
                }
            }
            best
        }

        fn wall(&self, p: [f32; 3], radius: f32, low: f32, high: f32) -> Option<[f32; 2]> {
            for (lo, hi) in &self.boxes {
                if hi[1] < low || lo[1] > high {
                    continue;
                }
                let c = [p[0].clamp(lo[0], hi[0]), p[2].clamp(lo[2], hi[2])];
                let d = [p[0] - c[0], p[2] - c[1]];
                let l = len2(d);
                if l > 1e-3 {
                    if l < radius {
                        return Some([d[0] / l, d[1] / l]);
                    }
                    continue;
                }
                // inside the block: out through the nearest side
                let sides = [
                    (p[0] - lo[0], [-1.0, 0.0]),
                    (hi[0] - p[0], [1.0, 0.0]),
                    (p[2] - lo[2], [0.0, -1.0]),
                    (hi[2] - p[2], [0.0, 1.0]),
                ];
                return sides.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|s| s.1);
            }
            None
        }

        fn ray(&self, from: [f32; 3], to: [f32; 3]) -> Option<RayHit> {
            let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
            // (t, axis) of every face crossing
            let mut hits: Vec<(f32, usize)> = Vec::new();
            if d[1] != 0.0 {
                hits.push(((self.floor - from[1]) / d[1], 1));
            }
            for (lo, hi) in &self.boxes {
                for axis in 0..3 {
                    if d[axis] == 0.0 {
                        continue;
                    }
                    for v in [lo[axis], hi[axis]] {
                        let t = (v - from[axis]) / d[axis];
                        let on_face = (0..3)
                            .filter(|&k| k != axis)
                            .all(|k| (lo[k] - 1e-3..=hi[k] + 1e-3).contains(&(from[k] + d[k] * t)));
                        if on_face {
                            hits.push((t, axis));
                        }
                    }
                }
            }
            // a surface the cast starts on does not count
            let (t, axis) = hits
                .into_iter()
                .filter(|(t, _)| (1e-4..=1.0).contains(t))
                .min_by(|a, b| a.0.total_cmp(&b.0))?;
            let mut normal = [0.0; 3];
            normal[axis] = -d[axis].signum();
            Some(RayHit {
                point: [from[0] + d[0] * t, from[1] + d[1] * t, from[2] + d[2] * t],
                normal,
            })
        }

        fn sphere(&self, c: [f32; 3], r: f32) -> bool {
            if (c[1] - self.floor).abs() < r {
                return true;
            }
            self.boxes.iter().any(|(lo, hi)| {
                let inside = (0..3).all(|k| (lo[k]..=hi[k]).contains(&c[k]));
                let dist = if inside {
                    (0..3)
                        .map(|k| (c[k] - lo[k]).min(hi[k] - c[k]))
                        .fold(f32::MAX, f32::min)
                } else {
                    (0..3)
                        .map(|k| (lo[k] - c[k]).max(0.0).max(c[k] - hi[k]))
                        .map(|e| e * e)
                        .sum::<f32>()
                        .sqrt()
                };
                dist < r
            })
        }
    }

    const EAST: f32 = std::f32::consts::FRAC_PI_2;

    fn east(jump: bool, sprint: bool) -> Input {
        Input {
            stick: [0.0, 1.0],
            jump,
            sprint,
            camera_yaw: EAST,
            ..Default::default()
        }
    }

    fn near(a: [f32; 3], b: [f32; 3], tol: f32) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() <= tol)
    }

    #[test]
    fn ledge_tops_accepted_and_refused() {
        let lp = LEDGE_SHIPPED;
        // falling past a wall 50 cm ahead, nothing below: tops from just over
        // the low probe (50) up to the down cast's start (200 + 25)
        for (top, ok) in [
            (40.0, false),
            (60.0, true),
            (150.0, true),
            (220.0, true),
            (230.0, false),
        ] {
            let w = Boxes::wall(-1000.0, 50.0, top);
            let l = ledge::detect(&w, &lp, [0.0; 3], EAST, lp.reach_fall);
            assert_eq!(l.is_some(), ok, "top {top}: {l:?}");
            if let Some(l) = l {
                assert!(near(l.edge, [50.0, top, 0.0], 1e-3), "{l:?}");
                assert!(near(l.normal, [-1.0, 0.0, 0.0], 1e-6), "{l:?}");
            }
        }
        // standing: the ledge must be more than 100 cm over the ground below
        assert!(ledge::detect(&Boxes::wall(0.0, 50.0, 90.0), &lp, [0.0; 3], EAST, lp.reach_land).is_none());
        assert!(ledge::detect(&Boxes::wall(0.0, 50.0, 150.0), &lp, [0.0; 3], EAST, lp.reach_land).is_some());
        // reach: a wall 120 cm ahead needs the landing's 150
        let far = Boxes::wall(-1000.0, 120.0, 150.0);
        assert!(ledge::detect(&far, &lp, [0.0; 3], EAST, lp.reach_fall).is_none());
        assert!(ledge::detect(&far, &lp, [0.0; 3], EAST, lp.reach_land).is_some());
        // something on the top where the body goes (a block 40 cm above it)
        let mut blocked = Boxes::wall(-1000.0, 50.0, 150.0);
        blocked.boxes.push(([40.0, 190.0, -100.0], [400.0, 400.0, 100.0]));
        assert!(ledge::detect(&blocked, &lp, [0.0; 3], EAST, lp.reach_fall).is_none());
    }

    #[test]
    fn hang_position_from_the_foot_search() {
        let lp = LEDGE_SHIPPED;
        let w = Boxes::wall(-1000.0, 100.0, 300.0);
        let feet = [70.0, 150.0, 0.0];
        let l = ledge::detect(&w, &lp, feet, EAST, lp.reach_fall).unwrap();
        assert!(near(l.edge, [100.0, 300.0, 0.0], 1e-3), "{l:?}");
        // nothing below within 210 cm: the first 15 cm step out, 210 down
        let (braced, foot) = ledge::foot_point(&w, &l, feet).unwrap();
        assert!(!braced);
        assert!(near(foot, [70.0, 90.0, 0.0], 1e-3), "{foot:?}");
        let h = ledge::hang_pose(&w, &lp, &l, braced, foot);
        assert!(near(h.hands, [78.0, 300.0, 0.0], 1e-3), "{h:?}");
        assert!(near(h.foot, [70.0, 90.0, 0.0], 1e-3), "{h:?}");
        // the body point lies 93 cm from the hands toward the feet
        let d = ((h.body[0] - 78.0).powi(2) + (h.body[1] - 300.0).powi(2)).sqrt();
        assert!((d - 93.0).abs() < 1e-3, "{h:?}");
    }

    #[test]
    fn braced_hang_over_low_ground() {
        let lp = LEDGE_SHIPPED;
        // a 190 cm wall standing on the floor: the body line meets the floor
        let w = Boxes::wall(0.0, 100.0, 190.0);
        let feet = [65.0, 0.0, 0.0];
        let l = ledge::detect(&w, &lp, feet, EAST, lp.reach_land).unwrap();
        let (braced, foot) = ledge::foot_point(&w, &l, feet).unwrap();
        assert!(braced);
        // the step out finds the floor: feet 20 cm over it
        assert!(near(foot, [70.0, 20.0, 0.0], 1e-3), "{foot:?}");
        let h = ledge::hang_pose(&w, &lp, &l, braced, foot);
        // hands 33 cm out; the body kept 33 cm off the wall; feet 100 cm
        // below the hands along that line
        assert!(near(h.hands, [67.0, 190.0, 0.0], 1e-3), "{h:?}");
        assert!(near(h.foot, [67.0, 90.0, 0.0], 1e-2), "{h:?}");
    }

    #[test]
    fn climb_ends_on_the_ledge() {
        let l = ledge::Ledge {
            edge: [100.0, 300.0, 0.0],
            normal: [-1.0, 0.0, 0.0],
            wall_normal: [-1.0, 0.0, 0.0],
        };
        assert_eq!(ledge::climb_end(&l), [110.0, 305.0, 0.0]);
        assert!((ledge::facing(&l) - EAST).abs() < 1e-6);
    }

    #[test]
    fn a_fall_grabs_hangs_and_climbs() {
        let mut s = sim();
        let w = Boxes::wall(-1000.0, 100.0, 300.0);
        {
            let p = s.player_mut(0).unwrap();
            p.position = [40.0, 400.0, 0.0];
            p.yaw = EAST;
            p.state = State::Fall;
        }
        let fwd = HashMap::from([(0, east(false, false))]);
        let none = HashMap::new();
        // fall toward the wall with the stick held until a ledge is taken
        let mut n = 0;
        while s.player(0).unwrap().hold.is_none() {
            s.step(TICK / MOTION_FPS, &fwd, &w);
            n += 1;
            assert!(n < 400, "never grabbed: {:?}", s.player(0).unwrap());
        }
        let p = s.player(0).unwrap();
        // only after dropping 100 cm, and with the low probe under the top
        assert!(p.position[1] < 250.0, "{p:?}");
        assert_eq!(p.state.motion_id(), 0x2D);
        // let go of the stick: the grab ends in a hang at the foot point
        for _ in 0..60 {
            s.step(1.0 / 30.0, &none, &w);
        }
        let p = s.player(0).unwrap();
        assert_eq!(p.state, State::LedgeHang { braced: false });
        assert_eq!(p.state.motion_id(), 0x2F);
        assert!(near(p.position, [70.0, 90.0, 0.0], 1e-3), "{p:?}");
        // push toward the wall: climb, then stand on the top 10 cm in
        s.step(1.0 / 30.0, &fwd, &w);
        assert_eq!(s.player(0).unwrap().state, State::LedgeClimb { braced: false });
        for _ in 0..80 {
            s.step(1.0 / 30.0, &none, &w);
        }
        let p = s.player(0).unwrap();
        assert_eq!(p.state, State::Wait);
        assert!(near(p.position, [110.0, 300.0, 0.0], 1e-3), "{p:?}");
    }

    #[test]
    fn a_fall_without_stick_or_drop_does_not_grab() {
        let w = Boxes::wall(-1000.0, 100.0, 300.0);
        // no stick
        let mut s = sim();
        {
            let p = s.player_mut(0).unwrap();
            p.position = [65.0, 400.0, 0.0];
            p.yaw = EAST;
            p.state = State::Fall;
        }
        for _ in 0..60 {
            s.step(1.0 / 30.0, &HashMap::new(), &w);
            assert!(s.player(0).unwrap().hold.is_none());
        }
        // stick held, but the fall starts beside the ledge: by the time it
        // has dropped 100 cm the top is out of the probes' range
        let mut s = sim();
        {
            let p = s.player_mut(0).unwrap();
            p.position = [65.0, 160.0, 0.0];
            p.yaw = EAST;
            p.state = State::Fall;
        }
        let mut inputs = HashMap::new();
        inputs.insert(0, east(false, false));
        let mut grabbed = false;
        for _ in 0..60 {
            s.step(1.0 / 30.0, &inputs, &w);
            grabbed |= s.player(0).unwrap().hold.is_some();
        }
        let p = s.player(0).unwrap();
        assert!(!grabbed && p.position[1] < 0.0, "{p:?}");
    }

    #[test]
    fn a_normal_jump_never_grabs() {
        let mut s = sim();
        s.player_mut(0).unwrap().position = [60.0, 0.0, 0.0];
        let w = Boxes::wall(0.0, 100.0, 150.0);
        for f in 0..200 {
            let mut inputs = HashMap::new();
            inputs.insert(0, east(f % 40 < 2, false));
            s.step(1.0 / 30.0, &inputs, &w);
            let p = s.player(0).unwrap();
            assert!(p.hold.is_none(), "{p:?}");
        }
    }

    #[test]
    fn a_sprint_jump_landing_grabs_a_low_ledge() {
        let mut s = sim();
        s.player_mut(0).unwrap().position = [-300.0, 0.0, 0.0];
        let w = Boxes::wall(0.0, 100.0, 190.0);
        let mut states = Vec::new();
        for f in 0..400 {
            let mut inputs = HashMap::new();
            // sprint, jump, keep pushing toward the wall until the climb
            if !matches!(states.last(), Some(State::LedgeClimb { .. } | State::Wait)) {
                inputs.insert(0, east((20..22).contains(&f), f < 22));
            }
            s.step(1.0 / 30.0, &inputs, &w);
            let st = s.player(0).unwrap().state;
            if states.last() != Some(&st) {
                states.push(st);
            }
            if st == State::Wait && f > 40 {
                break;
            }
        }
        assert!(states.contains(&State::SprintJumpLand), "{states:?}");
        assert!(
            states.contains(&State::LedgeGrab {
                braced: true,
                phase: GrabPhase::Reach
            }),
            "{states:?}"
        );
        assert!(states.contains(&State::LedgeClimb { braced: true }), "{states:?}");
        let p = s.player(0).unwrap();
        // 10 cm in from the edge, on the top
        assert!(near(p.position, [110.0, 190.0, 0.0], 1e-3), "{p:?}");
    }

    #[test]
    fn pulling_back_lets_go() {
        let mut s = sim();
        let w = Boxes::wall(-1000.0, 100.0, 300.0);
        {
            let p = s.player_mut(0).unwrap();
            p.position = [40.0, 400.0, 0.0];
            p.yaw = EAST;
            p.state = State::Fall;
        }
        let fwd = HashMap::from([(0, east(false, false))]);
        while s.player(0).unwrap().hold.is_none() {
            s.step(TICK / MOTION_FPS, &fwd, &w);
        }
        for _ in 0..60 {
            s.step(1.0 / 30.0, &HashMap::new(), &w);
        }
        assert!(matches!(s.player(0).unwrap().state, State::LedgeHang { .. }));
        // a half push away is not enough; a full one lets go
        let back = |k: f32| {
            HashMap::from([(
                0,
                Input {
                    stick: [0.0, -k],
                    camera_yaw: EAST,
                    ..Default::default()
                },
            )])
        };
        s.step(TICK / MOTION_FPS, &back(0.5), &w);
        assert!(matches!(s.player(0).unwrap().state, State::LedgeHang { .. }));
        s.step(TICK / MOTION_FPS, &back(1.0), &w);
        let p = s.player(0).unwrap();
        assert_eq!(p.state, State::Fall);
        assert!(p.hold.is_none());
        assert_eq!(p.velocity, [-ledge::LET_GO_SPEED, 0.0, 0.0]);
    }

    #[test]
    fn a_vanished_ledge_is_let_go() {
        let w = Boxes::wall(-1000.0, 100.0, 300.0);
        let l = ledge::detect(&w, &LEDGE_SHIPPED, [70.0, 150.0, 0.0], EAST, 100.0).unwrap();
        assert!(ledge::holds(&w, &l));
        let empty = Boxes {
            floor: -1000.0,
            boxes: vec![],
        };
        assert!(!ledge::holds(&empty, &l));
    }

    #[test]
    fn ledge_param_reads_the_adjust_table() {
        let mut t = vec![0.0f32; 0x192];
        for (id, v) in [
            (0x5A, 150.0),
            (0x5B, 200.0),
            (0x5C, 50.0),
            (0x5D, 100.0),
            (0x5E, 100.0),
            (0x5F, 25.0),
            (0x17C, 30.0),
            (0x17D, 20.0),
            (0x189, 93.0),
            (0x18A, 22.0),
            (0x190, 60.0),
            (0x191, 33.0),
        ] {
            t[id] = v;
        }
        assert_eq!(LedgeParam::from_adjust(&t), Some(LEDGE_SHIPPED));
        assert_eq!(LedgeParam::from_adjust(&t[..0x191]), None);
    }

    #[test]
    fn relaxed_run_uses_the_town_motion_and_speed() {
        let mut s = sim();
        s.player_mut(0).unwrap().relaxed = true;
        let mut inputs = HashMap::new();
        inputs.insert(0, fwd());
        s.step(10.0 / MOTION_FPS, &inputs, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert_eq!(p.state.motion_id(), 0x55);
        // 12.0 x 0.8471 = 10.165 cm per frame, as recorded (uPlayer +0x2530)
        assert!((p.speed - 10.165).abs() < 1e-3, "{}", p.speed);
        assert!((p.position[2] - 101.65).abs() < 1e-2, "{:?}", p.position);
    }

    #[test]
    fn sprint_start_ramps_from_the_run_speed_to_the_sprint_speed() {
        let mut s = sim();
        let mut inputs = HashMap::new();
        inputs.insert(0, fwd());
        s.step(10.0 / MOTION_FPS, &inputs, &Flat(0.0));
        inputs.insert(
            0,
            Input {
                sprint: true,
                ..fwd()
            },
        );
        let mut speeds = Vec::new();
        for _ in 0..20 {
            s.step(1.0 / MOTION_FPS, &inputs, &Flat(0.0));
            speeds.push(s.player(0).unwrap().speed);
        }
        // never slower than the run it started from, sprint speed by frame 5
        assert!(speeds.iter().all(|&v| v >= MOVE_SHIPPED.run - 1e-3), "{speeds:?}");
        assert!(speeds[1] < MOVE_SHIPPED.sprint, "{speeds:?}");
        assert!((speeds[5] - MOVE_SHIPPED.sprint).abs() < 1e-3, "{speeds:?}");
        // the start motion (14 frames at the locomotion rate) gives way to the sprint
        assert_eq!(s.player(0).unwrap().state, State::Sprint);
    }

    #[test]
    fn move_param_reads_the_adjust_table() {
        let mut t = vec![0.0f32; 0x110];
        t[ADJUST_WALK] = 5.0;
        t[ADJUST_WALK_RELAXED] = 5.0;
        t[ADJUST_RUN] = 15.0;
        t[ADJUST_RUN_RELAXED] = 12.0;
        t[ADJUST_SPRINT] = 22.5;
        assert_eq!(MoveParam::from_adjust(&t), Some(MOVE_SHIPPED));
        assert_eq!(MoveParam::from_adjust(&t[..0x100]), None);
    }

    /// Flat ground with a wall plane x = 100 (facing -X) reported by `wall`
    /// only, like an open-sided prop in the collision mesh.
    struct Shell;
    impl Ground for Shell {
        fn height(&self, _: f32, _: f32, below_y: f32) -> Option<f32> {
            (0.0 <= below_y).then_some(0.0)
        }
        fn wall(&self, p: [f32; 3], radius: f32, _: f32, _: f32) -> Option<[f32; 2]> {
            ((100.0 - p[0]).abs() < radius).then_some([-1.0, 0.0])
        }
    }

    #[test]
    fn wall_triangles_block_and_slide() {
        let mut s = sim();
        let mut inputs = HashMap::new();
        // run diagonally into the wall: blocked in x, keeps moving in z
        inputs.insert(
            0,
            Input {
                stick: [-0.7071, 0.7071],
                ..Default::default()
            },
        );
        s.step(2.0, &inputs, &Shell);
        let p = s.player(0).unwrap();
        assert!(p.position[0] <= 100.0 - BODY_RADIUS + 1e-3, "{:?}", p.position);
        assert!(p.position[2] > 200.0, "did not slide: {:?}", p.position);
    }

    #[test]
    fn root_motion_wraps_at_the_loop() {
        let w = run();
        let d = w.delta(8.0, 4.0);
        assert!((d[2] - 40.0).abs() < 1e-3, "{d:?}");
    }
}

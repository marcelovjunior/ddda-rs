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
//!   airborne 23, ledge hang 47, ledge climb 48 (`cPlAct*::vf05`);
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
//!   sets 1-8); here it is a per-player capability.
//!
//! Approximations are marked *placeholder* or *assumed*.

use std::collections::HashMap;

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
    LedgeHang,
    LedgeClimb,
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
            State::LedgeHang => 47,
            State::LedgeClimb => 48,
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
                | State::LedgeHang
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
    /// Ledge being climbed: hanging and standing feet positions.
    climb: Option<([f32; 3], [f32; 3])>,
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

/// Ground under a point, from the world (collision).
pub trait Ground {
    /// Highest ground height (cm) at (x, z) at or below `below_y`.
    fn height(&self, x: f32, z: f32, below_y: f32) -> Option<f32>;
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
/// Ledge grab: look-ahead (cm) and the ledge-top heights above the feet that
/// can be grabbed. *Placeholder*: the original's detection
/// (`cPlCliffHangBegin`) is long geometry code not traced yet.
pub const LEDGE_PROBE: f32 = 60.0;
pub const LEDGE_MIN: f32 = 90.0;
pub const LEDGE_MAX: f32 = 230.0;
/// Hands-to-feet distance while hanging, cm. *Placeholder*.
pub const HANG_DROP: f32 = 190.0;

pub struct Sim {
    pub players: Vec<Player>,
    pub motions: HashMap<u16, RootMotion>,
    pub jump_normal: JumpParam,
    pub jump_sprint: JumpParam,
    pub move_param: MoveParam,
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
            climb: None,
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
            | State::Land { .. } => {
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
                // a landing plays out unless the player moves
                if matches!(state, State::Land { .. })
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
                self.air_move(i, moving.then_some(input_yaw), ground, dt);
            }
            State::LedgeHang => {
                let p = &mut self.players[i];
                p.frame += dt;
                // *placeholder*: forward or jump climbs, back lets go
                let back = moving && (input_yaw - p.yaw).cos() < -0.5;
                if back {
                    p.climb = None;
                    p.velocity = [0.0; 3];
                    p.jump = jn;
                    Self::set_state(p, State::Fall);
                } else if pressed || (moving && p.frame > 6.0) {
                    Self::set_state(p, State::LedgeClimb);
                }
            }
            State::LedgeClimb => {
                let len = self.period_of(State::LedgeClimb.motion_id());
                let p = &mut self.players[i];
                p.frame += dt;
                if let Some((from, to)) = p.climb {
                    // *placeholder*: rise during the first 60%, step onto the top after
                    let t = (p.frame / len).min(1.0);
                    let up = (t / 0.6).min(1.0);
                    let fwd = ((t - 0.6) / 0.4).clamp(0.0, 1.0);
                    p.position = [
                        from[0] + (to[0] - from[0]) * fwd,
                        from[1] + (to[1] - from[1]) * up,
                        from[2] + (to[2] - from[2]) * fwd,
                    ];
                }
                if p.frame >= len {
                    if let Some((_, to)) = p.climb.take() {
                        p.position = to;
                    }
                    Self::set_state(p, State::Wait);
                }
            }
        }
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
        let wall = ground.height(next[0], next[2], p.position[1] + STEP_UP).is_none()
            && ground.height(next[0], next[2], f32::MAX).is_some();
        if wall {
            // ledge grab: a top within reach in front of the player
            let (s, c) = p.yaw.sin_cos();
            let probe = [p.position[0] + LEDGE_PROBE * s, p.position[2] + LEDGE_PROBE * c];
            if let Some(top) = ground.height(probe[0], probe[1], p.position[1] + LEDGE_MAX) {
                let rise = top - p.position[1];
                if (LEDGE_MIN..=LEDGE_MAX).contains(&rise) {
                    let hang = [p.position[0], top - HANG_DROP, p.position[2]];
                    p.climb = Some((hang, [probe[0], top, probe[1]]));
                    p.position = hang;
                    p.velocity = [0.0; 3];
                    Self::set_state(p, State::LedgeHang);
                    return;
                }
            }
            next[0] = p.position[0];
            next[2] = p.position[2];
            p.velocity[0] = 0.0;
            p.velocity[2] = 0.0;
        }
        // landing: the ground under the new position, reached on the way down
        match ground.height(next[0], next[2], p.position[1].max(next[1]) + 1.0) {
            Some(g) if next[1] <= g && p.velocity[1] <= 0.0 => {
                let standing = matches!(p.state, State::Air { standing: true, .. }) || steer.is_none();
                p.position = [next[0], g, next[2]];
                p.velocity = [0.0; 3];
                Self::set_state(p, State::Land { standing });
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
        m.insert(48, RootMotion::none(36));
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

    #[test]
    fn jumping_at_a_ledge_climbs_it() {
        let mut s = sim();
        s.player_mut(0).unwrap().position = [60.0, 0.0, 0.0];
        let mut climbed = false;
        for f in 0..200 {
            let mut inputs = HashMap::new();
            inputs.insert(
                0,
                Input {
                    stick: [0.0, 1.0],
                    jump: f < 2,
                    camera_yaw: std::f32::consts::FRAC_PI_2,
                    ..Default::default()
                },
            );
            s.step(1.0 / 30.0, &inputs, &Wall(150.0));
            let p = s.player(0).unwrap();
            if p.position[1] == 150.0 && p.position[0] >= 100.0 {
                climbed = true;
                break;
            }
        }
        assert!(climbed, "{:?}", s.player(0).unwrap());
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

    #[test]
    fn root_motion_wraps_at_the_loop() {
        let w = run();
        let d = w.delta(8.0, 4.0);
        assert!((d[2] - 40.0).abs() < 1e-3, "{d:?}");
    }
}

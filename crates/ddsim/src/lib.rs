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

pub mod attack;
pub mod skills;
pub mod ledge;
pub mod stamina;
pub mod sweep;

pub use attack::{Attack, AttackParam, BowInput, BowParam, BowPhase, BowState, BowUnsupported, ShotPhase, Weapon, ATTACK_SHIPPED};
pub use ledge::{LedgeParam, LEDGE_SHIPPED};
pub use stamina::{StaminaRow, StaminaTables, WeightTables};

/// Motion frames per second. *Assumed* 30 (not traced). Supporting
/// evidence: the recorded jump peaks 0.325 s after takeoff (9.5 frames at
/// 30 fps gives 0.317 s), and non-locomotion motions advance 30 frames/s.
pub const MOTION_FPS: f32 = 30.0;

/// The run and sprint rows' motion speed at load class 1 (1.15), for tests
/// and docs; the simulation reads the player's row (`Sim::time_rate`).
///
/// The player's own time runs faster than game time while walking, running
/// or sprinting: `uObjModel::vf90` (0x00885E30) sets the unit's
/// `mDeltaTime` (+0x14) to sMain's x sWorkRate rates x the product of
/// +0xEE4 .. +0xEF8, and `0x00B81920` stores in +0xEE4 the `mMotSpd` of the
/// current action's stamina row (Walk, Run or Dash table by load class; 1
/// for other actions). The traces show exactly 1.15 x sMain's delta in runs
/// and sprints (docs/ACCEPTANCE.md). The per-frame velocity is not scaled,
/// so both the motion frame and the movement advance that much faster. The
/// original applies the factor two game frames after the action changes
/// (the stamina update runs after the actions); that lag is not modelled.
pub const LOCOMOTION_RATE: f32 = 1.15;

/// Stamina of the recorded level-7 Arisen (the acceptance traces start at
/// 640, full). *Measured*: the maximum's derivation (`0x00782390`) is not
/// modelled; hosts set `Player::stamina_max`.
pub const STAMINA_MAX_RECORDED: f32 = 640.0;

/// Factor on walk and run speed (uPlayer +0xDA8, multiplied in
/// `0x00B80690` / `0x00B80710`). Recorded as 0.8471090 on the ground for a
/// male Arisen; its writer is not traced. *Measured*.
pub const GROUND_SPEED_FACTOR: f32 = 0.847_109;

/// Sprint start reaches sprint speed this many motion frames after it
/// begins, rising linearly. Used only when the sprint-start motion has no
/// curve 9 (`SPEED_CURVE`); the game's motion (m0000_co slot 43) ramps 0 to
/// 1 over motion frames 0-5, i.e. 4.35 game frames at the locomotion rate,
/// as recorded (4-5 frames).
pub const SPRINT_RAMP_FRAMES: f32 = 5.0;

/// The motion curve the sprint start and the sprint stop scale their speed
/// by (`0x010571C0(9)` in `cPlActDashBegin::vf06` 0x00ACAA50 and
/// `cPlActRunEnd::vf06` 0x00ACB6C0).
pub const SPEED_CURVE: usize = 9;

/// The motion curve the ledge climb moves the player toward its target by
/// (`0x010571C0(10)` in `cPlCliffHangClimb::vf06` 0x00ADDC00); curve 9
/// raises the target.
pub const CLIMB_POSITION_CURVE: usize = 10;

/// The walk and run motions of the common set that a new walk or run
/// phase-syncs to (0x018763D0: 3, 4, 0x54, 0x55; the weapon sets' 0x603,
/// 0x604, 0x654, 0x655 are not used by the simulation).
pub const LOCOMOTION_MOTIONS: [u16; 4] = [3, 4, 0x54, 0x55];

/// Start frames when not phase-synced, field walk, town walk, run, for the
/// feet test 0x00AC5FB0 returning false (cPlActWalk::vf05: 18 field, 0
/// town; cPlActRun::vf05: 20). When it returns true they are 0, 17 and 0.
pub const LOCOMOTION_START_FEET_NOT_AHEAD: [f32; 3] = [18.0, 0.0, 20.0];

/// The motion event (group 0, bit 16) that lets the stick cancel an action
/// into movement (`MoveCancel` in `Sim::move_cancel_allowed`).
pub const MOVE_CANCEL_GROUP: usize = 0;
pub const MOVE_CANCEL_BIT: u32 = 0x1_0000;

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
    /// Largest horizontal speed a fall starts with (adjust 0xD1, read by
    /// cPlActFall::vf05 0x00ACCDD0), cm per frame.
    pub fall_max: f32,
    /// The model's turn toward the movement heading, radians per frame
    /// (adjust 3 in degrees; `uPlayerBase::vf90` 0x00B61A20 stores it at
    /// uPlayer +0x2584 for the rotator 0x007975D0).
    pub turn: f32,
    /// A landing this far (cm) below the fall's start is a hard landing
    /// with damage (adjust 0x55 = -1000, compared with the signed drop by
    /// `0x00B78A00`; 0x1DC = -1500 with the Grace augment, not modelled).
    pub hard_landing: f32,
    /// Its damage: `base + rate x (drop - hard_landing)` (adjust 0x56 = 200,
    /// 0x57 = 80 percent; `0x00B78BF0`).
    pub fall_damage_base: f32,
    pub fall_damage_rate: f32,
}

/// Indices of the hard landing's threshold and damage in `rAdjustParam`.
pub const ADJUST_HARD_LANDING: usize = 0x55;
pub const ADJUST_FALL_DAMAGE_BASE: usize = 0x56;
pub const ADJUST_FALL_DAMAGE_RATE: usize = 0x57;

/// Index of the model's turn rate (degrees per frame) in the `rAdjustParam` table.
pub const ADJUST_TURN_RATE: usize = 3;

/// Index of the fall's starting speed cap in the `rAdjustParam` table.
pub const ADJUST_FALL_MAX: usize = 0xD1;

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
            fall_max: get(ADJUST_FALL_MAX)?,
            turn: get(ADJUST_TURN_RATE)?.to_radians(),
            hard_landing: -get(ADJUST_HARD_LANDING)?,
            fall_damage_base: get(ADJUST_FALL_DAMAGE_BASE)?,
            fall_damage_rate: get(ADJUST_FALL_DAMAGE_RATE)? / 100.0,
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
    fall_max: 7.0,
    turn: 22.5 * std::f32::consts::PI / 180.0,
    hard_landing: 1000.0,
    fall_damage_base: 200.0,
    fall_damage_rate: 0.8,
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

/// `pljumpparam` rows 0x36, 0x37 and 0x47 as shipped (the air attacks).
pub const AIR_ROWS_SHIPPED: [(usize, JumpParam); 3] = [
    (0x36, JumpParam { speed_y: 0.0, speed_z: 0.0, gravity: -2.0, add_move_xz: 4.25, damping_z: 0.25 }),
    (0x37, JumpParam { speed_y: 0.0, speed_z: 0.0, gravity: -8.0, add_move_xz: 0.0, damping_z: 0.25 }),
    (0x47, JumpParam { speed_y: 0.0, speed_z: 0.0, gravity: -2.0, add_move_xz: 4.25, damping_z: 0.0 }),
];

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
    /// Stopping after a sprint (`cPlActRunEnd`, action 3).
    SprintStop,
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
    /// The landing after a fall of `MoveParam::hard_landing` or more:
    /// `cPlActDmgGround` (action 0x19) variant 3, motion 0x107.
    HardLanding,
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
    /// A sword attack or staff strike (`attack`).
    Attack {
        attack: Attack,
    },
    /// The staff's light attack, `cPlActWpnWandShot` (`attack::shot`).
    Shot {
        phase: ShotPhase,
    },
    /// Secondary bow action, with the runtime-selected lower-layer motion.
    Bow { phase: BowPhase, motion: u16 },
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
    /// The push-out rank the state's action writes to actor +0x1DF0 (the
    /// character push-out 0x004797E0: the higher rank gives way): 13 for
    /// `cPlActWait` (0x00AC9E67; 14 while its turn motion plays, not
    /// modelled), 14 for `cPlActWalk` (0x00ACA205), 15 for the run, the
    /// sprint and its stop, jumps, falls and landings (0x00ACA5E5,
    /// 0x00ACAF2F, 0x00ACB671, 0x00ACC68A, 0x00ACD19E, 0x00ACD5A7, ...),
    /// 0 (never pushed) while on a ledge (`cPlCliffHang*` vf05 0x00ADA908,
    /// 0x00ADCD7B, 0x00ADDBC0), 14 for the sword attacks and staff strikes
    /// (S1_1 0x00B3C795, S1_2 0x00B3C9B9, S1_3 0x00B3CC09, LandL 0x00B3D5F6,
    /// Strike1 0x00B51228, Strike2 0x00B5141B), 13 for WandShot (0x00B4E200).
    pub fn push_rank(self) -> u32 {
        match self {
            State::Wait => 13,
            State::Walk { .. } | State::Attack { .. } => 14,
            State::Shot { .. } | State::Bow { .. } => attack::shot::PUSH_RANK,
            State::LedgeGrab { .. } | State::LedgeHang { .. } | State::LedgeClimb { .. } => 0,
            _ => 15,
        }
    }

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
            // cPlActRunEnd::vf05 (0x00ACB570), after the sprint's 0x14 / 0x4F
            State::SprintStop => 0x15,
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
            // cPlActDmgGround::vf05 (0x00AD2630): bank 1 = m0000_dm
            State::HardLanding => 0x107,
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
            // bank 7: the weapon's `_at` list (`Sim::weapon_motions`)
            State::Attack { attack } => attack.motion(),
            State::Shot { phase } => phase.motion(),
            State::Bow { motion, .. } => motion,
        }
    }

    /// The gaits: the states whose facing turns toward the movement heading.
    pub fn gait(self) -> bool {
        matches!(
            self,
            State::Wait
                | State::Walk { .. }
                | State::Run { .. }
                | State::SprintStart
                | State::Sprint
                | State::SprintStop
        )
    }

    /// Whether the attack transition lists are read in this state: on the
    /// ground (the lists' 0x301 rows; the air rows are not modelled).
    pub fn on_ground_for_attacks(self) -> bool {
        matches!(
            self,
            State::Wait
                | State::Walk { .. }
                | State::Run { .. }
                | State::SprintStart
                | State::Sprint
                | State::SprintStop
                | State::Land { .. }
                | State::SprintJumpLand
        ) || matches!(self, State::Attack { attack } if !attack.air())
            || matches!(self, State::Shot { phase } if !phase.air())
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
                | State::Bow { phase: BowPhase::Ready, .. }
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
    /// The light attack button is held (a press is the rising edge).
    pub light: bool,
    /// The heavy attack button is held.
    pub heavy: bool,
    /// Secondary bow command 7 held (uPlayer +0x32E4 bit 0x8).
    pub bow: bool,
    /// Bow-mode toggle key held; its rising edge toggles this seat's mode.
    pub bow_toggle: bool,
    /// Camera yaw (radians, 0 looks along +Z).
    pub camera_yaw: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub id: PlayerId,
    /// Feet position, cm, stage frame.
    pub position: [f32; 3],
    /// Facing (radians, 0 faces +Z): the model's yaw (uPlayer +0x257C).
    pub yaw: f32,
    /// Movement heading (uPlayer +0x2654): follows the stick at once while
    /// it is held; the facing turns toward it at `MoveParam::turn`.
    pub heading: f32,
    /// Fall damage not yet taken by the host (HP), from hard landings.
    pub fall_damage: f32,
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
    /// cPlayerInfo +0x2A8 and its maximum.
    pub stamina: f32,
    pub stamina_max: f32,
    /// Body and load classes picking stamina rows (uPlayer +0x34C8 /
    /// +0x34CC). *Assumed* 1 by default: the weights that select them
    /// (`0x00B81120`, `0x00B81270`) are not modelled; 1 matches the
    /// recordings.
    pub body_class: usize,
    pub load_class: usize,
    /// Stamina ran out (crossed to 0) at least once since the host cleared
    /// it. What the original does next (`cPlActTired`) is not modelled.
    pub ran_out: bool,
    /// The ring of safe ground positions (uPlayerBase +0x4A60, 4 slots,
    /// newest first; `Sim::record_safe`).
    pub safe: [[f32; 3]; 4],
    /// Frames airborne without moving down (uPlayerBase +0x4AA4).
    pub stuck: f32,
    /// Times the body was warped back to a safe position (a host may fade
    /// the screen as the original does for the Arisen).
    pub warps: u32,
    /// The player's own time per game time in the current `Sim::step` (the
    /// unit delta factor, `LOCOMOTION_RATE` while walking, running or
    /// sprinting) and the factors the last two steps ended with: a factor
    /// applies two engine frames after the action that sets it, as in the
    /// traces (the stamina update writes uPlayer +0xEE4 after the actions,
    /// `uObjModel::vf90` reads it at the next frame's start).
    pub unit_rate: f32,
    rate_queue: [f32; 2],
    /// The weapon whose basic attacks the player uses (`attack`).
    pub weapon: Weapon,
    /// Secondary slot: a Strider keeps its daggers in `weapon` and its
    /// shortbow (item 0x1F8 in the opening) here. Equipment is host-owned.
    pub secondary_weapon: Weapon,
    pub bow: Option<BowState>,
    pub bow_latched: bool,
    bow_toggle_held: bool,
    /// Host-owned check/release context; held/toggle come from Input.
    pub bow_context: BowInput,
    /// How far the equipped weapons' level is above the player's (item
    /// +0x66 minus cPlayerInfo +0x700), for the attack cost tier
    /// (`attack::tier`). Set by the host; 0 by default.
    pub weapon_level_gap: i32,
    /// The heading an attack turns to when it starts: the target's
    /// direction, set by the host (the original takes it from its target,
    /// `0x00B79090`, whose choice is *not traced*). Without one, the
    /// stick's direction while it is held.
    pub attack_heading: Option<f32>,
    /// The heading the playing attack turns to (`0x007975D0`).
    turn_to: Option<f32>,
    /// The request waiting in the attack slot (uPlayer +0x38B8[1]).
    pub request: Option<attack::Request>,
    /// Attack actions started so far (each one frees its hit history).
    pub attacks_started: u32,
    /// WandShot volleys fired so far (the host spawns their shells).
    pub volleys_fired: u32,
    /// The playing WandShot's state.
    pub shot: attack::ShotState,
    pub skills: skills::Skills,
    light_held: bool,
    heavy_held: bool,
}

impl Player {
    /// Action constructors set the push-out rank independently of motion:
    /// Broad Cut 0x00B3E0E0, Tusk Toss 0x00B3E7D0 (Steam 2017-12-13).
    pub fn push_rank(&self) -> u32 {
        // cPlActWpnBow::vf05 0x00B16FE0 sets 13 independently of layers.
        if self.bow.is_some() { return 13; }
        match self.skills.active.map(|a| a.id) {
            Some(41) => 15,
            Some(43) => 14,
            _ => self.state.push_rank(),
        }
    }
    /// The active skill selects a per-character bank; ordinary actions keep
    /// their existing motion IDs.
    pub fn motion_id(&self) -> u16 {
        self.skills.active.map_or_else(|| self.state.motion_id(), |a| a.motion)
    }
    /// Positive item stamina restoration, 0x00B81E30 (DDDA.exe
    /// 2017-12-13 Steam). Curse caps stamina at 60% of maximum. The
    /// tired-action message is not modelled by ddsim yet; ran_out is a
    /// historical event flag and stays set until its host acknowledges it.
    pub fn recover_item_stamina(&mut self, amount: f32, cursed: bool) -> f32 {
        if !amount.is_finite() || amount <= 0.0 { return 0.0; }
        let before = self.stamina;
        let maximum = self.stamina_max * if cursed { 0.6 } else { 1.0 };
        self.stamina = (self.stamina + amount).min(maximum);
        self.stamina - before
    }
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
    /// The motion's curves (`mtlmt::MotionList::curves`), read by index.
    pub curves: Vec<mtlmt::Curve>,
    /// The motion's event groups (`mtlmt::MotionList::events`).
    pub events: Vec<Vec<mtlmt::EventRun>>,
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
            curves: Vec::new(),
            events: Vec::new(),
        }
    }

    /// The same motion with its event groups (`mtlmt::MotionList::events[slot]`).
    pub fn with_events(mut self, events: Vec<Vec<mtlmt::EventRun>>) -> Self {
        self.events = events;
        self
    }

    /// Event bits of `group` over whole motion frames `from`..`to`
    /// (`mtlmt::event_bits`); 0 without event data.
    pub fn event_bits(&self, group: usize, from: i32, to: i32) -> u32 {
        self.events
            .get(group)
            .map_or(0, |runs| mtlmt::event_bits(runs, from, to))
    }

    /// The same motion with its curves (`mtlmt::MotionList::curves[slot]`).
    pub fn with_curves(mut self, curves: Vec<mtlmt::Curve>) -> Self {
        self.curves = curves;
        self
    }

    /// Curve `k` at motion frame `frame`, if the motion has it and it has keys
    /// (the game's `0x010571C0(k)`; a curve without keys reads 0 there).
    pub fn curve(&self, k: usize, frame: f32) -> Option<f32> {
        self.curves
            .get(k)
            .filter(|c| !c.keys.is_empty())
            .map(|c| c.value(frame))
    }

    /// A motion of `frame_count` frames without root movement.
    pub fn none(frame_count: u32) -> Self {
        RootMotion {
            frame_count,
            track: None,
            curves: Vec::new(),
            events: Vec::new(),
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

    /// Nearest ray hit and its material flag word (camera getter
    /// 0x007c6f90). Query first, then filter the returned hit; skipping
    /// flagged triangles before the query changes the camera's behavior.
    /// Geometry-only implementations retain their existing ray behavior.
    fn ray_with_flags(&self, from: [f32; 3], to: [f32; 3]) -> Option<(RayHit, u32)> {
        self.ray(from, to).map(|hit| (hit, 0))
    }

    /// Whether any surface lies within `radius` of `center` (the
    /// original's sphere test `0x00FAF4B0`).
    fn sphere(&self, _center: [f32; 3], _radius: f32) -> bool {
        false
    }

    /// Adds the collision triangles that may lie within the box `lo`-`hi`
    /// to `out`; false when the world has no triangle data. With triangles
    /// the character moves by the capsule sweep (`sweep`); without, by the
    /// older point-and-cylinder rules (`height`, `wall`).
    fn triangles_in(&self, _lo: [f32; 3], _hi: [f32; 3], _out: &mut Vec<sweep::Face>) -> bool {
        false
    }
}

/// The ground check's drop per frame while on the ground: cScrAdjust's
/// `mGroundCheckOffset` (adjust 0x7E, -20, written by `uPlayerBase::vf90`)
/// times the unit delta, added to the move's height by `0x0079E200`. The
/// sweep then meets the floor, which keeps the character on the ground.
pub const GROUND_CHECK_DROP: f32 = 20.0;

/// The fall check below the feet: `0x00B6E5C0` falls when the ground cast
/// from 100 cm above (`0x0084FEA0`, 1000 cm down) is more than 30 cm below.
pub const FALL_CHECK_UP: f32 = 100.0;

/// The body's radius against scenery, cm: the player's scenery-adjust
/// volume is a sphere of radius 35 on the model origin (`collision\pl\m0001`,
/// rObjCollision group 109, `mIsScrAdj`, node 0: shape 1, joint -1). The
/// original resolves it with `cScrAdjust` (uObjModel +0x19C0: `mCapsule`
/// +0xD0, ground check offset adjust 0x7E = -20, axis check type 2) through
/// a generic capsule sweep-and-slide (0x0079E200 -> 0x010B4960 ->
/// 0x010B3000), which is not translated.
pub const BODY_RADIUS: f32 = 35.0;
/// Top of the body capsule's segment above the feet, cm. Read live from
/// the Arisen (claude-4, board #931): `cScrAdjust::mCapsule` runs from 35
/// to 145 cm above the feet with radius 35 (the body spans 0-180 cm). ddsim
/// tests walls along the segment from `STEP_UP` (not 35: it has no sweep
/// that rides over steps, *placeholder*) to here.
pub const BODY_HEIGHT: f32 = 145.0;

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

/// Whether the ground at `at` (a point on it) is a floor: the surface a cast
/// down through it meets faces up at least `ledge::FLOOR_MIN_NY`. Worlds
/// without casts count everything as floor.
fn is_floor(ground: &dyn Ground, at: [f32; 3]) -> bool {
    ground
        .ray([at[0], at[1] + 1.0, at[2]], [at[0], at[1] - 1.0, at[2]])
        .is_none_or(|h| h.normal[1] >= ledge::FLOOR_MIN_NY)
}

/// The horizontal step `m` (x, z) of a velocity-driven move, projected onto
/// the ground plane under `feet` and flattened again: on a slope of angle
/// t along the move it shrinks to m cos^2 t. *Measured*, not traced: in the
/// run and sprint traces (docs/ACCEPTANCE.md) the horizontal step over
/// |velocity| x unit delta is 1 / (1 + slope^2) within 0.5% over slopes
/// -0.25..0.29 (e.g. 0.957 at 0.208), which is what removing the velocity's
/// component along the ground normal gives. Floors only (`ledge::FLOOR_MIN_NY`).
fn along_ground(ground: &dyn Ground, feet: [f32; 3], m: [f32; 2]) -> [f32; 2] {
    let Some(h) = ground.ray(
        [feet[0], feet[1] + 1.0, feet[2]],
        [feet[0], feet[1] - STEP_DOWN, feet[2]],
    ) else {
        return m;
    };
    let n = h.normal;
    if n[1] < ledge::FLOOR_MIN_NY {
        return m;
    }
    let d = m[0] * n[0] + m[1] * n[2];
    [m[0] - d * n[0], m[1] - d * n[2]]
}

/// Stick magnitude above which the player moves. *Placeholder*.
pub const MOVE_THRESHOLD: f32 = 0.2;
/// Highest step the feet climb while on the ground, cm. *Placeholder*.
pub const STEP_UP: f32 = 60.0;
/// Drop below the feet that starts a fall instead of following the ground,
/// cm: `0x00B6E5C0` enters cPlActFall when the character is not grounded
/// and the ground cast from 100 cm above the feet (`0x0084FEA0`) is more
/// than 30 cm (0x014EA160) below them. Here "grounded" is standing on a
/// floor triangle (`ledge::FLOOR_MIN_NY`, *assumed*): steeper ground is
/// never walked on, so a slope is fallen down whatever the step size.
pub const STEP_DOWN: f32 = 30.0;
/// Gravity while falling (cm/frame²): the normal jump's. *Placeholder* until
/// `cPlActFall` is traced.
pub const FALL_GRAVITY: f32 = -2.0;
/// A fall may grab a ledge only after dropping more than this, cm
/// (`0x00B6FA00`: in cPlActFall a ledge is refused while uPlayer +0x207C,
/// the fall distance, is >= -100; constant 0x0161C308).
pub const LEDGE_FALL_MIN_DROP: f32 = 100.0;

/// A fall of more than this (cm, uPlayer +0x207C below -20000) in the air
/// warps the body back to a safe position (`0x00B63520`; not on stage
/// 0x259).
pub const LOST_FALL: f32 = 20000.0;
/// Frames in the air without moving down before the body is taken as stuck
/// and warped back (`0x00B63520`, uPlayerBase +0x4AA4).
pub const STUCK_FRAMES: f32 = 90.0;
/// A new safe position needs this distance (cm) from the newest one
/// (`0x00B62980`: squared 10000).
pub const SAFE_STEP: f32 = 100.0;
/// The ground under a safe position may be at most this far below the
/// feet (`0x00B62980`: the ground hit at or above y - 50).
pub const SAFE_GROUND_BELOW: f32 = 50.0;
/// The warp's choice between the two newest positions (`0x00B62D20`:
/// squared 40000).
pub const SAFE_NEAR: f32 = 200.0;

pub struct Sim {
    pub players: Vec<Player>,
    pub motions: HashMap<u16, RootMotion>,
    pub jump_normal: JumpParam,
    pub jump_sprint: JumpParam,
    /// `pljumpparam` rows the air attacks load (`Attack::jump_row`), by row.
    pub air_rows: HashMap<usize, JumpParam>,
    pub move_param: MoveParam,
    pub ledge_param: LedgeParam,
    pub stamina_tables: StaminaTables,
    pub weight_tables: WeightTables,
    /// The capsule sweep's values (`sweep::SweepParam`).
    pub sweep_param: sweep::SweepParam,
    /// The attacks' values (`attack::AttackParam`).
    pub attack_param: AttackParam,
    /// Each weapon's motions by id (banks 6 and 7: its `_co` and `_at`
    /// lists); a player's weapon motions are looked up here before
    /// [`Sim::motions`].
    pub weapon_motions: HashMap<Weapon, HashMap<u16, RootMotion>>,
    /// Shortbow record zero, supplied from the install by the host.
    pub bow_param: Option<BowParam>,
    pub skill_definitions: HashMap<u32, skills::Definition>,
    /// Fraction of a frame not yet simulated.
    pending: f32,
}

/// The landing is the standing one (motion 15) unless the stick points
/// within 45 degrees of the facing (moving landing, motion 12).
// Translated from 0x00ACD2C0 (DDDA.exe 2017-12-13 Steam), cPlActLand::vf05:
// uPlayer +0x32C0 (stick held) and the angle between +0x2578 and +0x32C4
fn landing_stands(yaw: f32, steer: Option<f32>) -> bool {
    steer.is_none_or(|s| {
        let d = (yaw - s).rem_euclid(std::f32::consts::TAU);
        d.min(std::f32::consts::TAU - d) > std::f32::consts::FRAC_PI_4
    })
}

fn lookup<'a>(
    motions: &'a HashMap<u16, RootMotion>,
    weapon_motions: &'a HashMap<Weapon, HashMap<u16, RootMotion>>,
    weapon: Weapon,
    id: u16,
) -> Option<&'a RootMotion> {
    weapon_motions
        .get(&weapon)
        .and_then(|m| m.get(&id))
        .or_else(|| motions.get(&id))
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
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
            air_rows: AIR_ROWS_SHIPPED.iter().copied().collect(),
            move_param: MOVE_SHIPPED,
            ledge_param: LEDGE_SHIPPED,
            stamina_tables: StaminaTables::shipped(),
            weight_tables: WeightTables::shipped(),
            sweep_param: sweep::SweepParam::default(),
            attack_param: ATTACK_SHIPPED,
            weapon_motions: HashMap::new(),
            bow_param: None,
            skill_definitions: HashMap::new(),
            pending: 0.0,
        }
    }

    pub fn add_player(&mut self, id: PlayerId, position: [f32; 3], yaw: f32) {
        self.players.push(Player {
            id,
            position,
            yaw,
            heading: yaw,
            fall_damage: 0.0,
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
            stamina: STAMINA_MAX_RECORDED,
            stamina_max: STAMINA_MAX_RECORDED,
            body_class: 1,
            load_class: 1,
            ran_out: false,
            // 0x00B628A0 seeds the ring with the spawn position
            safe: [position; 4],
            stuck: 0.0,
            warps: 0,
            unit_rate: 1.0,
            rate_queue: [1.0; 2],
            weapon: Weapon::None,
            secondary_weapon: Weapon::None,
            bow: None,
            bow_latched: false,
            bow_toggle_held: false,
            bow_context: BowInput::default(),
            weapon_level_gap: 0,
            attack_heading: None,
            turn_to: None,
            request: None,
            attacks_started: 0,
            volleys_fired: 0,
            shot: attack::ShotState::default(),
            skills: skills::Skills::default(),
            light_held: false,
            heavy_held: false,
        });
    }

    /// Sets a player's body and load classes from its body weight and its
    /// total (body plus carried) weight, as `0x00B81120` does each frame in
    /// the original. `capacity` is the carrying abilities' adjustment
    /// (0x00B7E710, 0 without them). The weights come from the inventory,
    /// which the host owns.
    pub fn set_weights(&mut self, id: PlayerId, body_weight: f32, total_weight: f32, capacity: f32) {
        let body = self.weight_tables.body_class(body_weight);
        let load = self.weight_tables.load_class(body_weight, total_weight, capacity);
        if let Some(p) = self.player_mut(id) {
            p.body_class = body;
            p.load_class = load;
        }
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

    /// Motion `id` as `weapon`'s player plays it: from the weapon's motions
    /// if it has it, else the common ones.
    pub fn motion_for(&self, weapon: Weapon, id: u16) -> Option<&RootMotion> {
        lookup(&self.motions, &self.weapon_motions, weapon, id)
    }

    /// The motion player `i` is playing.
    fn playing(&self, i: usize) -> Option<&RootMotion> {
        let p = &self.players[i];
        p.skills.active.and_then(|a| self.skill_definitions.get(&a.id)?.motions.get(&a.motion))
            .or_else(|| self.motion_for(p.weapon, p.state.motion_id()))
    }

    /// Advances every player by `dt` seconds, in steps of `TICK` motion frames.
    ///
    /// One call is one engine frame for the players' own time: the motion
    /// speed a frame ends with applies two calls later (`Player::unit_rate`).
    pub fn step(&mut self, dt: f32, inputs: &HashMap<PlayerId, Input>, ground: &dyn Ground) {
        for p in &mut self.players {
            p.unit_rate = p.rate_queue[0];
        }
        self.pending += dt * MOTION_FPS;
        while self.pending >= TICK {
            self.pending -= TICK;
            for i in 0..self.players.len() {
                let input = inputs.get(&self.players[i].id).copied().unwrap_or_default();
                let bow = BowInput { held: input.bow, toggle: input.bow_toggle,
                    ..self.players[i].bow_context };
                if !self.bow_frame(i, input, bow, TICK * self.players[i].unit_rate) {
                    self.frame(i, input, ground, TICK);
                }
            }
        }
        for i in 0..self.players.len() {
            let r = if self.players[i].bow.is_some() { 1.0 }
                else { self.time_rate(&self.players[i], self.players[i].state) };
            let p = &mut self.players[i];
            p.rate_queue = [p.rate_queue[1], r];
        }
    }

    /// Translated stationary shortbow slice of 0x00B52A00 (mode latch),
    /// 0x00B16FE0 (entry), 0x00B17940 / 0x00B1C220 / 0x00B1AC90
    /// (phases), DDDA.exe 2017-12-13 Steam. Untranslated paths are exposed.
    fn bow_frame(&mut self, i: usize, input: Input, control: BowInput, dt: f32) -> bool {
        let p = &mut self.players[i];
        if p.secondary_weapon != Weapon::Bow {
            p.bow = None;
            if matches!(p.state, State::Bow { .. }) { Self::set_state(p, State::Wait); }
            p.bow_latched = false;
            p.bow_toggle_held = control.toggle;
            return false;
        }
        if control.toggle && !p.bow_toggle_held { p.bow_latched = !p.bow_latched; }
        p.bow_toggle_held = control.toggle;
        let held = control.held || p.bow_latched;
        let Some(param) = self.bow_param else { return false };
        if p.bow.is_none() {
            if !held || !attack::bow_check34(control.unit_mode, control.stage_mode)
                || p.skills.active.is_some() || !p.state.on_ground_for_attacks() {
                return false;
            }
            // Slot 1 must execute: 0x00ABAAC0 / 0x00B6DC10. With no
            // motion event data the existing host fallback executes it.
            let slot = self.attack_events(i).0;
            if attack::slot_state(slot, attack::ATTACK_SLOT) != 2
                || self.players[i].stamina <= 0.0 { return false; }
            let p = &mut self.players[i];
            p.bow = Some(BowState::draw(param));
            p.state = State::Bow { phase: BowPhase::Draw, motion: 0x900 | u16::from(param.draw_lower) };
            p.frame = 0.0;
            p.request = None;
            p.turn_to = None;
        }
        let p = &mut self.players[i];
        if param.end_at_zero_stamina && p.stamina <= 0.0 {
            p.bow = None;
            Self::set_state(p, State::Wait);
            return true;
        }
        let mut bow = p.bow.unwrap();
        // vf06 calls 0x00B17DA0 -> 0x00B18CB0 before 0x00B1AC90's
        // upper phase handler. A new upper Ready phase does not replace
        // the lower draw until the following action update.
        if bow.phase == BowPhase::Ready {
            let motion = 0x900 | u16::from(param.wait_lower);
            if bow.lower.motion != motion {
                bow.lower.motion = motion;
                bow.lower.frame = 0.0;
                bow.lower.rate = 1.0;
                bow.lower.looping = true;
            }
        }
        bow.unsupported = if len2(input.stick) >= MOVE_THRESHOLD || input.jump {
            Some(BowUnsupported::Movement)
        } else if input.light || input.heavy {
            Some(BowUnsupported::Shot)
        } else { None };
        let Some(upper) = self.motion_for(Weapon::Bow, bow.upper.motion) else {
            bow.unsupported = Some(BowUnsupported::MissingMotion(bow.upper.motion));
            self.players[i].bow = Some(bow);
            return true;
        };
        let done = bow.upper.frame >= upper.period();
        if bow.update(param, held, !control.release_blocked, done) {
            self.players[i].bow = None;
            Self::set_state(&mut self.players[i], State::Wait);
            return true;
        }
        for layer in [&mut bow.upper, &mut bow.lower] {
            let Some(motion) = self.motion_for(Weapon::Bow, layer.motion) else {
                bow.unsupported = Some(BowUnsupported::MissingMotion(layer.motion));
                continue;
            };
            let frame = layer.frame + dt * layer.rate;
            layer.frame = if layer.looping { frame.rem_euclid(motion.period()) } else { frame };
        }
        let p = &mut self.players[i];
        // 0x00B19540 charges the record's statminaWait every unit frame.
        Self::spend(p, dt * param.wait_stamina);
        p.bow = Some(bow);
        p.state = State::Bow { phase: bow.phase, motion: bow.lower.motion };
        p.frame = bow.lower.frame;
        p.speed = 0.0;
        true
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
        let (state0, frame0) = (self.players[i].state, self.players[i].frame);
        // the player's own time (cUnit +0x14) for its actions and physics;
        // stamina runs in game time (0x00B81450 uses sMain's delta)
        let unit_dt = dt * self.players[i].unit_rate;
        if !self.skill_frame(i, ground, unit_dt) {
            self.frame_states(i, input, ground, unit_dt);
        }
        // Outside the gaits, actions set the facing themselves; the heading
        // starts from it when the player moves again.
        let p = &mut self.players[i];
        if !p.state.gait() {
            p.heading = p.yaw;
        }
        self.stamina_step(i, state0, frame0, dt);
        // uPlayerBase::vf90: the safe ring, then the lost-body check
        self.record_safe(i, ground);
        if self.lost(i, unit_dt) {
            self.warp_to_safe(i, ground);
        }
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
        let (light, heavy) = {
            let p = &mut self.players[i];
            let pressed = (input.light && !p.light_held, input.heavy && !p.heavy_held);
            p.light_held = input.light;
            p.heavy_held = input.heavy;
            pressed
        };
        let state = self.players[i].state;
        // the transition lists come before the default list (the jump)
        // the dagger's air attacks read the list too (AirS1 -> AirS2); the
        // sword's and staff's air rows have no chain from an air attack
        let air_chain = matches!(state, State::Attack { attack } if attack.air())
            && self.players[i].weapon == Weapon::Dagger;
        if (state.on_ground_for_attacks() || air_chain)
            && self.attack_input(i, light, heavy, moving.then_some(input_yaw), ground)
        {
            return;
        }
        match state {
            State::Bow { .. } => {
                // Authority/body correction can remove the layer state;
                // do not retain a bow action with no underlying controller.
                Self::set_state(&mut self.players[i], State::Wait);
            }
            State::Wait
            | State::Walk { .. }
            | State::Run { .. }
            | State::SprintStart
            | State::Sprint
            | State::SprintStop
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
                // a landing or a sprint stop plays out unless the player moves
                // and its motion allows moving off (event group 0 bit 0x10000)
                if matches!(
                    state,
                    State::Land { .. } | State::SprintJumpLand | State::SprintStop
                ) && (!moving || !self.move_cancel_allowed(i))
                    && self.players[i].frame < self.period_of(state.motion_id())
                {
                    self.ground_move(i, state, ground, dt);
                    return;
                }
                if state == State::Sprint && !moving {
                    // cPlActDash::vf06 (0x00ACAF80) sets the frame's sprint
                    // velocity before it requests cPlActRunEnd (action 3)
                    // for a released stick: the stop starts a frame later
                    self.ground_move(i, state, ground, dt);
                    Self::set_state(&mut self.players[i], State::SprintStop);
                    return;
                }
                let next = if !moving {
                    // letting go of a sprint enters cPlActRunEnd (action 3,
                    // recorded); a run stops at once
                    if state == State::Sprint {
                        State::SprintStop
                    } else {
                        State::Wait
                    }
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
                let start = (next != state)
                    .then(|| self.locomotion_start(&self.players[i], next, dt))
                    .flatten();
                let p = &mut self.players[i];
                if next == State::SprintStart && p.state != State::SprintStart {
                    p.sprint_entry = p.speed;
                }
                Self::set_state(p, next);
                if let Some(f) = start {
                    p.frame = f;
                }
                if moving {
                    // the heading follows the stick at once (uPlayerBase::vf90
                    // copies the stick angle +0x32C4 to +0x2654 while it is held)
                    p.heading = input_yaw;
                }
                self.ground_move(i, next, ground, dt);
                self.turn_to_heading(i, dt);
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
                // the transition lists' air rows (0x302) come before the
                // default list (the second jump); the jump and fall motions
                // keep the attack slots open throughout
                if self.attack_input(i, light, heavy, moving.then_some(input_yaw), ground) {
                    return;
                }
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
                // progress: the motion's curve 9 (0x010571C0(9) in
                // cPlCliffHangBegin::vf06), linear if the motion has none
                let t = self.progress(state.motion_id(), SPEED_CURVE, self.players[i].frame + dt, len);
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
            State::Attack { .. } | State::Shot { .. } => {
                if moving && state.on_ground_for_attacks() && self.attack_move_cancel(i) {
                    // the stick takes over (slot 0): the ground states run
                    // this frame
                    Self::set_state(&mut self.players[i], State::Wait);
                    self.players[i].turn_to = None;
                    self.frame_states(i, input, ground, dt);
                    return;
                }
                self.attack_frame(i, light, ground, dt);
            }
            State::HardLanding => {
                // cPlActDmgGround::vf06: the motion plays out, then the wait
                // (*assumed*: its end handling is not traced)
                self.ground_move(i, state, ground, dt);
                if self.players[i].state == state && self.players[i].frame >= self.period_of(state.motion_id()) {
                    Self::set_state(&mut self.players[i], State::Wait);
                }
            }
            State::LedgeClimb { .. } => {
                let len = self.period_of(state.motion_id());
                // cPlCliffHangClimb::vf06 (0x00ADDC00): the target rises by
                // the motion's curve 9 and the position follows it by curve
                // 10 (linear if the motion has none)
                let f = self.players[i].frame + dt;
                let (t_up, t_pos) = (
                    self.progress(state.motion_id(), SPEED_CURVE, f, len),
                    self.progress(state.motion_id(), CLIMB_POSITION_CURVE, f, len),
                );
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
                let target = ledge::climb_target(ground, &h.ledge, h.wall, h.from_y, t_up);
                p.position = ledge::lerp(h.start, target, t_pos);
                if p.frame >= len {
                    p.position = ledge::climb_end(&h.ledge);
                    p.hold = None;
                    Self::set_state(p, State::Wait);
                }
            }
        }
    }

    /// The group 0 event word of player `i`'s motion at its frame and whether
    /// bit 31 has not been seen since the motion started (check 22). A
    /// motion without event data executes every slot (for hosts without
    /// motion data).
    fn attack_events(&self, i: usize) -> (u32, bool) {
        let f = self.players[i].frame as i32;
        match self.playing(i) {
            Some(m) if !m.events.is_empty() => (
                m.event_bits(0, f, f),
                m.event_bits(0, 0, f) & attack::CHAIN_CLOSED_BIT == 0,
            ),
            _ => (1 << (0x10 + 2 * attack::ATTACK_SLOT), true),
        }
    }

    /// Bit 30 seen since the motion started, or the motion at its end: the
    /// action ends (`0x01057100` and uPlayer +0x365C).
    fn attack_motion_done(&self, i: usize) -> bool {
        let p = &self.players[i];
        let f = p.frame as i32;
        match self.playing(i) {
            Some(m) => m.event_bits(0, 0, f) & attack::ACTION_END_BIT != 0 || p.frame >= m.period(),
            None => p.frame >= 30.0,
        }
    }

    /// The stick may cancel the playing attack: slot 0 executes (event group
    /// 0 bit 0x10 without its buffer bit 0x11). Not without event data.
    fn attack_move_cancel(&self, i: usize) -> bool {
        let f = self.players[i].frame as i32;
        self.playing(i)
            .is_some_and(|m| !m.events.is_empty() && attack::slot_state(m.event_bits(0, f, f), 0) == 2)
    }

    /// The attack slot of one frame (`uPlayerBase::vf136`): a light or heavy
    /// press is matched against the weapon's transition list and kept while
    /// the slot is open (state 1 or 2) and stamina is above 0; the slot
    /// dropping to 0 drops it; the slot executing starts it. Returns true
    /// when an attack started.
    // Translated from 0x00B6D510 (the request loops at 0x00B6DC10), 0x00ABA690
    // and 0x00ABAAC0 (DDDA.exe 2017-12-13 Steam).
    fn attack_input(&mut self, i: usize, light: bool, heavy: bool, stick: Option<f32>, ground: &dyn Ground) -> bool {
        if self.players[i].weapon == Weapon::None {
            return false;
        }
        // check 10 (uPlayer::vf188 0x00B72DB0): the height above ground of
        // 0x0084FEA0's ray (from 100 above to 1000 below) reaches
        // m_pl000_00[0x75]; *assumed* high when the ray finds no ground
        let high = {
            let at = self.players[i].position;
            ground
                .height(at[0], at[2], at[1] + 100.0)
                .filter(|g| *g >= at[1] - 1000.0)
                .is_none_or(|g| at[1] - g >= self.attack_param.air_heavy_height)
        };
        let (word, chain_open) = self.attack_events(i);
        let slot = attack::slot_state(word, attack::ATTACK_SLOT);
        let p = &mut self.players[i];
        if slot == 0 {
            p.request = None;
        }
        let current = match p.state {
            State::Attack { attack } => Some(attack),
            _ => None,
        };
        if (light || heavy) && slot != 0 && p.stamina > 0.0 {
            let air = matches!(p.state, State::Air { .. } | State::Fall | State::SecondJump)
                || current.is_some_and(Attack::air);
            if let Some(r) = attack::next(p.weapon, current, light, heavy, chain_open, air, high) {
                p.request = Some(r);
            }
        }
        if slot == 2 {
            if let Some(r) = p.request.take() {
                self.start_attack(i, r, stick);
                return true;
            }
        }
        false
    }

    /// An attack action's vf05: its motion from frame 0 (force 1, so the
    /// same attack restarts), velocity zeroed (it moves by root motion), the
    /// turn toward its heading, and the stamina of a strike.
    // Translated from cPlActWpnSwordAtckLandS1_1::vf05 0x00B3C570 (the other
    // strikes' vf05s do the same with their motion) and
    // cPlActWpnWandShot::vf05 0x00B4E200 (DDDA.exe 2017-12-13 Steam).
    fn start_attack(&mut self, i: usize, r: attack::Request, stick: Option<f32>) {
        let ap = self.attack_param;
        let p = &mut self.players[i];
        let cost_id = match r {
            attack::Request::Attack(a) => {
                p.state = State::Attack { attack: a };
                Some(a.cost_id())
            }
            attack::Request::Shot => {
                p.state = State::Shot {
                    phase: ShotPhase::Start,
                };
                None
            }
            attack::Request::AirShot => {
                p.state = State::Shot {
                    phase: ShotPhase::AirStart,
                };
                None
            }
        };
        p.frame = 0.0;
        p.speed = 0.0;
        // The ground strikes zero the velocity; the air attacks' vf05s keep
        // it and their vf06s load a jump row (speed, gravity, damping).
        match p.state {
            State::Attack { attack } if attack.air() => {
                if let Some(row) = attack.jump_row().and_then(|r| self.air_rows.get(&r)) {
                    p.jump = *row;
                }
            }
            State::Shot { phase } if phase.air() => {}
            _ => p.velocity = [0.0; 3],
        }
        p.turn_to = p.attack_heading.or(stick);
        p.shot = attack::ShotState::default();
        p.attacks_started += 1;
        if let Some(id) = cost_id {
            Self::spend(p, attack::attack_cost(&ap, ap.cost_of(id), p.weapon_level_gap));
        }
    }

    /// Takes `cost` stamina (`0x00B81E30` with a negative amount).
    fn spend(p: &mut Player, cost: f32) {
        if cost > 0.0 {
            let before = p.stamina;
            p.stamina = (before - cost).clamp(0.0, p.stamina_max);
            p.ran_out |= before > 0.0 && p.stamina <= 0.0;
        }
    }

    /// The rotator 0x007975D0 for the gaits: the facing steps toward the
    /// heading by at most `MoveParam::turn` per frame, the short way round.
    /// Recorded (2026-10-09, keyboard, town run): a 90 degree turn in steps
    /// of 0.3927 x the frame's unit delta, no turn motion.
    fn turn_to_heading(&mut self, i: usize, dt: f32) {
        let step = self.move_param.turn * dt;
        let p = &mut self.players[i];
        let d = (p.heading - p.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        if d.abs() <= step {
            p.yaw = p.heading;
        } else {
            p.yaw = (p.yaw + step.copysign(d) + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
        }
    }

    /// One frame of an attack action: the turn, the root motion, then its
    /// vf06 (the end, or WandShot's phases).
    fn attack_frame(&mut self, i: usize, light: bool, ground: &dyn Ground, dt: f32) {
        let turn = self.attack_param.turn_deg.to_radians() * dt;
        {
            // 0x007975D0: toward the heading at m_pl000_00[0xAE] degrees a
            // frame (*assumed* per frame; the turn controller is not read)
            let p = &mut self.players[i];
            if let Some(h) = p.turn_to {
                let d = (h - p.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                if d.abs() <= turn {
                    p.yaw = h;
                    p.turn_to = None;
                } else {
                    p.yaw += turn.copysign(d);
                }
            }
        }
        let state = self.players[i].state;
        if !state.on_ground_for_attacks() {
            self.air_attack_frame(i, state, light, ground, dt);
            return;
        }
        self.ground_move(i, state, ground, dt);
        if self.players[i].state != state {
            return; // fell
        }
        match state {
            State::Attack { .. } => {
                // cPlActWpnSwordAtckLandS1_1::vf06 (0x00B3CC20) and the rest:
                // the action ends; *assumed* back to the wait (where the end
                // flag action +0x50 is read is not traced)
                if self.attack_motion_done(i) {
                    Self::set_state(&mut self.players[i], State::Wait);
                }
            }
            State::Shot { phase } => self.shot_frame(i, phase, light),
            _ => {}
        }
    }

    /// The landing for a fall: from `hard_landing` below the fall's start
    /// (`Player::drop_from`) `0x00B6E5C0` requests `cPlActDmgGround`
    /// variant 3 instead of `cPlActLand`, and the fall damage is queued.
    // Translated from 0x00B6E5C0, 0x00B78A00 and 0x00B78BF0 (DDDA.exe
    // 2017-12-13 Steam).
    fn hard_landing(mp: &MoveParam, p: &mut Player, land: State) -> State {
        let drop = p.drop_from - p.position[1];
        if drop < mp.hard_landing {
            return land;
        }
        p.fall_damage += mp.fall_damage_base + mp.fall_damage_rate * (drop - mp.hard_landing);
        State::HardLanding
    }

    /// One frame of an air attack.
    /// - AirS and WandAirStrike: physics every frame (`0x00AC51D0`) under
    ///   their jump row; their vf05s set +0x35E8 bit 1, so touching ground
    ///   lands (`cPlActLand`); at their end the fall takes over.
    /// - AirL: hangs (no physics) until the first bit-31 frame of its
    ///   motion, then `0x00AC5830` sets the vertical speed to the row's
    ///   SpeedY (0) and its gravity (-8) and it drops; on ground phase 2
    ///   plays 0x704 with the velocity zeroed (`SwordAirHeavyLand`).
    /// - The air WandShot: its phases call no physics; the caster hangs.
    ///
    /// What follows an air attack's end in the air is *assumed* to be the
    /// fall (where the end flag +0x50 is read is not traced).
    // Translated from cPlActWpnSwordAtckAirS::vf06 0x00B3D360,
    // cPlActWpnSwordAtckAirL::vf06 0x00B3DA10, cPlActWpnWandAirStrike::vf06
    // 0x00B51590 and cPlActWpnWandShot's phases (DDDA.exe 2017-12-13 Steam).
    fn air_attack_frame(&mut self, i: usize, state: State, light: bool, ground: &dyn Ground, dt: f32) {
        match state {
            State::Shot { phase } => {
                self.players[i].frame += dt;
                self.shot_frame(i, phase, light);
            }
            State::Attack { attack } if attack.hangs() => {
                let closed = |s: &Self, f: f32| {
                    s.playing(i)
                        .is_some_and(|m| m.event_bits(0, 0, f as i32) & attack::CHAIN_CLOSED_BIT != 0)
                };
                if !closed(self, self.players[i].frame) {
                    self.players[i].frame += dt;
                    if closed(self, self.players[i].frame) {
                        let p = &mut self.players[i];
                        p.velocity[1] = p.jump.speed_y;
                    }
                    return;
                }
                self.air_move(i, None, ground, dt);
                if self.players[i].state != state {
                    // landed: phase 2's motion, standing still (AirL); AirS2
                    // lands like any other (+0x35E8 bit 1)
                    if let Some(land) = attack.landing() {
                        let p = &mut self.players[i];
                        p.velocity = [0.0; 3];
                        Self::set_state(p, State::Attack { attack: land });
                    }
                } else if attack.landing().is_none() && self.attack_motion_done(i) {
                    Self::set_state(&mut self.players[i], State::Fall);
                }
            }
            State::Attack { .. } => {
                self.air_move(i, None, ground, dt);
                if self.players[i].state != state {
                    return; // landed
                }
                if self.attack_motion_done(i) {
                    Self::set_state(&mut self.players[i], State::Fall);
                }
            }
            _ => {}
        }
    }

    /// `cPlActWpnWandShot::vf06` (0x00B4F190), ground variant.
    // Translated from 0x00B4F490 (phase 0), 0x00B4F610 (phase 1) and
    // 0x00B50590 (phase 3) (DDDA.exe 2017-12-13 Steam).
    fn shot_frame(&mut self, i: usize, phase: ShotPhase, light: bool) {
        let done = self.attack_motion_done(i);
        let cost = attack::attack_cost(
            &self.attack_param,
            self.attack_param.staff_cost,
            self.players[i].weapon_level_gap,
        );
        let p = &mut self.players[i];
        match phase {
            // the start motion's hold time (+0x30) is -1 in variant 0: it
            // plays to its end (or bit 30), then the loop
            ShotPhase::Start | ShotPhase::AirStart => {
                if done {
                    let next = if phase.air() { ShotPhase::AirLoop } else { ShotPhase::Loop };
                    Self::set_state(p, State::Shot { phase: next });
                }
            }
            ShotPhase::AirLoop => {
                // variant 1: one volley, no refire
                if !p.shot.fired && attack::shot::FIRE_FRAME <= p.frame {
                    p.shot.fired = true;
                    p.shot.volleys += 1;
                    p.volleys_fired += attack::shot::SHELLS;
                    Self::spend(p, cost);
                }
                if done {
                    Self::set_state(p, State::Shot { phase: ShotPhase::AirEnd });
                }
            }
            ShotPhase::AirEnd => {
                if done {
                    Self::set_state(p, State::Fall);
                }
            }
            ShotPhase::Loop => {
                if !p.shot.fired {
                    if attack::shot::FIRE_FRAME <= p.frame {
                        // 0x00B4FA00 per shell (the host spawns them), then the
                        // volley's stamina
                        p.shot.fired = true;
                        p.shot.volleys += 1;
                        p.volleys_fired += attack::shot::SHELLS;
                        Self::spend(p, cost);
                    }
                } else {
                    // a light press (uPlayer +0x32E0 bit 0) before 5 volleys
                    // asks for another; it restarts the loop from frame 7 (an
                    // aim beyond 90 degrees would play a turn motion first:
                    // not modelled)
                    if light && p.shot.volleys < attack::shot::VOLLEYS {
                        p.shot.refire = true;
                    }
                    if p.shot.refire && attack::shot::REFIRE_FRAME <= p.frame {
                        p.shot.fired = false;
                        p.shot.refire = false;
                        p.frame = 0.0;
                        return;
                    }
                }
                // the loop motion's end: *assumed* its end flag (0x01057100,
                // motion +6 & 5) rises at its last frame although it loops
                if done {
                    Self::set_state(
                        p,
                        State::Shot {
                            phase: ShotPhase::End,
                        },
                    );
                }
            }
            ShotPhase::End => {
                if done {
                    Self::set_state(p, State::Wait);
                }
            }
        }
    }

    /// Keeps the ring of safe positions: while standing or moving on the
    /// ground (the wait, walk, run and sprint), at least `SAFE_STEP` from the
    /// newest and with ground within `SAFE_GROUND_BELOW` under the feet, the
    /// position is pushed in front and the oldest dropped. The original
    /// also refuses water, kill-zone latches and ground with attribute bits
    /// 0x80800 (not modelled).
    // Translated from 0x00B62980 (DDDA.exe 2017-12-13 Steam); the action
    // whitelist there also has 0x3D, 0x41, 0x42, 0x4F, 0x53, 0x54, 0x61,
    // 0x65, 0x66, 0xB4 and some weapon actions, which ddsim does not have.
    fn record_safe(&mut self, i: usize, ground: &dyn Ground) {
        let p = &self.players[i];
        if !matches!(
            p.state,
            State::Wait | State::Walk { .. } | State::Run { .. } | State::Sprint
        ) {
            return;
        }
        let q = p.position;
        let d = sub3(q, p.safe[0]);
        if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < SAFE_STEP * SAFE_STEP {
            return;
        }
        // the original reads the body's last ground hit (uPlayer +0x3284);
        // here the ground within SAFE_GROUND_BELOW above or below the feet
        if ground
            .height(q[0], q[2], q[1] + SAFE_GROUND_BELOW)
            .is_none_or(|g| g < q[1] - SAFE_GROUND_BELOW)
        {
            return;
        }
        let p = &mut self.players[i];
        p.safe.rotate_right(1);
        p.safe[0] = q;
    }

    /// The lost-body check: true after a fall of more than `LOST_FALL` in
    /// the air, or `STUCK_FRAMES` in the air without moving down. The stuck
    /// count restarts whenever the body moves down in the air; climbing
    /// and the original's other exemptions (some actions and status flags)
    /// stop it.
    // Translated from 0x00B63520 (DDDA.exe 2017-12-13 Steam).
    fn lost(&mut self, i: usize, dt: f32) -> bool {
        let p = &mut self.players[i];
        let airborne = matches!(p.state, State::Air { .. } | State::SecondJump | State::Fall);
        if !airborne {
            return false;
        }
        if p.velocity[1] < 0.0 {
            p.stuck = 0.0;
        }
        p.stuck += dt;
        if STUCK_FRAMES <= p.stuck {
            p.stuck = 0.0;
            return true;
        }
        p.position[1] - p.drop_from < -LOST_FALL
    }

    /// Back to a safe position: the second newest if the body is within
    /// `SAFE_NEAR` of the newest and the two are that close, else the newest
    /// if the body is that close to it, else the first of the four with
    /// ground from 100 above to 50 below it (at the higher of the two). The
    /// body then waits, falling at 2 cm a frame, its fall measured afresh.
    /// No slot with ground leaves the body where it is. For the Arisen the
    /// original runs this behind a 15-frame fade (stage mode 0x27,
    /// `0x005149E0`); a host can show one on `Player::warps`.
    // Translated from 0x00B62D20 (DDDA.exe 2017-12-13 Steam).
    fn warp_to_safe(&mut self, i: usize, ground: &dyn Ground) {
        let p = &self.players[i];
        let near = |a: [f32; 3], b: [f32; 3]| {
            let d = sub3(a, b);
            d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < SAFE_NEAR * SAFE_NEAR
        };
        let to = if near(p.position, p.safe[0]) {
            Some(if near(p.safe[0], p.safe[1]) {
                p.safe[1]
            } else {
                p.safe[0]
            })
        } else {
            p.safe.iter().find_map(|&s| {
                ground
                    .height(s[0], s[2], s[1] + 100.0)
                    .filter(|&g| g >= s[1] - 50.0)
                    .map(|g| [s[0], s[1].max(g), s[2]])
            })
        };
        let Some(to) = to else { return };
        let p = &mut self.players[i];
        p.position = to;
        p.velocity = [0.0, -2.0, 0.0];
        p.speed = 0.0;
        p.hold = None;
        p.drop_from = to[1];
        p.stuck = 0.0;
        p.warps += 1;
        Self::set_state(p, State::Wait);
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

    /// The player's own time per game time in `state` (`LOCOMOTION_RATE`):
    /// the motion speed of the walk, run or dash row at the player's load
    /// class; 1 for other states (the sprint stop included, as recorded).
    // Translated from 0x00B81920 (DDDA.exe 2017-12-13 Steam): actions 1, 2,
    // 5 / 0x7D read Walk, Run, Dash row +0xC into uPlayer +0xEE4.
    fn time_rate(&self, p: &Player, state: State) -> f32 {
        let t = &self.stamina_tables;
        let rows = match state {
            State::Walk { .. } => &t.walk,
            State::Run { .. } => &t.run,
            State::SprintStart | State::Sprint => &t.dash,
            _ => return 1.0,
        };
        rows.get(p.load_class.min(rows.len().saturating_sub(1)))
            .map_or(1.0, |r| r.motion_speed)
    }

    /// One stamina update after the player's states (`0x00B81450`):
    /// recovery unless the playing motion's events suppress it over the
    /// frames just passed, then the sprint's drain.
    fn stamina_step(&mut self, i: usize, state0: State, frame0: f32, dt: f32) {
        let t = &self.stamina_tables;
        let p = &self.players[i];
        let (from, to) = if p.state == state0 && p.frame >= frame0 {
            (frame0 as i32, p.frame as i32)
        } else {
            (p.frame as i32, p.frame as i32)
        };
        let suppressed = self.playing(i).is_some_and(|m| {
            m.event_bits(stamina::RECOVERY_SUPPRESS_GROUP, from, to) & stamina::RECOVERY_SUPPRESS_BIT != 0
        });
        let recovery = if suppressed {
            0.0
        } else {
            t.recovery(p.body_class, p.load_class)
        };
        let drain = match p.state {
            State::SprintStart | State::Sprint => t.dash_drain(p.load_class),
            _ => 0.0,
        };
        let (s, out) = stamina::step(p.stamina, p.stamina_max, recovery, drain, dt);
        let p = &mut self.players[i];
        p.stamina = s;
        p.ran_out |= out;
    }

    /// Whether the playing motion lets the stick cancel it: event group 0
    /// bit 0x10000 at the current whole frame. `cPlActWpnShieldGuardJumpLand::vf06`
    /// (0x00B36140) cancels on exactly "stick held (uPlayer +0x32C0) and
    /// event word +0x490 (layer 0, group 0) & 0x10000"; the landing motions
    /// 12 and 15 set it from frame 3, which is the recorded hold (about 3.3
    /// game frames, docs/ACCEPTANCE.md). That `cPlActLand` and the sprint
    /// stop use the same bit (through `uPlayerBase::vf136`) is *assumed*.
    /// Motions without event data allow it.
    fn move_cancel_allowed(&self, i: usize) -> bool {
        let p = &self.players[i];
        let Some(m) = self.motion_for(p.weapon, p.state.motion_id()) else {
            return true;
        };
        if m.events.is_empty() {
            return true;
        }
        let f = p.frame as i32;
        m.event_bits(MOVE_CANCEL_GROUP, f, f) & MOVE_CANCEL_BIT != 0
    }

    /// The frame a walk or run motion starts on when the player enters `next`
    /// (None for other states: they start at 0). Coming from another walk
    /// or run motion the new one is phase-synced; otherwise it starts on
    /// the action's given frame.
    // Translated from 0x00B80260 (DDDA.exe 2017-12-13 Steam), with the start
    // frames of cPlActWalk::vf05 (0x00AC9FB0) and cPlActRun::vf05
    // (0x00ACA390). Those pick between two start frames by 0x00AC5FB0, which
    // compares two joints (the feet) along the facing; the simulation has
    // no skeleton, so the "false" branch is taken (*placeholder*).
    fn locomotion_start(&self, p: &Player, next: State, dt: f32) -> Option<f32> {
        let given = match next {
            State::Walk { relaxed: false } => LOCOMOTION_START_FEET_NOT_AHEAD[0],
            State::Walk { relaxed: true } => LOCOMOTION_START_FEET_NOT_AHEAD[1],
            State::Run { .. } => LOCOMOTION_START_FEET_NOT_AHEAD[2],
            _ => return None,
        };
        let old = p.state.motion_id();
        if !LOCOMOTION_MOTIONS.contains(&old) {
            return Some(given);
        }
        let old_last = self.period_of(old);
        let new_last = self.period_of(next.motion_id());
        // the old motion's frame advanced by this step, scaled to the new one
        let mut f = (dt + p.frame) * (new_last / old_last);
        if new_last <= f {
            f -= new_last;
        }
        Some(f.trunc())
    }

    /// Curve `k` of motion `id` at `frame`, or the linear progress over
    /// `len` frames when the motion has no such curve.
    fn progress(&self, id: u16, k: usize, frame: f32, len: f32) -> f32 {
        self.motions
            .get(&id)
            .and_then(|m| m.curve(k, frame))
            .unwrap_or((frame / len).min(1.0))
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
            // cPlActDashBegin::vf06 (0x00ACAA50): max(entry speed, sprint
            // speed x curve 9 of the motion at its frame) (x uPlayer +0x68,
            // 1.0 here)
            State::SprintStart => {
                let ramp = self
                    .motions
                    .get(&state.motion_id())
                    .and_then(|m| m.curve(SPEED_CURVE, p.frame))
                    .unwrap_or((p.frame / SPRINT_RAMP_FRAMES).min(1.0));
                (mp.sprint * ramp).max(p.sprint_entry)
            }
            // cPlActRunEnd::vf06 (0x00ACB6C0): sprint speed x curve 9 (x
            // uPlayer +0x68 x +0x29A8, both 1.0 in the recordings); no
            // movement if the motion has no curve
            State::SprintStop => {
                mp.sprint
                    * self
                        .motions
                        .get(&state.motion_id())
                        .and_then(|m| m.curve(SPEED_CURVE, p.frame))
                        .unwrap_or(0.0)
            }
            _ => return None,
        })
    }

    /// Movement on the ground (velocity or root motion), with step-up /
    /// step-down rules.
    fn ground_move(&mut self, i: usize, state: State, ground: &dyn Ground, dt: f32) {
        let jn = self.jump_normal;
        let fall_max = self.move_param.fall_max;
        let speed = self.ground_speed(&self.players[i], state);
        let weapon = self.players[i].weapon;
        let period = self.playing(i).map_or(30.0, RootMotion::period);
        let looping = state.looping() && self.players[i].skills.active.is_none();
        let p = &mut self.players[i];
        let mut target = p.position;
        if let Some(v) = speed {
            // velocity along the facing, in the player's own (scaled) time;
            // the motion only animates. The capsule sweep's floor slide
            // projects it onto slopes; without triangles `along_ground` does.
            // cPlActRunEnd (the sprint stop) moves along the facing (+0x257C);
            // the other gaits along the heading (+0x2654)
            let dir = if state == State::SprintStop { p.yaw } else { p.heading };
            let (s, c) = dir.sin_cos();
            let flat = [v * dt * s, v * dt * c];
            let step = if ground.triangles_in(p.position, p.position, &mut Vec::new()) {
                flat
            } else {
                along_ground(ground, p.position, flat)
            };
            target[0] += step[0];
            target[2] += step[1];
            p.speed = v;
            let f = p.frame + dt;
            p.frame = if looping { f.rem_euclid(period) } else { f };
        } else if let Some(m) = p.skills.active.and_then(|a| self.skill_definitions.get(&a.id)?.motions.get(&a.motion))
            .or_else(|| lookup(&self.motions, &self.weapon_motions, weapon, state.motion_id())) {
            p.speed = 0.0;
            let d = if looping {
                m.delta(p.frame, dt)
            } else {
                m.delta_once(p.frame, dt)
            };
            let (s, c) = p.yaw.sin_cos();
            target[0] += d[0] * c + d[2] * s;
            target[2] += -d[0] * s + d[2] * c;
            p.frame = if looping {
                (p.frame + dt).rem_euclid(m.period())
            } else {
                p.frame + dt
            };
        } else {
            p.speed = 0.0;
            p.frame += dt;
        }
        if ground.triangles_in(p.position, p.position, &mut Vec::new()) {
            // the capsule sweep (0x0079E200): the step plus the ground check's
            // drop; meeting a floor keeps the character on the ground
            let motion = [
                target[0] - p.position[0],
                target[1] - p.position[1] - GROUND_CHECK_DROP * dt,
                target[2] - p.position[2],
            ];
            let r = sweep::sweep(
                &self.sweep_param,
                &sweep::PLAYER_CAPSULE,
                p.position,
                motion,
                true,
                &|lo, hi, out| {
                    ground.triangles_in(lo, hi, out);
                },
            );
            if r.grounded() || matches!(state, State::JumpStart { .. }) {
                p.position = r.position;
                return;
            }
            // not on the ground: 0x00B6E5C0 falls if the ground is more than
            // STEP_DOWN below, else the next drops bring the feet down
            let below = ground
                .height(r.position[0], r.position[2], r.position[1] + FALL_CHECK_UP)
                .map_or(f32::MAX, |h| r.position[1] - h);
            if below <= STEP_DOWN {
                p.position = r.position;
                return;
            }
            let mut v = [
                (r.position[0] - p.position[0]) / dt,
                0.0,
                (r.position[2] - p.position[2]) / dt,
            ];
            let hv = len2([v[0], v[2]]);
            if hv > fall_max {
                v[0] *= fall_max / hv;
                v[2] *= fall_max / hv;
            }
            p.velocity = v;
            p.position = r.position;
            p.jump = jn;
            p.jumps_left = u8::from(p.double_jump);
            Self::set_state(p, State::Fall);
            return;
        }
        let target = wall_slide(ground, p.position, target);
        let under = ground.height(target[0], target[2], p.position[1] + STEP_UP);
        let floor = under.is_some_and(|h| is_floor(ground, [target[0], h, target[2]]));
        match under {
            Some(h) if h >= p.position[1] - STEP_DOWN && floor => p.position = [target[0], h, target[2]],
            // steep ground rising ahead: not walked onto
            Some(h) if h >= p.position[1] => {}
            Some(_) if !matches!(state, State::JumpStart { .. }) => {
                // walked off an edge or onto steep ground: fall. cPlActFall::vf05
                // (0x00ACCDD0) takes the last step as the velocity (per frame),
                // its horizontal length capped at adjust 0xD1 (7.0)
                let mut v = [
                    (target[0] - p.position[0]) / dt,
                    0.0,
                    (target[2] - p.position[2]) / dt,
                ];
                let hv = len2([v[0], v[2]]);
                if hv > fall_max {
                    v[0] *= fall_max / hv;
                    v[2] *= fall_max / hv;
                }
                p.velocity = v;
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
        if ground.triangles_in(p.position, p.position, &mut Vec::new()) {
            // the capsule sweep in the air (no ground check): the velocity
            // stays, the collision only moves the character
            let motion = sub3(next, p.position);
            let r = sweep::sweep(
                &self.sweep_param,
                &sweep::PLAYER_CAPSULE,
                p.position,
                motion,
                false,
                &|lo, hi, out| {
                    ground.triangles_in(lo, hi, out);
                },
            );
            if r.grounded() && p.velocity[1] <= 0.0 {
                let standing =
                    matches!(p.state, State::Air { standing: true, .. }) || landing_stands(p.yaw, steer);
                let land = if matches!(p.state, State::Air { sprint: true, .. }) {
                    State::SprintJumpLand
                } else {
                    State::Land { standing }
                };
                p.position = r.position;
                p.velocity = [0.0; 3];
                let land = Self::hard_landing(&self.move_param, p, land);
                Self::set_state(p, land);
            } else {
                p.position = r.position;
            }
            return;
        }
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
        // ground rising above the new position within a step (a jump or a
        // fall meeting a slope): on the way up the body is held back from
        // it; on the way down it lands on it. Looking only below the body
        // let a single-layer surface pass over it (#1149).
        let rise = ground
            .height(next[0], next[2], p.position[1].max(next[1]) + STEP_UP)
            .filter(|&g| next[1] < g);
        if rise.is_some() && p.velocity[1] > 0.0 {
            next[0] = p.position[0];
            next[2] = p.position[2];
            p.velocity[0] = 0.0;
            p.velocity[2] = 0.0;
        }
        // landing: the ground under the new position, reached on the way down;
        // steep ground holds the character up without landing (as the
        // original stays in cPlActFall on a steep edge, docs/ACCEPTANCE.md)
        match ground.height(next[0], next[2], p.position[1].max(next[1]) + STEP_UP) {
            Some(g) if next[1] <= g && p.velocity[1] <= 0.0 && !is_floor(ground, [next[0], g, next[2]]) => {
                p.position = [next[0], g, next[2]];
            }
            Some(g) if next[1] <= g && p.velocity[1] <= 0.0 => {
                let standing =
                    matches!(p.state, State::Air { standing: true, .. }) || landing_stands(p.yaw, steer);
                // cPlActDashJump::vf06 (0x00ACDED0) lands into action 0x0C
                let land = if matches!(p.state, State::Air { sprint: true, .. }) {
                    State::SprintJumpLand
                } else {
                    State::Land { standing }
                };
                p.position = [next[0], g, next[2]];
                p.velocity = [0.0; 3];
                let land = Self::hard_landing(&self.move_param, p, land);
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
            curves: Vec::new(),
            events: Vec::new(),
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

    #[test]
    fn item_stamina_restores_only_the_recipient_and_respects_curse_cap() {
        let mut s = sim();
        let other = s.players[1].stamina;
        let p = &mut s.players[0];
        p.stamina = 25.0;
        p.stamina_max = 1000.0;
        assert_eq!(p.recover_item_stamina(250.0, false), 250.0);
        assert_eq!(p.recover_item_stamina(1000.0, true), 325.0);
        assert_eq!(p.stamina, 600.0);
        assert_eq!(s.players[1].stamina, other);
    }

    fn fwd() -> Input {
        Input {
            stick: [0.0, 1.0],
            ..Default::default()
        }
    }

    /// Five frames of running: the velocity in the player's scaled time.
    const RUN5: f32 = (2.0 + 3.0 * LOCOMOTION_RATE) * MOVE_SHIPPED.run;

    /// Steps `n` engine frames of 1/30 s: the run's 1.15 time factor applies
    /// from the third (`Player::unit_rate`), so five frames of running
    /// cover 2 + 3 x 1.15 frames of player time (`RUN5`).
    fn frames(s: &mut Sim, n: usize, inputs: &HashMap<PlayerId, Input>, ground: &dyn Ground) {
        for _ in 0..n {
            s.step(1.0 / MOTION_FPS, inputs, ground);
        }
    }

    #[test]
    fn running_moves_forward_at_the_run_speed() {
        let mut s = sim();
        let mut inputs = HashMap::new();
        inputs.insert(0, fwd());
        frames(&mut s, 5, &inputs, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert_eq!(p.state, State::Run { relaxed: false });
        assert!((p.position[2] - RUN5).abs() < 1e-3, "{:?}", p.position);
        // the motion plays faster than game time
        assert!(
            (p.frame - (2.0 + 3.0 * LOCOMOTION_RATE)).abs() < 1e-3,
            "{}",
            p.frame
        );
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
        frames(&mut s, 5, &inputs, &Flat(0.0));
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
        frames(&mut s, 5, &inputs, &Flat(0.0));
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
        frames(&mut s, 2, &inputs, &Flat(0.0));
        let mut last = s.player(0).unwrap().position[2];
        for _ in 0..200 {
            s.step(1.0 / MOTION_FPS, &inputs, &Flat(0.0));
            let z = s.player(0).unwrap().position[2];
            assert!(
                (z - last - MOVE_SHIPPED.run * LOCOMOTION_RATE).abs() < 1e-2,
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
        // quarter-frame steps, so that every state is seen
        for k in 0..1600 {
            let f = k / 4;
            let mut inputs = HashMap::new();
            // sprint, jump, keep pushing toward the wall until the climb
            if !matches!(states.last(), Some(State::LedgeClimb { .. } | State::Wait)) {
                inputs.insert(0, east((20..22).contains(&f), f < 22));
            }
            s.step(TICK / MOTION_FPS, &inputs, &w);
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
        // 10 cm in from the edge, 5 above it, then down onto the top
        assert!(near(s.player(0).unwrap().position, [110.0, 195.0, 0.0], 1e-3));
        s.step(1.0 / 30.0, &HashMap::new(), &w);
        let p = s.player(0).unwrap();
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

    /// m0000_co slot 21 curve 9 (the sprint stop's speed): 1 at 0, 0.5 at 5,
    /// 0 from 25 (linear keys).
    fn stop_curve() -> Vec<mtlmt::Curve> {
        let key = |frame: u16, v: f32| mtlmt::CurveKey {
            interp: 1,
            frame,
            value: v.to_bits(),
            tangent_in: 0.0,
            tangent_out: 0.0,
        };
        let mut c = vec![
            mtlmt::Curve {
                kind: 1,
                keys: Vec::new()
            };
            16
        ];
        c[SPEED_CURVE].keys = vec![key(0, 1.0), key(5, 0.5), key(25, 0.0), key(29, 0.0)];
        c
    }

    #[test]
    fn releasing_a_sprint_slides_to_a_stop() {
        let sprinter = || {
            let mut s = sim();
            s.motions
                .insert(0x15, RootMotion::none(31).with_curves(stop_curve()));
            s
        };
        let (mut s, mut held) = (sprinter(), sprinter());
        let sprint = Input {
            sprint: true,
            ..fwd()
        };
        let mut inputs = HashMap::from([(0, sprint)]);
        s.step(1.0, &inputs, &Flat(0.0));
        held.step(1.0, &inputs, &Flat(0.0));
        assert_eq!(s.player(0).unwrap().state, State::Sprint);
        let z0 = s.player(0).unwrap().position[2];
        // the frame the stick is let go still sprints (cPlActDash::vf06
        // requests the stop after setting the velocity), as a held one does
        held.step(TICK / MOTION_FPS, &inputs, &Flat(0.0));
        inputs.clear();
        s.step(TICK / MOTION_FPS, &inputs, &Flat(0.0));
        assert_eq!(s.player(0).unwrap().state, State::SprintStop);
        let z1 = s.player(0).unwrap().position[2];
        assert!((z1 - held.player(0).unwrap().position[2]).abs() < 1e-4);
        assert!(z1 > z0);
        s.step(2.0, &inputs, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert_eq!(p.state, State::Wait);
        // 22.5 x (5 x 0.75 + 20 x 0.25) = 196.9 cm (recorded: 198.1), plus
        // 2.8 cm from sampling the curve at the start of each quarter frame,
        // plus the stop's first frames still in the sprint's 1.15x time
        let d = p.position[2] - z1;
        assert!((d - 196.875 - 2.8125).abs() < 1.0, "{d}");
    }

    /// Group 1 suppressing recovery for `frames` motion frames.
    fn suppress(frames: u32, total: u32) -> Vec<Vec<mtlmt::EventRun>> {
        let run = |bits, frames| mtlmt::EventRun { bits, frames };
        vec![
            vec![],
            vec![
                run(stamina::RECOVERY_SUPPRESS_BIT, frames),
                run(0, total - frames),
            ],
            vec![],
            vec![],
        ]
    }

    #[test]
    fn sprinting_spends_stamina_and_the_stop_holds_recovery() {
        let mut s = sim();
        // as m0000_co: sprint (20) and sprint start (43) suppress recovery
        // throughout, the stop (21) for frames 0-17
        s.motions.insert(20, run().with_events(suppress(10, 10)));
        s.motions
            .insert(43, RootMotion::none(15).with_events(suppress(14, 14)));
        s.motions.insert(
            0x15,
            RootMotion::none(31)
                .with_curves(stop_curve())
                .with_events(suppress(18, 31)),
        );
        let sprint = Input {
            sprint: true,
            ..fwd()
        };
        let mut inputs = HashMap::from([(0, sprint)]);
        s.step(2.0, &inputs, &Flat(0.0));
        // 60 frames at -1.5 (the dash row at load class 1): 640 - 90
        let p = s.player(0).unwrap();
        assert!((p.stamina - 550.0).abs() < 1e-3, "{}", p.stamina);
        inputs.clear();
        // the stop holds stamina for 18 motion frames (0.6 s), then recovers
        s.step(0.5, &inputs, &Flat(0.0));
        assert!((s.player(0).unwrap().stamina - 550.0).abs() < 1e-3);
        s.step(0.5, &inputs, &Flat(0.0));
        let st = s.player(0).unwrap().stamina;
        assert!(st > 550.0 && st < 550.0 + 1.625 * 15.0, "{st}");
        // idle: +1.625 per frame
        s.step(1.0, &inputs, &Flat(0.0));
        let gain = s.player(0).unwrap().stamina - st;
        assert!((gain - 48.75).abs() < 1e-2, "{gain}");
        assert!(!s.player(0).unwrap().ran_out);
    }

    #[test]
    fn stamina_runs_out_and_clamps() {
        let mut s = sim();
        s.motions.insert(20, run().with_events(suppress(10, 10)));
        s.motions
            .insert(43, RootMotion::none(15).with_events(suppress(14, 14)));
        s.player_mut(0).unwrap().stamina = 3.0;
        let inputs = HashMap::from([(
            0,
            Input {
                sprint: true,
                ..fwd()
            },
        )]);
        s.step(1.0, &inputs, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert_eq!(p.stamina, 0.0);
        assert!(p.ran_out);
    }

    /// Flat at y 0 for x < 100, a 3:1 slope (72 degrees) down to y -300 at
    /// x = 200, flat beyond.
    struct Steep;
    impl Steep {
        fn at(x: f32) -> (f32, [f32; 3]) {
            if x < 100.0 {
                (0.0, [0.0, 1.0, 0.0])
            } else if x < 200.0 {
                let l = 10f32.sqrt();
                (-(x - 100.0) * 3.0, [3.0 / l, 1.0 / l, 0.0])
            } else {
                (-300.0, [0.0, 1.0, 0.0])
            }
        }
    }
    impl Ground for Steep {
        fn height(&self, x: f32, _: f32, below_y: f32) -> Option<f32> {
            let h = Self::at(x).0;
            (h <= below_y).then_some(h)
        }
        /// Vertical casts only (what the movement uses).
        fn ray(&self, from: [f32; 3], to: [f32; 3]) -> Option<RayHit> {
            let (h, normal) = Self::at(from[0]);
            (from[1] >= h && to[1] <= h).then_some(RayHit {
                point: [from[0], h, from[2]],
                normal,
            })
        }
    }

    #[test]
    fn steep_ground_is_fallen_down_not_walked() {
        let mut s = sim();
        let inputs = HashMap::from([(
            0,
            Input {
                stick: [0.0, 1.0],
                camera_yaw: std::f32::consts::FRAC_PI_2,
                ..Default::default()
            },
        )]);
        let mut states = Vec::new();
        for _ in 0..90 {
            s.step(1.0 / 30.0, &inputs, &Steep);
            let p = s.player(0).unwrap();
            if states.last() != Some(&p.state) {
                states.push(p.state);
            }
            // never stands on the slope
            if (100.5..199.5).contains(&p.position[0]) {
                assert!(
                    !matches!(p.state, State::Run { .. } | State::Land { .. }),
                    "{p:?}"
                );
            }
        }
        assert!(states.contains(&State::Fall), "{states:?}");
        let p = s.player(0).unwrap();
        assert_eq!(p.position[1], -300.0, "{p:?}");
    }

    #[test]
    fn a_fall_starts_at_most_at_the_capped_speed() {
        // the ground drops 300 cm at x = 100; sprinting off it
        struct Drop;
        impl Ground for Drop {
            fn height(&self, x: f32, _: f32, below_y: f32) -> Option<f32> {
                let h = if x < 100.0 { 0.0 } else { -300.0 };
                (h <= below_y).then_some(h)
            }
        }
        let mut s = sim();
        s.player_mut(0).unwrap().position = [-200.0, 0.0, 0.0];
        let inputs = HashMap::from([(
            0,
            Input {
                stick: [0.0, 1.0],
                sprint: true,
                camera_yaw: std::f32::consts::FRAC_PI_2,
                ..Default::default()
            },
        )]);
        for _ in 0..240 {
            s.step(TICK / MOTION_FPS, &inputs, &Drop);
            let p = s.player(0).unwrap();
            if p.state == State::Fall {
                let h = len2([p.velocity[0], p.velocity[2]]);
                assert!(h <= MOVE_SHIPPED.fall_max + 1e-4, "{p:?}");
                return;
            }
        }
        panic!("never fell");
    }

    #[test]
    fn climb_progress_follows_the_motion_curves() {
        let mut s = sim();
        // m0000_co slot 48 curve 10: 0 at frame 0, 1 from frame 4
        let key = |frame: u16, v: f32| mtlmt::CurveKey {
            interp: 1,
            frame,
            value: v.to_bits(),
            tangent_in: 0.0,
            tangent_out: 0.0,
        };
        let mut c = vec![
            mtlmt::Curve {
                kind: 1,
                keys: Vec::new()
            };
            16
        ];
        c[CLIMB_POSITION_CURVE].keys = vec![key(0, 0.0), key(4, 1.0)];
        s.motions.insert(0x30, RootMotion::none(35).with_curves(c));
        assert_eq!(s.progress(0x30, CLIMB_POSITION_CURVE, 2.0, 34.0), 0.5);
        assert_eq!(s.progress(0x30, CLIMB_POSITION_CURVE, 20.0, 34.0), 1.0);
        // no curve 9 in this motion: linear over its length
        assert_eq!(s.progress(0x30, SPEED_CURVE, 17.0, 34.0), 0.5);
    }

    #[test]
    fn a_landing_holds_the_stick_until_its_motion_allows_moving() {
        let mut s = sim();
        // as m0000_co slot 12: group 0 without bit 0x10000 for frames 0-2
        let run = |bits, frames| mtlmt::EventRun { bits, frames };
        let events = vec![
            vec![run(0x2a8a_0003, 3), run(0x1545_0003, 14), run(0, 1)],
            vec![],
            vec![],
            vec![],
        ];
        s.motions.insert(12, RootMotion::none(18).with_events(events));
        s.player_mut(0).unwrap().state = State::Land { standing: false };
        let inputs = HashMap::from([(0, fwd())]);
        let mut held = 0.0;
        while s.player(0).unwrap().state == (State::Land { standing: false }) {
            s.step(TICK / MOTION_FPS, &inputs, &Flat(0.0));
            held += TICK;
            assert!(held < 10.0);
        }
        // released once the motion reaches frame 3
        assert!((held - 3.0).abs() < 0.3, "{held}");
        assert!(matches!(s.player(0).unwrap().state, State::Run { .. }));
    }

    #[test]
    fn runs_start_on_frame_20_or_in_phase_with_a_walk() {
        let mut s = sim();
        // the walk motion 3 has 38 frames in m0000_co; the run 4 has 22
        s.motions.insert(3, RootMotion::none(38));
        s.motions.insert(4, RootMotion::none(22));
        let run_in = HashMap::from([(0, fwd())]);
        s.step(TICK / MOTION_FPS, &run_in, &Flat(0.0));
        let p = s.player(0).unwrap();
        // from idle: frame 20, plus this quarter frame
        assert!((p.frame - 20.25).abs() < 1e-4, "{}", p.frame);
        let mut s = sim();
        s.motions.insert(3, RootMotion::none(38));
        s.motions.insert(4, RootMotion::none(22));
        {
            let p = s.player_mut(0).unwrap();
            p.state = State::Walk { relaxed: false };
            p.frame = 18.5;
        }
        s.step(TICK / MOTION_FPS, &run_in, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert_eq!(p.state, State::Run { relaxed: false });
        // (0.25 + 18.5) x 21 / 37 = 10.64 -> frame 10, plus this quarter frame
        assert!((p.frame - 10.25).abs() < 1e-4, "{}", p.frame);
    }

    #[test]
    fn landings_stand_unless_the_stick_points_ahead() {
        assert!(landing_stands(0.0, None));
        assert!(!landing_stands(0.0, Some(0.5)));
        assert!(landing_stands(0.0, Some(1.0)));
        // across the wrap
        assert!(!landing_stands(3.0, Some(-3.0)));
        assert!(landing_stands(1.05, Some(-0.83)));
    }

    #[test]
    fn a_run_stops_at_once() {
        let mut s = sim();
        s.motions
            .insert(0x15, RootMotion::none(31).with_curves(stop_curve()));
        let mut inputs = HashMap::from([(0, fwd())]);
        s.step(1.0, &inputs, &Flat(0.0));
        let z0 = s.player(0).unwrap().position[2];
        inputs.clear();
        s.step(1.0, &inputs, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert_eq!(p.state, State::Wait);
        assert_eq!(p.position[2], z0);
    }

    #[test]
    fn the_facing_turns_toward_the_heading_at_the_adjust_rate() {
        // Recorded 2026-10-09 (keyboard, run, W then D): the velocity takes
        // the new direction at once; the facing (+0x257C) follows in steps
        // of 0.3927 (adjust 3, 22.5 degrees) x the unit delta, no turn motion.
        let mut s = sim();
        let mut inputs = HashMap::from([(0, fwd())]);
        frames(&mut s, 10, &inputs, &Flat(0.0));
        assert!(s.player(0).unwrap().yaw.abs() < 1e-6);
        // stick right: heading -90 degrees (screen-right is -X here)
        inputs.insert(0, Input { stick: [1.0, 0.0], ..Default::default() });
        let x0 = s.player(0).unwrap().position[0];
        frames(&mut s, 1, &inputs, &Flat(0.0));
        let p = s.player(0).unwrap();
        let right = -std::f32::consts::FRAC_PI_2;
        assert!((p.heading - right).abs() < 1e-5, "{}", p.heading);
        assert!(p.position[0] < x0, "moves right at once: {:?}", p.position);
        let step = MOVE_SHIPPED.turn * 1.15;
        assert!((p.yaw + step).abs() < 1e-3, "one frame's step: {}", p.yaw);
        assert!(matches!(p.state, State::Run { .. }), "no turn state: {:?}", p.state);
        // 90 degrees / (22.5 x 1.15) = 3.5 frames
        frames(&mut s, 2, &inputs, &Flat(0.0));
        assert!(s.player(0).unwrap().yaw > right + 1e-3);
        frames(&mut s, 1, &inputs, &Flat(0.0));
        assert!((s.player(0).unwrap().yaw - right).abs() < 1e-5);
    }

    #[test]
    fn a_released_stick_finishes_the_turn() {
        let mut s = sim();
        let mut inputs = HashMap::from([(0, fwd())]);
        frames(&mut s, 10, &inputs, &Flat(0.0));
        inputs.insert(0, Input { stick: [0.0, -1.0], ..Default::default() });
        frames(&mut s, 1, &inputs, &Flat(0.0));
        inputs.clear();
        frames(&mut s, 10, &inputs, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert_eq!(p.state, State::Wait);
        assert!((p.yaw.abs() - std::f32::consts::PI).abs() < 1e-4, "{}", p.yaw);
    }

    /// Jumps in place and steps until the player is in the air.
    fn airborne(s: &mut Sim) {
        let mut inputs = HashMap::from([(0, Input { jump: true, ..Default::default() })]);
        frames(s, 1, &inputs, &Flat(0.0));
        inputs.insert(0, Input::default());
        for _ in 0..30 {
            if matches!(s.player(0).unwrap().state, State::Air { .. }) {
                return;
            }
            frames(s, 1, &inputs, &Flat(0.0));
        }
        panic!("never airborne: {:?}", s.player(0).unwrap().state);
    }

    #[test]
    fn the_staff_air_shot_hangs_and_fires_once() {
        let mut s = sim();
        s.player_mut(0).unwrap().weapon = Weapon::Staff;
        airborne(&mut s);
        let light = HashMap::from([(0, Input { light: true, ..Default::default() })]);
        frames(&mut s, 1, &light, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert_eq!(p.state, State::Shot { phase: ShotPhase::AirStart });
        let y = p.position[1];
        assert!(y > 0.0);
        // its phases call no physics: the caster hangs
        let none = HashMap::new();
        frames(&mut s, 20, &none, &Flat(0.0));
        assert_eq!(s.player(0).unwrap().position[1], y);
        let mut states = Vec::new();
        for _ in 0..120 {
            frames(&mut s, 1, &none, &Flat(0.0));
            states.push(s.player(0).unwrap().state);
        }
        assert!(states.contains(&State::Shot { phase: ShotPhase::AirEnd }), "{states:?}");
        assert!(states.contains(&State::Fall), "{states:?}");
        // one volley (variant 1), even with more presses
        assert_eq!(s.player(0).unwrap().volleys_fired, attack::shot::SHELLS);
    }

    #[test]
    fn the_sword_plunge_hangs_until_bit_31_then_drops_and_lands() {
        let mut s = sim();
        s.player_mut(0).unwrap().weapon = Weapon::Sword;
        // as m0001_at 0x703: bit 31 from frame 6
        let run = |bits, frames| mtlmt::EventRun { bits, frames };
        let events = vec![vec![run(0, 6), run(attack::CHAIN_CLOSED_BIT, 30)], vec![], vec![], vec![]];
        let bank = s.weapon_motions.entry(Weapon::Sword).or_default();
        bank.insert(0x703, RootMotion::none(36).with_events(events));
        bank.insert(0x704, RootMotion::none(10));
        airborne(&mut s);
        let heavy = HashMap::from([(0, Input { heavy: true, ..Default::default() })]);
        frames(&mut s, 1, &heavy, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert_eq!(p.state, State::Attack { attack: Attack::SwordAirHeavy });
        assert_eq!(p.jump.gravity, -8.0, "row 0x37");
        let y = p.position[1];
        let none = HashMap::new();
        frames(&mut s, 4, &none, &Flat(0.0));
        assert_eq!(s.player(0).unwrap().position[1], y, "hangs before bit 31");
        let mut landed = false;
        for _ in 0..60 {
            frames(&mut s, 1, &none, &Flat(0.0));
            if s.player(0).unwrap().state == (State::Attack { attack: Attack::SwordAirHeavyLand }) {
                landed = true;
                break;
            }
        }
        assert!(landed, "{:?}", s.player(0).unwrap());
        let p = s.player(0).unwrap();
        assert!(p.position[1].abs() < 1e-3 && p.velocity == [0.0; 3]);
        frames(&mut s, 12, &none, &Flat(0.0));
        assert_eq!(s.player(0).unwrap().state, State::Wait);
    }

    #[test]
    fn a_fall_from_ten_metres_lands_hard_with_damage() {
        let mut s = sim();
        s.motions.insert(0x107, RootMotion::none(20));
        s.motions.insert(7, RootMotion::none(30));
        let none = HashMap::new();
        for (top, hard) in [(1200.0, true), (900.0, false)] {
            let p = s.player_mut(0).unwrap();
            p.position = [0.0, top, 0.0];
            p.drop_from = top;
            p.velocity = [0.0; 3];
            p.fall_damage = 0.0;
            p.state = State::Fall;
            let mut landed = None;
            for _ in 0..200 {
                frames(&mut s, 1, &none, &Flat(0.0));
                let st = s.player(0).unwrap().state;
                if st != State::Fall {
                    landed = Some(st);
                    break;
                }
            }
            let p = s.player(0).unwrap();
            if hard {
                // 200 + 0.8 x (1200 - 1000)
                assert_eq!(landed, Some(State::HardLanding));
                assert!((p.fall_damage - 360.0).abs() < 1.0, "{}", p.fall_damage);
                frames(&mut s, 25, &none, &Flat(0.0));
                assert_eq!(s.player(0).unwrap().state, State::Wait);
            } else {
                assert!(matches!(landed, Some(State::Land { .. })), "{landed:?}");
                assert_eq!(p.fall_damage, 0.0);
            }
        }
    }

    #[test]
    fn relaxed_run_uses_the_town_motion_and_speed() {
        let mut s = sim();
        s.player_mut(0).unwrap().relaxed = true;
        let mut inputs = HashMap::new();
        inputs.insert(0, fwd());
        frames(&mut s, 10, &inputs, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert_eq!(p.state.motion_id(), 0x55);
        // 12.0 x 0.8471 = 10.165 cm per frame, as recorded (uPlayer +0x2530)
        assert!((p.speed - 10.165).abs() < 1e-3, "{}", p.speed);
        // 10 frames x 10.165, the last 8 in 1.15x player time
        assert!((p.position[2] - 10.165 * 11.2).abs() < 1e-2, "{:?}", p.position);
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
        t[ADJUST_FALL_MAX] = 7.0;
        t[ADJUST_TURN_RATE] = 22.5;
        t[ADJUST_HARD_LANDING] = -1000.0;
        t[ADJUST_FALL_DAMAGE_BASE] = 200.0;
        t[ADJUST_FALL_DAMAGE_RATE] = 80.0;
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

    /// A single-layer ground: flat, then rising 1:1 east of x = 100 (no
    /// triangles: the point rules).
    struct Rise;
    impl Ground for Rise {
        fn height(&self, x: f32, _: f32, below_y: f32) -> Option<f32> {
            let h = (x - 100.0).max(0.0);
            (h <= below_y).then_some(h)
        }
    }

    /// #1149 on the point rules: jumps into ground rising 1:1 used to pass
    /// under the single-layer surface (the landing looked only below the
    /// body) and fall without end; now they land on it and climb it.
    #[test]
    fn jumps_into_rising_ground_land_on_it() {
        let mut s = sim();
        let p = s.player_mut(0).unwrap();
        p.yaw = std::f32::consts::FRAC_PI_2;
        for f in 0..120 {
            let input = Input {
                // screen-left with the camera along +z: toward +x
                stick: [-1.0, 0.0],
                jump: f % 20 < 2,
                ..Default::default()
            };
            s.step(1.0 / 30.0, &HashMap::from([(0, input)]), &Rise);
            let p = s.player(0).unwrap();
            let g = (p.position[0] - 100.0).max(0.0);
            assert!(p.position[1] >= g - 1.0, "frame {f}: {:?} under {g}", p.position);
        }
        let p = s.player(0).unwrap();
        assert!(p.position[0] > 300.0, "{:?}", p.position);
    }

    /// Flat ground east of x = 0; west of it a pit 1 km deep.
    struct Edge;
    impl Ground for Edge {
        fn height(&self, x: f32, _: f32, below_y: f32) -> Option<f32> {
            let h = if x >= 0.0 { 0.0 } else { -100_000.0 };
            (h <= below_y).then_some(h)
        }
    }

    /// A body that walks off into a deep pit falls 200 m, then is put back
    /// on a safe position it walked over (0x00B63520, 0x00B62D20).
    #[test]
    fn a_body_falling_200_m_is_warped_back_to_safe_ground() {
        let mut s = sim();
        s.players[0].position = [450.0, 0.0, 0.0];
        // west: stick screen-right with the camera along +z
        let west = Input {
            stick: [1.0, 0.0],
            ..Default::default()
        };
        let mut fell = None;
        for f in 0..400 {
            s.step(1.0 / 30.0, &HashMap::from([(0, west)]), &Edge);
            let p = s.player(0).unwrap();
            if p.position[1] < -100.0 && fell.is_none() {
                fell = Some(f);
            }
            if p.warps > 0 {
                break;
            }
        }
        let p = s.player(0).unwrap();
        assert!(fell.is_some());
        assert_eq!(p.warps, 1);
        // the walk west recorded safe positions every metre: back on one
        // (the rest of the step ran on from it with the stick held)
        assert!(
            matches!(p.state, State::Wait | State::Run { .. }),
            "{:?}",
            p.state
        );
        assert_eq!(p.position[1], 0.0);
        assert!(p.position[0] >= 0.0, "{:?}", p.position);
        assert!(
            p.safe.iter().any(|q| (q[0] - p.position[0]).abs() < 20.0),
            "{:?} {:?}",
            p.position,
            p.safe
        );
    }

    /// 90 frames in the air without moving down count as stuck.
    #[test]
    fn a_body_stuck_in_the_air_is_warped_back() {
        let mut s = sim();
        s.jump_normal.gravity = 0.0;
        s.jump_normal.speed_y = 1.0;
        let jump = Input {
            jump: true,
            ..Default::default()
        };
        s.step(1.0 / 30.0, &HashMap::from([(0, jump)]), &Flat(0.0));
        let mut n = 0;
        while s.player(0).unwrap().warps == 0 && n < 200 {
            s.step(1.0 / 30.0, &HashMap::new(), &Flat(0.0));
            n += 1;
        }
        let p = s.player(0).unwrap();
        assert_eq!(p.warps, 1, "{:?}", p.state);
        // takeoff (5 frames) then 90 in the air
        assert!((90..=97).contains(&n), "{n}");
        // near the newest safe slot, whose neighbour is close too: the
        // second newest, here the spawn
        assert_eq!(p.position, [0.0; 3]);
    }
}

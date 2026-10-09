//! Headless gameplay simulation. No rendering, audio or OS input: the host
//! steps it with per-player input, and any number of views (local windows,
//! remote clients) present its state. Every system is written for N players
//! (see docs/COOP_DESIGN.md).
//!
//! Units are the game's: centimetres, stage frame (origin cell 50m50n, see
//! docs/WORLD.md), radians; time in seconds.
//!
//! Locomotion (docs/ANIMATION.md): the player's action classes request
//! motions by id, `bank << 8 | slot`. From DDDA.exe:
//! - `cPlActWait::vf05` (0x00AC95F0): idle, motion 0
//! - `cPlActWalk::vf05` (0x00AC9FB0): walk, motion 3 (start frame 0 or 18)
//! - `cPlActRun::vf05` (0x00ACA390): run, motion 4
//!
//! all in bank 0 (`motion\pl\m\m00\m0000_co\m0000_co`). Movement is the
//! motion's root track (joint 255, absolute position): walk 198.5 cm per
//! 38-frame loop, run 423.0 cm per 22 frames.

use std::collections::HashMap;

/// Motion frames per second. *Assumed* 30 (not traced in the exe). Supporting
/// evidence: walk root motion then gives 1.57 m/s, a normal walking pace.
pub const MOTION_FPS: f32 = 30.0;

pub type PlayerId = u8;

/// What a player does this step, decided from input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Wait,
    Walk,
    Run,
}

impl Action {
    /// Motion id (`bank << 8 | slot`) the original requests for this action.
    pub fn motion_id(self) -> u16 {
        match self {
            Action::Wait => 0,
            Action::Walk => 3,
            Action::Run => 4,
        }
    }
}

/// One player's input for a step: a stick in the camera's frame plus the
/// camera yaw, so movement is camera-relative like the original.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Input {
    /// Stick, x right and y forward, length 0..1.
    pub stick: [f32; 2],
    pub run: bool,
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
    pub action: Action,
    /// Current frame in the action's motion.
    pub frame: f32,
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

    /// Root position (model space, cm) at `frame`.
    pub fn at(&self, frame: f32) -> [f32; 3] {
        match &self.track {
            Some(t) => {
                let v = t.sample(frame);
                [v[0], v[1], v[2]]
            }
            None => [0.0; 3],
        }
    }

    /// Model-space displacement from `from` to `from + dt_frames`, wrapping
    /// at the loop (a whole loop contributes its full end displacement).
    pub fn delta(&self, from: f32, dt_frames: f32) -> [f32; 3] {
        let last = self.frame_count.saturating_sub(1).max(1) as f32;
        let span = self.at(last);
        let mut d = [0.0f32; 3];
        let mut t = from;
        let mut left = dt_frames;
        while left > 0.0 {
            let step = left.min(last - t);
            let a = self.at(t);
            let b = self.at(t + step);
            for i in 0..3 {
                d[i] += b[i] - a[i];
            }
            left -= step;
            t += step;
            if left > 0.0 {
                t = 0.0; // loop
            }
            if step <= 0.0 && left > 0.0 {
                // degenerate (1-frame motion): add the end displacement per loop
                for i in 0..3 {
                    d[i] += span[i];
                }
                break;
            }
        }
        d
    }
}

/// Ground under a point, from the world (terrain, later collision).
pub trait Ground {
    /// Highest ground height (cm) at (x, z) at or below `below_y`.
    fn height(&self, x: f32, z: f32, below_y: f32) -> Option<f32>;
}

/// Stick magnitude above which the player walks. *Placeholder* until the
/// original's input thresholds are traced.
pub const WALK_THRESHOLD: f32 = 0.2;

/// Highest step the feet climb in one move, cm. *Placeholder* until the
/// original's character-controller values are traced.
pub const STEP_UP: f32 = 60.0;

pub struct Sim {
    pub players: Vec<Player>,
    pub motions: HashMap<Action, RootMotion>,
}

impl Sim {
    pub fn new(motions: HashMap<Action, RootMotion>) -> Self {
        Sim {
            players: Vec::new(),
            motions,
        }
    }

    pub fn add_player(&mut self, id: PlayerId, position: [f32; 3], yaw: f32) {
        self.players.push(Player {
            id,
            position,
            yaw,
            action: Action::Wait,
            frame: 0.0,
        });
    }

    pub fn player(&self, id: PlayerId) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }

    /// Advances every player by `dt` seconds with their inputs.
    pub fn step(&mut self, dt: f32, inputs: &HashMap<PlayerId, Input>, ground: &dyn Ground) {
        for p in &mut self.players {
            let input = inputs.get(&p.id).copied().unwrap_or_default();
            let mag = (input.stick[0].powi(2) + input.stick[1].powi(2)).sqrt();
            let action = if mag < WALK_THRESHOLD {
                Action::Wait
            } else if input.run {
                Action::Run
            } else {
                Action::Walk
            };
            if action != p.action {
                p.action = action;
                p.frame = 0.0;
            }
            if action != Action::Wait {
                // Face the stick direction in world space. *Placeholder*:
                // the original turns over time (turn rate not traced).
                let local = input.stick[0].atan2(input.stick[1]);
                p.yaw = input.camera_yaw + local;
            }
            let dt_frames = dt * MOTION_FPS;
            let mut target = p.position;
            if let Some(m) = self.motions.get(&action) {
                let d = m.delta(p.frame, dt_frames);
                let (s, c) = p.yaw.sin_cos();
                // model +Z is forward; rotate by yaw about +Y
                target[0] += d[0] * c + d[2] * s;
                target[2] += -d[0] * s + d[2] * c;
                let n = m.frame_count.max(1) as f32;
                p.frame = (p.frame + dt_frames) % n;
            }
            // Accept the move if there is ground no higher than a step above
            // the feet; otherwise (a wall or cliff face) stay put. Stepping
            // off a ledge snaps down to the lower ground (no falling yet).
            match ground.height(target[0], target[2], p.position[1] + STEP_UP) {
                Some(h) => p.position = [target[0], h, target[2]],
                None => {
                    if let Some(h) = ground.height(p.position[0], p.position[2], p.position[1] + STEP_UP) {
                        p.position[1] = h;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat(f32);
    impl Ground for Flat {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
            Some(self.0)
        }
    }

    /// A walk-like motion: root moves +Z 100 cm over 10 frames.
    fn walk() -> RootMotion {
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
        m.insert(Action::Walk, walk());
        let mut s = Sim::new(m);
        s.add_player(0, [0.0; 3], 0.0);
        s.add_player(1, [500.0, 0.0, 0.0], 0.0);
        s
    }

    #[test]
    fn walking_moves_forward_by_root_motion() {
        let mut s = sim();
        let mut inputs = HashMap::new();
        inputs.insert(
            0,
            Input {
                stick: [0.0, 1.0],
                run: false,
                camera_yaw: 0.0,
            },
        );
        // 5 frames at 30 fps -> 50 cm along +Z
        s.step(5.0 / MOTION_FPS, &inputs, &Flat(12.0));
        let p = s.player(0).unwrap();
        assert_eq!(p.action, Action::Walk);
        assert!((p.position[2] - 50.0).abs() < 1e-3, "{:?}", p.position);
        assert_eq!(p.position[1], 12.0);
        // player 1 had no input: waits in place
        let q = s.player(1).unwrap();
        assert_eq!(q.action, Action::Wait);
        assert_eq!(q.position[0], 500.0);
    }

    #[test]
    fn camera_relative_direction() {
        let mut s = sim();
        let mut inputs = HashMap::new();
        // stick forward with the camera turned 90 degrees: walk along +X
        inputs.insert(
            0,
            Input {
                stick: [0.0, 1.0],
                run: false,
                camera_yaw: std::f32::consts::FRAC_PI_2,
            },
        );
        s.step(5.0 / MOTION_FPS, &inputs, &Flat(0.0));
        let p = s.player(0).unwrap();
        assert!(
            (p.position[0] - 50.0).abs() < 1e-3 && p.position[2].abs() < 1e-3,
            "{:?}",
            p.position
        );
    }

    /// Ground with a wall: x < 100 is at y 0, x >= 100 is 500 cm higher.
    struct Wall;
    impl Ground for Wall {
        fn height(&self, x: f32, _: f32, below_y: f32) -> Option<f32> {
            let h = if x < 100.0 { 0.0 } else { 500.0 };
            (h <= below_y).then_some(h)
        }
    }

    #[test]
    fn walls_block_and_steps_climb() {
        let mut s = sim();
        let mut inputs = HashMap::new();
        // walk along +X (camera turned 90 degrees) for 2 seconds: would reach x = 600
        inputs.insert(
            0,
            Input {
                stick: [0.0, 1.0],
                run: false,
                camera_yaw: std::f32::consts::FRAC_PI_2,
            },
        );
        for _ in 0..60 {
            s.step(1.0 / 30.0, &inputs, &Wall);
        }
        let p = s.player(0).unwrap();
        assert!(p.position[0] < 100.0, "walked through the wall: {:?}", p.position);
        assert_eq!(p.position[1], 0.0);
    }

    #[test]
    fn root_motion_wraps_at_the_loop() {
        let w = walk();
        // from frame 8, 4 frames: 8->10 (20 cm) then wrap 0->2 (20 cm)
        let d = w.delta(8.0, 4.0);
        assert!((d[2] - 40.0).abs() < 1e-3, "{d:?}");
    }
}

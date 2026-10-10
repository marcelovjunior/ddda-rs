//! Source action state for the opening dragon's landing and breath.
//! Motion, terrain contact and effect particles are authoritative inputs.

/// Bulk character parameters stay in the player's installation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PressParams {
    pub initial_velocity: f32,
    pub terminal_velocity: f32,
    pub acceleration: f32,
}

impl PressParams {
    /// 0x00a6aed0/0x00a6af70 copy cCharParamEmDragon via 0x00a3a0a0:
    /// +0x30 initial velocity, +0x34 lower limit, +0x38 acceleration.
    pub fn load(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        let bytes = vfs.read("charparam/em/em5800Dragon", mtarc::type_hash("rPropParam"))?;
        let document = mtprp::parse(&bytes)?.object;
        if document.class_of(&document.root).name() != Some("cCharParamEmDragon") {
            return Err("opening dragon character parameters have the wrong class".into());
        }
        let read = |name| match document.get(&document.root, name).and_then(|v| v.first()) {
            Some(mtxfs::Value::F(value)) if value.len() == 1 && value[0].is_finite() => Ok(value[0] as f32),
            _ => Err(format!("missing finite dragon parameter {name}")),
        };
        Ok(Self {
            initial_velocity: read("プレスATTACKの初速")?,
            terminal_velocity: read("プレスATTACKの最高速")?,
            acceleration: read("プレス攻撃の加速度")?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressPhase {
    Waiting,
    Falling,
    Landed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PressAttack {
    pub phase: PressPhase,
    pub wait_frames: f32,
    pub delay: f32,
    pub fall_frames: f32,
    pub velocity: f32,
    pub done: bool,
}

/// The original action reads these at phase 91, before motion/terrain.
pub struct PressInput {
    pub engine_step: f32,
    /// Primary motion layer +0x110, sampled by 0x01060730 from event group 0.
    pub primary_events: u32,
    /// Actual actor +0x1ed0, never a timer or estimated landing frame.
    pub ground_contact: bool,
    pub motion_frame: f32,
    pub motion_ended: bool,
    /// Actor +0x679a == 0 enables the landing GSF notification.
    pub notify_landing: bool,
    /// Actor +0xe0 cached position, distinct from the corrected current one.
    pub cached_position: [f32; 3],
}

#[derive(Debug, Default, PartialEq)]
pub struct PressUpdate {
    pub actor_velocity: Option<[f32; 3]>,
    pub motion: Option<(u16, f32)>,
    pub gsf: Option<(u16, bool)>,
    /// The caller must dispatch native action 60; silently completing is wrong.
    pub escape_position: Option<[f32; 3]>,
}

/// Phase-90 projection of the previous terrain result into actor +0x1ed0.
/// `has_ground_body` is +0x1b60; `flags` is its resolved +0x1b68 state,
/// and `ground_mode` is +0x1b6d. The caller supplies actual contact flags;
/// a position below an arbitrary height is not equivalent to this result.
/// Translated from uObjModel::vf90 0x00885e30 (DDDA.exe 2017-12-13 Steam).
pub fn ground_contact(has_ground_body: bool, flags: u32, ground_mode: bool) -> bool {
    if has_ground_body && flags & 0x400 != 0 {
        return false;
    }
    flags & if ground_mode { 0x10_0003 } else { 3 } != 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreathPhase {
    Preparing,
    Emitting,
}

/// The sequence-4, controller+0x0c == 0 choice of BreathWide. Other
/// sequence/variant branches must be selected separately from their source.
#[derive(Debug, Clone, PartialEq)]
pub struct BreathWide {
    pub phase: BreathPhase,
    pub started: bool,
    pub stopped: bool,
    pub done: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct BreathUpdate {
    pub motion: Option<(u16, f32)>,
    pub start_effects: bool,
    /// Query the actual type-11 particles, up to 15, via 0x00e6ca90.
    /// This boolean is not a guessed cone or synthetic target volume.
    pub collect_particles: bool,
    pub stop_effects: bool,
}

impl BreathWide {
    /// Translated opening branch of 0x00a56a30 (DDDA.exe 2017-12-13 Steam).
    /// The caller binds motion 263, force=true, interpolation=5, speed=1.
    pub fn enter() -> Self {
        Self {
            phase: BreathPhase::Preparing,
            started: false,
            stopped: false,
            done: false,
        }
    }

    /// Translated from 0x00a56d10 (DDDA.exe 2017-12-13 Steam).
    /// 35/165 are embedded frame gates at entry A56BEC, not resource data.
    pub fn update(&mut self, frame: f32, motion_ended: bool) -> BreathUpdate {
        let mut out = BreathUpdate::default();
        match self.phase {
            BreathPhase::Preparing => {
                if motion_ended {
                    self.phase = BreathPhase::Emitting;
                    out.motion = Some((264, 5.0));
                }
            }
            BreathPhase::Emitting => {
                if frame >= 35.0 {
                    if !self.started {
                        self.started = true;
                        out.start_effects = true;
                    }
                    // The original collects positions before calling its
                    // stop helper, so frame 165 still has a collision pass.
                    out.collect_particles = !self.stopped;
                }
                if frame >= 165.0 && !self.stopped {
                    self.stopped = true;
                    out.stop_effects = true;
                }
                self.done |= motion_ended;
            }
        }
        out
    }
}

impl PressAttack {
    /// Translated from 0x00a6aed0 (DDDA.exe 2017-12-13 Steam).
    pub fn enter(params: PressParams, phase: PressPhase, delay: f32) -> Self {
        Self {
            phase,
            wait_frames: 0.0,
            delay,
            fall_frames: 0.0,
            velocity: params.initial_velocity,
            done: false,
        }
    }

    /// Translated from 0x00a6af70 (DDDA.exe 2017-12-13 Steam).
    /// The velocity increment is per action update, not multiplied by dt.
    pub fn update(&mut self, params: PressParams, input: PressInput) -> PressUpdate {
        let mut out = PressUpdate::default();
        match self.phase {
            PressPhase::Waiting => {
                if self.wait_frames >= self.delay {
                    self.phase = PressPhase::Falling;
                }
                self.wait_frames += input.engine_step;
            }
            PressPhase::Falling if input.primary_events & 0x2000_0000 != 0 => {
                out.actor_velocity = Some([0.0, self.velocity, 0.0]);
                self.velocity = (self.velocity + params.acceleration).max(params.terminal_velocity);
                if input.ground_contact {
                    self.phase = PressPhase::Landed;
                    if input.notify_landing {
                        out.gsf = Some((463, true));
                    }
                    out.actor_velocity = Some([0.0; 3]);
                    out.motion = Some((299, 0.0));
                }
                // This branch still executes on a frame that also lands.
                // Both 150 and 500 are small embedded values at A6B1E9..A6B241.
                if self.fall_frames >= 150.0 {
                    let mut position = input.cached_position;
                    position[1] += 500.0;
                    out.escape_position = Some(position);
                }
                self.fall_frames += input.engine_step;
            }
            PressPhase::Falling => {}
            PressPhase::Landed => {
                if input.motion_frame >= 60.0 {
                    out.gsf = Some((463, false));
                }
                self.done |= input.motion_ended;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Synthetic parameters, not an embedded installed character record.
    fn params() -> PressParams {
        PressParams {
            initial_velocity: -12.0,
            terminal_velocity: -20.0,
            acceleration: -3.0,
        }
    }
    fn input() -> PressInput {
        PressInput {
            engine_step: 0.5,
            primary_events: 0,
            ground_contact: false,
            motion_frame: 0.0,
            motion_ended: false,
            notify_landing: true,
            cached_position: [1.0, 2.0, 3.0],
        }
    }
    #[test]
    fn contact_reads_terrain_bits_and_ground_mode_instead_of_actor_height() {
        assert!(ground_contact(true, 1, false));
        assert!(ground_contact(true, 2, false));
        assert!(!ground_contact(true, 4 | 8, false));
        assert!(!ground_contact(true, 0x10_0000, false));
        assert!(ground_contact(true, 0x10_0000, true));
        assert!(!ground_contact(true, 0x400 | 3, true));
        assert!(ground_contact(false, 0x400 | 3, false));
    }
    #[test]
    fn delay_and_motion_event_gate_fall_before_velocity_is_incremented() {
        let mut action = PressAttack::enter(params(), PressPhase::Waiting, 0.5);
        assert_eq!(action.update(params(), input()), PressUpdate::default());
        assert_eq!(action.phase, PressPhase::Waiting);
        action.update(params(), input());
        assert_eq!(action.phase, PressPhase::Falling);
        action.update(params(), input());
        assert_eq!((action.velocity, action.fall_frames), (-12.0, 0.0));
        let mut input = input();
        input.primary_events = 0x2000_0000;
        assert_eq!(
            action.update(params(), input).actor_velocity,
            Some([0.0, -12.0, 0.0])
        );
        assert_eq!((action.velocity, action.fall_frames), (-15.0, 0.5));
    }
    #[test]
    fn ground_contact_binds_landing_and_clears_its_flag_at_frame_sixty() {
        let mut action = PressAttack::enter(params(), PressPhase::Falling, 0.0);
        let mut hit = input();
        hit.primary_events = 0x2000_0000;
        hit.ground_contact = true;
        let out = action.update(params(), hit);
        assert_eq!(out.motion, Some((299, 0.0)));
        assert_eq!(out.actor_velocity, Some([0.0; 3]));
        assert_eq!(out.gsf, Some((463, true)));
        let mut landed = input();
        landed.motion_frame = 60.0;
        landed.motion_ended = true;
        assert_eq!(action.update(params(), landed).gsf, Some((463, false)));
        assert!(action.done);
    }
    #[test]
    fn fall_timeout_uses_cached_position_even_on_a_landing_update() {
        let mut action = PressAttack::enter(params(), PressPhase::Falling, 0.0);
        action.fall_frames = 150.0;
        let mut hit = input();
        hit.primary_events = 0x2000_0000;
        hit.ground_contact = true;
        hit.notify_landing = false;
        let out = action.update(params(), hit);
        assert_eq!(out.escape_position, Some([1.0, 502.0, 3.0]));
        assert_eq!(out.motion, Some((299, 0.0)));
        assert_eq!(out.gsf, None);
    }
    #[test]
    fn breath_uses_real_motion_end_and_inclusive_effect_frame_gates() {
        let mut action = BreathWide::enter();
        assert_eq!(action.update(999.0, false), BreathUpdate::default());
        let out = action.update(999.0, true);
        assert_eq!(out.motion, Some((264, 5.0)));
        assert!(!action.done);
        assert_eq!(action.update(34.99, false), BreathUpdate::default());
        let out = action.update(35.0, false);
        assert!(out.start_effects && out.collect_particles && !out.stop_effects);
        assert!(!action.update(36.0, false).start_effects);
        let out = action.update(165.0, false);
        assert!(out.collect_particles && out.stop_effects);
        let out = action.update(166.0, true);
        assert!(!out.collect_particles && !out.start_effects && !out.stop_effects);
        assert!(action.done);
    }
}

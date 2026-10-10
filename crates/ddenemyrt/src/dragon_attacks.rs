//! Source action state for the opening dragon's landing and breath.
//! Motion, terrain contact and effect particles are authoritative inputs.

/// cParticleGenerator emission clock. Allocation, binding and life/movement
/// remain separate; a spawn request does not manufacture a collision volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorPhase {
    Initial,
    Waiting,
    Emitting,
    Finished,
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratorParams {
    /// Installed generator +0x08, copied to manager +0x8C.
    pub flags: u32,
    /// Installed generator +0xAC/+0xAE, not seconds.
    pub restart_delay: i16,
    pub restart_variation: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorClock {
    pub phase: GeneratorPhase,
    pub age: u32,
    pub delay: i16,
    pub random_counter: u32,
    /// Runtime unit +0x0C; distinct from installed generator flags.
    pub unit_flags: u32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GeneratorUpdate {
    pub prepare_binding: bool,
    pub spawn: bool,
}

impl GeneratorClock {
    /// Seed/counter selection is caller evidence, not a guessed RNG sequence.
    pub fn new(random_counter: u32, unit_flags: u32) -> Self {
        Self { phase: GeneratorPhase::Initial, age: 0, delay: 0,
            random_counter, unit_flags }
    }

    // Translated from 0x010D6930, cParticleGenerator::vf10
    // (DDDA.exe 2017-12-13 Steam). The parent 0x010FA5B0 clears a binding
    // latch on forced stop; that binding is not represented by this clock.
    pub fn stop(&mut self, params: GeneratorParams, forced: bool) {
        if params.flags & 1 != 0 { self.unit_flags |= 0x2000_0000; }
        if params.flags & 2 != 0 && !forced {
            self.phase = GeneratorPhase::Restart;
        } else {
            self.unit_flags |= 0x8000_0004;
            self.phase = GeneratorPhase::Finished;
        }
    }

    // Translated from 0x010CD0C0 and vf10 0x010D6930
    // (DDDA.exe 2017-12-13 Steam). One call is one effect update; no dt.
    // The caller reads the player's exe table at 0x01621420 for an index
    // requested here. The 4096-word table is never embedded in this crate.
    pub fn update(&mut self, params: GeneratorParams, owner_delay: i16,
        mut random_word: impl FnMut(usize) -> u32) -> GeneratorUpdate {
        let mut out = GeneratorUpdate::default();
        if self.phase == GeneratorPhase::Initial {
            self.age = 0;
            self.delay = owner_delay;
            self.phase = GeneratorPhase::Waiting;
        }
        match self.phase {
            GeneratorPhase::Waiting => {
                if self.unit_flags & 4 == 0 { return out; }
                if self.delay != 0 {
                    self.delay = self.delay.wrapping_sub(1);
                    return out;
                }
                out.prepare_binding = true; // 0x010FA670, before emission.
                self.phase = GeneratorPhase::Emitting;
            }
            GeneratorPhase::Restart => {
                // Even zero variation advances the native counter.
                self.random_counter = self.random_counter.wrapping_add(1);
                let extra = if params.restart_variation == 0 { 0 } else {
                    random_word((self.random_counter & 0xFFF) as usize)
                        % (u32::from(params.restart_variation) + 1)
                };
                self.delay = params.restart_delay.wrapping_add(extra as i16);
                self.age = self.age.wrapping_add(1);
                self.phase = GeneratorPhase::Waiting;
                return out;
            }
            GeneratorPhase::Finished => {
                self.age = self.age.wrapping_add(1);
                return out;
            }
            GeneratorPhase::Initial => unreachable!(),
            GeneratorPhase::Emitting => {}
        }
        // vf10 changes phase before the allocator receives this request.
        self.stop(params, false);
        self.age = self.age.wrapping_add(1);
        out.spawn = true;
        out
    }
}

/// Type1 movement's fade clock, after native sampling/duration adjustment.
/// Particle trajectory and allocation are separate from this lifetime gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadePhase { In, Hold, Out, Done }

#[derive(Debug, Clone, PartialEq)]
pub struct FadeClock {
    pub phase: FadePhase,
    pub opacity: f32,
    pub timer: u16,
    pub durations: [u16; 3],
    pub hold_limit: u16,
    pub hold_latch: bool,
}

impl FadeClock {
    // Translated from the state selection at 0x010FA8D0
    // (DDDA.exe 2017-12-13 Steam). Durations must already be sampled and
    // adjusted by 0x010F9CB0; RNG/curve/owner adapters are not translated here.
    pub fn enter(durations: [u16; 3], hold_limit: u16, hold_latch: bool) -> Self {
        let [fade_in, hold, fade_out] = durations;
        let (phase, timer, opacity) = if fade_in != 0 {
            (FadePhase::In, 0, 1.0 / (f32::from(fade_in) + 1.0))
        } else if hold != 0 {
            (FadePhase::Hold, hold, 1.0)
        } else {
            (FadePhase::Out, fade_out.max(1), 1.0)
        };
        Self { phase, opacity, timer, durations, hold_limit, hold_latch }
    }

    fn begin_out(&mut self) {
        if self.durations[2] == 0 { self.phase = FadePhase::Done; }
        else { self.phase = FadePhase::Out; self.timer = self.durations[2]; }
    }

    /// Returns native liveness. Reaching Done from In/Hold remains live for
    /// that update; reaching Done from Out retires during the same update.
    // Translated from 0x010FAD70 (DDDA.exe 2017-12-13 Steam).
    // `detached` is particle flags+0x0C bit1; `generator_holds` is manager
    // flags+0x3C bit28; `stop` is runtime unit+0x0C bit29.
    pub fn update(&mut self, generator_holds: bool, detached: bool, stop: bool) -> bool {
        if detached { self.hold_latch = false; }
        match self.phase {
            FadePhase::In => {
                self.timer = self.timer.wrapping_add(1);
                if self.timer < self.durations[0] {
                    self.opacity = (f32::from(self.timer) + 1.0)
                        / (f32::from(self.durations[0]) + 1.0);
                } else {
                    self.opacity = 1.0;
                    if self.durations[1] != 0 {
                        self.phase = FadePhase::Hold; self.timer = self.durations[1];
                    } else if generator_holds && self.hold_latch {
                        self.phase = FadePhase::Hold; self.timer = 1;
                    } else { self.begin_out(); }
                }
                if stop { self.begin_out(); }
                true
            }
            FadePhase::Hold => {
                if !generator_holds || !self.hold_latch {
                    self.timer = self.timer.wrapping_sub(1);
                } else if self.hold_limit != 0 {
                    self.hold_limit = self.hold_limit.wrapping_sub(1);
                    if self.hold_limit == 0 { self.timer = 0; }
                }
                if self.timer == 0 || stop { self.begin_out(); }
                true
            }
            FadePhase::Out => {
                self.timer = self.timer.wrapping_sub(1);
                if self.timer == 0 {
                    self.opacity = 0.0;
                    self.phase = FadePhase::Done;
                    false
                } else {
                    self.opacity = f32::from(self.timer) / f32::from(self.durations[2]);
                    true
                }
            }
            FadePhase::Done => false,
        }
    }
}

/// Default cObjCollision scenery branch, before any runtime type override.
/// The capsule is posed relative to the actor origin supplied to the sweep.
/// Type1 length/width overrides and special negative joint IDs are unported.
// Translated from 0x0076C540, default type0 branch and 0x00CCDE30/0x00CCE190
// (DDDA.exe 2017-12-13 Steam). The constructor 0x00769C70 initializes
// type0 and preserve-scale byte +0xD9E=1; radius uses model +0x64 (Y scale),
// not the combat volumes' arithmetic mean scale.
pub fn scenery_capsule(
    node: &ddcombat::ocl::Node,
    pose: &dyn ddcombat::volumes::Pose,
    origin: [f32; 3],
    model_y_scale: f32,
) -> Result<ddsim::sweep::CapsuleSegment, String> {
    if node.shape != 0 {
        return Err("dragon scenery shape is not the supported capsule".into());
    }
    let point = |joint: i32, offset: [f32; 3]| -> Result<[f32; 3], String> {
        let matrix = if joint == -1 { pose.model_matrix() }
            else if joint >= 0 { pose.joint_matrix(joint).ok_or_else(|| format!("dragon scenery joint {joint} missing"))? }
            else { return Err(format!("dragon scenery special joint {joint} unported")); };
        // normalize=1 preserves the actual scaled basis, unlike hit-volume
        // node_point's normalize=0 path. Refuse missing pose evidence above.
        let world = ddcombat::volumes::transform_point(&matrix, offset);
        Ok(std::array::from_fn(|i| world[i] - origin[i]))
    };
    Ok(ddsim::sweep::CapsuleSegment {
        start: point(node.joint0, node.offset0)?,
        end: point(node.joint1, node.offset1)?,
        radius: node.radius * model_y_scale,
    })
}

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
/// `axis_check` is +0x1b60 != 0 (cScrAdjust +0x1a0); `flags` is its
/// resolved +0x1b68 state,
/// and `ground_mode` is +0x1b6d. The caller supplies actual contact flags;
/// a position below an arbitrary height is not equivalent to this result.
/// Translated from uObjModel::vf90 0x00885e30 (DDDA.exe 2017-12-13 Steam).
pub fn ground_contact(axis_check: bool, flags: u32, ground_mode: bool) -> bool {
    if axis_check && flags & 0x400 != 0 {
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

/// Radius fields of an actual type-11 collision particle, not a substitute
/// for its emitter, movement or lifetime. Parameters come from EFL data.
#[derive(Debug, Clone, PartialEq)]
pub struct CollisionRadius {
    pub base: f32,
    pub increment: f32,
    pub upper_limit: f32,
    pub growing: bool,
    pub samples: [f32; 2],
    pub live: bool,
}

impl CollisionRadius {
    /// Translated from 0x012ac1e0 (DDDA.exe 2017-12-13 Steam).
    /// Without an override: particle resource +40/+44/+48; the resolved
    /// lifetime override supplies +8/+0c/+4 instead. Selection is external.
    pub fn enter(base: f32, limit_delta: f32, increment: f32, scale: f32) -> Self {
        Self {
            base,
            increment,
            upper_limit: base + limit_delta,
            growing: increment != 0.0,
            samples: [base * scale; 2],
            live: true,
        }
    }

    /// Translated from 0x012ac260 (DDDA.exe 2017-12-13 Steam).
    /// This is one effect update, with no invented dt multiplier. Native
    /// particle +0x0f bit0 selects the destination of the radius double buffer.
    pub fn update(&mut self, scale: f32, destination: bool) {
        if !self.live {
            return;
        }
        if self.growing {
            let mut next = self.base + self.increment;
            if next < 0.0 {
                self.live = false;
                return;
            }
            if self.upper_limit < next {
                self.growing = false;
                next = self.upper_limit;
            }
            self.base = next;
        }
        self.samples[usize::from(destination)] = self.base * scale;
    }
}

/// Type-11 output from 0x00e6ca90 (DDDA.exe 2017-12-13 Steam), for one
/// already-selected active particle. +0x20/+0x30 positions and +0x60/+0x64
/// radii are interpolated with effect +0x104. The effect producer supplies
/// the previous/current buffer selection; this function does not emit one.
pub fn sample_collision_particle(previous: [f32; 4], current: [f32; 4], fraction: f32) -> [f32; 4] {
    std::array::from_fn(|axis| (current[axis] - previous[axis]) * fraction + previous[axis])
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
    #[test]
    fn fade_clock_retirement_distinguishes_hold_and_fade_out_terminals() {
        // Synthetic durations; no installed movement record is copied.
        let mut hold = FadeClock::enter([0, 2, 0], 0, false);
        assert!(hold.update(false, false, false));
        assert!(hold.update(false, false, false));
        assert_eq!((hold.phase, hold.opacity), (FadePhase::Done, 1.0));
        assert!(!hold.update(false, false, false));
        let mut out = FadeClock::enter([0, 0, 2], 0, false);
        assert!(out.update(false, false, false));
        assert_eq!(out.opacity, 0.5);
        assert!(!out.update(false, false, false));
        assert_eq!((out.phase, out.opacity), (FadePhase::Done, 0.0));
        let mut zero = FadeClock::enter([0; 3], 0, false);
        assert_eq!(zero.timer, 1);
        assert!(!zero.update(false, false, false));
    }

    #[test]
    fn fade_clock_uses_native_fade_in_denominator_and_stop_transition() {
        let mut clock = FadeClock::enter([2, 3, 2], 0, false);
        assert_eq!(clock.opacity, 1.0 / 3.0);
        assert!(clock.update(false, false, false));
        assert_eq!(clock.opacity, 2.0 / 3.0);
        assert!(clock.update(false, false, false));
        assert_eq!((clock.phase, clock.timer, clock.opacity), (FadePhase::Hold, 3, 1.0));
        assert!(clock.update(false, false, true));
        assert_eq!((clock.phase, clock.timer, clock.opacity), (FadePhase::Out, 2, 1.0));
        assert!(clock.update(false, false, false));
        assert_eq!(clock.opacity, 0.5);
        assert!(!clock.update(false, false, false));
    }

    #[test]
    fn fade_clock_latch_holds_until_detachment_limit_or_stop() {
        let mut clock = FadeClock::enter([0, 3, 0], 0, true);
        for _ in 0..20 { assert!(clock.update(true, false, false)); }
        assert_eq!((clock.phase, clock.timer), (FadePhase::Hold, 3));
        assert!(clock.update(true, true, false));
        assert_eq!((clock.hold_latch, clock.timer), (false, 2));
        let mut limited = FadeClock::enter([0, 3, 0], 2, true);
        assert!(limited.update(true, false, false));
        assert_eq!((limited.timer, limited.hold_limit), (3, 1));
        assert!(limited.update(true, false, false));
        assert_eq!((limited.phase, limited.timer), (FadePhase::Done, 0));
        assert!(!limited.update(true, false, false));
        let mut stopped = FadeClock::enter([2, 0, 0], 0, true);
        assert!(stopped.update(true, false, true));
        assert_eq!(stopped.phase, FadePhase::Done);
        assert!(!stopped.update(true, false, false));
    }

    #[test]
    fn generator_single_spawn_waits_for_activation_and_owner_delay() {
        let params = GeneratorParams { flags: 4, restart_delay: 0, restart_variation: 0 };
        let mut clock = GeneratorClock::new(17, 0);
        let no_random = |_| panic!("single emission must not sample random table");
        assert_eq!(clock.update(params, 2, no_random), GeneratorUpdate::default());
        assert_eq!((clock.phase, clock.delay, clock.age), (GeneratorPhase::Waiting, 2, 0));
        clock.unit_flags |= 4;
        assert_eq!(clock.update(params, 99, no_random), GeneratorUpdate::default());
        assert_eq!(clock.update(params, 99, no_random), GeneratorUpdate::default());
        assert_eq!(clock.update(params, 99, no_random),
            GeneratorUpdate { prepare_binding: true, spawn: true });
        assert_eq!((clock.phase, clock.age), (GeneratorPhase::Finished, 1));
        assert_eq!(clock.unit_flags, 0x8000_0004);
        for _ in 0..35 { assert!(!clock.update(params, 0, no_random).spawn); }
        assert_eq!((clock.age, clock.random_counter), (36, 17));
    }

    #[test]
    fn generator_restart_consumes_counter_and_separate_delay_updates() {
        let params = GeneratorParams { flags: 3, restart_delay: 1, restart_variation: 2 };
        let mut clock = GeneratorClock::new(0xFFF, 4);
        assert!(clock.update(params, 0, |_| panic!("not sampled at spawn")).spawn);
        assert_eq!((clock.phase, clock.unit_flags), (GeneratorPhase::Restart, 0x2000_0004));
        let mut indices = Vec::new();
        assert!(!clock.update(params, 0, |index| { indices.push(index); 5 }).spawn);
        assert_eq!(indices, [0]);
        assert_eq!((clock.random_counter, clock.delay, clock.age), (0x1000, 3, 2));
        for delay in [2, 1, 0] {
            assert!(!clock.update(params, 0, |_| panic!("only restart samples")).spawn);
            assert_eq!(clock.delay, delay);
        }
        assert!(clock.update(params, 0, |_| panic!("only restart samples")).spawn);
        assert_eq!(clock.age, 3);
        clock.stop(params, true);
        assert_eq!(clock.phase, GeneratorPhase::Finished);
        assert_eq!(clock.unit_flags, 0xA000_0004);
        assert!(!clock.update(params, 0, |_| panic!("forced stop never restarts")).spawn);
    }

    #[test]
    fn generator_zero_variation_still_advances_counter_and_delay_wraps() {
        let params = GeneratorParams { flags: 2, restart_delay: 0, restart_variation: 0 };
        let mut clock = GeneratorClock::new(u32::MAX, 4);
        assert!(clock.update(params, 0, |_| panic!("no RNG read")).spawn);
        assert!(!clock.update(params, 0, |_| panic!("zero variation has no table read")).spawn);
        assert_eq!((clock.random_counter, clock.delay), (0, 0));
        assert!(clock.update(params, 0, |_| panic!("no RNG read")).spawn);
        let wrapping = GeneratorParams { restart_delay: i16::MAX, restart_variation: 1, ..params };
        clock.update(wrapping, 0, |_| 1);
        assert_eq!(clock.delay, i16::MIN);
        clock.update(wrapping, 0, |_| panic!("waiting does not sample"));
        assert_eq!(clock.delay, i16::MAX);
    }

    #[test]
    fn scenery_offsets_keep_joint_scale_and_radius_uses_model_y_scale() {
        use ddcombat::volumes::{Mat4, Pose};
        struct Posed;
        impl Pose for Posed {
            fn model_matrix(&self) -> Mat4 { self.joint_matrix(254).unwrap() }
            fn joint_matrix(&self, id: i32) -> Option<Mat4> {
                (id == 254).then_some([[2.0, 0.0, 0.0, 0.0], [0.0, 4.0, 0.0, 0.0],
                    [0.0, 0.0, 8.0, 0.0], [10.0, 20.0, 30.0, 1.0]])
            }
        }
        // Deliberately synthetic record, unrelated to the installed offsets.
        let mut node = ddcombat::ocl::Node { no: 0, shape: 0, radius: 3.0,
            joint0: 254, offset0: [1.0, 2.0, 3.0], joint1: 254, offset1: [-1.0, -2.0, -3.0],
            region_id: 0, hit_prio: 0, node_attr: 0, node_effect_attr: 0, node_free_work: 0 };
        let cap = scenery_capsule(&node, &Posed, [10.0, 20.0, 30.0], 4.0).unwrap();
        assert_eq!(cap.start, [2.0, 8.0, 24.0]);
        assert_eq!(cap.end, [-2.0, -8.0, -24.0]);
        assert_eq!(cap.radius, 12.0);
        node.joint0 = 253;
        assert!(scenery_capsule(&node, &Posed, [0.0; 3], 4.0).is_err());
        node.joint0 = -2;
        assert!(scenery_capsule(&node, &Posed, [0.0; 3], 4.0).is_err());
        node.shape = 1;
        assert!(scenery_capsule(&node, &Posed, [0.0; 3], 4.0).is_err());
    }
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
    fn particle_radius_clamps_only_after_crossing_and_preserves_previous_buffer() {
        let mut radius = CollisionRadius::enter(2.0, 2.0, 1.0, 3.0);
        radius.update(3.0, true);
        assert_eq!(radius.samples, [6.0, 9.0]);
        radius.update(3.0, false);
        assert_eq!((radius.base, radius.growing), (4.0, true));
        radius.update(2.0, true);
        assert_eq!((radius.base, radius.growing), (4.0, false));
        assert_eq!(radius.samples, [12.0, 8.0]);
        radius.update(5.0, false);
        assert_eq!(radius.samples, [20.0, 8.0]);
    }
    #[test]
    fn particle_radius_negative_growth_retires_only_below_zero() {
        let mut radius = CollisionRadius::enter(1.0, 0.0, -1.0, 1.0);
        radius.update(1.0, true);
        assert!(radius.live);
        assert_eq!(radius.samples, [1.0, 0.0]);
        radius.update(1.0, false);
        assert!(!radius.live);
        assert_eq!(radius.samples, [1.0, 0.0]);
    }
    #[test]
    fn particle_sampling_interpolates_radius_with_position_without_clamping_time() {
        let a = [0.0, 2.0, 4.0, 8.0];
        let b = [4.0, 6.0, 8.0, 12.0];
        assert_eq!(sample_collision_particle(a, b, 0.25), [1.0, 3.0, 5.0, 9.0]);
        assert_eq!(sample_collision_particle(a, b, 1.5), [6.0, 8.0, 10.0, 14.0]);
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

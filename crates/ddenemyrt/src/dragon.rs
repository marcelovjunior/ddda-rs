//! Dragon AI definitions read from the player's executable.
//!
//! Sequence-4 data and an explicitly limited dragon action simulator.
//! The entrance FSM is interpreted by ddai; movement and attack action
//! implementations must supply its completion and motion feedback.

// Preserve the exact single-precision constants of the cited exe routines.
#![allow(clippy::approx_constant)]

use crate::combat::{Attacker, Fighter, Victim};
use ddai::{Error, thinktbl::Tables};
use ddcombat::damage::{CollisionRates, EnemyAttacker, EnemyDefender, RegionPool};

#[path = "dragon_flight.rs"]
pub mod flight;
#[path = "dragon_attacks.rs"]
pub mod attacks;
#[path = "dragon_particles.rs"]
pub mod particles;
#[path = "dragon_actor.rs"]
mod actor;
pub use actor::OpeningDragon;
pub use ddai::think::Request as ActorRequest;

/// Phase-93 projection of character condition flags into collision flags.
/// `controller_flag_c` is the observed cAICtrl +0x0C byte; `actor_flags`
/// is the word returned by vf180 (actor +0x2738, 0x004c7480).
///
/// Translated from uEnemy::vf93 0x00aa9bf0 (0x00aa9d08..0x00aa9dbc),
/// reached by uEmDragonBase::vf93 0x00a40c20, DDDA.exe 2017-12-13 Steam.
/// No defaults are supplied: a host must provide the actual controller and
/// condition state before invoking this phase.
pub fn sync_collision_flags(rates: &mut CollisionRates, controller_flag_c: bool, actor_flags: u32) {
    if controller_flag_c {
        return;
    }
    let projected = ((actor_flags >> 27) & 1)
        | ((actor_flags >> 25) & 2)
        | ((actor_flags >> 26) & 4)
        | ((actor_flags >> 22) & 8);
    rates.flags = (rates.flags & !0xf) | projected;
}

/// The two defined HoverUp height modes. The actor's current think target
/// is read on every update; an explicit height is captured on entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HoverHeight {
    ThinkTarget,
    Explicit(f32),
}

/// Native HoverUp movement/completion state. This is the action's control
/// portion, not the actor's frame scheduler: motion binding, attached-actor
/// messages (0x00aad0c0), air flags and integration remain host work.
#[derive(Debug, Clone, PartialEq)]
pub struct HoverUp {
    pub height: HoverHeight,
    pub velocity: [f32; 3],
    pub done: bool,
}

impl HoverUp {
    /// Motion 100, blend 5, speed 1: 0x00a6ba90, DDDA.exe 2017-12-13 Steam.
    pub const MOTION: u16 = 100;

    /// Translated from cEm5800HoverUp::vf05 0x00a6ba90.
    pub fn enter(scale_y: f32, height: HoverHeight) -> Self {
        Self {
            height,
            velocity: [0.0, scale_y * 35.0, 0.0],
            done: false,
        }
    }

    /// Translated from vf06 0x00a6bb40. Called before velocity integration.
    /// Completion stops velocity; it does not snap the actor to the height.
    pub fn update(&mut self, actor_y: f32, think_target_y: f32) {
        let height = match self.height {
            HoverHeight::ThinkTarget => think_target_y,
            HoverHeight::Explicit(height) => height,
        };
        if height <= actor_y {
            self.velocity = [0.0; 3];
            self.done = true;
        }
    }

    /// Translated from vf08 0x00a6bc80: cancellation also stops velocity.
    pub fn exit(&mut self) {
        self.velocity = [0.0; 3];
    }
}

/// Shared hover-transition completion test, not an action implementation.
/// Both HoverKakku and KakkuHover use vf06 0x00a6be00 (Steam 2017-12-13).
/// Disassembly confirms both calls are the same primary-layer end getter;
/// model +0x494 bit31 is the additional completion path.
pub fn hover_transition_complete(motion_ended: bool, motion_flags: u32) -> bool {
    motion_ended || motion_flags & 0x8000_0000 != 0
}

/// Native yaw-controller state used by HoverKakku (action 54).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragonYaw {
    pub current: f32,
    pub target: f32,
    pub rate: f32,
    pub active: bool,
}

// Embedded angle constants and wrapping: 0x00a6bf00 / 0x00797470,
// DDDA.exe 2017-12-13 Steam. Preserve truncation and the inclusive +pi edge.
fn wrap_yaw(angle: f32) -> f32 {
    let mut wrapped = angle - (angle * 0.15915494).trunc() * 6.2831855;
    if angle < 0.0 {
        wrapped += 6.2831855;
    }
    if wrapped >= 3.1415927 {
        wrapped -= 6.2831855;
    }
    wrapped
}

fn yaw_difference(current: f32, target: f32) -> f32 {
    let (mut from, mut to) = (current, target);
    if current * target < 0.0 {
        if target < 0.0 {
            to += 6.2831855;
        }
        if current < 0.0 {
            from += 6.2831855;
        }
        let difference = to - from;
        if difference.abs() > 3.1415927 {
            if from <= to {
                from = 6.2831855;
                to = difference;
            } else {
                from -= to;
                to = 6.2831855;
            }
        }
    }
    to - from
}

impl DragonYaw {
    /// Translated from HoverKakku vf05 0x00a6bf00. Explicit headings are
    /// supplied in radians; native alternate-mode degrees are converted by
    /// the caller. The think-target mode uses [`Self::target_bearing`].
    pub fn enter(current: f32, target: f32) -> Self {
        let distance = yaw_difference(current, target).abs();
        let rate = distance * 0.033333335;
        let active = distance >= f32::EPSILON && rate >= f32::EPSILON;
        Self {
            current: if active { current } else { target },
            target,
            rate,
            active,
        }
    }

    /// a6bf5e..a6bfd5: normalize full displacement, then atan2(x,z).
    /// The two double arguments use XMM0/XMM1, obscured by the decompiler.
    pub fn target_bearing(pos: [f32; 3], target: [f32; 3]) -> f32 {
        let mut delta = std::array::from_fn::<_, 3, _>(|i| target[i] - pos[i]);
        let length = (delta[1] * delta[1] + delta[0] * delta[0] + delta[2] * delta[2]).sqrt();
        if length >= f32::EPSILON {
            let inverse = 1.0 / length;
            delta[0] *= inverse;
            delta[2] *= inverse;
        }
        wrap_yaw((f64::from(delta[0]).atan2(f64::from(delta[2]))) as f32)
    }

    /// 0x0084cc00 -> 0x00797470. `enabled` is actor +0x2576;
    /// `condition_flags` is +0x26A0, distinct from damage flags +0x2738.
    /// Slow factor +0x27B0 is used only for condition bit 0x1000.
    pub fn step(&mut self, enabled: bool, condition_flags: u32, slow_factor: f32, step: f32) {
        if !enabled || condition_flags & 0x800 != 0 || !self.active {
            return;
        }
        let step = if condition_flags & 0x1000 != 0 {
            step * slow_factor
        } else {
            step
        };
        self.target = wrap_yaw(self.target);
        let difference = yaw_difference(self.current, self.target);
        let distance = self.rate * step;
        if difference.abs() < distance {
            self.current = self.target;
            self.active = false;
        } else {
            let signed = if difference < 0.0 { -distance } else { distance };
            self.current = wrap_yaw(self.current + signed);
        }
    }
}

/// Effects requiring authoritative host state; callers must apply these in
/// order. The prefix does not pretend to simulate attachment or air control.
#[derive(Debug, Clone, PartialEq)]
pub enum DragonEffect {
    /// 0x00a49480 then 0x00a6bd80: clear secondary layer 2 and the
    /// flight controller's blend/steering state, including model +0x648.
    ResetFlightLayer,
    /// Action-54 exit 0x00a6c130 also clears the actor's acceleration.
    ClearAcceleration,
    /// Common action-entry bookkeeping, 0x00a4db70, including +0x603B clear.
    BaseActionEnter,
    /// Common action-exit bookkeeping, 0x00a76c90, conditional host flags.
    BaseActionExit,
    /// vf06 0x00a6bb40: clear bytes +0x1B6D/+0x1E80, set +0x600C bit 1.
    HoverAirControl,
    /// 0x00aad0c0, gated by actor +0x679A == 0.
    AttachedMessage { owner: u32, message: u32 },
    /// vf08 0x00a6bc80 clears byte +0x1B6C.
    ExitHover,
    /// Primary-layer binding from 0x00a6ba90; blending remains host work.
    BindMotion { id: u16, blend: f32, speed: f32 },
    /// PressAttack landing/exit, 0x00a6af70/0x00a6b290.
    Gsf { no: u16, value: bool },
    /// The native 150-frame fall recovery dispatches action 60.
    EscapeFly { position: [f32; 3] },
    /// BreathWide's effect engine requests, 0x00a56a30/0x00a56d10.
    BreathPrewarm,
    BreathStart,
    BreathParticles,
    BreathStop,
    /// Press phase-specific native air/ground bookkeeping at 0x00a6af70.
    PressControl { landed: bool },
    /// Breath phase-specific native actor bookkeeping at 0x00a56d10.
    BreathControl { emitting: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DragonError {
    Unsupported(u32),
    MissingMotion(u16),
    NoAction,
    MissingTerrainContact,
}

/// Resolved inputs for phases 91..93. External displacement includes the
/// host's push/correction; acceleration and clamps are actor state, not
/// invented gravity. `engine_step` and `motion_step` are independently scaled.
pub struct DragonStep<'a> {
    pub engine_step: f32,
    pub motion_step: f32,
    /// Resolved row-vector track-to-world translation transform. The host
    /// must include actor scale, root multipliers and orientation; geometry's
    /// mean `Body::scale` is insufficient. See the tail of 0x0105b740.
    /// This prefix samples one unblended translation track; root rotation
    /// and animation blending require host support and are not simulated.
    pub root_translation_transform: [[f32; 3]; 3],
    pub think_target_y: f32,
    pub motion_flags: u32,
    pub yaw_enabled: bool,
    pub movement_condition_flags: u32,
    pub yaw_slow_factor: f32,
    pub external_displacement: [f32; 3],
    pub acceleration: [f32; 3],
    /// Ordered status multipliers from +0x26A0 bits 0x800/0x1000;
    /// None means the corresponding condition is absent.
    pub horizontal_status_multipliers: [Option<f32>; 2],
    pub velocity_min: [f32; 3],
    pub velocity_max: [f32; 3],
    pub controller_flag_c: bool,
    pub actor_flags: u32,
    pub notify_attached: bool,
    pub attached_owners: &'a [u32],
}

#[derive(Debug, Clone, Copy)]
enum DragonAction {
    HoverUp(HoverHeight),
    KakkuHover,
    HoverKakku { speed: f32, acceleration: f32 },
    TargetFly,
    PressAttack,
    BreathWide,
}

impl DragonAction {
    fn motion(self) -> u16 {
        match self {
            Self::HoverUp(_) => 100,
            Self::KakkuHover => 121,
            Self::HoverKakku { .. } => 103,
            Self::TargetFly => 126,
            Self::PressAttack => 298,
            Self::BreathWide => 263,
        }
    }
}

/// Action 54's two native parameter modes (0x00a6bf00).
pub enum HoverKakkuTarget {
    ThinkTarget { position: [f32; 3], speed: f32 },
    Configured { heading_degrees: f32, speed: f32 },
}

/// TargetFly's translation substep, not a complete action. Quaternion
/// steering, turn-layer blending and rider effects must be resolved before
/// calling this; action 10 remains rejected by the actor prefix.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetFlyTravel {
    pub target: [f32; 3],
    pub desired_speed: f32,
    pub speed: f32,
    pub arrival_radius: f32,
    pub done: bool,
}

impl TargetFlyTravel {
    /// Parameter modes 0/2 in 0x00a60b70 (DDDA.exe 2017-12-13 Steam).
    /// Zero desired speed becomes one, but current speed remains the original
    /// parameter value; the native code changes only action +0xB4.
    pub fn enter(target: [f32; 3], speed: f32, arrival_radius: f32) -> Self {
        Self {
            target,
            desired_speed: if speed == 0.0 { 1.0 } else { speed },
            speed,
            arrival_radius,
            done: false,
        }
    }

    /// Translation in vf07 0x00a61590, dispatched by phase 97 (0x0084c800)
    /// in pass C, after terrain correction and before collision activation.
    /// `cached_forward` is vf22(-1)'s matrix row 2, from actor +0xB0
    /// (getter 0x00ff9a20), NOT the freshly updated flight quaternion.
    /// `lagging_turn` is the sourced action +0xD4 state, not guessed here.
    pub fn step(
        &mut self,
        pos: &mut [f32; 3],
        cached_forward: [f32; 3],
        lagging_turn: bool,
        engine_step: f32,
    ) {
        if self.done {
            return;
        }
        // 0x00a61992..0x00a619cd saves pre-move displacement on the stack.
        let delta = std::array::from_fn::<_, 3, _>(|i| self.target[i] - pos[i]);
        if lagging_turn {
            self.speed -= engine_step * 0.5;
            if self.speed < self.desired_speed - 30.0 {
                self.speed = self.desired_speed - 30.0;
            }
        } else {
            self.speed += engine_step * 2.0;
            if self.speed >= self.desired_speed {
                self.speed = self.desired_speed;
            }
        }
        let length = (cached_forward[0] * cached_forward[0]
            + cached_forward[1] * cached_forward[1]
            + cached_forward[2] * cached_forward[2])
            .sqrt();
        let mut forward = cached_forward;
        if length >= f32::EPSILON {
            let inverse = 1.0 / length;
            forward.iter_mut().for_each(|v| *v *= inverse);
        }
        for i in 0..3 {
            pos[i] += self.speed * forward[i] * engine_step;
        }
        // Disassembly at 0x00a62098..0x00a620fc repairs decompiler stack
        // aliases: this uses saved delta Y/X/Z, not the advanced position.
        let distance = (delta[1] * delta[1] + delta[0] * delta[0] + delta[2] * delta[2]).sqrt();
        if distance < self.arrival_radius {
            *pos = self.target;
            self.done = true;
        }
    }
}

/// Supported actor prefix. Unsupported actions fail before cancelling the
/// current action. This deliberately cannot run the whole entrance yet.
pub struct Dragon {
    pub pos: [f32; 3],
    pub yaw: f32,
    pub rotation: [f32; 4],
    pub fighter: Fighter,
    pub frame: f32,
    pub motion_ended: bool,
    action: Option<DragonAction>,
    velocity: [f32; 3],
    done: bool,
    turn: Option<DragonYaw>,
    pub flight: Option<flight::TargetFly>,
    pub press: Option<attacks::PressAttack>,
    pub breath: Option<attacks::BreathWide>,
    /// Prior terrain contact and actor +0x679a == 0, resolved by the host.
    /// Missing input rejects Press stepping rather than fabricating contact.
    pub press_contact: Option<(bool, bool)>,
    primary_events: u32,
}

impl Dragon {
    pub fn new(data: &DragonData, owner: u32, pos: [f32; 3], yaw: f32) -> Self {
        Self {
            pos,
            yaw,
            rotation: [0.0, (yaw * 0.5).sin(), 0.0, (yaw * 0.5).cos()],
            fighter: data.fighter(owner),
            frame: 0.0,
            motion_ended: false,
            action: None,
            velocity: [0.0; 3],
            done: false,
            turn: None,
            flight: None,
            press: None,
            breath: None,
            press_contact: None,
            primary_events: 0,
        }
    }

    pub fn done(&self) -> bool {
        self.done
    }

    /// The native action id owned by this actor, for transport/inspection.
    /// Uses the same typed variants accepted by start_action and
    /// start_target_fly; does not infer actions from their motion slots.
    pub fn current_action(&self) -> Option<u32> {
        self.action.map(|a| match a {
            DragonAction::HoverUp(_) => 52,
            DragonAction::KakkuHover => 53,
            DragonAction::HoverKakku { .. } => 54,
            DragonAction::TargetFly => 10,
            DragonAction::PressAttack => 33,
            DragonAction::BreathWide => 21,
        })
    }

    pub fn motion(&self) -> Option<crate::body::MotionId> {
        self.action.map(|a| {
            let encoded = match a {
                DragonAction::PressAttack if self.press.as_ref().is_some_and(|p| p.phase == attacks::PressPhase::Landed) => 299,
                DragonAction::BreathWide if self.breath.as_ref().is_some_and(|b| b.phase == attacks::BreathPhase::Emitting) => 264,
                _ => a.motion(),
            };
            crate::body::MotionId::new((encoded >> 8) as u8, encoded & 0xff)
        })
    }

    /// Native action-33 entry 0x00a6aed0. Serialized phase and delay are
    /// resolved by the controller, not inferred from the animation frame.
    pub fn start_press(&mut self, data: &DragonData, phase: attacks::PressPhase, delay: f32)
        -> Result<Vec<DragonEffect>, DragonError>
    {
        let effects = self.enter(data, DragonAction::PressAttack, 0.0, None)?;
        self.press = Some(attacks::PressAttack::enter(data.press_params, phase, delay));
        Ok(effects)
    }

    /// Only the opening sequence-4, controller+0x0c == 0 branch of
    /// 0x00a56a30. Alternate dragon variants require a separate entry.
    pub fn start_opening_breath(&mut self, data: &DragonData)
        -> Result<Vec<DragonEffect>, DragonError>
    {
        let mut effects = self.enter(data, DragonAction::BreathWide, 0.0, None)?;
        self.breath = Some(attacks::BreathWide::enter());
        effects.push(DragonEffect::BreathPrewarm);
        Ok(effects)
    }

    /// TargetFly modes 0/2, 0x00a60b70. Mode 1 needs the actor's
    /// configured flight point and is deliberately resolved by the caller.
    pub fn start_target_fly(&mut self, data: &DragonData, target: [f32;3],
        speed: f32, angular_degrees: f32, arrival_radius: f32)
        -> Result<Vec<DragonEffect>, DragonError>
    {
        let flight = flight::TargetFly::enter(self.pos,self.rotation,target,speed,
            angular_degrees,arrival_radius,0.0);
        let effects = self.enter(data,DragonAction::TargetFly,0.0,None)?;
        self.flight = Some(flight);
        Ok(effects)
    }

    /// Cached uCoord matrix, rebuilt outside TargetFly's phase 97.
    pub fn model_matrix(&self) -> ddcombat::volumes::Mat4 {
        mtlmt::pose::compose([1.0;3],self.rotation,self.pos)
    }

    /// Pose in the authoritative actor orientation (including flight pitch).
    pub fn pose(&self, data: &DragonData) -> Result<crate::body::Posed,DragonError> {
        let motion = self.motion().ok_or(DragonError::NoAction)?;
        let model = self.model_matrix();
        let local = data.body.skeleton.pose(data.body.motion(motion),self.frame);
        let joints = data.body.skeleton.joints.iter().zip(local)
            .map(|(joint, matrix)| (joint.id,mtlmt::pose::mul(&matrix,&model))).collect();
        Ok(crate::body::Posed {model,joints})
    }

    /// uCharacterBase::vf97 -> action vf07, 0x0084c800 -> 0x00a61590.
    /// Invoke after terrain/correction and before finish_pass_c.
    pub fn pass_c_action(&mut self, cached_matrix: &ddcombat::volumes::Mat4, step: f32) {
        if let Some(flight) = &mut self.flight {
            flight.step(&mut self.pos,[cached_matrix[2][0],cached_matrix[2][1],cached_matrix[2][2]],step);
            self.rotation = flight.rotation;
            let heading = flight::forward(self.rotation);
            self.yaw = heading[0].atan2(heading[2]);
            self.done = flight.travel.done;
        }
    }

    pub fn velocity(&self) -> Option<[f32; 3]> {
        self.action.map(|_| self.velocity)
    }

    /// Starts action 52 or 53. Action 54 requires [`Self::start_hover_kakku`]
    /// because it has additional sourced parameters. Typed HoverUp modes
    /// avoid accepting unknown serialized mode values. Entry/exit translated
    /// from 0x00a6ba90/0x00a6bc80; action 53 from 0x00a6bd80/0x00a76c90.
    pub fn start_action(
        &mut self,
        data: &DragonData,
        action: u32,
        scale_y: f32,
        height: HoverHeight,
    ) -> Result<Vec<DragonEffect>, DragonError> {
        let action = match action {
            52 => DragonAction::HoverUp(height),
            53 => DragonAction::KakkuHover,
            _ => return Err(DragonError::Unsupported(action)),
        };
        self.enter(data, action, scale_y, None)
    }

    /// Action 54 entry/exit: 0x00a6bf00/0x00a6c130 (Steam 2017-12-13).
    pub fn start_hover_kakku(
        &mut self,
        data: &DragonData,
        target: HoverKakkuTarget,
    ) -> Result<Vec<DragonEffect>, DragonError> {
        let (heading, speed) = match target {
            HoverKakkuTarget::ThinkTarget { position, speed } => {
                (DragonYaw::target_bearing(self.pos, position), speed)
            }
            HoverKakkuTarget::Configured {
                heading_degrees,
                speed,
            } => (heading_degrees * 0.017453292, speed),
        };
        self.enter(
            data,
            DragonAction::HoverKakku {
                speed,
                acceleration: speed * 0.04,
            },
            0.0,
            Some(DragonYaw::enter(self.yaw, heading)),
        )
    }

    /// Captured action +0x80/+0x7C fields; they do not imply translation
    /// during HoverKakku's vf06, which only checks motion completion.
    pub fn flight_speed_parameters(&self) -> Option<(f32, f32)> {
        match self.action {
            Some(DragonAction::HoverKakku { speed, acceleration }) => Some((speed, acceleration)),
            _ => None,
        }
    }

    fn enter(
        &mut self,
        data: &DragonData,
        action: DragonAction,
        scale_y: f32,
        turn: Option<DragonYaw>,
    ) -> Result<Vec<DragonEffect>, DragonError> {
        let motion = action.motion();
        let id = crate::body::MotionId::new((motion >> 8) as u8, motion & 0xff);
        if data.body.frames(id) == 0 {
            return Err(DragonError::MissingMotion(motion));
        }
        let mut effects = Vec::new();
        if let Some(previous) = self.action {
            if matches!(previous, DragonAction::HoverUp(_)) {
                self.velocity = [0.0; 3];
                effects.push(DragonEffect::ExitHover);
            }
            effects.push(DragonEffect::BaseActionExit);
            if matches!(previous, DragonAction::HoverKakku { .. }) {
                self.velocity = [0.0; 3];
                self.turn = None;
                effects.push(DragonEffect::ClearAcceleration);
            }
            if matches!(previous, DragonAction::PressAttack) {
                self.velocity = [0.0; 3];
                effects.push(DragonEffect::Gsf { no: 463, value: false });
            }
            if matches!(previous, DragonAction::BreathWide) {
                effects.push(DragonEffect::BreathStop);
            }
        }
        self.action = Some(action);
        self.flight = None;
        self.press = None;
        self.breath = None;
        self.primary_events = 0;
        // TargetFly entry disables the ordinary yaw rotator (+0x2576,
        // 0x00a60b70); its quaternion steering owns rotation instead.
        self.turn = None;
        self.done = false;
        if let DragonAction::HoverUp(height) = action {
            self.velocity = HoverUp::enter(scale_y, height).velocity;
        }
        if let Some(turn) = turn {
            self.yaw = turn.current;
            self.rotation = [0.0,(self.yaw*0.5).sin(),0.0,(self.yaw*0.5).cos()];
            self.turn = Some(turn);
        }
        self.frame = 0.0;
        self.motion_ended = false;
        effects.push(DragonEffect::BaseActionEnter);
        effects.push(DragonEffect::BindMotion {
            id: motion,
            blend: 5.0,
            speed: 1.0,
        });
        if matches!(action, DragonAction::KakkuHover) {
            effects.push(DragonEffect::ResetFlightLayer);
        }
        Ok(effects)
    }

    /// Phase 91 action update (0x00a6bb40/0x00a6be00), yaw rotator
    /// (0x0084cc00), velocity integration
    /// (0x0084ccc0), phase 92 motion/root travel (0x010629f0/0x0105b740),
    /// then phase 93 collision flags. DDDA.exe 2017-12-13 Steam.
    /// The host must consume returned effects before advancing other actors.
    /// Volumes retain the previous pass-C snapshot until [`Self::finish_pass_c`].
    pub fn step(
        &mut self,
        data: &DragonData,
        input: DragonStep<'_>,
    ) -> Result<Vec<DragonEffect>, DragonError> {
        let action = self.action.ok_or(DragonError::NoAction)?;
        let mut effects = Vec::new();
        match action {
            DragonAction::PressAttack => effects.push(DragonEffect::PressControl {
                landed: self.press.as_ref().unwrap().phase == attacks::PressPhase::Landed,
            }),
            DragonAction::BreathWide => effects.push(DragonEffect::BreathControl {
                emitting: self.breath.as_ref().unwrap().phase == attacks::BreathPhase::Emitting,
            }),
            _ => effects.push(DragonEffect::HoverAirControl),
        }
        if matches!(action, DragonAction::HoverUp(_)) && input.notify_attached {
            effects.extend(
                input
                    .attached_owners
                    .iter()
                    .map(|&owner| DragonEffect::AttachedMessage { owner, message: 4 }),
            );
        }
        match action {
            DragonAction::HoverUp(height) => {
                let mut hover = HoverUp {
                    height,
                    velocity: self.velocity,
                    done: self.done,
                };
                hover.update(self.pos[1], input.think_target_y);
                self.velocity = hover.velocity;
                self.done = hover.done;
            }
            DragonAction::TargetFly => {},
            DragonAction::PressAttack => {
                let (ground_contact, notify_landing) = self.press_contact.ok_or(DragonError::MissingTerrainContact)?;
                let out = self.press.as_mut().unwrap().update(data.press_params, attacks::PressInput {
                    engine_step: input.engine_step, primary_events: self.primary_events,
                    ground_contact, motion_frame: self.frame, motion_ended: self.motion_ended,
                    notify_landing, cached_position: self.pos,
                });
                if let Some(velocity) = out.actor_velocity { self.velocity = velocity; }
                if let Some((no, value)) = out.gsf { effects.push(DragonEffect::Gsf { no, value }); }
                if let Some(position) = out.escape_position { effects.push(DragonEffect::EscapeFly { position }); }
                if let Some((id, blend)) = out.motion { self.rebind(id, blend, &mut effects); }
                self.done = self.press.as_ref().unwrap().done;
            },
            DragonAction::BreathWide => {
                let out = self.breath.as_mut().unwrap().update(self.frame, self.motion_ended);
                if let Some((id, blend)) = out.motion { self.rebind(id, blend, &mut effects); }
                if out.start_effects { effects.push(DragonEffect::BreathStart); }
                if out.collect_particles { effects.push(DragonEffect::BreathParticles); }
                if out.stop_effects { effects.push(DragonEffect::BreathStop); }
                self.done = self.breath.as_ref().unwrap().done;
            },
            _ => self.done |= hover_transition_complete(self.motion_ended, input.motion_flags),
        }
        if let Some(turn) = &mut self.turn {
            turn.step(
                input.yaw_enabled,
                input.movement_condition_flags,
                input.yaw_slow_factor,
                input.engine_step,
            );
            self.yaw = turn.current;
            self.rotation = [0.0,(self.yaw*0.5).sin(),0.0,(self.yaw*0.5).cos()];
        }
        for multiplier in input.horizontal_status_multipliers.into_iter().flatten() {
            self.velocity[0] *= multiplier;
            self.velocity[2] *= multiplier;
        }
        for axis in 0..3 {
            self.pos[axis] += self.velocity[axis] * input.engine_step + input.external_displacement[axis];
            self.velocity[axis] += input.acceleration[axis] * input.engine_step;
            // Native upper-then-lower ordering; inputs must be finite actor state.
            if self.velocity[axis] > input.velocity_max[axis] {
                self.velocity[axis] = input.velocity_max[axis];
            }
            if self.velocity[axis] < input.velocity_min[axis] {
                self.velocity[axis] = input.velocity_min[axis];
            }
        }
        let id = self.motion().ok_or(DragonError::NoAction)?;
        let motion = data.body.motion(id).filter(|m| m.frame_count > 0)
            .ok_or(DragonError::MissingMotion((u16::from(id.bank) << 8) | id.slot))?;
        let root = ddsim::RootMotion::from_motion(motion);
        let delta = if motion.loop_frame >= 0 {
            root.delta(self.frame, input.motion_step)
        } else {
            root.delta_once(self.frame, input.motion_step)
        };
        let travel = transform_root_translation(delta, input.root_translation_transform);
        for (position, displacement) in self.pos.iter_mut().zip(travel) {
            *position += displacement;
        }
        let next = self.frame + input.motion_step;
        let last = motion.frame_count.saturating_sub(1) as f32;
        self.motion_ended = if motion.loop_frame >= 0 {
            next > last
        } else {
            next >= last
        };
        self.frame = data.body.wrap_frame(id, next);
        // Primary layer event word consumed by the following phase 91.
        // Translated from 0x01060730/0x01057060 (Steam 2017-12-13).
        if let Some(bank) = data.body.banks.get(usize::from(id.bank)).and_then(Option::as_ref) {
            if let Some(groups) = bank.list.events.get(usize::from(id.slot)) {
                self.primary_events = groups.first().map_or(0, |runs|
                    mtlmt::event_bits(runs, (next - input.motion_step) as i32, next as i32));
            }
        }
        if let Victim::Enemy { rates, .. } = &mut self.fighter.victim {
            sync_collision_flags(rates, input.controller_flag_c, input.actor_flags);
        }
        Ok(effects)
    }

    fn rebind(&mut self, id: u16, blend: f32, effects: &mut Vec<DragonEffect>) {
        self.frame = 0.0;
        self.motion_ended = false;
        self.primary_events = 0;
        effects.push(DragonEffect::BindMotion { id, blend, speed: 1.0 });
    }

    /// Final pass-C activation boundary, after authoritative terrain correction
    /// and action vf07/phase97 (uObjModel::vf09 0x00886530). The host supplies
    /// the resolved joint/model pose: this prefix cannot infer constraints,
    /// blend layers or the original's cached-matrix lifetime. Never invoke
    /// this during phase93 merely to obtain an early volume snapshot.
    pub fn finish_pass_c(
        &mut self,
        data: &DragonData,
        resolved_pose: &crate::body::Posed,
    ) -> Result<(), DragonError> {
        let id = self.motion().ok_or(DragonError::NoAction)?;
        if data.body.frames(id) == 0 {
            return Err(DragonError::MissingMotion((u16::from(id.bank) << 8) | id.slot));
        }
        let acts = data.body.activations(id, self.frame);
        (self.fighter.attacks, self.fighter.hurts) =
            data.body.volumes(&acts, resolved_pose, self.fighter.owner);
        Ok(())
    }
}

// Host boundary for the scaled/oriented root travel in 0x0105b740.
// Do not infer this transform from the collision body's mean scale.
fn transform_root_translation(delta: [f32; 3], transform: [[f32; 3]; 3]) -> [f32; 3] {
    std::array::from_fn(|axis| {
        delta[0] * transform[0][axis] + delta[1] * transform[1][axis] + delta[2] * transform[2][axis]
    })
}

/// Loaded opening-dragon body and combat parameters. Action stepping is
/// separate; this type does not manufacture action completion or volumes.
#[derive(Debug, Clone)]
pub struct DragonData {
    pub ai: DragonAiData,
    pub body: crate::body::Body,
    pub attacker: EnemyAttacker,
    pub defender: EnemyDefender,
    pub regions: Vec<RegionPool>,
    pub obj_size: u32,
    pub press_params: attacks::PressParams,
}

impl DragonData {
    pub fn opening(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>, scale: f32) -> Result<Self, String> {
        let ai = DragonAiData::opening(exe).map_err(|e| e.to_string())?;
        let body = ai.load_body(vfs, scale)?;
        // 0x00a49f2d clears EAX before 0x00770910 selects region kind 0.
        // 0x00a49f3d passes charparam\em\em5800_cmn to 0x00aafdb0.
        // DDDA.exe 2017-12-13 Steam; shared reader keeps bulk values in the install.
        let stats = crate::enemy_stats::load(vfs, "em5800")?;
        Ok(Self {
            ai,
            body,
            attacker: stats.attacker,
            defender: stats.defender,
            regions: stats.regions,
            obj_size: stats.obj_size,
            press_params: attacks::PressParams::load(vfs)?,
        })
    }

    /// Constructs the actual fight component with independent owner and pools.
    /// The sequence-4 initialization branch of 0x00a49f00 sets collision
    /// +0xCFC bit 0: HP damage is disabled initially. The later phase-93
    /// projection is [`sync_collision_flags`]; callers must supply actual
    /// controller/condition state before assuming the entrance is damageable.
    pub fn fighter(&self, owner: u32) -> Fighter {
        Fighter {
            owner,
            attacker: Attacker::Enemy(self.attacker),
            victim: Victim::Enemy {
                defender: self.defender,
                regions: self.regions.clone(),
                rates: CollisionRates {
                    flags: 1,
                    ..Default::default()
                },
            },
            attacks: Vec::new(),
            hurts: Vec::new(),
            history: Default::default(),
            statuses: Default::default(),
            dots: Default::default(),
            pools: Default::default(),
        }
    }
}

/// uEm5800 initialization 0x00a49f00, sequence-4 branch: assigns this root
/// to the think manager's current, default and base table pointers.
pub const OPENING_THINK_ROOT: u32 = 0x0169_9518;
/// Action descriptors in the supported executable (0x44 bytes each, DTI
/// at +0x28), shared by uEm5800 action dispatch. 0x00a3f89f passes the DTI
/// column 0x0185bd20 to 0x007f19c0, which advances by 0x44 per record.
pub const ACTION_TABLE: u32 = 0x0185_bcf8;
/// Embedded count passed in EDI at 0x00a3f8a4 (DDDA.exe 2017-12-13 Steam).
pub const ACTION_COUNT: u32 = 93;

/// uEm5800::vf05 0x00a49ed0 passes this table and count to 0x00889de0
/// (DDDA.exe 2017-12-13 Steam). Each descriptor occupies three words.
pub const RESOURCE_TABLE: u32 = 0x0185_d5d8;
pub const RESOURCE_COUNT: u32 = 57;

/// A resource registration, preserving executable order without assuming
/// that registration indices are motion-bank or collision-set indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragonResource {
    pub path: String,
    pub class: String,
    /// Flags after the executable's static initializer has run.
    pub flags: u32,
}

// Translated from 0x01363d50..0x0136412c (DDDA.exe 2017-12-13 Steam).
// Startup enables binding for the first 49 descriptors; the remaining
// effect/sound resources are preload-only. Disk flag words are zero.
fn initialized_resource_flags(index: u32, disk_flags: u32) -> u32 {
    if index < 49 {
        (disk_flags & !4) | 3
    } else {
        (disk_flags & !5) | 2
    }
}

#[derive(Debug, Clone)]
pub struct DragonAiData {
    pub tables: Tables,
    pub action_classes: Vec<String>,
    pub resources: Vec<DragonResource>,
}

impl DragonAiData {
    /// Binds the model, motion lists and collision resources selected by
    /// the initialized descriptor table. Scale belongs to the actor's
    /// placement; it is deliberately supplied by the caller.
    ///
    /// Translated from 0x008892b0 / 0x00889510 (DDDA.exe 2017-12-13 Steam):
    /// bit 0 enables binding, motion banks advance in descriptor order.
    /// Collision activations select the loaded file by its resource ID.
    pub fn load_body(&self, vfs: &ddvfs::Vfs, scale: f32) -> Result<crate::body::Body, String> {
        use crate::body::{Body, load};
        let bound: Vec<_> = self.resources.iter().filter(|r| r.flags & 1 != 0).collect();
        let models: Vec<_> = bound.iter().filter(|r| r.class == "rModel").collect();
        if models.len() != 1 {
            return Err(format!("dragon: expected one bound model, got {}", models.len()));
        }
        let skeleton = load::skeleton(vfs, &models[0].path)?;
        let banks = bound
            .iter()
            .filter(|r| r.class == "rMotionList")
            .map(|r| load::bank(vfs, &r.path).map(Some))
            .collect::<Result<_, _>>()?;
        let collision = bound
            .iter()
            .filter(|r| r.class == "rObjCollision")
            .map(|r| load::collision(vfs, &r.path, None))
            .collect::<Result<_, _>>()?;
        Ok(Body {
            skeleton,
            banks,
            collision,
            scale,
        })
    }

    /// Reads the Cassardis sequence's reachable think records and dragon
    /// action class names. No bulk game records are compiled into the port.
    pub fn opening(exe: &ddexe::Exe<'_>) -> Result<Self, Error> {
        let tables = Tables::read_roots(exe, vec![OPENING_THINK_ROOT], &[])?;
        let names = exe.class_names();
        let mut action_classes = Vec::new();
        // Translated from 0x00a3f89f / 0x007f19c0 (DDDA.exe 2017-12-13 Steam).
        for index in 0..ACTION_COUNT {
            let address = ACTION_TABLE + index * 0x44;
            let dti = exe.u32(address + 0x28).map_err(|e| Error::Data(e.0))?;
            action_classes.push(names.get(&dti).cloned().ok_or_else(|| {
                Error::Data(format!(
                    "dragon action {}: unknown DTI {dti:#x}",
                    action_classes.len()
                ))
            })?);
        }
        let mut resources = Vec::new();
        // Translated from 0x00889de0 (DDDA.exe 2017-12-13 Steam): null
        // path entries are skipped, and surviving entries retain order.
        for index in 0..RESOURCE_COUNT {
            let address = RESOURCE_TABLE + index * 12;
            let path = exe.u32(address).map_err(|e| Error::Data(e.0))?;
            if path == 0 {
                continue;
            }
            let dti = exe.u32(address + 4).map_err(|e| Error::Data(e.0))?;
            resources.push(DragonResource {
                path: exe.cstr(path).map_err(|e| Error::Data(e.0))?.to_owned(),
                class: names
                    .get(&dti)
                    .cloned()
                    .ok_or_else(|| Error::Data(format!("dragon resource {index}: unknown DTI {dti:#x}")))?,
                flags: initialized_resource_flags(index, exe.u32(address + 8).map_err(|e| Error::Data(e.0))?),
            });
        }
        Ok(Self {
            tables,
            action_classes,
            resources,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn target_fly_translation_uses_cached_forward_and_pre_move_strict_distance() {
        use super::TargetFlyTravel;
        let mut flight = TargetFlyTravel::enter([0.0, 0.0, 300.0], 115.0, 300.0);
        let mut pos = [0.0; 3];
        flight.step(&mut pos, [0.0, 0.0, 2.0], false, 1.0);
        assert_eq!(pos, [0.0, 0.0, 115.0]);
        assert!(!flight.done, "exact radius before movement does not finish");
        flight.step(&mut pos, [2.0, 0.0, 0.0], false, 1.0);
        assert_eq!(pos, flight.target, "inside radius snaps after translation");
        assert!(flight.done);
        flight.step(&mut pos, [1.0, 0.0, 0.0], true, 100.0);
        assert_eq!(pos, flight.target);
        assert_eq!(flight.speed, 115.0);
        let mut slow = TargetFlyTravel::enter([1000.0; 3], 115.0, 1.0);
        slow.step(&mut pos, [0.0, 0.0, 1.0], true, 100.0);
        assert_eq!(slow.speed, 85.0);
        slow.step(&mut pos, [0.0, 0.0, 1.0], false, 1.0);
        assert_eq!(slow.speed, 87.0);
        let mut zero = TargetFlyTravel::enter([1000.0; 3], 0.0, 1.0);
        assert_eq!((zero.desired_speed, zero.speed), (1.0, 0.0));
        zero.step(&mut pos, [0.0; 3], false, 0.25);
        assert_eq!(zero.speed, 0.5);
    }
    #[test]
    fn hover_kakku_yaw_preserves_gates_short_arc_and_strict_arrival() {
        use super::DragonYaw;
        assert_eq!(super::wrap_yaw(3.1415927), -3.1415927);
        assert_eq!(super::wrap_yaw(-3.1415927), -3.1415927);
        assert_eq!(super::wrap_yaw(6.2831855), 0.0);
        assert_eq!(super::wrap_yaw(-6.2831855), 0.0);
        let mut yaw = DragonYaw::enter(3.0, -3.0);
        assert!(yaw.active);
        assert!((yaw.rate - (6.2831855 - 6.0) / 30.0).abs() < 1e-7);
        let before = yaw;
        yaw.step(false, 0, 1.0, 1.0);
        yaw.step(true, 0x800, 1.0, 1.0);
        assert_eq!(yaw, before);
        yaw.step(true, 0x1000, 0.5, 1.0);
        assert!((yaw.current - (3.0 + yaw.rate * 0.5)).abs() < 1e-6);
        let mut edge = DragonYaw {
            current: 0.0,
            target: 1.0,
            rate: 1.0,
            active: true,
        };
        edge.step(true, 0, 1.0, 1.0);
        assert_eq!(edge.current, 1.0);
        assert!(edge.active, "native arrival comparison is strict");
        edge.step(true, 0, 1.0, 1.0);
        assert!(!edge.active);
        assert_eq!(DragonYaw::target_bearing([0.0; 3], [0.0, 8.0, 2.0]), 0.0);
        assert!((DragonYaw::target_bearing([0.0; 3], [2.0, 8.0, 0.0]) - 1.5707964).abs() < 1e-6);
    }
    #[test]
    fn root_translation_requires_resolved_scale_and_orientation() {
        // Nonuniform scale followed by a quarter turn about Y.
        let transform = [[0.0, 0.0, -2.0], [0.0, 3.0, 0.0], [4.0, 0.0, 0.0]];
        assert_eq!(
            super::transform_root_translation([1.0, 2.0, 3.0], transform),
            [12.0, 6.0, -2.0]
        );
    }
    use super::initialized_resource_flags;
    use super::{CollisionRates, sync_collision_flags};
    use super::{HoverHeight, HoverUp, hover_transition_complete};

    #[test]
    fn hover_up_tracks_live_target_and_stops_before_integrating_again() {
        let mut rise = HoverUp::enter(2.0, HoverHeight::ThinkTarget);
        assert_eq!(HoverUp::MOTION, 100);
        assert_eq!(rise.velocity, [0.0, 70.0, 0.0]);
        rise.update(100.0, 120.0);
        assert!(!rise.done);
        // A moved think target is read on this frame; the controller does
        // not retain the old 120 height or snap the actor down to 90.
        rise.update(100.0, 90.0);
        assert!(rise.done);
        assert_eq!(rise.velocity, [0.0; 3]);
        let mut fixed = HoverUp::enter(1.0, HoverHeight::Explicit(130.0));
        fixed.update(129.0, 0.0);
        assert!(!fixed.done);
        fixed.update(130.0, 1000.0);
        assert!(fixed.done);
        let mut cancelled = HoverUp::enter(1.0, HoverHeight::ThinkTarget);
        cancelled.update(100.0, f32::NAN);
        assert!(!cancelled.done);
        cancelled.exit();
        assert_eq!(cancelled.velocity, [0.0; 3]);
        assert!(!cancelled.done, "exit is not action completion");
    }

    #[test]
    fn hover_transition_has_only_the_two_sourced_completion_paths() {
        assert!(!hover_transition_complete(false, 0x7fff_ffff));
        assert!(hover_transition_complete(true, 0));
        assert!(hover_transition_complete(false, 0x8000_0000));
    }

    #[test]
    fn phase93_keeps_controller_guard_and_projects_only_four_collision_bits() {
        let mut rates = CollisionRates {
            flags: 0x8a01,
            ..Default::default()
        };
        sync_collision_flags(&mut rates, true, 0);
        assert_eq!(
            rates.flags, 0x8a01,
            "completed/killed controller preserves initialization"
        );
        sync_collision_flags(&mut rates, false, 0);
        assert_eq!(
            rates.flags, 0x8a00,
            "running controller clears the initial no-HP bit"
        );
        for (actor, collision) in [
            (0x0800_0000, 1),
            (0x0400_0000, 2),
            (0x1000_0000, 4),
            (0x0200_0000, 8),
        ] {
            sync_collision_flags(&mut rates, false, actor);
            assert_eq!(rates.flags, 0x8a00 | collision);
        }
        sync_collision_flags(&mut rates, false, 0xe1ff_ffff);
        assert_eq!(rates.flags, 0x8a00, "unrelated actor flags must not project");
    }

    #[test]
    fn static_initialization_preserves_unrelated_flag_bits() {
        assert_eq!(initialized_resource_flags(48, 0), 3);
        assert_eq!(initialized_resource_flags(49, 0), 2);
        assert_eq!(initialized_resource_flags(48, 0xffff_ffff), 0xffff_fffb);
        assert_eq!(initialized_resource_flags(49, 0xffff_ffff), 0xffff_fffa);
    }
}

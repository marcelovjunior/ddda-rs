//! Wolf and harpy runtime data and sourced action phases.
use crate::body::{load, Body, MotionId};
use ddai::thinktbl::families::{Action, Family};
use ddai::thinktbl::Tables;
use ddcombat::damage::{EnemyAttacker, EnemyDefender, RegionPool};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct BeastData {
    pub family: Family,
    pub body: Body,
    pub tables: Arc<Tables>,
    pub actions: Vec<Action>,
    pub attacker: EnemyAttacker,
    pub defender: EnemyDefender,
    pub regions: Vec<RegionPool>,
    pub obj_size: u32,
    pub wolf: Option<WolfParams>,
    /// Common bank 3 record 0x42, loaded from the installed executable.
    pub common_death_motion: MotionId,
}

#[derive(Debug, Clone, Copy)]
pub struct WolfParams {
    pub attack_range: f32,
    pub turn_rate: f32,
}

/// Harpy's family override after the common enemy death callback accepts it.
/// Caller must preserve the original guards: actor is not already dead,
/// +0x2364 bit 24 is clear and +0x2360 bit 29 is clear. Other branches retain
/// the common callback's result and are not represented by this selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarpyDeath {
    Ground,
    Air,
    FrozenBreak,
    PetrifiedBreak,
}
impl HarpyDeath {
    /// `freeze_visual_state` is actor+0x5cb4, updated by 0x00ab01a0;
    /// it may remain nonzero while the frozen status is being removed.
    /// `petrify_progress` is actor+0x27b0, updated by 0x00853260.
    // Translated from 0x0093c700 (DDDA.exe 2017-12-13 Steam).
    pub fn select(movement_flags: u32, freeze_visual_state: u32, petrify_progress: f32) -> Self {
        if petrify_progress <= 0.0 {
            Self::PetrifiedBreak
        } else if freeze_visual_state != 0 {
            Self::FrozenBreak
        } else if movement_flags & 1 != 0 {
            Self::Air
        } else {
            Self::Ground
        }
    }

    // Pending action +0x5b58 assignments in 0x0093c700.
    pub fn action(self) -> usize {
        match self {
            Self::Ground => 3,
            Self::Air => 4,
            Self::FrozenBreak => 0x0300_005f,
            Self::PetrifiedBreak => 0x0300_0060,
        }
    }
}

impl BeastData {
    pub fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>, family: Family) -> Result<Self, String> {
        let (id, group, model) = match family {
            Family::Wolf => ("em0200", "e02", "e0200"),
            Family::Harpy => ("em0600", "e06", "e0600"),
        };
        // Unit resource lists registered by 0x008f7b80 / 0x0093cbd0.
        let skeleton = load::skeleton(vfs, &format!(r"model\em\{group}\{model}\{model}"))?;
        let mut banks = Vec::new();
        for suffix in ["co", "at", "dm"] {
            banks.push(Some(load::bank(
                vfs,
                &format!(r"motion\em\{group}\{model}_{suffix}\{model}_{suffix}"),
            )?));
        }
        let collision_path = format!(r"collision\em\{group}\{model}\{model}");
        // Both resource lists register one rObjCollision containing the
        // body and attacks, unlike em0100's separate _00 attack resource.
        let collision = vec![load::collision(vfs, &collision_path, None)?];
        let stats = crate::enemy_stats::load(vfs, id)?;
        let wolf = if family == Family::Wolf {
            // uEm0200::vf33 0x008f7b80: rows0/1 -> +6090/+609c.
            let bytes = load::read(vfs, r"param\em0200", "rAdjustParam")?;
            let p = ddenemy::AdjustParam::parse(&bytes)?;
            if p.values.len() < 2 {
                return Err("wolf adjust missing steering/range".into());
            }
            Some(WolfParams {
                attack_range: p.values[0],
                turn_rate: p.values[1] * 0.017_453_292,
            })
        } else {
            None
        };
        // uEnemy::vf05 0x00aa8830 registers bank 3 at 0x01864b98.
        // 0x004c9e50 copies record +0x14 to the action's primary motion;
        // cEmActActingDie::vf05 0x008a66e0 starts it when present.
        let death_record = 0x0186_4b98 + 0x42 * 0x44;
        let dti = exe.u32(death_record)?;
        if exe.class_names().get(&dti).map(String::as_str) != Some("cEmActActingDie") {
            return Err("unexpected common grounded death action".into());
        }
        let death_motion = exe.u32(death_record + 0x14)?;
        let common_death_motion = MotionId::new(((death_motion >> 8) & 15) as u8, (death_motion & 255) as u16);
        Ok(Self {
            family,
            // Standard em0200/em0600 actor scale is not yet compared live.
            body: Body {
                skeleton,
                banks,
                collision,
                scale: 1.0,
            },
            tables: Arc::new(family.tables(exe).map_err(|e| e.to_string())?),
            actions: family.actions(exe).map_err(|e| e.to_string())?,
            attacker: stats.attacker,
            defender: stats.defender,
            regions: stats.regions,
            obj_size: stats.obj_size,
            wolf,
            common_death_motion,
        })
    }
}

/// Runtime for actions whose phases have been traced. The family think host
/// chooses actions separately; unsupported classes return an error rather
/// than borrowing the goblin's motions or behaviour.
#[derive(Debug)]
pub struct Beast {
    pub family: Family,
    /// Captured target selected by the family host, never a global player.
    pub target: Option<u32>,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub action: Option<usize>,
    pub motion: MotionId,
    pub frame: f32,
    pub motion_ended: bool,
    pub done: bool,
    pub movement_flags: u32,
    /// Wolf +602e, raised by AttackRun when its captured target enters the
    /// sourced distance/cone gate; consumed by the next unit think.
    pub attack_ready: bool,
    pub fighter: crate::combat::Fighter,
    phase: ActionPhase,
}

#[derive(Debug, Clone, Copy)]
enum ActionPhase {
    None,
    Bite(WolfBite),
    AttackRun,
    Howl,
    Run,
    Dash,
    Takeoff(HarpyTakeoff),
    GroundDeath,
    CommonDeath,
}

impl Beast {
    pub fn new(data: &BeastData, owner: u32, pos: [f32; 3], yaw: f32) -> Self {
        use crate::combat::{Attacker, Fighter, Victim};
        Self {
            family: data.family,
            target: None,
            pos,
            yaw,
            action: None,
            motion: MotionId::new(0, 0),
            frame: 0.0,
            motion_ended: false,
            done: true,
            movement_flags: 0,
            attack_ready: false,
            phase: ActionPhase::None,
            fighter: Fighter {
                owner,
                attacker: Attacker::Enemy(data.attacker),
                victim: Victim::Enemy {
                    defender: data.defender,
                    regions: data.regions.clone(),
                    rates: Default::default(),
                },
                attacks: vec![],
                hurts: vec![],
                history: Default::default(),
                statuses: Default::default(),
                dots: Default::default(),
                pools: Default::default(),
            },
        }
    }

    /// Only standard em0200/em0600 forms are handled here. The normal wolf
    /// grounded death uses common bank 3 action 0x42 (0x008f6230). The local
    /// cEm0200Die, alternate forms and flying deaths remain unsupported.
    pub fn start_action(&mut self, data: &BeastData, id: usize) -> Result<(), String> {
        if self.family != data.family { return Err("mismatched beast family data".into()); }
        let (phase, motion) = if data.family == Family::Wolf && id == 0x0300_0042 {
            (ActionPhase::CommonDeath, data.common_death_motion)
        } else {
          let action = data.actions.get(id).ok_or("family action outside table")?;
          match (data.family, action.class.as_str(), action.variant) {
            (Family::Wolf, "cEm0200Bite", _) => {
                (ActionPhase::Bite(WolfBite::Windup), WolfBite::Windup.motion())
            }
            // 0x008fe110 / 0x009026a0; standard uninterrupted howl.
            (Family::Wolf, "cEm0200AttackRun", _) => (ActionPhase::AttackRun, MotionId::new(0, 0x24)),
            (Family::Wolf, "cEm0200Howling", _) => (ActionPhase::Howl, MotionId::new(0, 0x64)),
            // 0x008fc720 / 0x008fca50: authored root-motion locomotion.
            (Family::Wolf, "cEm0200Run", _) => (ActionPhase::Run, MotionId::new(0, 0x1a)),
            (Family::Wolf, "cEm0200Dash", _) => (ActionPhase::Dash, MotionId::new(0, 0x24)),
            (Family::Harpy, "cEm0600ActGroundToHover", _) => (
                ActionPhase::Takeoff(HarpyTakeoff::TakingOff),
                HarpyTakeoff::initial_motion(),
            ),
            // 0x009471f0: standard harpy, no special subtype32/33 state.
            (Family::Harpy, "cEm0600ActDie", _) => (ActionPhase::GroundDeath, MotionId::new(2, 0x39)),
            _ => {
                return Err(format!(
                    "unported {:?} action {id}: {} variant{}",
                    data.family, action.class, action.variant
                ))
            }
          }
        };
        if data.body.motion(motion).is_none() {
            return Err(format!("missing family motion {motion:?}"));
        }
        self.action = Some(id);
        self.phase = phase;
        self.done = false;
        self.motion = motion;
        // Dash starts at frame one (0x008fca50); other supported initial
        // motions start at zero. Navigation/pack steering remain host work.
        self.frame = if matches!(phase, ActionPhase::Dash) { 1.0 } else { 0.0 };
        self.motion_ended = false;
        Ok(())
    }

    /// Action phase91 then motion phase92, as in the existing goblin runtime.
    /// The caller supplies captured target and world ground. This is action
    /// execution, not a substitute for the family's unconnected think host.
    pub fn step_action(
        &mut self,
        data: &BeastData,
        target: Option<[f32; 3]>,
        ground: &dyn ddsim::Ground,
        dt: f32,
    ) {
        let mut next = None;
        self.movement_flags = 0;
        match &mut self.phase {
            ActionPhase::Bite(bite) => {
                if bite.tracks_target(self.frame) {
                    if let Some(target) = target {
                        // Wolf phase91 0x008f0ff0 / target yaw0x008f5550.
                        let goal = (target[0] - self.pos[0]).atan2(target[2] - self.pos[2]);
                        self.yaw = wolf_turn(self.yaw, goal, 0.1, dt);
                    }
                }
                if self.motion_ended && *bite != WolfBite::Finished {
                    bite.motion_end();
                    self.done = *bite == WolfBite::Finished;
                    if !self.done {
                        next = Some((bite.motion(), 0.0));
                    }
                }
            }
            ActionPhase::AttackRun => {
                // 0x008fe160; phase91 later uses +609c through7975d0.
                if let (Some(target), Some(param)) = (target, data.wolf) {
                    let goal = (target[0] - self.pos[0]).atan2(target[2] - self.pos[2]);
                    // Cone gate precedes the phase91 angle-controller update.
                    self.attack_ready |= ddai::thinktbl::families::wolf_target_in_cone(
                        Some(self.pos),
                        target,
                        self.yaw,
                        f32::from_bits(0x3f860a92),
                    ) && distance(self.pos, target).powi(2)
                        < param.attack_range * param.attack_range;
                    self.yaw = wolf_turn(self.yaw, goal, param.turn_rate, dt);
                }
                self.done = true;
            }
            ActionPhase::Howl => {
                // 0x009027c0. Pack sensor broadcast/forced interruption are
                // not in this selected-action runner; the think host owns them.
                self.done = self.motion_ended;
            }
            ActionPhase::Run => {
                // Translated from 0x008fc770 (DDDA.exe 2017-12-13 Steam).
                // This phase completes at motion end; target/path steering
                // belongs to the family unit and is not fabricated here.
                self.done = self.motion_ended;
            }
            ActionPhase::Dash => {
                // Translated from 0x008fc960 (DDDA.exe 2017-12-13 Steam).
                // Permit the next think immediately, while root travel keeps
                // running. Think-node +0x124 bookkeeping is not connected.
                self.done = true;
            }
            ActionPhase::Takeoff(takeoff) => {
                let result = takeoff.step(self.frame, self.motion_ended);
                self.movement_flags = result.movement_flags;
                self.done = result.done;
                next = result.motion;
            }
            ActionPhase::GroundDeath => {
                // 0x009086f0 / 0x009472c0: animation end hands off to
                // corpse cleanup. That cleanup remains host work.
                self.done = self.motion_ended;
            }
            ActionPhase::CommonDeath => {
                // Translated from 0x008a6930 (DDDA.exe 2017-12-13 Steam):
                // a valid owner never sets the action's done byte. Corpse
                // flags/timers and forced-action selection remain host work.
                self.done = false;
            }
            ActionPhase::None => return,
        }
        if let Some((motion, frame)) = next {
            self.motion = motion;
            self.frame = frame;
            self.motion_ended = false;
        }
        self.advance(data, dt);
        if self.movement_flags & 1 == 0 {
            if let Some(y) = ground.height(self.pos[0], self.pos[2], self.pos[1]) {
                self.pos[1] = y;
            }
        }
        self.volumes(data);
    }

    /// Common uEnemy::vf90 0x00aa96e0 rank rule applied to the action
    /// bits written by8fe390 (bite) and8fe160 (attack run).
    pub fn push_rank(&self, base: u32) -> u32 {
        if base == 0 { return 0; }
        match self.phase {
            ActionPhase::Bite(WolfBite::Strike) => base + 1,
            ActionPhase::AttackRun => base + 2,
            _ => base,
        }
    }

    pub fn dead(&self) -> bool { self.fighter.victim.dead() }

    /// Ordinary em0200 death selection, served before thinking on the next
    /// enemy update (0x008f6230 -> pending +0x5b58 -> 0x008f3830).
    /// Only the ordinary path is modeled: knockdown, grab, subtype 0x91 and
    /// status-specific death overrides remain unported.
    pub fn begin_standard_wolf_death(&mut self, data: &BeastData) -> Result<(), String> {
        if self.family != Family::Wolf || !self.dead() {
            return Err("standard wolf death requires a dead wolf".into());
        }
        if !matches!(self.phase, ActionPhase::CommonDeath) {
            self.start_action(data, 0x0300_0042)?;
            self.target = None;
            self.attack_ready = false;
        }
        Ok(())
    }

    pub fn volumes(&mut self, data: &BeastData) {
        let pose = data.body.pose(self.motion, self.frame, self.pos, self.yaw);
        let active = data.body.activations(self.motion, self.frame);
        (self.fighter.attacks, self.fighter.hurts) = data.body.volumes(&active, &pose, self.fighter.owner);
        // Temporary integration guard until forced family death actions are
        // wired: a dead actor cannot keep dealing its last attack's damage.
        if self.dead() { self.fighter.attacks.clear(); }
    }

    // Shared animation phase92: 0x010629f0 / root travel0x0105b740,
    // DDDA.exe 2017-12-13 Steam. Same loop convention as goblin::advance.
    fn advance(&mut self, data: &BeastData, dt: f32) {
        let Some(motion) = data.body.motion(self.motion) else {
            return;
        };
        let root = ddsim::RootMotion::from_motion(motion);
        let looping = motion.loop_frame >= 0;
        let delta = if looping {
            root.delta(self.frame, dt)
        } else {
            root.delta_once(self.frame, dt)
        };
        let (sin, cos) = self.yaw.sin_cos();
        self.pos[0] += delta[0] * cos + delta[2] * sin;
        self.pos[1] += delta[1];
        self.pos[2] += -delta[0] * sin + delta[2] * cos;
        let last = motion.frame_count.saturating_sub(1) as f32;
        let frame = self.frame + dt;
        if looping {
            let begin = motion.loop_frame as f32;
            self.frame = if frame > last {
                begin + (frame - last).rem_euclid((last - begin).max(1.0))
            } else {
                frame
            };
            self.motion_ended = frame > last;
        } else {
            self.frame = frame.min(last);
            self.motion_ended = frame >= last;
        }
    }
}

/// The three sequential bite motions; transitions occur on motion end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WolfBite {
    #[default]
    Windup,
    Strike,
    Recovery,
    Finished,
}

impl WolfBite {
    // Translated from cEm0200Bite::vf05 0x008fe340 / vf06 0x008fe390
    // (DDDA.exe 2017-12-13 Steam).
    pub fn motion(self) -> MotionId {
        MotionId::new(
            1,
            match self {
                Self::Windup => 0,
                Self::Strike => 1,
                _ => 2,
            },
        )
    }

    pub fn motion_end(&mut self) {
        *self = match self {
            Self::Windup => Self::Strike,
            Self::Strike => Self::Recovery,
            Self::Recovery | Self::Finished => Self::Finished,
        };
    }

    /// During strike frames below 7 the unit enables its attack tracking
    /// and writes 0.1 to steering coefficient +0x6074.
    pub fn tracks_target(self, frame: f32) -> bool {
        self == Self::Strike && frame < 7.0
    }
}

/// Vertical target tracking of airborne actions, after both ground-ray
/// distances have been supplied by the world. State 1 uses a different
/// helper (0x00937f80) and is deliberately not handled here.
// Translated from 0x00937e10 (DDDA.exe 2017-12-13 Steam).
pub fn flight_height_step(current_y: f32, goal_y: f32, speed: f32, frames: f32) -> f32 {
    let delta = goal_y - current_y;
    let distance = delta.abs();
    if distance < f32::EPSILON {
        return current_y;
    }
    let step = (speed * speed * (-1.0 / distance) * frames + speed).max(0.0) * frames;
    current_y + if delta >= 0.0 { step } else { -step }
}

/// Harpy target-angle interpolation. The cap is per call, whereas the
/// proportional term scales with the simulation's 30-Hz frame step.
// Translated from 0x0093ca10 (DDDA.exe 2017-12-13 Steam).
pub fn harpy_turn(yaw: f32, goal: f32, cap: f32, divisor: f32, frames: f32) -> f32 {
    let delta = goal - yaw;
    let mut wrapped = delta - (delta * 0.159_154_94) as i32 as f32 * 6.283_185_5;
    if delta < 0.0 {
        wrapped += 6.283_185_5;
    }
    if wrapped >= 3.141_592_7 {
        wrapped -= 6.283_185_5;
    }
    let step = (wrapped / divisor * frames).clamp(-cap, cap);
    if wrapped.abs() - step.abs() <= 0.0 {
        goal
    } else {
        yaw + step
    }
}

/// The shared angle controller used by the wolf's phase91 steering.
/// The coefficient is radians per 30-Hz unit frame, not interpolation gain.
/// The caller supplies status-adjusted unit time (0x0084cc00).
// Translated from 0x007975d0 and 0x00797470 (DDDA.exe 2017-12-13 Steam).
pub fn wolf_turn(yaw: f32, goal: f32, coefficient: f32, frames: f32) -> f32 {
    fn wrap(angle: f32) -> f32 {
        let mut a = angle - (angle * 0.159_154_94) as i32 as f32 * 6.283_185_5;
        if angle < 0.0 {
            a += 6.283_185_5;
        }
        if a >= 3.141_592_7 {
            a -= 6.283_185_5;
        }
        a
    }
    let goal = wrap(goal);
    let (mut a, mut b) = (goal, yaw);
    if yaw * goal < 0.0 {
        if goal < 0.0 {
            a += 6.283_185_5;
        }
        if yaw < 0.0 {
            b += 6.283_185_5;
        }
        let delta = a - b;
        if delta.abs() > 3.141_592_7 {
            if b <= a {
                a = delta;
                b = 6.283_185_5;
            } else {
                b -= a;
                a = 6.283_185_5;
            }
        }
    }
    let delta = a - b;
    if delta.abs() < f32::EPSILON {
        return goal;
    }
    let step = coefficient.max(f32::EPSILON) * frames;
    if delta.abs() < step {
        goal
    } else {
        wrap(yaw + if delta < 0.0 { -step } else { step })
    }
}

/// AirAttack's approach state. The caller supplies the unit's target and
/// ground-ray result, and advances the selected motion separately.
#[derive(Debug, Clone, Copy)]
pub struct HarpyAirAttack {
    pub elapsed: f32,
    pub initial_distance: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AirAttackStep {
    pub yaw: f32,
    pub position: [f32; 3],
    /// New motion and starting frame; absent means keep the current one.
    pub motion: Option<(MotionId, f32)>,
    pub done: bool,
}

/// The grounded takeoff action, before the flight think takes over.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HarpyTakeoff {
    #[default]
    TakingOff,
    Hover,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TakeoffStep {
    pub movement_flags: u32,
    pub motion: Option<(MotionId, f32)>,
    pub done: bool,
}

impl HarpyTakeoff {
    // Translated from cEm0600ActGroundToHover::vf05 0x0093f580
    // (DDDA.exe 2017-12-13 Steam).
    pub fn initial_motion() -> MotionId {
        MotionId::new(0, 0x29)
    }

    // Translated from cEm0600ActGroundToHover::vf06 0x0093f5e0
    // (DDDA.exe 2017-12-13 Steam). Movement bit0 also disables the
    // actor's grounded/gravity flags +0x1b6d/+0x1e80.
    pub fn step(&mut self, frame: f32, motion_ended: bool) -> TakeoffStep {
        match *self {
            Self::TakingOff => {
                let movement_flags = if frame <= 18.0 { 4 } else { 1 };
                let motion = if motion_ended {
                    *self = Self::Hover;
                    Some((MotionId::new(0, 0x1f), 57.0))
                } else {
                    None
                };
                TakeoffStep {
                    movement_flags,
                    motion,
                    done: false,
                }
            }
            Self::Hover | Self::Finished => {
                *self = Self::Finished;
                TakeoffStep {
                    movement_flags: 1,
                    motion: None,
                    done: true,
                }
            }
        }
    }
}

impl HarpyAirAttack {
    // Translated from cEm0600ActAirAttack::vf05 0x009453c0
    // (DDDA.exe 2017-12-13 Steam).
    pub fn new(position: [f32; 3], target: [f32; 3]) -> Self {
        Self {
            elapsed: 0.0,
            initial_distance: distance(position, target),
        }
    }

    pub fn initial_motion() -> MotionId {
        MotionId::new(1, 1)
    }

    // Translated from cEm0600ActAirAttack::vf06 0x00945490, phase byte
    // +0x70 == 0 (DDDA.exe 2017-12-13 Steam). Other phase values perform
    // no approach update; the owning action dispatcher guards this call.
    pub fn approach(
        &mut self,
        position: [f32; 3],
        yaw: f32,
        target: [f32; 3],
        height_goal: f32,
        motion_ended: bool,
        grounded: bool,
        frames: f32,
    ) -> AirAttackStep {
        let range = distance(position, target);
        self.elapsed += frames;
        let mut heading = yaw;
        if self.elapsed < 30.0 && range > 400.0 {
            heading = harpy_turn(
                yaw,
                (target[0] - position[0]).atan2(target[2] - position[2]),
                std::f32::consts::FRAC_PI_4,
                4.0,
                frames,
            );
        }
        // The original constructs a point 300 cm ahead, then applies
        // speed-30 distance damping toward that point.
        let (sin, cos) = heading.sin_cos();
        let dx = (position[0] + sin * 300.0) - position[0];
        let dz = (position[2] + cos * 300.0) - position[2];
        let ahead = (dx * dx + dz * dz).sqrt();
        let rate = if ahead >= f32::EPSILON {
            (1.0 / ahead) * ((-1.0 / ahead) * 900.0 * frames + 30.0).max(0.0)
        } else {
            0.0
        };
        AirAttackStep {
            yaw: heading,
            position: [
                position[0] + rate * dx * frames,
                flight_height_step(position[1], height_goal, 8.0, frames),
                position[2] + dz * rate * frames,
            ],
            motion: if range < 400.0 {
                Some((MotionId::new(1, 2), 8.0))
            } else if motion_ended {
                Some((MotionId::new(0, 0x2c), 0.0))
            } else {
                None
            },
            done: (self.elapsed > 60.0 && range > self.initial_distance) || self.elapsed > 120.0 || grounded,
        }
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wolf_steering_uses_rate_times_unit_time_and_short_wrapped_route() {
        assert!((wolf_turn(0.0, 1.0, 0.1, 0.5) - 0.05).abs() < 1e-6);
        assert!((wolf_turn(0.0, -1.0, 0.1, 0.5) + 0.05).abs() < 1e-6);
        assert!((wolf_turn(3.1, -3.1, 0.1, 0.5) + 3.1331854).abs() < 1e-6);
        assert!((wolf_turn(-3.1, 3.1, 0.1, 0.5) - 3.1331854).abs() < 1e-6);
        assert!((wolf_turn(0.0, 0.02, 0.1, 1.0) - 0.02).abs() < 1e-6);
        assert_eq!(wolf_turn(0.0, 1.0, 0.0, 1.0), f32::EPSILON);
    }

    #[test]
    fn harpy_takeoff_releases_ground_after18_and_finishes_after_hover_entry() {
        let mut takeoff = HarpyTakeoff::default();
        assert_eq!(HarpyTakeoff::initial_motion(), MotionId::new(0, 0x29));
        assert_eq!(takeoff.step(18.0, false).movement_flags, 4);
        assert_eq!(takeoff.step(18.01, false).movement_flags, 1);
        let transition = takeoff.step(40.0, true);
        assert_eq!(transition.motion, Some((MotionId::new(0, 0x1f), 57.0)));
        assert!(!transition.done);
        assert_eq!(takeoff, HarpyTakeoff::Hover);
        assert!(takeoff.step(57.0, false).done);
        assert_eq!(takeoff, HarpyTakeoff::Finished);
    }

    #[test]
    fn harpy_dive_boundaries_and_turn_window() {
        let target = [0.0, 0.0, 400.0];
        let mut dive = HarpyAirAttack::new([0.0; 3], target);
        let out = dive.approach([0.0; 3], 0.0, target, 64.0, false, false, 1.0);
        for (actual, expected) in out.position.into_iter().zip([0.0, 7.0, 27.0]) {
            assert!((actual - expected).abs() < 0.00001);
        }
        assert_eq!(out.motion, None, "400 cm is outside the strike threshold");
        let out = dive.approach([0.0, 0.0, 0.01], 0.0, target, 0.0, false, false, 1.0);
        assert_eq!(out.motion, Some((MotionId::new(1, 2), 8.0)));
        dive.elapsed = 59.0;
        assert!(
            !dive
                .approach([0.0; 3], 0.0, [0.0, 0.0, 401.0], 0.0, false, false, 1.0)
                .done
        );
        assert!(
            dive.approach([0.0; 3], 0.0, [0.0, 0.0, 401.0], 0.0, false, false, 1.0)
                .done
        );
        dive.elapsed = 29.0;
        assert_eq!(
            dive.approach([0.0; 3], 0.0, [500.0, 0.0, 0.0], 0.0, false, false, 1.0)
                .yaw,
            0.0
        );
        assert!(dive.approach([0.0; 3], 0.0, target, 0.0, false, true, 1.0).done);
    }

    #[test]
    fn harpy_turn_wraps_and_caps_without_overshoot() {
        let pi = std::f32::consts::PI;
        let yaw = harpy_turn(pi - 0.1, -pi + 0.1, 1.0, 4.0, 1.0);
        assert!((yaw - (pi - 0.05)).abs() < 0.00001);
        assert_eq!(harpy_turn(0.0, 2.0, 0.1, 1.0, 0.5), 0.1);
        assert_eq!(harpy_turn(0.0, 0.01, 1.0, 1.0, 2.0), 0.01);
    }

    #[test]
    fn harpy_death_preserves_freeze_lifecycle_and_petrify_precedence() {
        assert_eq!(HarpyDeath::select(0, 0, 1.0).action(), 3);
        assert_eq!(HarpyDeath::select(4, 0, 1.0).action(), 3);
        assert_eq!(HarpyDeath::select(1, 0, 1.0).action(), 4);
        for state in 1..=5 {
            assert_eq!(HarpyDeath::select(1, state, 1.0).action(), 0x0300_005f);
            assert_eq!(HarpyDeath::select(1, state, 0.0).action(), 0x0300_0060);
        }
        assert_eq!(HarpyDeath::select(0, 0, -0.01), HarpyDeath::PetrifiedBreak);
        assert_eq!(HarpyDeath::select(0, 0, f32::NAN), HarpyDeath::Ground);
        assert_eq!(HarpyDeath::select(0, 1, f32::NAN), HarpyDeath::FrozenBreak);
    }

    #[test]
    fn bite_waits_for_each_motion_end_and_limits_tracking() {
        let mut bite = WolfBite::default();
        assert_eq!(bite.motion(), MotionId::new(1, 0));
        assert!(!bite.tracks_target(0.0));
        bite.motion_end();
        assert_eq!(bite.motion(), MotionId::new(1, 1));
        assert!(bite.tracks_target(6.99));
        assert!(!bite.tracks_target(7.0));
        bite.motion_end();
        assert_eq!(bite.motion(), MotionId::new(1, 2));
        bite.motion_end();
        assert_eq!(bite, WolfBite::Finished);
        bite.motion_end();
        assert_eq!(bite, WolfBite::Finished);
    }

    #[test]
    fn flight_height_uses_distance_damping_in_both_directions() {
        assert_eq!(flight_height_step(0.0, 64.0, 8.0, 1.0), 7.0);
        assert_eq!(flight_height_step(64.0, 0.0, 8.0, 1.0), 57.0);
        assert_eq!(flight_height_step(0.0, 4.0, 8.0, 1.0), 0.0);
        assert_eq!(flight_height_step(32.0, 32.0, 8.0, 1.0), 32.0);
        assert_eq!(flight_height_step(0.0, 64.0, 8.0, 0.5), 3.75);
    }
}

//! Per-player equipped skill actions. Values and motion lists are supplied
//! by the host from the installation. No rendering or OS input lives here.
use crate::{attack, Ground, PlayerId, RootMotion, Sim, State, Weapon};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// Broad Cut and Tusk Toss: vf06 B3E290 / B3E9A0.
    Sword { motion: u16 },
    /// Blink Strike, B3DBD0/B3DE40: loop the authored dash until its
    /// actual travelled distance or native contact ends it, then strike.
    Blink {
        dash: u16,
        end: u16,
        loop_frame: f32,
        maximum_distance: f32,
        turn_rate: f32,
    },
    /// Base Shield Summons: provoke at entry, then the real shield motion.
    /// Radius and amount are loaded from m_pl000_Shield (B38240/B38490).
    ShieldProvoke { motion: u16, radius: f32, amount: u32 },
    /// Scarlet Kisses base-level phases (0x00B23F10). Parameters are read
    /// from m_pl000_Dagger, rather than copied from that resource.
    DaggerSeries {
        intro: u16,
        strikes: [u16; 3],
        end: u16,
        rate: f32,
        maximum_cycles: u32,
    },
    /// Ground cast, base level, no manual targeting/chant sharing yet.
    Spell {
        start: u16,
        chant_loop: Option<u16>,
        release: u16,
        end: Option<u16>,
        chant_frames: f32,
    },
}

#[derive(Debug, Clone)]
pub struct Definition {
    pub weapon: Weapon,
    pub kind: Kind,
    /// Positive cost after the caller resolves augments and weapon level.
    pub cost: f32,
    pub motions: HashMap<u16, RootMotion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Intro,
    Strike,
    Chant,
    Release,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Active {
    pub id: u32,
    pub phase: Phase,
    pub motion: u16,
    pub elapsed: f32,
    pub previous_cost_word: u32,
    pub paid: bool,
    /// cPlActWpnDaggerCstmHyakuretsu +0x74/+0x78/+0x7C.
    pub cycles: u32,
    pub presses: u32,
    pub variant: usize,
    pub distance: f32,
    pub turn_ready: bool,
}

/// Native contact input at uPlayer +3CB0/+3CB4. The accepted-hit callback
/// supplies these bits; an obstructed root delta is not a substitute.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Contact {
    pub valid: bool,
    pub flags: u32,
}

impl Contact {
    // Translated from B3DE40/B1F7E0, Steam 2017-12-13.
    pub fn ends_blink(self) -> bool {
        self.valid && (self.flags & 0x80 != 0 || self.flags & 0x18 == 0x18)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Skills {
    /// Six active slots of this character, not a global player.
    pub equipped: [Option<u32>; 6],
    pub request: Option<usize>,
    pub active: Option<Active>,
    /// Spell releases, drained by the host after player movement.
    pub released: Vec<u32>,
    /// Native entry effects (B38240), distinct from spell release events.
    pub entered: Vec<u32>,
    pub contact: Contact,
}

/// Advance an authored loop without inventing a duration or discarding its
/// nonzero loop frame. Motion-list end/loop semantics: E9D330/1060730.
pub(super) fn loop_delta(root: &RootMotion, from: f32, dt: f32, loop_frame: f32) -> ([f32; 3], f32) {
    let last = root.period();
    let to = from + dt;
    if to <= last {
        return (root.delta_once(from, dt), to);
    }
    let period = last - loop_frame;
    let remainder = (to - last).rem_euclid(period);
    let cycles = ((to - last) / period).floor();
    let a = root.at(from);
    let b = root.at(last);
    let start = root.at(loop_frame);
    let end = root.at(loop_frame + remainder);
    (
        std::array::from_fn(|i| b[i] - a[i] + cycles * (b[i] - start[i]) + end[i] - start[i]),
        loop_frame + remainder,
    )
}

impl Sim {
    /// Queue one equipped-slot press. Unsupported or empty slots do nothing.
    pub fn request_skill(&mut self, seat: PlayerId, slot: usize) {
        if let Some(p) = self.player_mut(seat) {
            p.skills.request = Some(slot);
        }
    }

    pub(super) fn skill_frame(&mut self, i: usize, ground: &dyn Ground, dt: f32) -> bool {
        let p = &mut self.players[i];
        // Integration guard: a host-forced action replaces the skill. Full
        // original cancel/interrupt slot arbitration is not implemented yet.
        if p.skills.active.is_some() && p.state != State::Wait {
            p.skills.active = None;
        }
        if let Some(slot) = p.skills.request.take() {
            let id = p.skills.equipped.get(slot).copied().flatten();
            // 0x00B242CE/AC6320: only presses of this action's mapped
            // equipped input during its strike phase count toward repeats.
            // Intro presses are not carried into the first strike.
            if let Some(active) = p.skills.active.as_mut() {
                if id == Some(active.id)
                    && active.phase == Phase::Strike
                    && self
                        .skill_definitions
                        .get(&active.id)
                        .is_some_and(|d| matches!(d.kind, Kind::DaggerSeries { .. }))
                {
                    active.presses = active.presses.saturating_add(1);
                }
            }
            // Until cancel slots are traced, do not let a key press bypass
            // an existing basic attack or staff-shot action.
            let available = p.state.on_ground_for_attacks()
                && !matches!(p.state, State::Attack { .. } | State::Shot { .. });
            if p.skills.active.is_none() && available && p.stamina > 0.0 {
                if let Some((id, d)) = id.and_then(|id| self.skill_definitions.get(&id).map(|d| (id, d))) {
                    let secondary =
                        !matches!(d.kind, Kind::ShieldProvoke { .. }) || p.secondary_weapon == Weapon::Shield;
                    if d.weapon == p.weapon && secondary {
                        let (phase, motion) = match d.kind {
                            Kind::Sword { motion } => (Phase::Strike, motion),
                            Kind::Blink { dash, .. } => (Phase::Strike, dash),
                            Kind::ShieldProvoke { motion, .. } => (Phase::Strike, motion),
                            Kind::DaggerSeries { intro, .. } => (Phase::Intro, intro),
                            Kind::Spell { start, .. } => (Phase::Chant, start),
                        };
                        p.skills.active = Some(Active {
                            id,
                            phase,
                            motion,
                            elapsed: 0.,
                            previous_cost_word: 0,
                            paid: false,
                            cycles: 0,
                            presses: 0,
                            variant: 0,
                            distance: 0.0,
                            turn_ready: false,
                        });
                        p.state = State::Wait;
                        p.frame = 0.;
                        p.speed = 0.;
                        p.velocity = [0.; 3];
                        p.request = None;
                        if matches!(d.kind, Kind::Blink { .. }) {
                            p.turn_to = p.attack_heading;
                        } else if let Some(yaw) = p.attack_heading {
                            p.yaw = yaw;
                        }
                        p.attacks_started += 1;
                        if matches!(d.kind, Kind::ShieldProvoke { .. }) {
                            p.skills.entered.push(id);
                        }
                        return true;
                    }
                }
            }
        }
        let Some(mut active) = self.players[i].skills.active else {
            return false;
        };
        let Some(def) = self.skill_definitions.get(&active.id) else {
            self.players[i].skills.active = None;
            return false;
        };
        let kind = def.kind;
        let cost = def.cost;
        if let Kind::Blink {
            end,
            maximum_distance,
            turn_rate,
            ..
        } = kind
        {
            let p = &mut self.players[i];
            if active.phase == Phase::Strike {
                // Translated from B3DE40: the distance/contact check precedes
                // the next motion advance. Stop the rotator when ending.
                if active.distance >= maximum_distance || p.skills.contact.ends_blink() {
                    active.phase = Phase::End;
                    active.motion = end;
                    active.distance = active.distance.min(maximum_distance);
                    p.frame = 0.0;
                    p.turn_to = None;
                } else {
                    // B3DBD0 begins at common adjust AE; B3DE40 switches
                    // to sword adjust0 after the initial turn completes.
                    let rate = if active.turn_ready {
                        turn_rate
                    } else {
                        self.attack_param.turn_deg.to_radians()
                    };
                    let heading = if active.turn_ready {
                        p.attack_heading
                    } else {
                        p.turn_to
                    };
                    if let Some(heading) = heading {
                        let delta = (heading - p.yaw + std::f32::consts::PI)
                            .rem_euclid(std::f32::consts::TAU)
                            - std::f32::consts::PI;
                        let step = rate * dt;
                        // 797470 uses a strict step comparison. Equality
                        // reaches the heading but clears the flag next tick.
                        if delta.abs() < step {
                            p.yaw = heading;
                            p.turn_to = None;
                        } else {
                            p.yaw += step.copysign(delta);
                        }
                    }
                    if p.turn_to.is_none() {
                        active.turn_ready = true;
                    }
                }
                p.skills.active = Some(active);
            }
        }
        let p = &self.players[i];
        let Some(motion) = def.motions.get(&active.motion) else {
            self.players[i].skills.active = None;
            return false;
        };
        let from = p.frame;
        let previous_position = p.position;
        let rate = match (kind, active.phase) {
            (Kind::DaggerSeries { rate, .. }, Phase::Intro | Phase::Strike) => rate,
            _ => 1.0,
        };
        let to = from + dt * rate;
        let word = motion.event_bits(0, to as i32, to as i32);
        let cost_word = motion.event_bits(1, to as i32, to as i32);
        let done = word & attack::ACTION_END_BIT != 0 || to >= motion.period();
        let loop_terminal = to >= motion.period();
        // Reuse the collision-aware ground movement. It selects this
        // player's skill motion through `playing`, not another seat's bank.
        self.ground_move_clock(i, State::Wait, ground, dt, dt * rate);
        let p = &mut self.players[i];
        if p.state != State::Wait {
            p.skills.active = None;
            return true;
        }
        // Wait normally loops; skill phases use their own monotonically
        // increasing frame, with explicit transitions below.
        let blink_dash = matches!(kind, Kind::Blink { .. }) && active.phase == Phase::Strike;
        if blink_dash {
            active.distance += (0..3)
                .map(|i| (p.position[i] - previous_position[i]).powi(2))
                .sum::<f32>()
                .sqrt();
        } else {
            p.frame = to;
        }
        active.elapsed += dt;
        let mut next = None;
        let mut finish = false;
        if matches!(kind, Kind::DaggerSeries { .. }) {
            // B60F10/B60F70 charge the actual motion's group-1 bit-30
            // falling edge. New motions reset that event history.
            if !active.paid
                && (cost_word ^ active.previous_cost_word) & active.previous_cost_word & (1 << 30) != 0
            {
                Self::spend(p, cost);
                active.paid = true;
            }
            active.previous_cost_word = cost_word;
        }
        match (kind, active.phase) {
            // Translated from 0x00B23F10 (DDDA.exe 2017-12-13 Steam):
            // phases 1/2/3, +74 completed cycles, +78 pressed edges and
            // +7C variant. Native target-turn helper B24330 is not yet
            // translated; this slice keeps the host-provided start facing.
            (Kind::DaggerSeries { strikes, .. }, Phase::Intro) if done => {
                next = Some((Phase::Strike, strikes[0]));
                active.presses = 0;
            }
            (
                Kind::DaggerSeries {
                    strikes,
                    end,
                    maximum_cycles,
                    ..
                },
                Phase::Strike,
            ) => {
                // B240E2 uses 10570F0 (the actual loop terminal), rather
                // than the action-end sequence counter used by intro/end.
                // The endpoint comes from this installed LMT motion.
                if loop_terminal {
                    active.cycles += 1;
                    if active.cycles < maximum_cycles && active.presses >= 2 {
                        active.variant = (active.variant + 1) % strikes.len();
                        next = Some((Phase::Strike, strikes[active.variant]));
                    } else {
                        next = Some((Phase::End, end));
                    }
                    active.presses = 0;
                }
            }
            (Kind::Sword { .. } | Kind::ShieldProvoke { .. } | Kind::Blink { .. }, Phase::Strike) => {
                // Translated from B60F10/B60F70: group 1 bit 30 FALLS.
                if !active.paid
                    && (cost_word ^ active.previous_cost_word) & active.previous_cost_word & (1 << 30) != 0
                {
                    Self::spend(p, cost);
                    active.paid = true;
                }
                active.previous_cost_word = cost_word;
                finish = done && !blink_dash;
            }
            (
                Kind::Spell {
                    chant_loop,
                    release,
                    chant_frames,
                    ..
                },
                Phase::Chant,
            ) => {
                // B456C0: count unit time until the data's chantFrame;
                // B45DD0/B47890: release, then fire on bit31/30 or end.
                if active.elapsed >= chant_frames {
                    next = Some((Phase::Release, release));
                } else if done {
                    if let Some(m) = chant_loop {
                        next = Some((Phase::Chant, m));
                    }
                }
            }
            (Kind::Spell { end, .. }, Phase::Release) if done || word & attack::CHAIN_CLOSED_BIT != 0 => {
                p.skills.released.push(active.id);
                Self::spend(p, cost);
                active.paid = true;
                if let Some(m) = end {
                    next = Some((Phase::End, m));
                } else {
                    finish = true;
                }
            }
            (_, Phase::End) => finish = done,
            _ => {}
        }
        if let Some((phase, motion)) = next {
            if matches!(kind, Kind::DaggerSeries { .. }) {
                active.previous_cost_word = 0;
                active.paid = false;
            }
            active.phase = phase;
            active.motion = motion;
            p.frame = 0.;
        }
        if finish {
            p.skills.active = None;
            p.frame = 0.;
        } else {
            p.skills.active = Some(active);
        }
        true
    }
}

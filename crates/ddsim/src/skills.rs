//! Per-player equipped skill actions. Values and motion lists are supplied
//! by the host from the installation. No rendering or OS input lives here.
use crate::{attack, Ground, PlayerId, RootMotion, Sim, State, Weapon};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// Broad Cut and Tusk Toss: vf06 B3E290 / B3E9A0.
    Sword { motion: u16 },
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
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Skills {
    /// Six active slots of this character, not a global player.
    pub equipped: [Option<u32>; 6],
    pub request: Option<usize>,
    pub active: Option<Active>,
    /// Spell releases, drained by the host after player movement.
    pub released: Vec<u32>,
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
            // Until cancel slots are traced, do not let a key press bypass
            // an existing basic attack or staff-shot action.
            let available = p.state.on_ground_for_attacks()
                && !matches!(p.state, State::Attack { .. } | State::Shot { .. });
            if p.skills.active.is_none() && available && p.stamina > 0.0 {
                if let Some((id, d)) = id.and_then(|id| self.skill_definitions.get(&id).map(|d| (id, d))) {
                    if d.weapon == p.weapon {
                        let (phase, motion) = match d.kind {
                            Kind::Sword { motion } => (Phase::Strike, motion),
                            Kind::Spell { start, .. } => (Phase::Chant, start),
                        };
                        p.skills.active = Some(Active {
                            id,
                            phase,
                            motion,
                            elapsed: 0.,
                            previous_cost_word: 0,
                            paid: false,
                        });
                        p.state = State::Wait;
                        p.frame = 0.;
                        p.speed = 0.;
                        p.velocity = [0.; 3];
                        p.request = None;
                        if let Some(yaw) = p.attack_heading {
                            p.yaw = yaw;
                        }
                        p.attacks_started += 1;
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
        let p = &self.players[i];
        let Some(motion) = def.motions.get(&active.motion) else {
            self.players[i].skills.active = None;
            return false;
        };
        let from = p.frame;
        let to = from + dt;
        let word = motion.event_bits(0, to as i32, to as i32);
        let cost_word = motion.event_bits(1, to as i32, to as i32);
        let done = word & attack::ACTION_END_BIT != 0 || to >= motion.period();
        // Reuse the collision-aware ground movement. It selects this
        // player's skill motion through `playing`, not another seat's bank.
        self.ground_move(i, State::Wait, ground, dt);
        let p = &mut self.players[i];
        if p.state != State::Wait {
            p.skills.active = None;
            return true;
        }
        // Wait normally loops; skill phases use their own monotonically
        // increasing frame, with explicit transitions below.
        p.frame = to;
        active.elapsed += dt;
        let mut next = None;
        let mut finish = false;
        match (kind, active.phase) {
            (Kind::Sword { .. }, Phase::Strike) => {
                // Translated from B60F10/B60F70: group 1 bit 30 FALLS.
                if !active.paid
                    && (cost_word ^ active.previous_cost_word) & active.previous_cost_word & (1 << 30) != 0
                {
                    Self::spend(p, cost);
                    active.paid = true;
                }
                active.previous_cost_word = cost_word;
                finish = done;
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

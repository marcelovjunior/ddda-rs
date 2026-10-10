//! Pawns in the fight: a pawn is a character the simulation moves like a
//! player (the game runs `uCmc` through the player pipeline, `uCmc::vf200`
//! 0x00B537D0), driven by the virtual pad of its AI (`ddpawnai::pawn`).
//!
//! The host asks [`Fight::pawn_inputs`] for the pads before it steps the
//! simulation and passes them with the players' inputs; the rest of the
//! frame is the same as for an Arisen (hits, attacks, push-out, volumes).
//!
//! *Placeholders*: the pawn fights with the level-1 Fighter kit (its
//! values, body and weapon); the battle flag is "a live goblin within
//! [`BATTLE_RANGE`] of the pawn or its Arisen"; enemies' radius is 0.

use crate::fight::{Fight, Kit};
use ddpawnai::pawn::{Enemy, Pawn, PawnData, Senses};
use ddsim::{Input, State};
use std::collections::HashMap;

/// *Placeholder* for the battle flag sGameSys +0xBE2CC (its writer is not
/// traced): a live enemy this close (cm, horizontal) to the pawn or its
/// Arisen.
pub const BATTLE_RANGE: f32 = 3000.0;

/// The sword's weapon type (uCmc +0x3554 high byte, 1 or 2 -> goal 52).
pub const SWORD_TYPE: u8 = 1;

/// One pawn of the fight.
#[derive(Debug, Clone)]
pub struct PawnSeat {
    /// The seat of the Arisen it serves.
    pub owner: u8,
    pub brain: Pawn,
}

/// The pawn data from the install: `AI\PrioThink\cmc` and
/// `AIPlActParamSword`.
pub fn load(vfs: &ddvfs::Vfs) -> Result<PawnData, String> {
    let prio = vfs
        .read(r"AI\PrioThink\cmc", mtarc::type_hash("rAIPriorityThink"))
        .map_err(|e| format!("AI\\PrioThink\\cmc: {e}"))?;
    let sword = vfs
        .read(
            ddpawnai::plact::SWORD_BANK,
            mtarc::type_hash("rAIPlayerActionParameter"),
        )
        .map_err(|e| format!("{}: {e}", ddpawnai::plact::SWORD_BANK))?;
    Ok(PawnData {
        prio: ddpawnai::prio::PriorityThink::parse(&prio)?,
        sword: ddpawnai::plact::parse(&sword)?,
    })
}

/// The attack cancel slot's state of a player's playing motion, as ddsim's
/// attack input reads it (event group 0 at the current frame; a motion
/// without events executes).
fn attack_slot(sim: &ddsim::Sim, p: &ddsim::Player) -> u8 {
    let f = p.frame as i32;
    match sim.motion_for(p.weapon, p.state.motion_id()) {
        Some(m) if !m.events.is_empty() => {
            ddsim::attack::slot_state(m.event_bits(0, f, f), ddsim::attack::ATTACK_SLOT)
        }
        _ => 2,
    }
}

fn dist_xz(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl Fight {
    /// Adds a pawn at `seat` serving seat `owner`'s Arisen: a sim player
    /// with the Fighter kit, and its AI with nine inclination values.
    /// `seed` seeds its random picks.
    #[allow(clippy::too_many_arguments)]
    pub fn add_pawn(
        &mut self,
        sim: &mut ddsim::Sim,
        seat: u8,
        owner: u8,
        pos: [f32; 3],
        yaw: f32,
        inclinations: [f32; 9],
        seed: u32,
    ) {
        self.add_pawn_as(sim, seat, owner, pos, yaw, inclinations, seed, Kit::Fighter);
    }

    /// [`Fight::add_pawn`] with a vocation kit. *Assumed*: the brain keeps
    /// the sword weapon type for every kit (the pawn AI's per-weapon tables
    /// are only wired for the sword).
    #[allow(clippy::too_many_arguments)]
    pub fn add_pawn_as(
        &mut self,
        sim: &mut ddsim::Sim,
        seat: u8,
        owner: u8,
        pos: [f32; 3],
        yaw: f32,
        inclinations: [f32; 9],
        seed: u32,
        kit: Kit,
    ) {
        sim.add_player(seat, pos, yaw);
        self.add_arisen_as(seat, kit);
        self.kit(kit).equip(sim, seat);
        let brain = Pawn::new(&self.data.pawn, inclinations, SWORD_TYPE, seed);
        self.pawns.insert(seat, PawnSeat { owner, brain });
    }

    /// The pawns' pads for this frame, as simulation inputs (`dt`: frames).
    /// Call before stepping the simulation; merge with the players' inputs.
    pub fn pawn_inputs(&mut self, sim: &ddsim::Sim, dt: f32) -> HashMap<u8, Input> {
        let enemies: Vec<Enemy> = self
            .goblins
            .iter()
            .map(|g| Enemy {
                id: g.fighter.owner,
                pos: g.pos,
                alive: !g.dead(),
                radius: 0.0,
            })
            .collect();
        let data = self.data.clone();
        let mut out = HashMap::new();
        for (&seat, p) in &mut self.pawns {
            let Some(me) = sim.player(seat) else { continue };
            if self.arisen.get(&seat).is_some_and(|a| a.fighter.victim.dead()) {
                continue;
            }
            let owner = sim.player(p.owner).map(|o| o.position);
            let battle = enemies.iter().any(|e| {
                e.alive
                    && (dist_xz(e.pos, me.position) <= BATTLE_RANGE
                        || owner.is_some_and(|o| dist_xz(e.pos, o) <= BATTLE_RANGE))
            });
            let action = match me.state {
                State::Attack { attack } => Some(attack.action_id()),
                _ => None,
            };
            let slot = attack_slot(sim, me);
            let senses = Senses {
                pos: me.position,
                yaw: me.yaw,
                action,
                slot,
                owner,
                enemies: &enemies,
                battle,
                dt,
            };
            let pad = p.brain.step(&data.pawn, &senses);
            out.insert(
                seat,
                Input {
                    stick: if pad.heading.is_some() {
                        [0.0, 1.0]
                    } else {
                        [0.0, 0.0]
                    },
                    camera_yaw: pad.heading.unwrap_or(me.yaw),
                    light: pad.light,
                    heavy: pad.heavy,
                    ..Input::default()
                },
            );
        }
        out
    }
}

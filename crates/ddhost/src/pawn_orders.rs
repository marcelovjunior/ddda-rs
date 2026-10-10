//! Party-order delivery. Native single-Arisen broadcast is 0x00B768E0;
//! shared-main-pawn/hired-owner filtering is the co-op design decision.

use crate::{FrameInputs, PlayerId};
use ddenemyrt::pawn::Command;
use std::collections::BTreeMap;

/// A request to select a pawn reply. Chatter eligibility, cue selection
/// and voice playback remain unported; this does not assert it was spoken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplyCandidate {
    pub sender: PlayerId,
    pub pawn: PlayerId,
    pub command: Command,
}

/// The Arisen's source order-action parameter plus actual recipients.
/// Weapon/stance layers of cPlActCMCOrder remain a presentation follow-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub sender: PlayerId,
    pub command: Command,
    pub shout_index: u8,
    pub replies: Vec<ReplyCandidate>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Orders {
    held: BTreeMap<PlayerId, u32>,
    cooldown: BTreeMap<PlayerId, f32>,
}

impl Orders {
    /// B52A00 -> B52880: one seat's rising direction bits. Missing input
    /// releases the seat; repeatedly uploaded held states do not repeat.
    // Translated from 0x00B52880 (DDDA.exe 2017-12-13 Steam).
    pub fn presses(&mut self, inputs: &FrameInputs) -> Vec<(PlayerId, Command)> {
        let mut presses = Vec::new();
        for (&seat, input) in inputs {
            let bits = input.sim.pawn_orders.native_bits();
            let prior = self.held.insert(seat, bits).unwrap_or(0);
            if let Some(command) = Command::from_pressed_bits(bits & !prior) {
                presses.push((seat, command));
            }
        }
        self.held.retain(|seat, _| inputs.contains_key(seat));
        presses
    }
    /// Headless delivery for the host and the standalone fixed-step view.
    /// `ownership` contains admitted pawn seats; `shared_main` is optional.
    pub fn dispatch(&mut self, inputs: &FrameInputs, cooling: &[PlayerId],
        fight: &mut ddenemyrt::fight::Fight, shared_main: Option<PlayerId>,
        ownership: &BTreeMap<PlayerId, PlayerId>) -> Vec<Delivery> {
        let presses = self.presses(inputs);
        let mut delivered = Vec::new();
        // Same-frame ties use ascending seats/last mailbox write, an
        // explicit co-op decision approved by head (#5548), not native.
        for (sender, command) in presses {
            if fight.arisen.get(&sender).is_some_and(|body| body.fighter.victim.dead()) {
                continue;
            }
            if cooling.contains(&sender) {
                continue;
            }
            let recipients: Vec<_> = fight.pawns.keys().filter_map(|&seat| {
                let authorized = shared_main == Some(seat) || ownership.get(&seat) == Some(&sender);
                authorized.then_some(seat)
            }).collect();
            let replies: Vec<_> = recipients.into_iter().filter_map(|pawn| {
                fight.deliver_pawn_order(pawn, command)
                    .then_some(ReplyCandidate { sender, pawn, command })
            }).collect();
            if !replies.is_empty() {
                self.cooldown.insert(sender, fight.data.pawn.order_cooldown);
                delivered.push(Delivery {
                    sender,
                    command,
                    shout_index: command.shout_index().unwrap(),
                    replies,
                });
            }
        }
        delivered
    }

    /// Native cPlAction arbitration B6D510 subtracts the main-frame delta
    /// only while control is available; a positive entry timer blocks the
    /// entire update even if subtraction reaches zero.
    // Translated from 0x00B6D510 (DDDA.exe 2017-12-13 Steam).
    pub fn advance(&mut self, frames: f32, eligible: impl Fn(PlayerId) -> bool) -> Vec<PlayerId> {
        let mut blocked = Vec::new();
        for (&seat, timer) in &mut self.cooldown {
            if *timer > 0.0 && eligible(seat) {
                *timer -= frames;
                blocked.push(seat);
            }
        }
        blocked
    }
}

impl<D: ddstory::data::Data, W: crate::StoryWorld> crate::Host<D, W> {
    /// Existing viewer identity, a session convention rather than exe behavior.
    pub fn main_pawn_seat() -> PlayerId { 4 }

    pub(crate) fn dispatch_pawn_orders(&mut self, inputs: &FrameInputs, cooling: &[PlayerId]) -> Vec<Delivery> {
        let Some(fight) = self.fight.as_mut() else {
            self.pawn_orders.presses(inputs);
            return Vec::new();
        };
        let ownership = fight.pawns.iter().filter_map(|(&seat, pawn)| {
            let owner = if let Some(index) = (0..2).find(|&i| Self::hired_pawn_seat(i) == seat) {
                self.hired_by[index]
            } else { Some(pawn.owner) };
            owner.map(|owner| (seat, owner))
        }).collect();
        self.pawn_orders.dispatch(inputs, cooling, fight, Some(Self::main_pawn_seat()), &ownership)
    }

    pub(crate) fn advance_pawn_order_cooldowns(&mut self, frames: f32) -> Vec<PlayerId> {
        self.pawn_orders.advance(frames, |seat| !self.world.player_actions_blocked(seat)
            && !self.fight.as_ref().is_some_and(|fight|
                fight.arisen.get(&seat).is_some_and(|body| body.fighter.victim.dead())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlayerInput;
    use ddsim::PawnOrderButtons;

    #[test]
    fn order_edges_suppress_reuploads_and_conflicting_directions() {
        let input = |buttons| PlayerInput {
            sim: ddsim::Input { pawn_orders: buttons, ..Default::default() },
            ..Default::default()
        };
        let up = PawnOrderButtons { up: true, ..Default::default() };
        let mut orders = Orders::default();
        let inputs = BTreeMap::from([(0, input(up))]);
        assert_eq!(orders.presses(&inputs), vec![(0, Command::Go)]);
        assert!(orders.presses(&inputs).is_empty());
        assert!(orders.presses(&Default::default()).is_empty());
        assert_eq!(orders.presses(&inputs), vec![(0, Command::Go)]);
        orders.presses(&Default::default());
        let inputs = BTreeMap::from([(0, input(PawnOrderButtons { up: true, down: true, ..Default::default() }))]);
        assert!(orders.presses(&inputs).is_empty());
    }

    #[test]
    fn positive_entry_cooldown_blocks_exact_zero_update_and_pauses_when_locked() {
        let mut orders = Orders::default();
        orders.cooldown.insert(0, 1.0);
        assert!(orders.advance(10.0, |_| false).is_empty());
        assert_eq!(orders.cooldown[&0], 1.0);
        assert_eq!(orders.advance(1.0, |_| true), [0]);
        assert_eq!(orders.cooldown[&0], 0.0);
        assert!(orders.advance(1.0, |_| true).is_empty());
    }
}

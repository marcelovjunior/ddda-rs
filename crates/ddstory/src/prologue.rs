//! New-game party selection, separate from story progression.
//!
//! This consumes the installed-data story stock built by
//! `ddpawn::builder::build_story_stock`. It does not manufacture enemy
//! deaths, complete scripts, or change a session's player identities.

use crate::Error;
use ddpawn::cmc::Cmc;

/// The original's temporary hero and three shared companions. A co-op
/// host may instantiate the hero template for each Arisen seat; companion
/// records remain session-owned. That seat mapping is a co-op policy,
/// not a property of the original game's four-slot party.
#[derive(Debug, Clone, PartialEq)]
pub struct PartySetup {
    pub hero: Cmc,
    pub companions: [Cmc; 3],
    pub gold: u32,
    pub rift_crystals: u32,
    pub experience: u32,
}

/// Selects already-built stock entries and applies the new-game names.
/// `name` resolves NPC ledger `mNameJ` by id. `random` is the next value
/// supplied by the host-owned game RNG; this function never seeds an RNG.
///
/// This is a setup description, not the entire player-record conversion:
/// equipment-derived stats (0x00446860), active inventory (0x004460d0),
/// and the three pawn-presence flags still belong to the host adapters.
// Translated from 0x00445ab0 (DDDA.exe 2017-12-13 Steam): stock selection
// at 0x00445c58..0x00445d0c, names at 0x00445d96..0x0044601e, hero
// balances at 0x00445e16 and RNG bonus at 0x00446020..0x0044608c.
pub fn party_from_stock(
    stock: &[Cmc],
    name: &dyn Fn(u32) -> Option<Vec<u8>>,
    random: u32,
) -> Result<PartySetup, Error> {
    let mut members = Vec::with_capacity(4);
    for seat in 0..4 {
        let index = 10 + seat;
        let mut member = stock
            .get(index)
            .ok_or_else(|| Error::Data(format!("prologue: missing story stock {index}")))?
            .clone();
        let npc = 0x380 + seat as u32;
        let bytes = name(npc).ok_or_else(|| Error::Data(format!("prologue: missing NPC name {npc}")))?;
        member.edit.set_name(&bytes);
        members.push(member);
    }
    let [hero, a, b, c]: [Cmc; 4] = members.try_into().unwrap();
    Ok(PartySetup {
        hero,
        companions: [a, b, c],
        gold: 37_000 + random % 5_000 + 1_000,
        rift_crystals: 785,
        experience: 1068,
    })
}

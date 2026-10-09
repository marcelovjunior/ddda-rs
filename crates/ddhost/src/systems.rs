//! The host-owned systems: economy (ddeconomy), progression (ddprogress)
//! and the Rift (ddrift), wired to the seats, the party and the
//! interaction lock (ADR 0002 sections 4-6).
//!
//! Every action names the seat that takes it. The host checks the seat,
//! takes the NPC lock where one applies, builds what the crate needs from
//! the other systems, and writes results back. Examples: the hiring
//! Arisen's level and crystals for the Rift; the party's bags, levels and
//! augments for a shop.

use crate::host::{Host, HostError, StoryWorld};
use crate::player::{Member, Party, PlayerId};
use ddeconomy::inventory::Owner;
use ddprogress::party::{CharId, Share};
use ddprogress::{Award, Character, Role};
use ddskill::vocation::Vocation;
use ddstory::data::Data;
use std::sync::Arc;

/// Game data of the host-owned systems, loaded once per session.
#[derive(Clone, Default)]
pub struct SystemData {
    /// Items, shop lists, upgrades (ddeconomy).
    pub catalog: Option<Arc<ddeconomy::Catalog>>,
    /// Level, rank, skill and augment tables (ddprogress).
    pub tables: Option<Arc<ddprogress::Tables>>,
    /// Offline pawn data (ddrift).
    pub rift: Option<Arc<ddrift::RiftData>>,
}

/// An augment slot without an augment, as the economy reads `+0x258`.
const NO_AUGMENT: u32 = u32::MAX;

impl<D: Data, W: StoryWorld> Host<D, W> {
    /// Rebuilds the party from the roster and the Rift's hired pawns.
    pub(crate) fn refresh_party(&mut self) {
        let mut p = Party::for_roster(&self.roster);
        for k in 0..2 {
            if self.rift.party.hired[k].is_some() && p.slots[2 + k].is_none() {
                p.slots[2 + k] = Some(Member::Hired { owner: self.hired_by[k].unwrap_or(0), slot: k as u8 + 1 });
            }
        }
        self.party = p;
    }

    /// Play time in whole seconds of host frames. The unit of the Rift's
    /// hire times is *assumed* to be seconds.
    pub fn play_time(&self) -> u32 {
        (self.frame as f64 * f64::from(self.clock.seconds())) as u32
    }

    fn character_of(&self, role: Role) -> Option<&Character> {
        let m = self.progression.party.iter().flatten().find(|m| m.role == role)?;
        self.progression.characters.get(m.character)
    }

    /// Creates the progression record of seat `player`'s Arisen, a new
    /// level-1 character of `vocation` (`Character::new`), in its party
    /// slot.
    pub fn create_arisen(&mut self, player: PlayerId, vocation: Vocation) -> Result<CharId, HostError> {
        let t = self.data.tables.clone().ok_or(HostError::NoData("progression"))?;
        let slot = self
            .party
            .slots
            .iter()
            .position(|m| *m == Some(Member::Arisen(player)))
            .ok_or(HostError::UnknownPlayer(player))?;
        if self.progression.party[slot].is_some() {
            return Err(HostError::Rift(format!("seat {player} already has an Arisen record")));
        }
        let c = self.progression.add(Character::new(&t, vocation, false));
        self.progression.party[slot] = Some(ddprogress::Member { character: c, role: Role::Arisen(player) });
        Ok(c)
    }

    /// Creates the host's main pawn record (party slot 1, owned by seat 0).
    pub fn create_main_pawn(&mut self, vocation: Vocation) -> Result<CharId, HostError> {
        let t = self.data.tables.clone().ok_or(HostError::NoData("progression"))?;
        let c = self.progression.add(Character::new(&t, vocation, true));
        self.progression.party[1] = Some(ddprogress::Member { character: c, role: Role::MainPawn { owner: 0 } });
        Ok(c)
    }

    /// Gives an experience award to the party (0x00443470, via
    /// `Progression::award`). Levels rise one per frame in `Host::tick`,
    /// as `uPlayer` does (0x00B5BD20).
    pub fn award_exp(&mut self, award: Award) -> Result<[Option<Share>; 4], HostError> {
        let t = self.data.tables.clone().ok_or(HostError::NoData("progression"))?;
        Ok(self.progression.award(&t, award))
    }

    fn econ_member(&self, m: Member) -> ddeconomy::Member {
        let (owner, c) = match m {
            Member::Arisen(p) => (Owner::Arisen(p), self.character_of(Role::Arisen(p))),
            Member::MainPawn => (Owner::Pawn(0), self.character_of(Role::MainPawn { owner: 0 })),
            Member::Hired { slot, .. } => (Owner::Pawn(slot), None),
        };
        let (level, augments) = match (c, m) {
            (Some(c), _) => (c.progress.level, c.augments.map(|a| a.map_or(NO_AUGMENT, u32::from))),
            (None, Member::Hired { slot, .. }) => {
                let level = self.rift.party.hired[slot as usize - 1].as_ref().map_or(1, |p| u32::from(p.pawn.param.level));
                (level, [NO_AUGMENT; 6])
            }
            (None, _) => (1, [NO_AUGMENT; 6]),
        };
        ddeconomy::Member { owner, present: true, level, augments }
    }

    /// The party as the economy sees it from `player`: their own Arisen
    /// first, then the other members in party order.
    pub fn economy_party(&self, player: PlayerId) -> ddeconomy::Party {
        let own = Member::Arisen(player);
        let mut members = vec![self.econ_member(own)];
        members.extend(self.party.slots.iter().flatten().filter(|m| **m != own).map(|m| self.econ_member(*m)));
        ddeconomy::Party { members }
    }

    /// Opens NPC `npc`'s shop for `player`. The host's interaction lock is
    /// taken first; the economy's open conditions follow
    /// (`Economy::open_shop`, 0x00CDDF60).
    pub fn open_shop(&mut self, player: PlayerId, npc: u32) -> Result<ddeconomy::Session, HostError> {
        self.roster.seat(player).ok_or(HostError::UnknownPlayer(player))?;
        let cat = self.data.catalog.clone().ok_or(HostError::NoData("economy"))?;
        let party = self.economy_party(player);
        let world = &self.story.as_ref().ok_or(HostError::NoData("story"))?.world;
        self.locks.acquire(npc, player).map_err(HostError::Lock)?;
        self.economy.open_shop(&cat, world, &party, player, npc).map_err(|e| {
            let _ = self.locks.release(npc, player);
            HostError::Economy(e)
        })
    }

    /// Buys in an open shop session (`Economy::buy`).
    pub fn buy(&mut self, s: &ddeconomy::Session, item: u32, count: u32, to: Owner) -> Result<u32, HostError> {
        let cat = self.data.catalog.clone().ok_or(HostError::NoData("economy"))?;
        let party = self.economy_party(s.player);
        let world = &self.story.as_ref().ok_or(HostError::NoData("story"))?.world;
        self.economy.buy(&cat, world, &party, s, item, count, to).map_err(HostError::Economy)
    }

    /// Sells in an open shop session (`Economy::sell`).
    pub fn sell(&mut self, s: &ddeconomy::Session, from: Owner, index: usize, count: u32) -> Result<u32, HostError> {
        let cat = self.data.catalog.clone().ok_or(HostError::NoData("economy"))?;
        let party = self.economy_party(s.player);
        self.economy.sell(&cat, &party, s, from, index, count).map_err(HostError::Economy)
    }

    /// Ends a shop or upgrade session and frees its NPC.
    pub fn close_shop(&mut self, s: ddeconomy::Session) {
        self.economy.close(s);
        let _ = self.locks.release(s.npc, s.player);
    }

    /// `player` reaches a Riftstone. The first visitor opens the room
    /// (uRim). Both Arisen may stand in it; a pawn being talked to is
    /// locked for the other (ddrift's talk lock).
    pub fn rift_enter(&mut self, player: PlayerId) -> Result<(), HostError> {
        self.roster.seat(player).ok_or(HostError::UnknownPlayer(player))?;
        if self.rift_visitors.insert(player) && !self.rift.visit_open() {
            self.rift.open_room();
        }
        Ok(())
    }

    /// `player` leaves the Riftstone: their talks end, and the last one out
    /// closes the room.
    pub fn rift_leave(&mut self, player: PlayerId) {
        if !self.rift_visitors.remove(&player) {
            return;
        }
        for slot in 0..ddrift::ROOM_SLOTS {
            self.rift.stop_talking(player, slot);
        }
        if self.rift_visitors.is_empty() {
            self.rift.close_room();
        }
    }

    fn visitor(&self, player: PlayerId) -> Result<(), HostError> {
        if self.rift_visitors.contains(&player) {
            Ok(())
        } else {
            Err(HostError::Rift(format!("seat {player} is not at a Riftstone")))
        }
    }

    /// Summons a listed pawn into the room (`Rift::summon`).
    pub fn rift_summon(&mut self, player: PlayerId, row: &mut ddrift::Row) -> Result<usize, HostError> {
        self.visitor(player)?;
        self.rift.summon(player, row).map_err(|e| HostError::Rift(format!("{e:?}")))
    }

    /// Hires room pawn `room_slot` for the party (`Rift::hire`, 0x0073C2C0).
    /// The cost uses the hiring Arisen's level (progression; 1 without a
    /// record) and is paid from their wallet's Rift Crystals. Returns the
    /// party pawn slot (1 or 2).
    pub fn rift_hire(&mut self, player: PlayerId, room_slot: usize) -> Result<usize, HostError> {
        self.visitor(player)?;
        let level = self.character_of(Role::Arisen(player)).map_or(1, |c| c.progress.level);
        let rc = self.economy.wallet(player).rift_crystals;
        let mut arisen = ddrift::Arisen { id: player, level, rift_crystals: rc };
        let play_time = self.play_time();
        let slot = self.rift.hire(&mut arisen, room_slot, play_time).map_err(|e| HostError::Rift(format!("{e:?}")))?;
        self.economy.wallet(player).rift_crystals = arisen.rift_crystals;
        self.hired_by[slot - 1] = Some(player);
        self.refresh_party();
        Ok(slot)
    }

    /// Dismisses the pawn in party pawn slot `slot` (`Rift::release`).
    pub fn rift_release(&mut self, slot: usize) -> Option<ddrift::PawnRef> {
        let out = self.rift.release(slot, self.play_time(), false);
        if out.is_some() {
            self.hired_by[slot - 1] = None;
            self.refresh_party();
        }
        out
    }

    /// Rebuilds the Rift's stock, as the original does on a load
    /// (`Rift::refill`, 0x0047D330). `level` is the host Arisen's level.
    pub fn refill_rift(&mut self, load_mode: u32, rng: &mut impl ddpawn::rift::PawnRng) -> Result<(), HostError> {
        let data = self.data.rift.clone().ok_or(HostError::NoData("rift"))?;
        let level = self.character_of(Role::Arisen(0)).map_or(1, |c| c.progress.level);
        self.rift.refill(&data, level, load_mode, rng).map_err(HostError::Rift)
    }
}

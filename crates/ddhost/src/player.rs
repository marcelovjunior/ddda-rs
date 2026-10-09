//! Player identity, seats and the party (docs/COOP_DESIGN.md).
//!
//! Two identities, kept apart on purpose (design decision, ADR 0002):
//! - [`PlayerId`] is the seat in this session (0 = the host's Arisen).
//!   It indexes `ddsim` players and every per-player system for as long
//!   as the session runs.
//! - [`ProfileId`] names an Arisen across sessions and saves: the guest's
//!   character keeps its profile whichever seat it gets next time.

pub use ddid::{PlayerId, ProfileId};

/// Where a seat's input comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Input read on the host machine.
    Local,
    /// Input uploaded by a client over the LAN.
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    pub id: PlayerId,
    pub profile: ProfileId,
    pub control: Control,
}

/// The Arisen of the session, in seat order (the host first).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roster {
    seats: Vec<Seat>,
}

/// Two Arisen by design (COOP_DESIGN.md: two full players). Every system
/// takes a `PlayerId`, so raising this is a policy change, not a rewrite.
pub const MAX_ARISEN: usize = 2;

impl Roster {
    /// Seats a profile on the lowest free seat. `None` when full or when
    /// the profile is already seated.
    pub fn join(&mut self, profile: ProfileId, control: Control) -> Option<PlayerId> {
        if self.seats.len() >= MAX_ARISEN || self.seats.iter().any(|s| s.profile == profile) {
            return None;
        }
        let id = (0..=PlayerId::MAX).find(|i| self.seats.iter().all(|s| s.id != *i))?;
        self.seats.push(Seat { id, profile, control });
        self.seats.sort_by_key(|s| s.id);
        Some(id)
    }

    pub fn leave(&mut self, id: PlayerId) -> Option<Seat> {
        let i = self.seats.iter().position(|s| s.id == id)?;
        Some(self.seats.remove(i))
    }

    pub fn seats(&self) -> &[Seat] {
        &self.seats
    }

    pub fn seat(&self, id: PlayerId) -> Option<&Seat> {
        self.seats.iter().find(|s| s.id == id)
    }

    pub fn ids(&self) -> impl Iterator<Item = PlayerId> + '_ {
        self.seats.iter().map(|s| s.id)
    }

    pub fn len(&self) -> usize {
        self.seats.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seats.is_empty()
    }
}

/// One of the four party slots. The original keeps four `uPlayerBase*` at
/// `sPlayerManager` +0x99C: the Arisen (uPlayer) and three pawns (uCmc)
/// (docs/ORACLE.md, live). In co-op the second Arisen takes the last
/// hired-pawn slot (COOP_DESIGN.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Member {
    Arisen(PlayerId),
    /// The host's main pawn, shared: either Arisen may command it.
    MainPawn,
    /// A hired pawn; `owner` is the Arisen who hired it.
    Hired { owner: PlayerId, slot: u8 },
}

/// The party: four slots, in the original's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Party {
    pub slots: [Option<Member>; 4],
}

impl Party {
    /// The co-op party layout: host Arisen, main pawn, one hired pawn slot
    /// (empty), guest Arisen (if any).
    pub fn for_roster(roster: &Roster) -> Party {
        let mut slots = [None; 4];
        let mut ids = roster.ids();
        slots[0] = ids.next().map(Member::Arisen);
        slots[1] = Some(Member::MainPawn);
        slots[3] = ids.next().map(Member::Arisen);
        Party { slots }
    }

    pub fn arisen(&self) -> impl Iterator<Item = PlayerId> + '_ {
        self.slots.iter().filter_map(|m| match m {
            Some(Member::Arisen(p)) => Some(*p),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seats_and_party() {
        let mut r = Roster::default();
        assert_eq!(r.join(ProfileId(7), Control::Local), Some(0));
        assert_eq!(r.join(ProfileId(7), Control::Remote), None, "same profile twice");
        assert_eq!(r.join(ProfileId(9), Control::Remote), Some(1));
        assert_eq!(r.join(ProfileId(11), Control::Remote), None, "two Arisen at most");
        let p = Party::for_roster(&r);
        assert_eq!(p.arisen().collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(p.slots[3], Some(Member::Arisen(1)));
        // The guest leaves and comes back on the same seat.
        r.leave(1).unwrap();
        assert_eq!(r.join(ProfileId(9), Control::Remote), Some(1));
    }
}

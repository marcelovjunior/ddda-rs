//! The NPC interaction lock (COOP_DESIGN.md, decided by the maintainer):
//! while one Arisen talks to, trades with or otherwise interacts with an
//! NPC, that NPC is locked for the other. The original has one Arisen and
//! no such lock; this is a co-op design rule.
//!
//! The host owns the table. A system that starts an interaction (talk,
//! shop session, Rift, inn) acquires the NPC first and releases it when
//! the interaction ends; a disconnect releases everything the player held.

use crate::player::PlayerId;
use std::collections::BTreeMap;

/// NPC identity as the game keys it (the NPC id, npc +0x5858, the same id
/// `ddstory::NpcUnit` carries).
pub type NpcId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockError {
    /// Another Arisen holds the NPC.
    Held(PlayerId),
    /// The player does not hold the NPC it tried to release.
    NotHeld,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InteractionLocks {
    held: BTreeMap<NpcId, PlayerId>,
}

impl InteractionLocks {
    /// Locks `npc` for `player`. Re-acquiring one's own lock succeeds.
    pub fn acquire(&mut self, npc: NpcId, player: PlayerId) -> Result<(), LockError> {
        match self.held.get(&npc) {
            Some(&p) if p != player => Err(LockError::Held(p)),
            _ => {
                self.held.insert(npc, player);
                Ok(())
            }
        }
    }

    pub fn release(&mut self, npc: NpcId, player: PlayerId) -> Result<(), LockError> {
        match self.held.get(&npc) {
            Some(&p) if p == player => {
                self.held.remove(&npc);
                Ok(())
            }
            _ => Err(LockError::NotHeld),
        }
    }

    /// Releases every lock of `player` (end of session, disconnect).
    pub fn release_all(&mut self, player: PlayerId) -> Vec<NpcId> {
        let mine: Vec<NpcId> = self
            .held
            .iter()
            .filter(|(_, &p)| p == player)
            .map(|(&n, _)| n)
            .collect();
        for n in &mine {
            self.held.remove(n);
        }
        mine
    }

    pub fn holder(&self, npc: NpcId) -> Option<PlayerId> {
        self.held.get(&npc).copied()
    }

    /// Whether `player` may start interacting with `npc` now.
    pub fn available(&self, npc: NpcId, player: PlayerId) -> bool {
        self.holder(npc).is_none_or(|p| p == player)
    }

    pub fn iter(&self) -> impl Iterator<Item = (NpcId, PlayerId)> + '_ {
        self.held.iter().map(|(&n, &p)| (n, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_holder_per_npc() {
        let mut l = InteractionLocks::default();
        l.acquire(500, 0).unwrap();
        assert_eq!(l.acquire(500, 1), Err(LockError::Held(0)));
        assert!(!l.available(500, 1));
        assert!(l.available(500, 0));
        l.acquire(500, 0).unwrap();
        l.acquire(501, 1).unwrap();
        assert_eq!(l.release(500, 1), Err(LockError::NotHeld));
        l.release(500, 0).unwrap();
        l.acquire(500, 1).unwrap();
        assert_eq!(l.release_all(1), vec![500, 501]);
        assert_eq!(l.iter().count(), 0);
    }
}

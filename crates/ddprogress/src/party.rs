//! The host-owned progression state and the party experience share.

use crate::character::Character;
use crate::tables::Tables;
use ddskill::vocation::Vocation;

/// Index of a character in [`Progression::characters`].
pub type CharId = usize;
/// A player of the session: the seat, 0 = host (`ddid::PlayerId`).
pub use ddid::PlayerId;

/// What a party slot does with an experience award.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A player's Arisen: experience and discipline (slot-0 path, 0x00782d10).
    Arisen(PlayerId),
    /// The main pawn: its share plus a catch-up toward its owner's Arisen
    /// (slot-1 path). In the original the owner is the slot-0 Arisen.
    MainPawn { owner: PlayerId },
    /// A hired pawn: no experience; 10 % of its share becomes Rift Crystals
    /// for its owner (slots 2 and 3, +0x34c).
    Support { owner: PlayerId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member {
    pub character: CharId,
    pub role: Role,
}

/// One experience award, as 0x00443470 receives it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Award {
    pub exp: u32,
    /// The caller's multiplier (`param_3`), applied after the level share.
    pub rate: f32,
    /// `param_4`: give `exp` to every member as is (no doubling, no share).
    pub fixed: bool,
    /// The doubling condition (DAT_018fa4f8+0x20e77b bit 4 and stage !=
    /// 0x25d; meaning unresolved). Normally false.
    pub double: bool,
}

impl Award {
    pub fn new(exp: u32) -> Self {
        Award { exp, rate: 1.0, fixed: false, double: false }
    }
}

/// What one award produced per party slot.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Share {
    /// Experience passed to the character's award (after share and, for the
    /// main pawn, catch-up).
    pub exp: u32,
    /// Rift Crystals added (support pawns).
    pub rift_crystals: i32,
}

/// Session progression state, owned by the host. Characters are indexed by
/// [`CharId`]; the party holds up to four of them in the original's slot
/// order (co-op: P1 Arisen, main pawn, hired pawn, P2 Arisen,
/// docs/COOP_DESIGN.md).
#[derive(Debug, Clone, PartialEq)]
pub struct Progression {
    pub characters: Vec<Character>,
    pub party: [Option<Member>; 4],
}

/// Cap of a support pawn's Rift Crystals (0x00443470: 99,999,999).
pub const RC_CAP: i32 = 99_999_999;

impl Progression {
    /// Detach a profile's Arisen and remove every reference to its old seat.
    /// Remaining character indices stay coherent; this is host co-op policy.
    pub fn detach_arisen(&mut self, seat: PlayerId) -> Option<Character> {
        let index = self.party.iter().flatten()
            .find(|m| m.role == Role::Arisen(seat))?.character;
        for member in &mut self.party {
            if member.is_some_and(|m| m.character == index) { *member = None; }
            else if let Some(member) = member {
                if member.character > index { member.character -= 1; }
            }
        }
        Some(self.characters.remove(index))
    }

    pub fn new() -> Self {
        Progression { characters: vec![], party: [None; 4] }
    }

    pub fn add(&mut self, c: Character) -> CharId {
        self.characters.push(c);
        self.characters.len() - 1
    }

    /// Distributes an award over the party. Translated from 0x00443470
    /// (DDDA.exe 2017-12-13 Steam):
    /// - optional doubling, `trunc(exp * 2)`;
    /// - each present member's share `trunc(trunc(m * exp + 0.5) * rate)`
    ///   with `m` from `ddskill::growth::party_share` over the four slots
    ///   (0x004437d0), or `exp` when `fixed`;
    /// - Arisen: the share goes to [`Character::award`];
    /// - main pawn: `max(0, (owner_total - pawn_total) * k * 0.01 + share)`
    ///   truncated, `k` = `param\game`[0], totals taken before this award;
    /// - support pawn: `trunc(share_f * 0.1)` Rift Crystals, where `share_f`
    ///   is the share before its final truncation (the value left on the
    ///   x87 stack), capped at 99,999,999.
    ///
    /// Co-op (not in the original): roles decide the path instead of slot
    /// numbers, so a second Arisen in slot 3 earns experience; the main
    /// pawn catches up toward its owner. With one player in slot 0 and the
    /// main pawn in slot 1 this is exactly the original.
    pub fn award(&mut self, tables: &Tables, award: Award) -> [Option<Share>; 4] {
        let mut exp = award.exp;
        if award.double && !award.fixed {
            exp = (f64::from(exp) * 2.0) as u32;
        }
        let levels = self.party.map(|m| m.map(|m| self.characters[m.character].progress.level));
        // Totals before any award (the original reads the Arisen's before
        // awarding slot 0, then awards slot 1).
        let totals: Vec<f32> = self.characters.iter().map(|c| c.total_exp(tables)).collect();
        let arisen_of = |p: PlayerId| {
            self.party.iter().flatten().find(|m| m.role == Role::Arisen(p)).map(|m| m.character)
        };
        let mut out = [None; 4];
        let mut plan = vec![];
        for (slot, m) in self.party.iter().enumerate() {
            let Some(m) = *m else { continue };
            let (share, share_f) = if award.fixed {
                (exp as i64, f64::from(exp))
            } else {
                let mult = ddskill::growth::party_share(levels, slot);
                // x87: FLD m, FILD exp, FMULP, FADD 0.5, _ftol (truncate);
                // then CVTSI2SS, MULSS rate, FISTP truncating.
                let first = (f64::from(mult) * f64::from(exp) + 0.5) as i64;
                let f = first as f32 * award.rate;
                (f as i64, f64::from(f))
            };
            let entry = match m.role {
                Role::Arisen(_) => Share { exp: share as u32, rift_crystals: 0 },
                Role::MainPawn { owner } => {
                    let owner_total = arisen_of(owner).map_or(0.0, |a| totals[a]);
                    let pawn_total = totals[m.character];
                    let k = tables.pawn_catch_up();
                    let mut x = (owner_total - pawn_total) * k * 0.01f32 + share as f32;
                    if x < 0.0 {
                        x = 0.0;
                    }
                    Share { exp: x as u32, rift_crystals: 0 }
                }
                Role::Support { .. } => {
                    let rc = (share_f * f64::from(0.1f32)) as i32;
                    Share { exp: 0, rift_crystals: rc }
                }
            };
            plan.push((m, entry));
            out[slot] = Some(entry);
        }
        for (m, e) in plan {
            let c = &mut self.characters[m.character];
            match m.role {
                Role::Support { .. } => {
                    c.rift_crystals = c.rift_crystals.saturating_add(e.rift_crystals).min(RC_CAP);
                }
                _ => c.award(tables, e.exp),
            }
        }
        out
    }

    /// One simulation step for every character (level-up, then rank-up).
    pub fn step(&mut self, tables: &Tables) {
        for c in &mut self.characters {
            c.step(tables);
        }
    }

    /// Copies the host's own records into the shared story state for the
    /// systems that read the original's single-player fields:
    /// `World::player_job` (sGameSys+0xa76e0) from player 0's Arisen and
    /// the vocation flags 540..554 (GSF bits, set by 0x00401280) from player
    /// 0's Arisen and the main pawn.
    pub fn sync_to_world(&self, world: &mut ddstory::World) {
        for m in self.party.iter().flatten() {
            let c = &self.characters[m.character];
            let pawn = match m.role {
                Role::Arisen(0) => {
                    world.player_job = c.vocation as u32;
                    false
                }
                Role::MainPawn { .. } => true,
                _ => continue,
            };
            for v in Vocation::ALL {
                if let Some(flag) = v.unlock_flag(pawn) {
                    if c.paid[v as usize] {
                        world.gsf.set(flag.into(), true);
                    }
                }
            }
        }
    }

    /// Reads the vocation flags back from a story state loaded from a save
    /// (player 0's Arisen and the main pawn).
    pub fn sync_from_world(&mut self, world: &ddstory::World) {
        for m in self.party.iter().flatten() {
            let pawn = match m.role {
                Role::Arisen(0) => false,
                Role::MainPawn { .. } => true,
                _ => continue,
            };
            let c = &mut self.characters[m.character];
            for v in Vocation::ALL {
                if let Some(flag) = v.unlock_flag(pawn) {
                    c.paid[v as usize] |= world.gsf.get(flag.into());
                }
            }
        }
    }
}

impl Default for Progression {
    fn default() -> Self {
        Self::new()
    }
}

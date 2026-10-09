//! Host-owned character progression for Dragon's Dogma: Dark Arisen.
//!
//! Per character (each player's Arisen, the main pawn, hired pawns):
//! experience and level-ups with the current vocation's growth, discipline
//! from experience, vocation ranks, the party experience share (with
//! hired pawns' Rift Crystals), learning and equipping skills and
//! augments, and vocation change. The rules are `ddskill`'s translations
//! plus the screens' checks traced in `docs/VOCATIONS.md`; the data comes
//! from the install and the player's DDDA.exe ([`tables::Tables`]).
//!
//! One [`party::Progression`] lives on the host. Nothing here assumes one
//! player: characters are addressed by id and party roles name their
//! player. Shared story state (`ddstory::World`) is only mirrored for the
//! fields the original keeps there (`sync_to_world`).

pub mod change;
pub mod character;
pub mod learn;
pub mod party;
pub mod tables;

pub use character::Character;
pub use party::{Award, Member, Progression, Role};
pub use tables::Tables;

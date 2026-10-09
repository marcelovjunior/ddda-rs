//! A headless, host-owned Rift (docs/PAWNS.md, proposal 3).
//!
//! The host owns one [`Rift`] per session: the 120-slot pawn stock
//! (story pawns and the Rift pool), the party's pawn slots, the saved pawn
//! bookkeeping ([`log::PawnLog`]) and, while someone is at a Riftstone, the
//! Rift room. Every action names the player who takes it and is given that
//! player's [`Arisen`] (level and Rift Crystals), so two Arisen can share
//! one Rift (docs/COOP_DESIGN.md).
//!
//! Rules are translations of DDDA.exe (2017-12-13 Steam) cited per
//! function. The offline game is modelled: network pawns never appear.

pub mod data;
pub mod log;
pub mod rift;

pub use data::RiftData;
pub use log::{HistoryEntry, PawnLog, PawnRef};
pub use rift::*;

//! Combat resource readers for Dragon's Dogma: Dark Arisen (PC).
//!
//! `ocl` reads the hit volumes and attack tables; `damage` holds the hit
//! rules as pure functions translated from the exe. `docs/COMBAT.md`
//! documents both with addresses.

pub mod damage;
pub mod ocl;

pub use ocl::{Attack, Group, Node, ObjCollision, SeqIndex};

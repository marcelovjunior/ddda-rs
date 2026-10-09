//! Combat resource readers for Dragon's Dogma: Dark Arisen (PC).
//!
//! Readers only: no damage simulation lives here yet. The rules these
//! resources feed are documented, with exe addresses, in `docs/COMBAT.md`.

pub mod ocl;

pub use ocl::{Attack, Group, Node, ObjCollision, SeqIndex};

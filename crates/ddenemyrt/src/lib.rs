//! Headless enemy runtime and the fight pass.
//!
//! * [`body`]: a character's skeleton, motions and collision; posing
//!   (`mtlmt::pose`) and the hit volumes of a frame (`ddcombat::volumes`);
//! * [`combat`]: the hit pass (docs/FRAME.md 5.1): hits from last frame's
//!   volumes, valued, merged and applied to the victims' pools;
//! * [`arisen`]: the Arisen's body and values, and a scripted attack;
//! * [`goblin`]: the goblin unit (uEm0100): its think (`ddai`), actions,
//!   motions and root motion on the world's collision.
//!
//! Nothing here assumes one player: every fighter has an owner id, and
//! targets are picked from the host's list of Arisen.

pub mod arisen;
pub mod body;
pub mod combat;
pub mod fight;
pub mod goblin;
pub mod pushout;

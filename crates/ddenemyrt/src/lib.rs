//! Headless enemy runtime and the fight pass.
//!
//! * [`body`]: a character's skeleton, motions and collision; posing
//!   (`mtlmt::pose`) and the hit volumes of a frame (`ddcombat::volumes`);
//! * [`combat`]: the hit pass (docs/FRAME.md 5.1): hits from last frame's
//!   volumes, valued, merged and applied to the victims' pools;
//! * [`arisen`]: the Arisen's body, values and weapon (its attacks are
//!   ddsim's);
//! * [`shell`]: the staff's shells;
//! * [`goblin`]: the goblin unit (uEm0100): its think (`ddai`), actions,
//!   motions and root motion on the world's collision;
//! * [`pawn`]: pawns (`ddpawnai`): their virtual pads drive their bodies
//!   in ddsim like players.
//!
//! Nothing here assumes one player: every fighter has an owner id, and
//! targets are picked from the host's list of Arisen.

pub mod arisen;
pub mod beast;
pub mod body;
pub mod combat;
mod enemy_stats;
pub mod fight;
pub mod goblin;
pub mod pawn;
pub mod pushout;
pub mod shell;

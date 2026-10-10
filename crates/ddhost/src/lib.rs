//! The game host for ddda-rs.
//!
//! One `Host` owns a session: the seated Arisen (N players, two by
//! design), the party, the world and the headless systems, and steps them
//! at a fixed frame in the original's order (docs/FRAME.md). It is the
//! authority in LAN co-op: clients send inputs and receive state
//! (docs/adr/0002-game-host.md). Single-player is the same host with one
//! local seat.
//!
//! Composed so far: `ddsim` (movement, the `Player` move line) and
//! `ddstory` (scripts, quests, scenarios, the `Pre Process` line).
//!
//! Nothing here renders, plays sound or reads devices.

pub mod ambient;
pub mod clock;
pub mod doors;
pub mod dragon;
pub mod data;
pub mod hash;
pub mod host;
pub mod input;
pub mod install;
pub mod lock;
pub mod npcs;
pub mod npc_placements;
pub mod npc_motion;
pub mod opening;
pub mod player;
pub mod prologue;
pub mod save;
pub mod spawns;
pub mod story_pawns;
pub mod story_npcs;
pub mod systems;

/// The fight runtime the host composes (`Host::fight`), for views.
pub use ddenemyrt;
/// The NPC runtime the host composes (`Host::npcs`), for views.
pub use ddnpcrt;
pub use ddspawn;
/// Vocations and their first-choice equipment, for views.
pub use ddprogress;
pub use ddskill;

pub use clock::FrameClock;
pub use host::{FrameReport, Host, HostError, StoryWorld};
pub use input::{FrameInputs, PlayerInput};
pub use player::{Control, Member, Party, PlayerId, ProfileId, Roster};


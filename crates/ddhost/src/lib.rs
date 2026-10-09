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

pub mod clock;
pub mod data;
pub mod hash;
pub mod host;
pub mod input;
pub mod install;
pub mod lock;
pub mod player;
pub mod save;

pub use clock::FrameClock;
pub use host::{FrameReport, Host, HostError, StoryWorld};
pub use input::{FrameInputs, PlayerInput};
pub use player::{Control, Member, Party, PlayerId, ProfileId, Roster};

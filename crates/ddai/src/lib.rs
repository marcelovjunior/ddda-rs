//! Headless rAIFSM interpreter: inputs -> state -> action requests.
//!
//! The game runs every AI graph (`rAIFSM`, read by `mtfsm`) on one core
//! owned by a `cBaseFSM` subclass: `cFSMOrder` for quest and stage scripts
//! (run by `ddstory`) and `cThinkFSM` -> `cAIConsiderFSM` ->
//! `cAILevelAbstractFSM` for enemies. This crate holds that shared core and
//! the enemy owner:
//!
//! * [`cond`]: condition trees (`rAIConditionTree`, 0x0117c040 / 0x0117bd30);
//! * [`core`]: the level stepper (0x00e092a0, 0x00e06710, 0x00e05800),
//!   generic over an [`core::Owner`];
//! * [`think`]: [`think::Thinker`], the `cThinkFSM` / `cAILevelAbstractFSM`
//!   owner: its variables, its processes and its per-frame update
//!   (vf06 0x007a9cb0). It reads an [`think::Inputs`] snapshot and emits
//!   [`think::Request`]s for the host.
//!
//! `cond` and `core` are ports of `ddstory::cond` and `ddstory::script`
//! (claude-7) with the owner made generic; the addresses and semantics are
//! the same. Anything not implemented is an [`Error::Unimplemented`], never
//! skipped. One `Thinker` per enemy; nothing here assumes one player.

pub mod cond;
pub mod core;
pub mod fields;
pub mod think;

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// A process, variable, node or flag this runtime does not implement.
    Unimplemented(String),
    /// Bad or missing data.
    Data(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unimplemented(s) => write!(f, "unimplemented: {s}"),
            Error::Data(s) => write!(f, "data: {s}"),
        }
    }
}

impl std::error::Error for Error {}

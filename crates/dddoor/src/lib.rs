//! Authored door placement and headless door behavior.
//!
//! Regular hinged uOmObj7010 doors are supported. Sliding doors, lock/key
//! state, save persistence and player action selection remain explicit
//! caller inputs; an arbitrary distance check is not an opening request.

pub mod placement;
pub mod runtime;

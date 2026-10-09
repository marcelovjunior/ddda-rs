//! The enemies' built-in think tables.
//!
//! Enemy FSMs only set up combat: their CtrlThinkTbl process (0x005a9240)
//! hands a built-in table to the unit's `cThinkMgr`, and the unit's own
//! think (uEm0100: vf136 -> 0x008d2360) runs that table every time its
//! current action allows. A table is a list of 0xa0-byte records
//! ([`table::Act`]): chains of conditions/commands, then rows that test the
//! current target's distance, angle and height, flags, timers and callbacks,
//! and either pick an action id, jump/call another table, or fall through.
//! Random blocks pick one row by percent weights.
//!
//! * [`table`]: the records, read at run time from the player's exe;
//! * [`mgr`]: `cThinkMgr` and its interpreter (0x008b81a0, row filter
//!   0x008b8bb0, conditions 0x008b9500, commands 0x008ba2a0);
//! * [`em0100`]: the goblin's unit side (action classes, its callbacks that
//!   read only the current action, result rewrite 0x008d0fb0, driver).
//!
//! The world-facing parts are the host's: target slots
//! ([`mgr::Slot::measure`]), the unit's 32 think flags, the +0x4c / +0x50
//! callbacks (paths, line of sight, party state...) and rand.

pub mod em0100;
pub mod families;
pub mod mgr;
pub mod table;

pub use mgr::{Host, Slot, ThinkMgr};
pub use table::{Act, Tables};

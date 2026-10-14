//! L172 (claude-9, head decision after #14620): a per-thread view of the
//! scheduler global `[0x018D0B28]` (sMain, whose job queue DB63E0's queued
//! branch and the sPrimitive callbacks use).
//!
//! About 230 functions of the exe read the global with an absolute load
//! (`mov r32, [0x018D0B28]`); the lifter routes every such 32-bit load
//! through [`scheduler_global`] (research/native/lift.py `fast_memory`), and
//! the native ports that read it call the same function. Writes stay plain
//! memory writes (0x00DBE810 and 0x00DBD530, the frame's owner, outside the
//! parallel shadow). A thread that runs the parallel shadow traversal's
//! lanes sets the view to its private scheduler copy; every other thread
//! keeps reading the real global.
use crate::mem::{Mem, PageTable, Pages};
use std::cell::Cell;
use std::ptr::NonNull;

/// The scheduler global's address (DDDA.exe 2017-12-13 Steam).
pub const SCHEDULER_GLOBAL: u32 = 0x018D_0B28;

thread_local! {
    /// This thread's view of [`SCHEDULER_GLOBAL`] (0: none, read memory).
    static VIEW: Cell<u32> = const { Cell::new(0) };
}

/// This thread's view (0: none).
#[inline]
pub fn view() -> u32 {
    VIEW.with(Cell::get)
}

/// Set this thread's view (0 clears it); returns the previous one.
pub fn set_view(scheduler: u32) -> u32 {
    VIEW.with(|v| v.replace(scheduler))
}

thread_local! {
    /// This thread runs the parallel shadow's private top-level traversal
    /// (DB63E0 itself, not the units its lanes run).
    static TOP_LEVEL: Cell<bool> = const { Cell::new(false) };
}

/// Whether this thread runs a private top-level traversal: DB63E0's batch
/// flag test (0x018D0BB0, which stays set for the other threads' batch)
/// reads as clear.
#[inline]
pub fn top_level() -> bool {
    TOP_LEVEL.with(Cell::get)
}

/// Set [`top_level`]; returns the previous value.
pub fn set_top_level(on: bool) -> bool {
    TOP_LEVEL.with(|t| t.replace(on))
}

/// `[0x018D0B28]` as this thread sees it: its view, else the memory word.
#[inline]
pub fn scheduler_global(m: &dyn Mem, pt: Option<NonNull<PageTable>>) -> u32 {
    match view() {
        0 => m.fast_u32(pt, SCHEDULER_GLOBAL),
        private => private,
    }
}

/// [`scheduler_global`] for the ports that read through [`Pages`].
#[inline]
pub fn scheduler_global_pages(m: &dyn Mem, pg: &Pages) -> u32 {
    match view() {
        0 => pg.u32(m, SCHEDULER_GLOBAL),
        private => private,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_is_per_thread_and_cleared_by_zero() {
        assert_eq!(view(), 0);
        assert_eq!(set_view(0x1234_0000), 0);
        assert_eq!(view(), 0x1234_0000);
        std::thread::spawn(|| assert_eq!(view(), 0)).join().unwrap();
        assert_eq!(set_view(0), 0x1234_0000);
        assert_eq!(view(), 0);
    }
}

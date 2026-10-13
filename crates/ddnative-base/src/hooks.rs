//! The runtime policy behind the lifted code's slow paths (L74).
//!
//! Cargo rebuilds every crate that depends on this one whenever any of its
//! source changes, inline or not, and the 32 lifted shards depend on it.
//! So this crate holds only what the shards must compile against: the
//! machine state, the instruction semantics and the inline fast paths
//! (memory accesses, flags, x87 instructions, the override bit test). What
//! the slow paths do (the call dispatch cache, preemption, where a guest's
//! x87 state lives while it is not in the FPU) is runtime policy in
//! crates/ddnative-host, installed here once before lifted code runs.
//! Changing it rebuilds ddnative-host and the runtime, not the shards.
use crate::{
    mem::Mem,
    x86::Cpu,
    x87::X87,
};

/// The slow paths, as plain function pointers: a call through one is an
/// indirect call on a path that was already out of line.
pub struct Hooks {
    /// [`crate::x86::call_lifted`]: a lifted call to `target` (the stack
    /// push is done).
    pub call_lifted: fn(&mut dyn Mem, &mut Cpu, u32),
    /// The id of the x87 state live on this thread, when the TEB slot of
    /// [`crate::x87::LIVE_SLOT`] is not (yet) available.
    pub x87_live: fn() -> u64,
    /// Make the state this thread's live FPU state, displacing another.
    pub x87_acquire: fn(&mut X87),
    /// The FSAVE image of a state held by a host thread (`home != 0`),
    /// instruction and data pointers zero. Changes nothing.
    pub x87_image: fn(&X87) -> [u8; 108],
    /// Forget the host thread's copy of a state (`home != 0`) because its
    /// image is about to be replaced; sets `home` to 0.
    pub x87_release: fn(&mut X87),
    /// Displace this thread's live x87 state, if any (see
    /// [`crate::x87::park`]).
    pub x87_park: fn(),
}

static HOOKS: std::sync::OnceLock<Hooks> = std::sync::OnceLock::new();

/// Install the hooks; the first installation wins. Returns whether these
/// were installed.
pub fn install(hooks: Hooks) -> bool {
    HOOKS.set(hooks).is_ok()
}

/// Whether hooks are installed.
pub fn installed() -> bool {
    HOOKS.get().is_some()
}

/// The installed hooks.
#[inline]
pub fn get() -> &'static Hooks {
    match HOOKS.get() {
        Some(h) => h,
        None => missing(),
    }
}

/// The installed hooks, if any.
#[inline]
pub fn try_get() -> Option<&'static Hooks> {
    HOOKS.get()
}

#[cold]
#[inline(never)]
fn missing() -> ! {
    panic!("lifted code ran before its runtime hooks were installed: call ddnative_host::install() first")
}

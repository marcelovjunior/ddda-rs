//! The state of one running guest, shared by all of its threads
//! (docs/RUNTIME.md, "Platform services and guest threads").
//!
//! Each guest thread is a host thread with its own `Cpu` and Rust call
//! stack. Exactly one of them runs guest code at a time: it holds the
//! guest lock ([`crate::threads`]), and hands it over only inside runtime
//! services (waits, sleeps, yields), never in the middle of translated
//! code. The game's atomics are imported Interlocked* calls (the lifter
//! emits no `lock`-prefixed instructions), so every guest-visible
//! interleaving point is a service call, as on a single-core machine.
//!
//! All runtime state lives in one [`Runtime`] per session, reached through
//! the host thread's current-runtime pointer. Its parts are [`GuestCell`]s:
//! a `RefCell` that is shared between host threads, which is sound because
//! only the guest-lock holder touches it. A host thread that never entered
//! a session (unit tests calling services directly) gets a fresh runtime
//! of its own, so tests stay isolated from each other.
use std::cell::RefCell;
use std::sync::Arc;

/// Runtime state shared by the guest's threads, accessed only by the
/// holder of the guest lock.
pub struct GuestCell<T>(RefCell<T>);

// SAFETY: a GuestCell is only accessed by the host thread holding the
// guest lock (crate::threads), whose mutex hand-over orders the accesses;
// the RefCell then catches reentrant mutable borrows as usual.
unsafe impl<T: Send> Sync for GuestCell<T> {}

impl<T: Default> Default for GuestCell<T> {
    fn default() -> Self {
        GuestCell(RefCell::new(T::default()))
    }
}

impl<T> GuestCell<T> {
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        f(&mut self.0.borrow_mut())
    }
}

#[derive(Default)]
pub struct Runtime {
    pub(crate) movies: GuestCell<crate::movies::Movies>,
    pub(crate) input: GuestCell<crate::input::Input>,
    pub(crate) audio: GuestCell<crate::audio::Audio>,
    pub(crate) lock: crate::threads::GuestLock,
    pub(crate) kernel: GuestCell<crate::threads::Kernel>,
    /// The address space and dispatcher new guest threads run on.
    pub(crate) machine: GuestCell<Option<crate::threads::Machine>>,
    pub(crate) state: GuestCell<crate::services::State>,
    pub(crate) files: GuestCell<crate::files::Files>,
    pub(crate) steam: GuestCell<crate::steam::Steam>,
    pub(crate) desktop: GuestCell<crate::user32::Desktop>,
    pub(crate) d3d: GuestCell<crate::d3d9dev::Objects>,
    /// The display the adapter answers from (crate::d3d9::display).
    pub(crate) display: GuestCell<Option<crate::d3d9::Display>>,
    pub(crate) graphics: GuestCell<crate::d3d9draw::Graphics>,
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Runtime>>> = const { RefCell::new(None) };
}

/// Make `rt` this host thread's runtime (a new session, or a guest thread
/// starting).
pub fn enter(rt: Arc<Runtime>) {
    CURRENT.with(|c| *c.borrow_mut() = Some(rt));
}

/// This host thread's runtime (a fresh one if it never entered one).
pub fn current() -> Arc<Runtime> {
    CURRENT.with(|c| c.borrow_mut().get_or_insert_with(|| Arc::new(Runtime::default())).clone())
}

/// Run `f` with this thread's runtime (the pointer is not borrowed while
/// `f` runs, so `f` may reenter).
pub(crate) fn with<R>(f: impl FnOnce(&Runtime) -> R) -> R {
    f(&current())
}

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
    pub(crate) input_poll: Arc<crate::input::PollState>,
    pub(crate) audio: GuestCell<crate::audio::Audio>,
    pub(crate) lock: crate::threads::GuestLock,
    pub(crate) kernel: GuestCell<crate::threads::Kernel>,
    /// The address space and dispatcher new guest threads run on.
    pub(crate) machine: GuestCell<Option<crate::threads::Machine>>,
    pub(crate) state: GuestCell<crate::services::State>,
    pub(crate) files: GuestCell<crate::files::Files>,
    pub(crate) steam: GuestCell<crate::steam::Steam>,
    pub(crate) desktop: GuestCell<crate::user32::Desktop>,
    /// behind their own lock, not the guest lock (L93: crate::d3d9dev::D3dCell)
    pub(crate) d3d: crate::d3d9dev::D3dCell<crate::d3d9dev::Objects>,
    /// whether `d3d` holds destructions deferred by a method without the
    /// guest lock (L101: the D3D lock's per-call check reads only this)
    pub(crate) d3d_deferred: std::sync::atomic::AtomicBool,
    /// The live queries' state by guest address (crate::d3d9dev::QueryCell)
    pub(crate) queries: std::sync::Mutex<crate::fasthash::FxMap<u32, std::sync::Arc<crate::d3d9dev::QueryCell>>>,
    /// The display the adapter answers from (crate::d3d9::display).
    pub(crate) display: GuestCell<Option<crate::d3d9::Display>>,
    /// The graphics backend, behind its own lock (not a GuestCell): draws
    /// and Presents run their backend work without the guest lock in
    /// parallel mode (crate::d3d9draw::backend_unlocked, L64).
    pub(crate) graphics: std::sync::Mutex<crate::d3d9draw::Graphics>,
}

/// This host thread's runtime. A runtime replaced by [`enter`] while a
/// `with` on this thread still uses it is kept in `retired` until the
/// outermost `with` returns.
struct Current {
    current: RefCell<Option<Arc<Runtime>>>,
    retired: RefCell<Vec<Arc<Runtime>>>,
}

/// [`with`]'s fast path: the runtime `current` holds (L101: an Arc clone
/// per access was a tenth of a D3D call), the pins on this thread, and
/// whether `retired` holds any. Apart from [`Current`] (L144): a
/// thread-local with no destructor is a plain TLS access, one with a
/// destructor checks its registration on every access, and a D3D call
/// pins three times.
struct Pin {
    ptr: std::cell::Cell<*const Runtime>,
    depth: std::cell::Cell<u32>,
    retired: std::cell::Cell<bool>,
}

impl Drop for Current {
    fn drop(&mut self) {
        // the thread is ending: a pin from here on asks for CURRENT (and
        // fails) instead of using the runtime this held
        let _ = PIN.try_with(|p| p.ptr.set(std::ptr::null()));
    }
}

thread_local! {
    static CURRENT: Current = const {
        Current { current: RefCell::new(None), retired: RefCell::new(Vec::new()) }
    };
    static PIN: Pin = const {
        Pin {
            ptr: std::cell::Cell::new(std::ptr::null()),
            depth: std::cell::Cell::new(0),
            retired: std::cell::Cell::new(false),
        }
    };
}

/// Make `rt` this host thread's runtime (a new session, or a guest thread
/// starting).
pub fn enter(rt: Arc<Runtime>) {
    CURRENT.with(|c| {
        PIN.with(|p| p.ptr.set(Arc::as_ptr(&rt)));
        let old = c.current.borrow_mut().replace(rt);
        if let Some(old) = old.filter(|_| PIN.with(|p| p.depth.get()) > 0) {
            c.retired.borrow_mut().push(old);
            PIN.with(|p| p.retired.set(true));
        }
    });
}

/// This host thread's runtime (a fresh one if it never entered one).
pub fn current() -> Arc<Runtime> {
    CURRENT.with(|c| {
        if let Some(rt) = c.current.borrow().as_ref() {
            return rt.clone();
        }
        let rt = Arc::new(Runtime::default());
        PIN.with(|p| p.ptr.set(Arc::as_ptr(&rt)));
        *c.current.borrow_mut() = Some(rt.clone());
        rt
    })
}

/// This thread's runtime, held until the guard drops (no reference count:
/// [`enter`] keeps a replaced runtime alive while a guard exists).
pub(crate) struct Pinned {
    ptr: *const Runtime,
}

#[inline]
pub(crate) fn pin() -> Pinned {
    let ptr = PIN.with(|p| {
        if p.ptr.get().is_null() {
            drop(current());
        }
        p.depth.set(p.depth.get() + 1);
        p.ptr.get()
    });
    Pinned { ptr }
}

impl std::ops::Deref for Pinned {
    type Target = Runtime;
    fn deref(&self) -> &Runtime {
        // SAFETY: the Arc in CURRENT.current, or one `enter` moved to
        // `retired` while a guard exists; either lives until this one drops
        unsafe { &*self.ptr }
    }
}

impl Drop for Pinned {
    #[inline]
    fn drop(&mut self) {
        let last = PIN.with(|p| {
            p.depth.set(p.depth.get() - 1);
            p.depth.get() == 0 && p.retired.replace(false)
        });
        if last {
            release_retired();
        }
    }
}

/// The runtimes `enter` replaced while pinned, once the last pin is gone.
#[cold]
#[inline(never)]
fn release_retired() {
    // (at thread exit the runtime has already gone with CURRENT)
    let _ = CURRENT.try_with(|c| {
        let retired = std::mem::take(&mut *c.retired.borrow_mut());
        drop(retired);
    });
}

/// Run `f` with this thread's runtime (the pointer is not borrowed while
/// `f` runs, so `f` may reenter, also [`enter`]).
pub(crate) fn with<R>(f: impl FnOnce(&Runtime) -> R) -> R {
    f(&pin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_helpers_avoid_retains_and_keep_replaced_runtimes_alive() {
        let original = current();
        let a = Arc::new(Runtime::default());
        let b = Arc::new(Runtime::default());
        enter(a.clone());
        let retained = Arc::strong_count(&a);
        with(|outer| {
            assert_eq!(Arc::strong_count(&a), retained);
            with(|inner| assert!(std::ptr::eq(inner, outer)));
            enter(b.clone());
            with(|changed| assert!(std::ptr::eq(changed, b.as_ref())));
            assert!(std::ptr::eq(outer, a.as_ref()));
            assert_eq!(Arc::strong_count(&a), retained);
        });
        assert_eq!(Arc::strong_count(&a), 1);
        assert_eq!(PIN.with(|p| p.depth.get()), 0);
        CURRENT.with(|c| assert!(c.retired.borrow().is_empty()));
        enter(original);
    }

    #[test]
    fn nested_pins_release_retired_runtimes_on_unwind() {
        let original = current();
        let a = Arc::new(Runtime::default());
        let b = Arc::new(Runtime::default());
        enter(a.clone());
        with(|outer| {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                with(|_| { enter(b.clone()); panic!("nested service"); });
            }));
            assert!(result.is_err());
            assert!(std::ptr::eq(outer, a.as_ref()));
            with(|rt| assert!(std::ptr::eq(rt, b.as_ref())));
            assert_eq!(PIN.with(|p| p.depth.get()), 1);
        });
        assert_eq!(Arc::strong_count(&a), 1);
        assert_eq!(PIN.with(|p| p.depth.get()), 0);
        CURRENT.with(|c| assert!(c.retired.borrow().is_empty()));
        enter(original);
    }
}

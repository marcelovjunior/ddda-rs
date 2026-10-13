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
//! Shared state lives in one [`Runtime`] per session, reached through the
//! host thread's cached pointer. [`GuestCell`] fields require the guest
//! service lock; graphics and synchronization have their own locks/atomics.
//! Last-error and TLS values are thread-owned. A host thread that never
//! entered a session gets a fresh runtime, keeping direct-service tests
//! isolated. Parallel guest execution is described in [`crate::threads`].
use std::cell::{Cell, RefCell};
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
    pub(crate) tls: crate::win32::TlsSlots,
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

/// This host thread's runtime. Local.runtime caches the Arc pointer for
/// [`with`]'s fast path (L101: an Arc clone per access was a tenth of a D3D
/// call). A runtime replaced by [`enter`] while a `with` on this thread
/// still uses it is kept in `retired` until the outermost `with` returns.
struct Current {
    current: RefCell<Option<Arc<Runtime>>>,
    retired: RefCell<Vec<Arc<Runtime>>>,
    contexts: RefCell<std::collections::HashMap<(u64, u32), Box<ThreadContext>>>,
}

/// One const-initialized TLS access for the hot runtime bookkeeping. Only
/// the cold path enters the destructor-bearing CURRENT owner. Pointers refer
/// to stable boxes owned until host-thread exit; they never cross threads.
pub(crate) struct Local {
    current: Cell<*const Current>,
    runtime: Cell<*const Runtime>,
    pin_depth: Cell<u32>,
    retired_pending: Cell<bool>,
    /// Prevent preemption while a callback may retain runtime borrows.
    pub(crate) service_depth: Cell<u32>,
    pub(crate) guest_tid: Cell<u32>,
    pub(crate) d3d_key: Cell<u64>,
    pub(crate) d3d_held: Cell<u32>,
    context_key: Cell<(u64, u32)>,
    context: Cell<*const ThreadContext>,
}

pub(crate) struct ThreadContext {
    pub(crate) last_error: Cell<u32>,
    pub(crate) tls: [Cell<(u64, u32)>; 1088],
}

thread_local! {
    static LOCAL: Local = const { Local {
        current: Cell::new(std::ptr::null()), runtime: Cell::new(std::ptr::null()),
        pin_depth: Cell::new(0), retired_pending: Cell::new(false), service_depth: Cell::new(0),
        guest_tid: Cell::new(crate::services::MAIN_THREAD_ID),
        d3d_key: Cell::new(0), d3d_held: Cell::new(0),
        context_key: Cell::new((0, 0)), context: Cell::new(std::ptr::null()),
    } };
    static CURRENT: Box<Current> = Box::new(Current {
        current: RefCell::new(None), retired: RefCell::new(Vec::new()),
        contexts: RefCell::new(std::collections::HashMap::new()),
    });
}

impl Drop for Current {
    fn drop(&mut self) {
        let _ = LOCAL.try_with(|l| {
            l.current.set(std::ptr::null());
            l.runtime.set(std::ptr::null());
            l.context.set(std::ptr::null());
        });
    }
}

#[inline]
pub(crate) fn local<R>(f: impl FnOnce(&Local) -> R) -> R { LOCAL.with(f) }

#[inline]
fn current_record(l: &Local) -> &Current {
    let mut p = l.current.get();
    if p.is_null() {
        p = CURRENT.with(|c| &**c as *const Current);
        l.current.set(p);
    }
    // SAFETY: CURRENT owns a stable Box on this host thread and clears
    // this pointer before dropping it. CURRENT refuses access after exit.
    unsafe { &*p }
}

#[cfg(test)]
fn with_current<R>(f: impl FnOnce(&Current) -> R) -> R {
    local(|l| f(current_record(l)))
}

/// One hot TLS access supplies both the pinned runtime and this guest
/// thread's values. No RefCell borrow or Arc/Rc retain on a cache hit.
pub(crate) fn with_thread<R>(f: impl FnOnce(&Runtime, &ThreadContext) -> R) -> R {
    local(|l| {
        let rt = pin_local(l);
        let key = (rt.lock.identity(), l.guest_tid.get());
        if l.context_key.get() != key || l.context.get().is_null() {
            let mut contexts = current_record(l).contexts.borrow_mut();
            let p = &**contexts.entry(key).or_insert_with(|| Box::new(ThreadContext {
                last_error: Cell::new(0), tls: [const { Cell::new((0, 0)) }; 1088],
            })) as *const ThreadContext;
            l.context.set(p);
            l.context_key.set(key);
        }
        // SAFETY: boxes are retained in CURRENT until thread exit, including
        // runtime switches and reentrant calls; fields use Cell, never &mut.
        f(&rt, unsafe { &*l.context.get() })
    })
}

pub(crate) fn thread_context<R>(f: impl FnOnce(&ThreadContext) -> R) -> R {
    with_thread(|_, c| f(c))
}

pub(crate) fn last_error() -> u32 { thread_context(|c| c.last_error.get()) }
pub(crate) fn set_last_error(value: u32) { thread_context(|c| c.last_error.set(value)); }

/// Make `rt` this host thread's runtime (a new session, or a guest thread
/// starting).
pub fn enter(rt: Arc<Runtime>) {
    local(|l| {
        let c = current_record(l);
        l.runtime.set(Arc::as_ptr(&rt));
        let old = c.current.borrow_mut().replace(rt);
        if let Some(old) = old.filter(|_| l.pin_depth.get() > 0) {
            c.retired.borrow_mut().push(old);
            l.retired_pending.set(true);
        }
    });
}

/// This host thread's runtime (a fresh one if it never entered one).
pub fn current() -> Arc<Runtime> {
    local(|l| {
        let c = current_record(l);
        if let Some(rt) = c.current.borrow().as_ref() { return rt.clone(); }
        let rt = Arc::new(Runtime::default());
        l.runtime.set(Arc::as_ptr(&rt));
        *c.current.borrow_mut() = Some(rt.clone());
        rt
    })
}

/// This thread's runtime, held until the guard drops (no reference count:
/// [`enter`] keeps a replaced runtime alive while a guard exists).
pub(crate) struct Pinned {
    ptr: *const Runtime,
    local: *const Local,
}

#[inline]
fn pin_local(l: &Local) -> Pinned {
    if l.runtime.get().is_null() { drop(current()); }
    l.pin_depth.set(l.pin_depth.get() + 1);
    Pinned { ptr: l.runtime.get(), local: l }
}

#[inline]
pub(crate) fn pin() -> Pinned { local(pin_local) }

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
        // SAFETY: Pinned is !Send/!Sync (raw pointers), and cannot outlive
        // CURRENT through this module's scoped uses. Retired owners are only
        // released when the final pin ends, including unwinding/reentry.
        let l = unsafe { &*self.local };
        l.pin_depth.set(l.pin_depth.get() - 1);
        if l.pin_depth.get() == 0 && l.retired_pending.replace(false) {
            let retired = std::mem::take(&mut *current_record(l).retired.borrow_mut());
            drop(retired);
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
    fn cached_context_survives_reentry_and_thread_exit_releases_owners() {
        let original = current();
        let a = Arc::new(Runtime::default());
        let b = Arc::new(Runtime::default());
        enter(a.clone());
        with_thread(|outer_rt, outer_context| {
            outer_context.last_error.set(123);
            enter(b.clone());
            with_thread(|inner_rt, inner_context| {
                assert!(std::ptr::eq(inner_rt, &*b));
                assert!(!std::ptr::eq(inner_context, outer_context));
                inner_context.last_error.set(456);
            });
            assert!(std::ptr::eq(outer_rt, &*a));
            assert_eq!(outer_context.last_error.get(), 123);
            enter(a.clone());
            thread_context(|again| assert!(std::ptr::eq(again, outer_context)));
        });
        enter(original);
        let weak = std::thread::spawn(|| {
            let rt = current();
            set_last_error(789); // also instantiate the cached context box
            Arc::downgrade(&rt)
        }).join().unwrap();
        assert!(weak.upgrade().is_none(), "the cache must not leak a runtime at host-thread exit");
    }

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
        with_current(|c| { assert_eq!(local(|l| l.pin_depth.get()), 0); assert!(c.retired.borrow().is_empty()); });
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
            local(|l| assert_eq!(l.pin_depth.get(), 1));
        });
        assert_eq!(Arc::strong_count(&a), 1);
        with_current(|c| { assert_eq!(local(|l| l.pin_depth.get()), 0); assert!(c.retired.borrow().is_empty()); });
        enter(original);
    }
}

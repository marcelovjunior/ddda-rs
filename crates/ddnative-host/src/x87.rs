//! Where a guest's x87 state lives between instructions (L62, L62b): the
//! live/park policy behind ddnative-base's `x87` hooks. The model is in
//! crates/ddnative-base/src/x87.rs's module documentation: one state per
//! host thread is live in the FPU, others wait in the thread's spill
//! table, and [`park`] gives the FPU back to host code.
use ddnative_base::x87::{live_id, LIVE_SLOT, NONE, UNASSIGNED, X87};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// The id of the X87 whose state is in this thread's FPU (where the
    /// TEB slot is unavailable).
    static LIVE: Cell<u64> = const { Cell::new(NONE) };
    /// States displaced from this thread's FPU, by X87 id.
    static SPILLED: RefCell<HashMap<u64, [u8; 108]>> = RefCell::new(HashMap::new());
    /// This thread's own control word, restored by [`park`].
    static HOST_CW: Cell<Option<u16>> = const { Cell::new(None) };
    static TOKEN: u64 = NEXT_ID.fetch_add(1, Ordering::Relaxed);
}

/// The live id where the TEB slot is unavailable (ddnative-base's
/// `x87_live` hook).
pub(crate) fn live_fallback() -> u64 {
    LIVE.with(Cell::get)
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn set_live(v: u64) {
    let mut slot = LIVE_SLOT.load(Ordering::Relaxed);
    if slot == u32::MAX {
        #[link(name = "kernel32")]
        extern "system" {
            fn TlsAlloc() -> u32;
        }
        // SAFETY: plain Win32 call; one slot for the process, never freed
        let new = unsafe { TlsAlloc() };
        let new = if new < 64 { new } else { 64 }; // 64+: the thread_local fallback
        slot = match LIVE_SLOT.compare_exchange(u32::MAX, new, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => new,
            Err(won) => won,
        };
    }
    if slot >= 64 {
        LIVE.with(|l| l.set(v));
        return;
    }
    // SAFETY: TEB TlsSlots[slot] of the current thread, a slot this
    // process allocated (TlsAlloc)
    unsafe { std::arch::asm!("mov qword ptr gs:[{off}], {v}", v = in(reg) v, off = in(reg) 0x1480 + 8 * slot as u64,
                             options(nostack, preserves_flags)) }
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn set_live(v: u64) {
    let _ = &LIVE_SLOT;
    LIVE.with(|l| l.set(v))
}

fn token() -> u64 {
    TOKEN.with(|t| *t)
}

/// Displace this thread's live guest x87 state (if any) into the spill
/// table and give the FPU back to host code (FNINIT state with the
/// thread's own control word). The owner reloads it on its next
/// instruction.
pub fn park() {
    let live = live_id();
    if live == NONE {
        return;
    }
    let mut image = [0u8; 108];
    fnsave_host(&mut image);
    image[12..28].fill(0);
    SPILLED.with(|s| s.borrow_mut().insert(live, image));
    set_live(NONE);
    if let Some(cw) = HOST_CW.with(Cell::get) {
        fldcw_host(cw);
    }
}

/// The image of a state held by this thread (live or spilled).
pub(crate) fn image(x: &X87) -> [u8; 108] {
    assert_eq!(x.home, token(), "x87 state used on another host thread than the one it is live on");
    let mut image = if live_id() == x.id {
        let mut image = [0u8; 108];
        fnsave_host(&mut image);
        frstor_host(&image);
        image
    } else {
        SPILLED.with(|s| *s.borrow().get(&x.id).expect("spilled x87 state"))
    };
    image[12..28].fill(0);
    image
}

/// Forget this thread's copy of the state (its image is being replaced).
pub(crate) fn release(x: &mut X87) {
    assert_eq!(x.home, token(), "x87 state used on another host thread than the one it is live on");
    if live_id() == x.id {
        set_live(NONE);
    } else {
        SPILLED.with(|s| s.borrow_mut().remove(&x.id));
    }
    x.home = 0;
}

/// Make the state the thread's live FPU state (displacing another).
pub(crate) fn acquire(x: &mut X87) {
    let me = token();
    if x.id == UNASSIGNED {
        x.id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    }
    let image = if x.home == 0 {
        x.state
    } else {
        assert_eq!(x.home, me, "x87 state used on another host thread than the one it is live on");
        SPILLED.with(|s| s.borrow_mut().remove(&x.id)).expect("spilled x87 state")
    };
    // Unmasked exceptions are unsupported (a guest trap must not trap Rust).
    assert_eq!(image[0] & 0x3f, 0x3f, "unmasked guest x87 exceptions");
    if HOST_CW.with(Cell::get).is_none() {
        HOST_CW.with(|c| c.set(Some(fnstcw_host())));
    }
    park();
    frstor_host(&image);
    set_live(x.id);
    x.home = me;
}

#[cfg(target_arch = "x86_64")]
fn fnsave_host(image: &mut [u8; 108]) {
    unsafe { std::arch::asm!("fnsave [{}]", in(reg) image.as_mut_ptr()) }
}

#[cfg(target_arch = "x86_64")]
fn frstor_host(image: &[u8; 108]) {
    unsafe { std::arch::asm!("frstor [{}]", in(reg) image.as_ptr()) }
}

#[cfg(target_arch = "x86_64")]
fn fldcw_host(cw: u16) {
    unsafe { std::arch::asm!("fldcw [{}]", in(reg) &cw) }
}

#[cfg(target_arch = "x86_64")]
fn fnstcw_host() -> u16 {
    let mut cw = 0u16;
    unsafe { std::arch::asm!("fnstcw [{}]", in(reg) &mut cw) }
    cw
}

#[cfg(not(target_arch = "x86_64"))]
fn fnsave_host(_: &mut [u8; 108]) { panic!("exact lifted x87 requires an x86-64 host") }
#[cfg(not(target_arch = "x86_64"))]
fn frstor_host(_: &[u8; 108]) { panic!("exact lifted x87 requires an x86-64 host") }
#[cfg(not(target_arch = "x86_64"))]
fn fldcw_host(_: u16) { panic!("exact lifted x87 requires an x86-64 host") }
#[cfg(not(target_arch = "x86_64"))]
fn fnstcw_host() -> u16 { panic!("exact lifted x87 requires an x86-64 host") }

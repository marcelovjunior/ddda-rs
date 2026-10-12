//! Machine-level helpers for lifted functions (research/native/lift.py):
//! exact x86 flags from the last flag-setting operation, and scalar SSE
//! operations with the original operand order.

/// The last flag-setting operation: kind, operands, result, width in bits.
#[derive(Debug, Clone, Copy)]
pub struct Flags {
    pub kind: u8, // 1 add, 2 sub, 3 logic, 4 inc, 5 dec, 6 shift-zs, 7 comiss; 12 raw EFLAGS
    pub a: u32,
    pub b: u32,
    pub r: u32,
    pub bits: u32,
    pub cf_in: bool,
}

pub const ADD: u8 = 1;
pub const SUB: u8 = 2;
pub const LOGIC: u8 = 3;
pub const INC: u8 = 4;
pub const DEC: u8 = 5;
pub const SHIFT: u8 = 6;
pub const COMISS: u8 = 7;
pub const SHL: u8 = 8;
pub const SHR: u8 = 9;
pub const SAR: u8 = 10;
pub const IMUL: u8 = 11;
pub const RAW: u8 = 12;
pub const PARTIAL: u8 = 13;

#[path = "integer.rs"]
mod integer;
pub use integer::*;
#[cfg(target_arch = "x86_64")]
#[path = "sse2.rs"]
mod sse2;
#[cfg(target_arch = "x86_64")]
pub use sse2::*;

impl Default for Flags {
    fn default() -> Self { Self::new(RAW, 0, 0, 2, 32) }
}

/// The machine state a lifted function starts from (cdecl/thiscall/fastcall
/// alike: everything is in registers and the stack in memory).
#[derive(Debug, Clone, Copy, Default)]
pub struct Entry {
    pub eax: u32, pub ecx: u32, pub edx: u32, pub ebx: u32,
    pub esp: u32, pub ebp: u32, pub esi: u32, pub edi: u32,
    pub xmm0: u32,
}

/// The machine state a lifted function returns with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit {
    pub eax: u32, pub ecx: u32, pub edx: u32, pub ebx: u32,
    pub esp: u32, pub ebp: u32, pub esi: u32, pub edi: u32,
    pub xmm0: u32,
    /// x87 top of stack (single-precision bits) if the stack is not empty.
    pub st0: Option<u32>,
}

/// The whole machine state a lifted function runs on. Lifted functions
/// take it by `&mut` so a lifted `call` hands the callee exactly what the
/// original would see (every register, all of xmm0-7, flags, x87 stack)
/// and the caller continues from whatever the callee leaves.
#[derive(Debug, Clone, Default)]
pub struct Cpu {
    pub eax: u32, pub ecx: u32, pub edx: u32, pub ebx: u32,
    pub esp: u32, pub ebp: u32, pub esi: u32, pub edi: u32,
    pub xmm: [[u32; 4]; 8],
    pub fl: Flags,
    pub df: bool,
    /// Guest thread's FS segment base (TEB); other supported segments are flat.
    pub fs_base: u32,
    /// Segment selectors, read by `mov r/m, sreg` (L22).
    pub seg: Segments,
    /// EFLAGS bits outside `fl` and `df` that POPFD can change at CPL3:
    /// NT, AC and ID (L22; IF and reserved bit 1 read as set).
    pub sys_flags: u32,
    /// The guest's MXCSR while control is outside lifted code (L22). Lifted
    /// code runs with it live in the host MXCSR (crate::sse2), so the SSE
    /// instructions round and set sticky flags exactly as on hardware; the
    /// runtime saves it here at service boundaries and loads it on entry.
    pub mxcsr: Mxcsr,
    /// Full x87 state, including 80-bit stack values and control/status words.
    pub st: crate::x87::X87,
}

impl From<Entry> for Cpu {
    fn from(r: Entry) -> Cpu {
        let mut xmm = [[0u32; 4]; 8];
        xmm[0][0] = r.xmm0;
        Cpu { eax: r.eax, ecx: r.ecx, edx: r.edx, ebx: r.ebx, esp: r.esp, ebp: r.ebp, esi: r.esi, edi: r.edi,
              xmm, fl: Flags::default(), df: false, fs_base: 0, seg: Segments::default(), sys_flags: 0,
              mxcsr: Mxcsr::default(),
              st: Default::default() }
    }
}

impl Cpu {
    pub fn exit(&self) -> Exit {
        Exit { eax: self.eax, ecx: self.ecx, edx: self.edx, ebx: self.ebx, esp: self.esp, ebp: self.ebp,
               esi: self.esi, edi: self.edi, xmm0: self.xmm[0][0], st0: self.st.top_f32() }
    }
}

/// A guest MXCSR value; the power-on and Windows thread default is 0x1F80
/// (all exceptions masked, round to nearest; Intel SDM vol. 1, 10.2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mxcsr(pub u32);

impl Default for Mxcsr {
    fn default() -> Self {
        Mxcsr(crate::x86::MXCSR_DEFAULT)
    }
}

/// The segment selectors of a 32-bit Windows thread (WOW64): CS 0x23, FS
/// 0x53 (its base is `Cpu::fs_base`), the others 0x2B. Measured on Windows
/// 11 26200 with research/native/probe_machine.rs (L22).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segments {
    pub es: u16, pub cs: u16, pub ss: u16, pub ds: u16, pub fs: u16, pub gs: u16,
}

impl Default for Segments {
    fn default() -> Self {
        Segments { es: 0x2b, cs: 0x23, ss: 0x2b, ds: 0x2b, fs: 0x53, gs: 0x2b }
    }
}

/// Where a lifted function's call to code that is not lifted goes
/// (`call_ext`): a direct call to an untranslated function, any indirect
/// call (vtables, function pointers, imports) or a tail jump out of it.
/// The handler runs with the return address already pushed and must leave
/// the state as the callee's `ret` would. Tests install a replay of the
/// calls a vector recorded; the game installs its dispatcher.
pub type ExtCall = dyn Fn(&mut dyn crate::mem::Mem, &mut Cpu, u32);

/// A lifted function and the immutable resolver installed by the facade.
/// Direct cross-shard calls must never use the external/mock call handler.
pub type LiftFn = fn(&mut dyn crate::mem::Mem, &mut Cpu);
pub type LiftLookup = fn(u32) -> Option<LiftFn>;
static LIFTED: std::sync::OnceLock<LiftLookup> = std::sync::OnceLock::new();

pub fn set_lifted_lookup(lookup: LiftLookup) {
    let installed = LIFTED.get_or_init(|| lookup);
    assert!(std::ptr::fn_addr_eq(*installed, lookup), "a different lifted program is already installed");
}

/// Ordinary lifted call: the caller already performed any stack push.
/// Copy the function pointer before calling, so nested calls are reentrant.
#[inline]
pub fn call_lifted(m: &mut dyn crate::mem::Mem, cpu: &mut Cpu, target: u32) {
    if PREEMPT.load(std::sync::atomic::Ordering::Relaxed) {
        preempt_point();
    }
    let f = lifted_target(target);
    f(m, cpu);
}

/// Set by the runtime while another guest thread waits for the guest lock
/// (serial mode). Lifted calls are then preemption points: without them a
/// thread whose loops make no service call (the game's Vorbis decoder)
/// holds the lock for seconds and starves the main loop. The real OS may
/// switch threads at any instruction, so a call boundary is a legal point.
pub static PREEMPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PREEMPT_HOOK: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// The runtime's preemption check (it yields when the time slice is used).
pub fn set_preempt_hook(hook: fn()) {
    let _ = PREEMPT_HOOK.set(hook);
}

#[cold]
#[inline(never)]
fn preempt_point() {
    thread_local! {
        static CALLS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }
    // the hook reads the clock: once per 1,024 lifted calls is plenty
    let due = CALLS.with(|c| {
        let n = c.get().wrapping_add(1);
        c.set(n);
        n % 1024 == 0
    });
    if due {
        if let Some(hook) = PREEMPT_HOOK.get() {
            hook();
        }
    }
}

/// Slots of the per-thread cache of resolved lifted targets.
const RESOLVED_SLOTS: usize = 4096;

thread_local! {
    /// Lifted targets already resolved on this thread, direct-mapped by
    /// address: a cross-shard call resolves its target once instead of
    /// searching the shard's table every time. The program's lookup never
    /// changes once installed (set_lifted_lookup), so entries never go stale.
    static RESOLVED: [std::cell::Cell<(u32, Option<LiftFn>)>; RESOLVED_SLOTS] =
        const { [const { std::cell::Cell::new((0, None)) }; RESOLVED_SLOTS] };
}

/// The lifted function at `target` (what `call_lifted` calls).
#[inline]
pub fn lifted_target(target: u32) -> LiftFn {
    // functions are mostly 16-byte aligned: drop those bits, fold the high ones in
    let slot = ((target >> 4) ^ (target >> 16)) as usize & (RESOLVED_SLOTS - 1);
    let hit = RESOLVED.with(|r| match r[slot].get() {
        (t, Some(f)) if t == target => Some(f),
        _ => None,
    });
    hit.unwrap_or_else(|| {
        let f = lifted_target_uncached(target);
        RESOLVED.with(|r| r[slot].set((target, Some(f))));
        f
    })
}

/// The lifted function at `target`, from the program's lookup (no cache).
pub fn lifted_target_uncached(target: u32) -> LiftFn {
    let lookup = LIFTED.get().expect("initialize the lifted program before entering a raw shard");
    lookup(target).unwrap_or_else(|| panic!("missing lifted target {target:#010x}"))
}

thread_local! {
    static EXT: std::cell::RefCell<Option<std::rc::Rc<ExtCall>>> = std::cell::RefCell::new(None);
}

/// Install the handler for `call_ext` on this thread; returns the old one.
pub fn set_ext_calls(f: Option<Box<ExtCall>>) -> Option<std::rc::Rc<ExtCall>> {
    EXT.with(|e| std::mem::replace(&mut *e.borrow_mut(), f.map(std::rc::Rc::from)))
}

/// A call from lifted code to `target`, which is not lifted. The handler is
/// cloned out of the thread-local slot (a reference count), not taken, so a
/// handler that dispatches into lifted code which calls `call_ext` again
/// (nested calls, callbacks, recursion) finds it installed, and a panic
/// inside the handler leaves it in place.
#[inline]
pub fn call_ext(m: &mut dyn crate::mem::Mem, cpu: &mut Cpu, target: u32) {
    let f = EXT.with(|e| e.borrow().clone())
        .unwrap_or_else(|| panic!("call to {target:#010x}: no handler for untranslated code"));
    f(m, cpu, target);
}

impl Flags {
    #[inline]
    fn defined(&self, bit: u32) {
        assert!(self.kind != PARTIAL || self.a & bit != 0, "read of undefined x86 flag {bit:#x}");
        assert!(self.kind != IMUL || bit & 0x801 != 0, "read of undefined IMUL flag {bit:#x}");
    }
    #[inline]
    pub fn af(&self) -> bool {
        self.defined(0x10);
        if matches!(self.kind, RAW | PARTIAL) { return self.r & 0x10 != 0; }
        matches!(self.kind, ADD | SUB | INC | DEC) && (self.a ^ self.b ^ self.r) & 0x10 != 0
    }
    #[inline]
    pub fn lahf(&self) -> u8 {
        2 | self.cf() as u8 | ((self.pf() as u8) << 2) | ((self.af() as u8) << 4)
            | ((self.zf() as u8) << 6) | ((self.sf() as u8) << 7)
    }
    #[inline]
    pub fn sahf(&mut self, ah: u8) {
        let (old, valid) = self.snapshot();
        *self = Self::partial((ah as u32 & 0xd5) | (old & 0x800), 0xd5 | (valid & 0x800));
    }
    /// Binary lifted ALU operations. The operands are evaluated by the
    /// caller before this helper; the destination store follows it. Width
    /// is 8, 16 or 32, as in the literal instruction templates in lift.py.
    /// Keep even flags that the current function does not read: a callee
    /// (or caller) can observe the full machine state.
    #[inline]
    pub fn alu_add(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = a.wrapping_add(b) & mask;
        *self = Self::new(ADD, a, b, r, bits);
        r
    }
    #[inline]
    pub fn alu_sub(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = a.wrapping_sub(b) & mask;
        *self = Self::new(SUB, a, b, r, bits);
        r
    }
    #[inline]
    pub fn alu_and(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = (a & b) & mask;
        *self = Self::new(LOGIC, a, b, r, bits);
        r
    }
    #[inline]
    pub fn alu_or(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = (a | b) & mask;
        *self = Self::new(LOGIC, a, b, r, bits);
        r
    }
    #[inline]
    pub fn alu_xor(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = (a ^ b) & mask;
        *self = Self::new(LOGIC, a, b, r, bits);
        r
    }
    #[inline]
    pub fn new(kind: u8, a: u32, b: u32, r: u32, bits: u32) -> Self {
        Flags { kind, a, b, r, bits, cf_in: false }
    }
    /// imul (2/3-operand, 32-bit): CF = OF = the signed product does not fit.
    #[inline]
    fn imul_overflow(&self) -> bool {
        (self.a as i32 as i64) * (self.b as i32 as i64) != self.r as i32 as i64
    }
    #[inline]
    fn mask(&self) -> u32 { if self.bits == 32 { u32::MAX } else { (1 << self.bits) - 1 } }
    #[inline]
    fn sign(&self, v: u32) -> bool { (v >> (self.bits - 1)) & 1 != 0 }
    #[inline]
    pub fn zf(&self) -> bool {
        self.defined(0x40);
        if matches!(self.kind, RAW | PARTIAL) { return self.r & 0x40 != 0; }
        if self.kind == COMISS { return self.r != 1 && self.r != 2 } // r: 0 equal, 1 greater, 2 less, 3 unordered
        self.r & self.mask() == 0
    }
    #[inline]
    pub fn sf(&self) -> bool { self.defined(0x80); if matches!(self.kind, RAW | PARTIAL) { self.r & 0x80 != 0 } else { self.kind != COMISS && self.sign(self.r) } }
    #[inline]
    pub fn cf(&self) -> bool {
        self.defined(1);
        let m = self.mask();
        match self.kind {
            RAW | PARTIAL => self.r & 1 != 0,
            ADD => (self.a & m) as u64 + (self.b & m) as u64 > m as u64,
            SUB => (self.a & m) < (self.b & m),
            INC | DEC => self.cf_in,
            COMISS => self.r == 2 || self.r == 3,
            SHL => (self.a >> (self.bits - self.b)) & 1 != 0 && self.b <= self.bits,
            SHR => self.b <= self.bits && (self.a >> (self.b - 1)) & 1 != 0,
            SAR => if self.b >= self.bits { self.sign(self.a) } else { ((((self.a << (32 - self.bits)) as i32) >> (32 - self.bits)) >> (self.b - 1)) & 1 != 0 },
            IMUL => self.imul_overflow(),
            _ => false, // logic
        }
    }
    #[inline]
    pub fn of(&self) -> bool {
        self.defined(0x800);
        if matches!(self.kind, RAW | PARTIAL) { return self.r & 0x800 != 0; }
        let (a, b, r) = (self.sign(self.a), self.sign(self.b), self.sign(self.r));
        match self.kind {
            ADD => a == b && r != a,
            SUB => a != b && r != a,
            INC => !a && r,
            DEC => a && !r,
            SHL => self.b == 1 && (r != self.cf()),
            SHR => self.b == 1 && a,
            IMUL => self.imul_overflow(),
            _ => false, // logic, sar
        }
    }
    #[inline]
    pub fn pf(&self) -> bool {
        self.defined(4);
        if matches!(self.kind, RAW | PARTIAL) { return self.r & 4 != 0; }
        if self.kind == COMISS { return self.r == 3 }
        (self.r & 0xFF).count_ones() % 2 == 0
    }
    /// Condition codes as in `jcc`/`setcc` (the mnemonic's suffix).
    #[inline]
    pub fn cond(&self, cc: &str) -> bool {
        match cc {
            "e" | "z" => self.zf(), "ne" | "nz" => !self.zf(),
            "b" | "c" | "nae" => self.cf(), "ae" | "nb" | "nc" => !self.cf(),
            "be" | "na" => self.cf() || self.zf(), "a" | "nbe" => !self.cf() && !self.zf(),
            "s" => self.sf(), "ns" => !self.sf(),
            "l" | "nge" => self.sf() != self.of(), "ge" | "nl" => self.sf() == self.of(),
            "le" | "ng" => self.zf() || self.sf() != self.of(), "g" | "nle" => !self.zf() && self.sf() == self.of(),
            "o" => self.of(), "no" => !self.of(),
            "p" | "pe" => self.pf(), "np" | "po" => !self.pf(),
            _ => panic!("condition {cc}"),
        }
    }
}

/// `comiss`/`ucomiss` a, b: 0 equal, 1 a > b, 2 a < b, 3 unordered.
#[inline]
pub fn comiss(a: u32, b: u32) -> Flags {
    let (x, y) = (f32::from_bits(a), f32::from_bits(b));
    let r = if x.is_nan() || y.is_nan() { 3 } else if x > y { 1 } else if x < y { 2 } else { 0 };
    Flags { kind: COMISS, a, b, r, bits: 32, cf_in: false }
}

#[cfg(target_arch = "x86_64")]
mod sse {
    //! Scalar SSE as the original instructions in non-pure asm (L22): they
    //! run under the guest MXCSR live in the host register (crate::sse2), so
    //! rounding and the sticky flags are the hardware's, and the compiler
    //! can neither fold nor drop them (an intrinsic may be).
    use std::arch::asm;
    macro_rules! binary {
        ($($op:ident),*) => {$(
            /// `op a, b` with a as the destination: the original's operand order.
            #[inline]
            pub fn $op(a: u32, b: u32) -> u32 {
                let r: u32;
                unsafe {
                    asm!("movd {x}, {a:e}", "movd {y}, {b:e}", concat!(stringify!($op), " {x}, {y}"), "movd {r:e}, {x}",
                         a = in(reg) a, b = in(reg) b, r = lateout(reg) r, x = out(xmm_reg) _, y = out(xmm_reg) _,
                         options(nostack, preserves_flags));
                }
                r
            }
        )*};
    }
    binary!(addss, subss, mulss, divss, minss, maxss);
    #[inline]
    pub fn sqrtss(b: u32) -> u32 {
        let r: u32;
        unsafe {
            asm!("movd {x}, {b:e}", "sqrtss {x}, {x}", "movd {r:e}, {x}",
                 b = in(reg) b, r = lateout(reg) r, x = out(xmm_reg) _, options(nostack, preserves_flags));
        }
        r
    }
    /// cvttss2si (truncation; out of range gives 0x80000000).
    #[inline]
    pub fn cvttss2si(a: u32) -> u32 {
        let r: u32;
        unsafe {
            asm!("movd {x}, {a:e}", "cvttss2si {r:e}, {x}",
                 a = in(reg) a, r = lateout(reg) r, x = out(xmm_reg) _, options(nostack, preserves_flags));
        }
        r
    }
    /// cvtsi2ss under the live MXCSR rounding mode.
    #[inline]
    pub fn cvtsi2ss(v: u32) -> u32 {
        let r: u32;
        unsafe {
            asm!("xorps {x}, {x}", "cvtsi2ss {x}, {v:e}", "movd {r:e}, {x}",
                 v = in(reg) v, r = lateout(reg) r, x = out(xmm_reg) _, options(nostack, preserves_flags));
        }
        r
    }
}
#[cfg(target_arch = "x86_64")]
pub use sse::*;


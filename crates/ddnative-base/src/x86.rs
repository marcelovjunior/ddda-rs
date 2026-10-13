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
#[path = "private_calls.rs"]
pub mod private_calls;
static LIFTED: std::sync::OnceLock<LiftLookup> = std::sync::OnceLock::new();

pub fn set_lifted_lookup(lookup: LiftLookup) {
    let installed = LIFTED.get_or_init(|| lookup);
    assert!(std::ptr::fn_addr_eq(*installed, lookup), "a different lifted program is already installed");
}

/// Ordinary lifted call: the caller already performed any stack push.
/// What happens on the way to the callee (preemption points, the dispatch
/// cache, the lookup) is the runtime's (crate::hooks, L74). Not inline:
/// the shards (opt-level 0 or 1, where LLVM does not inline it anyway)
/// call this one copy, compiled at the runtime's opt-level, instead of a
/// local unoptimised copy of it and of `OnceLock::get` (hookbench: 29 ns
/// per call inline at opt-level 0).
#[inline(never)]
pub fn call_lifted(m: &mut dyn crate::mem::Mem, cpu: &mut Cpu, target: u32) {
    (crate::hooks::get().call_lifted)(m, cpu, target)
}

/// The lifted function at `target`, from the program's lookup (no cache):
/// an override registered for it first (L60).
pub fn lifted_target_uncached(target: u32) -> LiftFn {
    RESOLVING.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(f) = override_of(target) {
        return f;
    }
    let lookup = LIFTED.get().expect("initialize the lifted program before entering a raw shard");
    lookup(target).unwrap_or_else(|| panic!("missing lifted target {target:#010x}"))
}

// --- overrides (L60) ---------------------------------------------------------
//
// A readable port or a mod replaces the lifted function at an address
// without recompiling the shards: it registers (address, function) once at
// startup, before any lifted target is resolved. Every call reaches it:
// cross-shard calls resolve through lifted_target, in-shard calls and
// platform-model calls through call_local's bit test, the runtime's
// indirect dispatch through the facade's all().

/// The first and last address an override may have (the image's code).
const OVERRIDE_BASE: u32 = 0x0040_0000;
const OVERRIDE_LIMIT: u32 = 0x0200_0000;
/// One bit per 16 bytes of code: an override sets its address's bit. A
/// clear bit is the whole cost of a call that is not overridden; a set bit
/// (an override, or another function in the same 16 bytes) takes the cold
/// path, which looks the address up.
static OVERRIDE_BITS: [std::sync::atomic::AtomicU32; ((OVERRIDE_LIMIT - OVERRIDE_BASE) >> 9) as usize] =
    [const { std::sync::atomic::AtomicU32::new(0) }; ((OVERRIDE_LIMIT - OVERRIDE_BASE) >> 9) as usize];
static OVERRIDES: std::sync::OnceLock<Box<[(u32, LiftFn)]>> = std::sync::OnceLock::new();
/// Set once a lifted target was resolved (and may be cached): overrides
/// must be registered before.
static RESOLVING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Register the overrides, once, before the lifted program runs. A second
/// registration, an address registered twice or outside the image's code,
/// or a registration after a target was resolved panics.
pub fn set_overrides(list: &[(u32, LiftFn)]) {
    assert!(!RESOLVING.load(std::sync::atomic::Ordering::Relaxed),
            "register overrides before any lifted target is resolved");
    let mut v: Vec<(u32, LiftFn)> = list.to_vec();
    v.sort_unstable_by_key(|e| e.0);
    for w in v.windows(2) {
        assert!(w[0].0 != w[1].0, "two overrides for {:#010x}", w[0].0);
    }
    for &(a, _) in &v {
        assert!((OVERRIDE_BASE..OVERRIDE_LIMIT).contains(&a), "override outside the image's code: {a:#010x}");
        let bit = (a - OVERRIDE_BASE) >> 4;
        OVERRIDE_BITS[(bit >> 5) as usize].fetch_or(1 << (bit & 31), std::sync::atomic::Ordering::Relaxed);
    }
    assert!(OVERRIDES.set(v.into_boxed_slice()).is_ok(), "overrides are already registered");
}

/// The override registered for `target`, if any.
pub fn override_of(target: u32) -> Option<LiftFn> {
    let v = OVERRIDES.get()?;
    v.binary_search_by_key(&target, |e| e.0).ok().map(|k| v[k].1)
}

/// Every registered override (address order).
pub fn overrides() -> &'static [(u32, LiftFn)] {
    OVERRIDES.get().map(|v| &v[..]).unwrap_or(&[])
}

/// Whether a call to `target` may be overridden (its 16 bytes hold one).
#[inline(always)]
pub fn may_be_overridden(target: u32) -> bool {
    let bit = target.wrapping_sub(OVERRIDE_BASE) >> 4;
    (bit as usize >> 5) < OVERRIDE_BITS.len()
        && OVERRIDE_BITS[bit as usize >> 5].load(std::sync::atomic::Ordering::Relaxed) & (1 << (bit & 31)) != 0
}

/// A direct call from a shard to `f`, the lifted function (or platform
/// model) at `target`, unless an override is registered there.
#[inline(always)]
pub fn call_local(m: &mut dyn crate::mem::Mem, cpu: &mut Cpu, target: u32, f: LiftFn) {
    if may_be_overridden(target) {
        return call_overridden(m, cpu, target, f);
    }
    f(m, cpu)
}

#[cold]
#[inline(never)]
fn call_overridden(m: &mut dyn crate::mem::Mem, cpu: &mut Cpu, target: u32, f: LiftFn) {
    #[cfg(feature = "private-replay")]
    if private_calls::dispatch(m, cpu, target) { return; }
    override_of(target).unwrap_or(f)(m, cpu)
}

thread_local! {
    static EXT: std::cell::RefCell<Option<std::rc::Rc<ExtCall>>> = std::cell::RefCell::new(None);
    /// The installed handler, for `call_ext`'s fast path: a plain pointer
    /// read instead of a RefCell borrow and a reference-count round trip
    /// per call (L171). Kept valid by EXT and, for a handler replaced while
    /// a call through it is still running, by RETIRED.
    static EXT_PTR: std::cell::Cell<Option<std::ptr::NonNull<ExtCall>>> = const { std::cell::Cell::new(None) };
    /// `call_ext` calls in progress on this thread.
    static EXT_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Handlers replaced while calls were in progress, kept until none is.
    static RETIRED: std::cell::RefCell<Vec<std::rc::Rc<ExtCall>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Install the handler for `call_ext` on this thread; returns the old one.
pub fn set_ext_calls(f: Option<Box<ExtCall>>) -> Option<std::rc::Rc<ExtCall>> {
    let new: Option<std::rc::Rc<ExtCall>> = f.map(std::rc::Rc::from);
    EXT_PTR.with(|p| p.set(new.as_ref().map(|r| std::ptr::NonNull::from(&**r))));
    let old = EXT.with(|e| std::mem::replace(&mut *e.borrow_mut(), new));
    if EXT_DEPTH.with(|d| d.get()) == 0 {
        RETIRED.with(|r| r.borrow_mut().clear());
    } else if let Some(old) = &old {
        // a call through `old` may still be running below us
        RETIRED.with(|r| r.borrow_mut().push(std::rc::Rc::clone(old)));
    }
    old
}

/// Restores the call depth even when the handler unwinds.
struct ExtDepth;
impl Drop for ExtDepth {
    fn drop(&mut self) {
        EXT_DEPTH.with(|d| d.set(d.get() - 1));
    }
}

/// A call from lifted code to `target`, which is not lifted. A handler that
/// dispatches into lifted code which calls `call_ext` again (nested calls,
/// callbacks, recursion) finds itself installed; a handler replaced during
/// a call stays alive until the outermost call returns, and a panic inside
/// the handler leaves it in place.
#[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
pub fn call_ext(m: &mut dyn crate::mem::Mem, cpu: &mut Cpu, target: u32) {
    let f = EXT_PTR.with(|p| p.get())
        .unwrap_or_else(|| panic!("call to {target:#010x}: no handler for untranslated code"));
    EXT_DEPTH.with(|d| d.set(d.get() + 1));
    let _depth = ExtDepth;
    // SAFETY: EXT (or RETIRED while this call is in progress) owns the
    // handler `f` points at: set_ext_calls retires a replaced handler
    // whenever a call is in progress, and frees retired ones only at depth 0.
    unsafe { (*f.as_ptr())(m, cpu, target) }
}

impl Flags {
    #[inline]
    fn defined(&self, bit: u32) {
        if (self.kind == PARTIAL && self.a & bit == 0)
            || (self.kind == IMUL && bit & 0x801 == 0) {
            self.undefined(bit);
        }
    }
    #[cold]
    #[inline(never)]
    fn undefined(&self, bit: u32) -> ! {
        if self.kind == IMUL { panic!("read of undefined IMUL flag {bit:#x}"); }
        panic!("read of undefined x86 flag {bit:#x}");
    }
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn af(&self) -> bool {
        self.defined(0x10);
        if matches!(self.kind, RAW | PARTIAL) { return self.r & 0x10 != 0; }
        matches!(self.kind, ADD | SUB | INC | DEC) && (self.a ^ self.b ^ self.r) & 0x10 != 0
    }
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn lahf(&self) -> u8 {
        2 | self.cf_value() as u8 | ((self.pf_value() as u8) << 2) | ((self.af() as u8) << 4)
            | ((self.zf_value() as u8) << 6) | ((self.sf_value() as u8) << 7)
    }
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn sahf(&mut self, ah: u8) {
        let (old, valid) = self.snapshot();
        *self = Self::partial((ah as u32 & 0xd5) | (old & 0x800), 0xd5 | (valid & 0x800));
    }
    /// Binary lifted ALU operations. The operands are evaluated by the
    /// caller before this helper; the destination store follows it. Width
    /// is 8, 16 or 32, as in the literal instruction templates in lift.py.
    /// Keep even flags that the current function does not read: a callee
    /// (or caller) can observe the full machine state.
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn alu_add(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = a.wrapping_add(b) & mask;
        *self = Self::from_parts(ADD, a, b, r, bits);
        r
    }
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn alu_sub(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = a.wrapping_sub(b) & mask;
        *self = Self::from_parts(SUB, a, b, r, bits);
        r
    }
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn alu_and(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = (a & b) & mask;
        *self = Self::from_parts(LOGIC, a, b, r, bits);
        r
    }
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn alu_or(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = (a | b) & mask;
        *self = Self::from_parts(LOGIC, a, b, r, bits);
        r
    }
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn alu_xor(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = (a ^ b) & mask;
        *self = Self::from_parts(LOGIC, a, b, r, bits);
        r
    }
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn new(kind: u8, a: u32, b: u32, r: u32, bits: u32) -> Self {
        Self::from_parts(kind, a, b, r, bits)
    }
    // Internal construction avoids an out-of-line sret temporary/copy in each
    // ALU helper. The public constructor remains out of line for the shards.
    #[inline]
    fn from_parts(kind: u8, a: u32, b: u32, r: u32, bits: u32) -> Self {
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
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn zf(&self) -> bool { self.zf_value() }
    // Only helper-to-helper composition inlines; shard calls stay out of line.
    #[inline]
    fn zf_value(&self) -> bool {
        self.defined(0x40);
        if matches!(self.kind, RAW | PARTIAL) { return self.r & 0x40 != 0; }
        if self.kind == COMISS { return self.r != 1 && self.r != 2 } // r: 0 equal, 1 greater, 2 less, 3 unordered
        self.r & self.mask() == 0
    }
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn sf(&self) -> bool { self.sf_value() }
    // Only helper-to-helper composition inlines; shard calls stay out of line.
    #[inline]
    fn sf_value(&self) -> bool { self.defined(0x80); if matches!(self.kind, RAW | PARTIAL) { self.r & 0x80 != 0 } else { self.kind != COMISS && self.sign(self.r) } }
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn cf(&self) -> bool { self.cf_value() }
    // Only helper-to-helper composition inlines; shard calls stay out of line.
    #[inline]
    fn cf_value(&self) -> bool {
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
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn of(&self) -> bool { self.of_value() }
    // Only helper-to-helper composition inlines; shard calls stay out of line.
    #[inline]
    fn of_value(&self) -> bool {
        self.defined(0x800);
        if matches!(self.kind, RAW | PARTIAL) { return self.r & 0x800 != 0; }
        let (a, b, r) = (self.sign(self.a), self.sign(self.b), self.sign(self.r));
        match self.kind {
            ADD => a == b && r != a,
            SUB => a != b && r != a,
            INC => !a && r,
            DEC => a && !r,
            SHL => self.b == 1 && (r != self.cf_value()),
            SHR => self.b == 1 && a,
            IMUL => self.imul_overflow(),
            _ => false, // logic, sar
        }
    }
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn pf(&self) -> bool { self.pf_value() }
    // Only helper-to-helper composition inlines; shard calls stay out of line.
    #[inline]
    fn pf_value(&self) -> bool {
        self.defined(4);
        if matches!(self.kind, RAW | PARTIAL) { return self.r & 4 != 0; }
        if self.kind == COMISS { return self.r == 3 }
        (self.r & 0xFF).count_ones() % 2 == 0
    }
    /// Condition codes as in `jcc`/`setcc` (the mnemonic's suffix).
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn cond(&self, cc: &str) -> bool {
        match cc {
            "e" | "z" => self.zf_value(), "ne" | "nz" => !self.zf_value(),
            "b" | "c" | "nae" => self.cf_value(), "ae" | "nb" | "nc" => !self.cf_value(),
            "be" | "na" => self.cf_value() || self.zf_value(), "a" | "nbe" => !self.cf_value() && !self.zf_value(),
            "s" => self.sf_value(), "ns" => !self.sf_value(),
            "l" | "nge" => self.sf_value() != self.of_value(), "ge" | "nl" => self.sf_value() == self.of_value(),
            "le" | "ng" => self.zf_value() || self.sf_value() != self.of_value(), "g" | "nle" => !self.zf_value() && self.sf_value() == self.of_value(),
            "o" => self.of_value(), "no" => !self.of_value(),
            "p" | "pe" => self.pf_value(), "np" | "po" => !self.pf_value(),
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
            #[inline(never)] // L77
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
    #[inline(never)] // L77
    pub fn sqrtss(b: u32) -> u32 {
        let r: u32;
        unsafe {
            asm!("movd {x}, {b:e}", "sqrtss {x}, {x}", "movd {r:e}, {x}",
                 b = in(reg) b, r = lateout(reg) r, x = out(xmm_reg) _, options(nostack, preserves_flags));
        }
        r
    }
    /// cvttss2si (truncation; out of range gives 0x80000000).
    #[inline(never)] // L77
    pub fn cvttss2si(a: u32) -> u32 {
        let r: u32;
        unsafe {
            asm!("movd {x}, {a:e}", "cvttss2si {r:e}, {x}",
                 a = in(reg) a, r = lateout(reg) r, x = out(xmm_reg) _, options(nostack, preserves_flags));
        }
        r
    }
    /// cvtsi2ss under the live MXCSR rounding mode.
    #[inline(never)] // L77
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


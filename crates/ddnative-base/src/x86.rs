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
    /// Full x87 state, including 80-bit stack values and control/status words.
    pub st: crate::x87::X87,
}

impl From<Entry> for Cpu {
    fn from(r: Entry) -> Cpu {
        let mut xmm = [[0u32; 4]; 8];
        xmm[0][0] = r.xmm0;
        Cpu { eax: r.eax, ecx: r.ecx, edx: r.edx, ebx: r.ebx, esp: r.esp, ebp: r.ebp, esi: r.esi, edi: r.edi,
              xmm, fl: Flags::default(), df: false, st: Default::default() }
    }
}

impl Cpu {
    pub fn exit(&self) -> Exit {
        Exit { eax: self.eax, ecx: self.ecx, edx: self.edx, ebx: self.ebx, esp: self.esp, ebp: self.ebp,
               esi: self.esi, edi: self.edi, xmm0: self.xmm[0][0], st0: self.st.top_f32() }
    }
}

/// Where a lifted function's call to code that is not lifted goes
/// (`call_ext`): a direct call to an untranslated function, any indirect
/// call (vtables, function pointers, imports) or a tail jump out of it.
/// The handler runs with the return address already pushed and must leave
/// the state as the callee's `ret` would. Tests install a replay of the
/// calls a vector recorded; the game installs its dispatcher.
pub type ExtCall = dyn FnMut(&mut dyn crate::mem::Mem, &mut Cpu, u32);

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
pub fn call_lifted(m: &mut dyn crate::mem::Mem, cpu: &mut Cpu, target: u32) {
    let lookup = LIFTED.get().expect("initialize the lifted program before entering a raw shard");
    let f = lookup(target).unwrap_or_else(|| panic!("missing lifted target {target:#010x}"));
    f(m, cpu);
}

thread_local! {
    static EXT: std::cell::RefCell<Option<Box<ExtCall>>> = std::cell::RefCell::new(None);
}

/// Install the handler for `call_ext` on this thread; returns the old one.
pub fn set_ext_calls(f: Option<Box<ExtCall>>) -> Option<Box<ExtCall>> {
    EXT.with(|e| std::mem::replace(&mut *e.borrow_mut(), f))
}

/// A call from lifted code to `target`, which is not lifted.
pub fn call_ext(m: &mut dyn crate::mem::Mem, cpu: &mut Cpu, target: u32) {
    let mut f = EXT.with(|e| e.borrow_mut().take())
        .unwrap_or_else(|| panic!("call to {target:#010x}: no handler for untranslated code"));
    f(m, cpu, target);
    EXT.with(|e| *e.borrow_mut() = Some(f));
}

impl Flags {
    fn defined(&self, bit: u32) {
        assert!(self.kind != PARTIAL || self.a & bit != 0, "read of undefined x86 flag {bit:#x}");
        assert!(self.kind != IMUL || bit & 0x801 != 0, "read of undefined IMUL flag {bit:#x}");
    }
    pub fn af(&self) -> bool {
        self.defined(0x10);
        if matches!(self.kind, RAW | PARTIAL) { return self.r & 0x10 != 0; }
        matches!(self.kind, ADD | SUB | INC | DEC) && (self.a ^ self.b ^ self.r) & 0x10 != 0
    }
    pub fn lahf(&self) -> u8 {
        2 | self.cf() as u8 | ((self.pf() as u8) << 2) | ((self.af() as u8) << 4)
            | ((self.zf() as u8) << 6) | ((self.sf() as u8) << 7)
    }
    pub fn sahf(&mut self, ah: u8) {
        let (old, valid) = self.snapshot();
        *self = Self::partial((ah as u32 & 0xd5) | (old & 0x800), 0xd5 | (valid & 0x800));
    }
    /// Binary lifted ALU operations. The operands are evaluated by the
    /// caller before this helper; the destination store follows it. Width
    /// is 8, 16 or 32, as in the literal instruction templates in lift.py.
    /// Keep even flags that the current function does not read: a callee
    /// (or caller) can observe the full machine state.
    pub fn alu_add(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = a.wrapping_add(b) & mask;
        *self = Self::new(ADD, a, b, r, bits);
        r
    }
    pub fn alu_sub(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = a.wrapping_sub(b) & mask;
        *self = Self::new(SUB, a, b, r, bits);
        r
    }
    pub fn alu_and(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = (a & b) & mask;
        *self = Self::new(LOGIC, a, b, r, bits);
        r
    }
    pub fn alu_or(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = (a | b) & mask;
        *self = Self::new(LOGIC, a, b, r, bits);
        r
    }
    pub fn alu_xor(&mut self, a: u32, b: u32, bits: u32) -> u32 {
        let mask = u32::MAX >> (32 - bits);
        let (a, b) = (a & mask, b & mask);
        let r = (a ^ b) & mask;
        *self = Self::new(LOGIC, a, b, r, bits);
        r
    }
    pub fn new(kind: u8, a: u32, b: u32, r: u32, bits: u32) -> Self {
        Flags { kind, a, b, r, bits, cf_in: false }
    }
    /// imul (2/3-operand, 32-bit): CF = OF = the signed product does not fit.
    fn imul_overflow(&self) -> bool {
        (self.a as i32 as i64) * (self.b as i32 as i64) != self.r as i32 as i64
    }
    fn mask(&self) -> u32 { if self.bits == 32 { u32::MAX } else { (1 << self.bits) - 1 } }
    fn sign(&self, v: u32) -> bool { (v >> (self.bits - 1)) & 1 != 0 }
    pub fn zf(&self) -> bool {
        self.defined(0x40);
        if matches!(self.kind, RAW | PARTIAL) { return self.r & 0x40 != 0; }
        if self.kind == COMISS { return self.r != 1 && self.r != 2 } // r: 0 equal, 1 greater, 2 less, 3 unordered
        self.r & self.mask() == 0
    }
    pub fn sf(&self) -> bool { self.defined(0x80); if matches!(self.kind, RAW | PARTIAL) { self.r & 0x80 != 0 } else { self.kind != COMISS && self.sign(self.r) } }
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
    pub fn pf(&self) -> bool {
        self.defined(4);
        if matches!(self.kind, RAW | PARTIAL) { return self.r & 4 != 0; }
        if self.kind == COMISS { return self.r == 3 }
        (self.r & 0xFF).count_ones() % 2 == 0
    }
    /// Condition codes as in `jcc`/`setcc` (the mnemonic's suffix).
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
pub fn comiss(a: u32, b: u32) -> Flags {
    let (x, y) = (f32::from_bits(a), f32::from_bits(b));
    let r = if x.is_nan() || y.is_nan() { 3 } else if x > y { 1 } else if x < y { 2 } else { 0 };
    Flags { kind: COMISS, a, b, r, bits: 32, cf_in: false }
}

#[cfg(target_arch = "x86_64")]
mod sse {
    use std::arch::x86_64::*;
    #[inline]
    fn set(v: u32) -> __m128 { unsafe { _mm_castsi128_ps(_mm_cvtsi32_si128(v as i32)) } }
    #[inline]
    fn get(v: __m128) -> u32 { unsafe { _mm_cvtsi128_si32(_mm_castps_si128(v)) as u32 } }
    /// `op a, b` with a as the destination: the original's operand order.
    pub fn addss(a: u32, b: u32) -> u32 { unsafe { get(_mm_add_ss(set(a), set(b))) } }
    pub fn subss(a: u32, b: u32) -> u32 { unsafe { get(_mm_sub_ss(set(a), set(b))) } }
    pub fn mulss(a: u32, b: u32) -> u32 { unsafe { get(_mm_mul_ss(set(a), set(b))) } }
    pub fn divss(a: u32, b: u32) -> u32 { unsafe { get(_mm_div_ss(set(a), set(b))) } }
    pub fn minss(a: u32, b: u32) -> u32 { unsafe { get(_mm_min_ss(set(a), set(b))) } }
    pub fn maxss(a: u32, b: u32) -> u32 { unsafe { get(_mm_max_ss(set(a), set(b))) } }
    pub fn sqrtss(b: u32) -> u32 { unsafe { get(_mm_sqrt_ss(set(b))) } }
    /// cvttss2si (truncation; out of range gives 0x80000000).
    pub fn cvttss2si(a: u32) -> u32 { unsafe { _mm_cvttss_si32(set(a)) as u32 } }
    /// cvtsi2ss with the current (round-to-nearest) mode.
    pub fn cvtsi2ss(v: u32) -> u32 { unsafe { get(_mm_cvtsi32_ss(_mm_setzero_ps(), v as i32)) } }
}
#[cfg(target_arch = "x86_64")]
pub use sse::*;


//! Machine-level helpers for lifted functions (research/native/lift.py):
//! exact x86 flags from the last flag-setting operation, and scalar SSE
//! operations with the original operand order.

/// The last flag-setting operation: kind, operands, result, width in bits.
#[derive(Debug, Clone, Copy, Default)]
pub struct Flags {
    pub kind: u8, // 0 none, 1 add, 2 sub (cmp), 3 logic (and/or/xor/test), 4 inc, 5 dec, 6 shift-zs, 7 comiss
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
    /// x87 stack as single-precision bits, top last.
    pub st: Vec<u32>,
}

impl From<Entry> for Cpu {
    fn from(r: Entry) -> Cpu {
        let mut xmm = [[0u32; 4]; 8];
        xmm[0][0] = r.xmm0;
        Cpu { eax: r.eax, ecx: r.ecx, edx: r.edx, ebx: r.ebx, esp: r.esp, ebp: r.ebp, esi: r.esi, edi: r.edi,
              xmm, fl: Flags::default(), st: Vec::new() }
    }
}

impl Cpu {
    pub fn exit(&self) -> Exit {
        Exit { eax: self.eax, ecx: self.ecx, edx: self.edx, ebx: self.ebx, esp: self.esp, ebp: self.ebp,
               esi: self.esi, edi: self.edi, xmm0: self.xmm[0][0], st0: self.st.last().copied() }
    }
}

impl Flags {
    pub fn new(kind: u8, a: u32, b: u32, r: u32, bits: u32) -> Self {
        Flags { kind, a, b, r, bits, cf_in: false }
    }
    fn mask(&self) -> u32 { if self.bits == 32 { u32::MAX } else { (1 << self.bits) - 1 } }
    fn sign(&self, v: u32) -> bool { (v >> (self.bits - 1)) & 1 != 0 }
    pub fn zf(&self) -> bool {
        if self.kind == COMISS { return self.r != 1 && self.r != 2 } // r: 0 equal, 1 greater, 2 less, 3 unordered
        self.r & self.mask() == 0
    }
    pub fn sf(&self) -> bool { self.kind != COMISS && self.sign(self.r) }
    pub fn cf(&self) -> bool {
        let m = self.mask();
        match self.kind {
            ADD => (self.a & m) as u64 + (self.b & m) as u64 > m as u64,
            SUB => (self.a & m) < (self.b & m),
            INC | DEC => self.cf_in,
            COMISS => self.r == 2 || self.r == 3,
            SHL => (self.a >> (self.bits - self.b)) & 1 != 0 && self.b <= self.bits,
            SHR => self.b <= self.bits && (self.a >> (self.b - 1)) & 1 != 0,
            SAR => if self.b >= self.bits { self.sign(self.a) } else { ((((self.a << (32 - self.bits)) as i32) >> (32 - self.bits)) >> (self.b - 1)) & 1 != 0 },
            IMUL => panic!("flags after imul are not modelled"),
            _ => false, // logic
        }
    }
    pub fn of(&self) -> bool {
        let (a, b, r) = (self.sign(self.a), self.sign(self.b), self.sign(self.r));
        match self.kind {
            ADD => a == b && r != a,
            SUB => a != b && r != a,
            INC => !a && r,
            DEC => a && !r,
            SHL => self.b == 1 && (r != self.cf()),
            SHR => self.b == 1 && a,
            IMUL => panic!("flags after imul are not modelled"),
            _ => false, // logic, sar
        }
    }
    pub fn pf(&self) -> bool {
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

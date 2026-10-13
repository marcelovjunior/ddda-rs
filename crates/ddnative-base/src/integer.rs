//! IA-32 integer semantics: Intel SDM volume 2, corresponding instructions.
//! Undefined flags are marked unavailable, never guessed from emulator output.
use super::{Cpu, Flags, PARTIAL, RAW};
use crate::mem::Mem;

pub const ARITH_FLAGS: u32 = 0x8d5;
fn mask(bits: u32) -> u32 {
    u32::MAX >> (32 - bits)
}
fn signed(v: u32, bits: u32) -> i64 {
    ((v << (32 - bits)) as i32 >> (32 - bits)) as i64
}
fn zsp(r: u32, bits: u32) -> u32 {
    ((r == 0) as u32 * 0x40)
        | (((r >> (bits - 1)) & 1) * 0x80)
        | (((r as u8).count_ones() % 2 == 0) as u32 * 4)
}
impl Flags {
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn partial(value: u32, defined: u32) -> Self {
        Self::new(PARTIAL, defined, 0, value, 32)
    }
    /// INC/DEC (Intel SDM vol. 2): OF, SF, ZF, AF and PF from the operation,
    /// CF kept. A CF that is undefined (after DIV, MUL or a partial SAHF)
    /// stays undefined instead of being read: the CRT `_qsort` runs
    /// `div ebx; inc eax; cmp eax, 8` (0x012EEBC6), so that CF is never
    /// observed (L46).
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn inc_dec(&mut self, kind: u8, a: u32, r: u32, bits: u32) {
        let cf_defined = self.kind != PARTIAL || self.a & 1 != 0;
        if cf_defined {
            let cf = self.cf();
            *self = Self::new(kind, a, 1, r, bits);
            self.cf_in = cf;
        } else {
            let (value, _) = Self::new(kind, a, 1, r, bits).snapshot();
            *self = Self::partial(value & ARITH_FLAGS & !1, ARITH_FLAGS & !1);
        }
    }
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn snapshot(&self) -> (u32, u32) {
        if self.kind == PARTIAL {
            return (self.r, self.a);
        }
        if self.kind == super::IMUL {
            return (if self.cf() { 0x801 } else { 0 }, 0x801);
        }
        (self.lahf() as u32 | ((self.of() as u32) << 11), ARITH_FLAGS)
    }
    #[inline(never)] // L77: one optimised copy, called from opt-level-0 shards
    pub fn alu_carry(&mut self, a: u32, b: u32, bits: u32, subtract: bool) -> u32 {
        let carry = self.cf() as u64;
        let (a, b) = (a & mask(bits), b & mask(bits));
        let r = if subtract {
            a.wrapping_sub(b).wrapping_sub(carry as u32)
        } else {
            a.wrapping_add(b).wrapping_add(carry as u32)
        } & mask(bits);
        let cf = if subtract {
            (a as u64) < b as u64 + carry
        } else {
            a as u64 + b as u64 + carry > mask(bits) as u64
        };
        let of = (if subtract {
            (a ^ b) & (a ^ r)
        } else {
            !(a ^ b) & (a ^ r)
        }) >> (bits - 1)
            & 1;
        *self = Self::new(
            RAW,
            0,
            0,
            zsp(r, bits) | cf as u32 | ((a ^ b ^ r) & 0x10) | (of << 11),
            32,
        );
        r
    }
    pub fn multiply(&mut self, a: u32, b: u32, bits: u32, is_signed: bool) -> (u32, u32) {
        let (a, b) = (a & mask(bits), b & mask(bits));
        let p = if is_signed {
            (signed(a, bits) * signed(b, bits)) as u64
        } else {
            a as u64 * b as u64
        };
        let lo = p as u32 & mask(bits);
        let hi = (p >> bits) as u32 & mask(bits);
        let overflow = if is_signed {
            p as i64 != signed(lo, bits)
        } else {
            hi != 0
        };
        *self = Self::partial(if overflow { 0x801 } else { 0 }, 0x801);
        (lo, hi)
    }
    pub fn rotate(&mut self, a: u32, count: u32, bits: u32, right: bool) -> u32 {
        let count = count & 31;
        let n = count % bits;
        let a = a & mask(bits);
        if count == 0 {
            return a;
        }
        let r = if n == 0 {
            a
        } else if right {
            (a >> n) | (a << (bits - n))
        } else {
            (a << n) | (a >> (bits - n))
        } & mask(bits);
        let cf = if right { (r >> (bits - 1)) & 1 } else { r & 1 };
        let of = if right {
            ((r >> (bits - 1)) ^ (r >> (bits - 2))) & 1
        } else {
            ((r >> (bits - 1)) ^ cf) & 1
        };
        let (old, valid) = self.snapshot();
        *self = Self::partial(
            (old & !0x801) | cf | (of << 11),
            (valid & !0x801) | 1 | if count == 1 { 0x800 } else { 0 },
        );
        r
    }
    /// RCL/RCR: a (bits + 1)-bit rotate through CF. The masked count is
    /// taken modulo bits + 1 (SDM: tempCOUNT). SF/ZF/AF/PF keep their
    /// values; OF is defined only for a masked count of one (RCR: MSB of
    /// the operand XOR the old CF, before rotating; RCL: MSB of the result
    /// XOR the new CF). A zero count changes nothing.
    pub fn rotate_carry(&mut self, a: u32, count: u32, bits: u32, right: bool) -> u32 {
        let count = count & 31;
        let n = count % (bits + 1);
        let a = a & mask(bits);
        if count == 0 {
            return a;
        }
        let old_cf = self.cf() as u64;
        // CF sits above the operand: a (bits + 1)-bit value.
        let wide = ((old_cf << bits) | a as u64) as u128;
        let width = bits + 1;
        let full = (1u128 << width) - 1;
        let rotated = if n == 0 {
            wide
        } else if right {
            ((wide >> n) | (wide << (width - n))) & full
        } else {
            ((wide << n) | (wide >> (width - n))) & full
        };
        let r = rotated as u32 & mask(bits);
        let cf = (rotated >> bits) as u32 & 1;
        let of = if right {
            ((a >> (bits - 1)) ^ old_cf as u32) & 1
        } else {
            ((r >> (bits - 1)) ^ cf) & 1
        };
        let (old, valid) = self.snapshot();
        *self = Self::partial(
            (old & !0x801) | cf | (of << 11),
            (valid & !0x801) | 1 | if count == 1 { 0x800 } else { 0 },
        );
        r
    }
    pub fn double_shift(&mut self, a: u32, b: u32, count: u32, bits: u32, right: bool) -> u32 {
        let n = count & 31;
        let (a, b) = (a & mask(bits), b & mask(bits));
        if n == 0 {
            return a;
        }
        assert!(n <= bits, "undefined double-shift count");
        let r = if right {
            ((a as u64 >> n) | ((b as u64) << (bits - n))) as u32
        } else {
            (((a as u64) << n) | (b as u64 >> (bits - n))) as u32
        } & mask(bits);
        let cf = if right {
            (a >> (n - 1)) & 1
        } else {
            (a >> (bits - n)) & 1
        };
        let of = ((a ^ r) >> (bits - 1)) & 1;
        *self = Self::partial(
            zsp(r, bits) | cf | (of << 11),
            0xc5 | if n == 1 { 0x800 } else { 0 },
        );
        r
    }
    pub fn bit_scan(&mut self, value: u32, bits: u32, reverse: bool) -> u32 {
        let v = value & mask(bits);
        assert!(v != 0, "undefined bit-scan destination");
        *self = Self::partial(0, 0x40);
        if reverse {
            31 - v.leading_zeros()
        } else {
            v.trailing_zeros()
        }
    }
    pub fn bit_test(&mut self, value: u32, index: u32, bits: u32, change: u32) -> u32 {
        let bit = 1u32 << (index & (bits - 1));
        let (old, valid) = self.snapshot();
        *self = Self::partial((old & 0x40) | ((value & bit != 0) as u32), (valid & 0x40) | 1);
        match change {
            1 => value | bit,
            2 => value & !bit,
            3 => value ^ bit,
            _ => value,
        }
    }
}

/// None corresponds to #DE (zero divisor or quotient overflow).
pub fn divide(lo: u32, hi: u32, d: u32, bits: u32, is_signed: bool) -> Option<(u32, u32)> {
    let d = d & mask(bits);
    if d == 0 {
        return None;
    }
    let n = (((hi & mask(bits)) as u64) << bits) | (lo & mask(bits)) as u64;
    if is_signed {
        let n = ((n << (64 - 2 * bits)) as i64 >> (64 - 2 * bits)) as i128;
        let d = signed(d, bits) as i128;
        let (q, r) = (n / d, n % d);
        if q < -(1i128 << (bits - 1)) || q >= (1i128 << (bits - 1)) {
            return None;
        }
        Some((q as u32 & mask(bits), r as u32 & mask(bits)))
    } else {
        let q = n / d as u64;
        if q > mask(bits) as u64 {
            return None;
        }
        Some((q as u32, (n % d as u64) as u32))
    }
}

/// Literal sequential accesses preserve overlap and DF, including wraparound.
pub fn string(m: &mut dyn Mem, cpu: &mut Cpu, width: u32, copy: bool, repeat: bool) {
    string_from(m, cpu, width, copy, repeat, 0);
}

/// MOVS may override its source segment; the ES destination stays flat.
pub fn string_from(m: &mut dyn Mem, cpu: &mut Cpu, width: u32, copy: bool, repeat: bool, source_base: u32) {
    let mut count = if repeat { cpu.ecx } else { 1 };
    while count != 0 {
        let v = if copy {
            let source = source_base.wrapping_add(cpu.esi);
            match width {
                1 => m.u8(source) as u32,
                2 => m.u16(source) as u32,
                _ => m.u32(source),
            }
        } else {
            cpu.eax
        };
        match width {
            1 => m.set_u8(cpu.edi, v as u8),
            2 => m.set_u16(cpu.edi, v as u16),
            _ => m.set_u32(cpu.edi, v),
        }
        let step = if cpu.df { 0u32.wrapping_sub(width) } else { width };
        if copy {
            cpu.esi = cpu.esi.wrapping_add(step);
        }
        cpu.edi = cpu.edi.wrapping_add(step);
        count -= 1;
        if repeat {
            cpu.ecx = count;
        }
    }
}

/// SCAS: compare AL/AX/EAX with [EDI] (ES flat) like CMP, then step EDI.
/// `repeat`: 0 none, 1 REPE (stop when unequal), 2 REPNE (stop when equal);
/// a repeated form with ECX = 0 does nothing and keeps the flags.
pub fn scan(m: &mut dyn Mem, cpu: &mut Cpu, width: u32, repeat: u8) {
    let mut count = if repeat != 0 { cpu.ecx } else { 1 };
    let bits = width * 8;
    while count != 0 {
        let v = match width {
            1 => m.u8(cpu.edi) as u32,
            2 => m.u16(cpu.edi) as u32,
            _ => m.u32(cpu.edi),
        };
        let a = cpu.eax & mask(bits);
        cpu.fl = Flags::new(super::SUB, a, v, a.wrapping_sub(v) & mask(bits), bits);
        cpu.edi = cpu.edi.wrapping_add(if cpu.df { 0u32.wrapping_sub(width) } else { width });
        count -= 1;
        if repeat != 0 {
            cpu.ecx = count;
            if (repeat == 1) != cpu.fl.zf() {
                break;
            }
        }
    }
}

/// EFLAGS bits besides the arithmetic flags and DF that a POPFD at CPL3
/// changes: NT (14), AC (18), ID (21). IF, IOPL, RF, VIF and VIP keep their
/// values (measured with research/native/probe_machine.rs, L22).
pub const SYSTEM_FLAGS: u32 = 0x4000 | 0x4_0000 | 0x20_0000;

/// The EFLAGS image PUSHFD stores: the arithmetic flags (all must be
/// defined), DF, IF and reserved bit 1 set, and the NT/AC/ID state.
pub fn pushfd_image(cpu: &Cpu) -> u32 {
    let (value, defined) = cpu.fl.snapshot();
    assert_eq!(defined & ARITH_FLAGS, ARITH_FLAGS, "pushfd of undefined x86 flags");
    (value & ARITH_FLAGS) | ((cpu.df as u32) << 10) | 0x202 | (cpu.sys_flags & SYSTEM_FLAGS)
}

/// POPFD at CPL3. TF would single-step (a trap): unsupported.
pub fn popfd(cpu: &mut Cpu, value: u32) {
    assert_eq!(value & 0x100, 0, "popfd sets TF (single-step trap unsupported)");
    cpu.fl = Flags::new(RAW, 0, 0, value & ARITH_FLAGS, 32);
    cpu.df = value & 0x400 != 0;
    cpu.sys_flags = value & SYSTEM_FLAGS;
}

//! L185: the faster ZF/CF/INC/DEC paths of x86::Flags against the previous
//! implementation (copied here verbatim, over the public fields), on every
//! kind of lazy state including the ones that must panic when read.
use ddnative_base::x86::*;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// The implementation before L185.
mod old {
    use super::*;
    fn mask(f: &Flags) -> u32 {
        if f.bits == 32 { u32::MAX } else { (1 << f.bits) - 1 }
    }
    fn sign(f: &Flags, v: u32) -> bool {
        (v >> (f.bits - 1)) & 1 != 0
    }
    fn defined(f: &Flags, bit: u32) {
        if (f.kind == PARTIAL && f.a & bit == 0) || (f.kind == IMUL && bit & 0x801 == 0) {
            panic!("read of undefined x86 flag {bit:#x}");
        }
    }
    fn imul_overflow(f: &Flags) -> bool {
        (f.a as i32 as i64) * (f.b as i32 as i64) != f.r as i32 as i64
    }
    pub fn sf(f: &Flags) -> bool {
        defined(f, 0x80);
        if matches!(f.kind, RAW | PARTIAL) { f.r & 0x80 != 0 } else { f.kind != COMISS && sign(f, f.r) }
    }
    pub fn pf(f: &Flags) -> bool {
        defined(f, 4);
        if matches!(f.kind, RAW | PARTIAL) { return f.r & 4 != 0; }
        if f.kind == COMISS { return f.r == 3 }
        (f.r & 0xFF).count_ones() % 2 == 0
    }
    pub fn of(f: &Flags) -> bool {
        defined(f, 0x800);
        if matches!(f.kind, RAW | PARTIAL) { return f.r & 0x800 != 0; }
        let (a, b, r) = (sign(f, f.a), sign(f, f.b), sign(f, f.r));
        match f.kind {
            ADD => a == b && r != a,
            SUB => a != b && r != a,
            INC => !a && r,
            DEC => a && !r,
            SHL => f.b == 1 && (r != cf(f)),
            SHR => f.b == 1 && a,
            IMUL => imul_overflow(f),
            _ => false,
        }
    }
    pub fn zf(f: &Flags) -> bool {
        defined(f, 0x40);
        if matches!(f.kind, RAW | PARTIAL) {
            return f.r & 0x40 != 0;
        }
        if f.kind == COMISS {
            return f.r != 1 && f.r != 2;
        }
        f.r & mask(f) == 0
    }
    pub fn cf(f: &Flags) -> bool {
        defined(f, 1);
        let m = mask(f);
        match f.kind {
            RAW | PARTIAL => f.r & 1 != 0,
            ADD => (f.a & m) as u64 + (f.b & m) as u64 > m as u64,
            SUB => (f.a & m) < (f.b & m),
            INC | DEC => f.cf_in,
            COMISS => f.r == 2 || f.r == 3,
            SHL => (f.a >> (f.bits - f.b)) & 1 != 0 && f.b <= f.bits,
            SHR => f.b <= f.bits && (f.a >> (f.b - 1)) & 1 != 0,
            SAR => if f.b >= f.bits { sign(f, f.a) } else { ((((f.a << (32 - f.bits)) as i32) >> (32 - f.bits)) >> (f.b - 1)) & 1 != 0 },
            IMUL => imul_overflow(f),
            _ => false,
        }
    }
    /// inc_dec's result state (None: the CF-undefined branch, compared through the new code only).
    pub fn inc_dec(f: &Flags, kind: u8, a: u32, r: u32, bits: u32) -> Option<Flags> {
        let cf_defined = f.kind != PARTIAL || f.a & 1 != 0;
        if cf_defined {
            let cf = cf(f);
            Some(Flags { kind, a, b: 1, r, bits, cf_in: cf })
        } else {
            None
        }
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    fn value(&mut self) -> u32 {
        match self.next() % 6 {
            0 => 0,
            1 => u32::MAX,
            2 => 1 << (self.next() % 32),
            3 => self.next() & 0xff,
            _ => self.next(),
        }
    }
}

fn random_state(rng: &mut Rng) -> Flags {
    let kind = [ADD, SUB, LOGIC, INC, DEC, SHIFT, COMISS, SHL, SHR, SAR, IMUL, RAW, PARTIAL, 0, 14, 255][(rng.next() % 16) as usize];
    let bits = [8, 16, 32, 32, 7][(rng.next() % 5) as usize];
    let (a, b, r) = (rng.value(), rng.value(), rng.value());
    // shifts keep their operand ranges valid for the flag formulas
    let b = if matches!(kind, SHL | SHR | SAR) { 1 + b % bits } else { b };
    // half the ALU states carry the real result
    let m = u32::MAX >> (32 - bits);
    let r = if rng.next() & 1 == 0 {
        match kind {
            ADD | INC => (a & m).wrapping_add(b & m) & m,
            SUB | DEC => (a & m).wrapping_sub(b & m) & m,
            LOGIC => (a & b) & m,
            _ => r,
        }
    } else {
        r
    };
    Flags { kind, a, b, r, bits, cf_in: rng.next() & 1 == 0 }
}

fn same<T: PartialEq + std::fmt::Debug>(old: Result<T, ()>, new: Result<T, ()>, what: &str, f: &Flags) {
    assert_eq!(old, new, "{what}: {f:?}");
}

#[test]
fn zf_and_cf_match_the_previous_implementation() {
    let mut rng = Rng(0x2545f4914f6cdd1d);
    // a read that panics is an expected outcome: keep the test output quiet
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    for _ in 0..400_000 {
        let f = random_state(&mut rng);
        // kind 0, 14 and 255 are not states the helpers build: CF/ZF read them as logic
        let bad_bits = f.bits == 7 && matches!(f.kind, COMISS);
        if bad_bits {
            continue;
        }
        let old_zf = catch_unwind(AssertUnwindSafe(|| old::zf(&f))).map_err(|_| ());
        let new_zf = catch_unwind(AssertUnwindSafe(|| f.zf())).map_err(|_| ());
        same(old_zf, new_zf, "zf", &f);
        let old_cf = catch_unwind(AssertUnwindSafe(|| old::cf(&f))).map_err(|_| ());
        let new_cf = catch_unwind(AssertUnwindSafe(|| f.cf())).map_err(|_| ());
        same(old_cf, new_cf, "cf", &f);
        let old_sf = catch_unwind(AssertUnwindSafe(|| old::sf(&f))).map_err(|_| ());
        let new_sf = catch_unwind(AssertUnwindSafe(|| f.sf())).map_err(|_| ());
        same(old_sf, new_sf, "sf", &f);
        let old_pf = catch_unwind(AssertUnwindSafe(|| old::pf(&f))).map_err(|_| ());
        let new_pf = catch_unwind(AssertUnwindSafe(|| f.pf())).map_err(|_| ());
        same(old_pf, new_pf, "pf", &f);
        let old_of = catch_unwind(AssertUnwindSafe(|| old::of(&f))).map_err(|_| ());
        let new_of = catch_unwind(AssertUnwindSafe(|| f.of())).map_err(|_| ());
        same(old_of, new_of, "of", &f);
    }
    std::panic::set_hook(hook);
}

#[test]
fn inc_dec_matches_the_previous_implementation() {
    let mut rng = Rng(0x9e3779b97f4a7c15);
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    for _ in 0..400_000 {
        let before = random_state(&mut rng);
        if before.bits == 7 {
            continue;
        }
        let kind = if rng.next() & 1 == 0 { INC } else { DEC };
        let bits = [8, 16, 32][(rng.next() % 3) as usize];
        let (a, r) = (rng.value(), rng.value());
        let old = catch_unwind(AssertUnwindSafe(|| old::inc_dec(&before, kind, a, r, bits))).map_err(|_| ());
        let mut after = before;
        let new = catch_unwind(AssertUnwindSafe(|| {
            after.inc_dec(kind, a, r, bits);
            after
        }))
        .map_err(|_| ());
        match (old, new) {
            (Ok(Some(want)), Ok(got)) => {
                assert_eq!((got.kind, got.a, got.b, got.r, got.bits, got.cf_in), (want.kind, want.a, want.b, want.r, want.bits, want.cf_in), "{before:?}");
            }
            (Ok(None), Ok(got)) => assert_eq!(got.kind, PARTIAL, "{before:?}"), // the CF-undefined branch is untouched code
            (Err(()), Err(())) => {}
            (old, new) => panic!("inc_dec on {before:?}: old {old:?} new {new:?}"),
        }
    }
    std::panic::set_hook(hook);
}

#[test]
fn zf_cf_exhaustive_small_alu() {
    // every 8-bit operand pair of every ALU kind, ZF and CF against the old formulas
    for kind in [ADD, SUB, LOGIC] {
        for a in 0..256u32 {
            for b in 0..256u32 {
                let r = match kind {
                    ADD => (a + b) & 0xff,
                    SUB => a.wrapping_sub(b) & 0xff,
                    _ => a & b,
                };
                let f = Flags { kind, a, b, r, bits: 8, cf_in: false };
                assert_eq!(f.zf(), old::zf(&f));
                assert_eq!(f.cf(), old::cf(&f));
                assert_eq!((f.sf(), f.pf(), f.of()), (old::sf(&f), old::pf(&f), old::of(&f)));
            }
        }
    }
}

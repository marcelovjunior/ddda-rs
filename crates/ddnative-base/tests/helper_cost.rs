//! Hot ALU helpers against host x86 instructions, including every condition
//! alias. No recorded game data is needed.
#![cfg(target_arch = "x86_64")]
use ddnative_base::x86::*;
use std::arch::asm;

fn native(kind: u8, mut a: u32, b: u32, bits: u32) -> (u32, u32) {
    let flags: u64;
    macro_rules! op {
        ($op:literal) => {
            match bits {
                8 => unsafe { asm!(concat!($op, " al, cl"), "pushfq", "pop {f}", inout("eax") a, in("ecx") b, f = lateout(reg) flags); },
                16 => unsafe { asm!(concat!($op, " ax, cx"), "pushfq", "pop {f}", inout("eax") a, in("ecx") b, f = lateout(reg) flags); },
                32 => unsafe { asm!(concat!($op, " eax, ecx"), "pushfq", "pop {f}", inout("eax") a, in("ecx") b, f = lateout(reg) flags); },
                _ => unreachable!(),
            }
        }
    }
    match kind { ADD => op!("add"), SUB => op!("sub"), 3 => op!("and"), 4 => op!("or"), 5 => op!("xor"), _ => unreachable!() }
    (a & (u32::MAX >> (32 - bits)), flags as u32)
}

fn conditions(f: &Flags, raw: u32) {
    let (c, p, z, s, o) = (raw & 1 != 0, raw & 4 != 0, raw & 64 != 0, raw & 128 != 0, raw & 2048 != 0);
    for (aliases, expected) in [
        ("e z", z), ("ne nz", !z), ("b c nae", c), ("ae nb nc", !c),
        ("be na", c || z), ("a nbe", !c && !z), ("s", s), ("ns", !s),
        ("l nge", s != o), ("ge nl", s == o), ("le ng", z || s != o),
        ("g nle", !z && s == o), ("o", o), ("no", !o), ("p pe", p), ("np po", !p),
    ] {
        for cc in aliases.split_whitespace() { assert_eq!(f.cond(cc), expected, "{cc}: {f:?}"); }
    }
}

#[test]
fn alu_flags_match_hardware_exhaustive_bytes_and_wide_inputs() {
    for bits in [8, 16, 32] {
        let values: Vec<u32> = if bits == 8 { (0..256).collect() } else {
            let mut v = vec![0, 1, 15, 16, 127, 128, 255, 256, 32767, 32768, 65535, 65536, 0x7fffffff, 0x80000000, u32::MAX];
            let mut seed = 0x132u32;
            for _ in 0..64 { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); v.push(seed); }
            v
        };
        for a in &values { for b in &values { for kind in 1..=5 {
            let mut f = Flags::default();
            let r = match kind { ADD => f.alu_add(*a, *b, bits), SUB => f.alu_sub(*a, *b, bits), 3 => f.alu_and(*a, *b, bits), 4 => f.alu_or(*a, *b, bits), _ => f.alu_xor(*a, *b, bits) };
            let (want, raw) = native(kind, *a, *b, bits);
            assert_eq!(r, want);
            // Logic AF is architecturally undefined; preserve the existing
            // model's false value without comparing that bit to hardware.
            let mask = if kind <= SUB { ARITH_FLAGS } else { ARITH_FLAGS & !0x10 };
            assert_eq!(f.snapshot().0 & mask, raw & mask, "{a:x} {b:x} {bits} {kind}");
            assert_eq!(f.a, *a & (u32::MAX >> (32 - bits)));
            assert_eq!(f.b, *b & (u32::MAX >> (32 - bits)));
            assert_eq!(f.bits, bits);
            assert!(!f.cf_in);
            conditions(&f, raw);
        } } }
    }
}

#[test]
fn raw_comparison_and_partial_conditions_keep_short_circuit_and_traps() {
    for raw in 0..=0xfff { conditions(&Flags::new(RAW, 0, 0, raw, 32), raw); }
    for (r, raw) in [(0, 0x40), (1, 0), (2, 1), (3, 0x45)] {
        conditions(&Flags::new(COMISS, 0, 0, r, 32), raw);
    }
    // BE does not read undefined ZF when CF is true; LE does not read
    // undefined SF/OF when ZF is true. Invalid reads must still panic.
    assert!(Flags::partial(1, 1).cond("be"));
    assert!(Flags::partial(0x40, 0x40).cond("le"));
    assert!(std::panic::catch_unwind(|| Flags::partial(0, 1).cond("be")).is_err());
    assert!(std::panic::catch_unwind(|| Flags::new(IMUL, 2, 3, 6, 32).zf()).is_err());
}

#![cfg(target_arch = "x86_64")]
use ddnative_base::x86::*;
use std::arch::asm;

fn carry_cpu(mut a: u32, b: u32, carry: bool, bits: u32, subtract: bool) -> (u32, u32) {
    let flags: u64;
    macro_rules! op {($op:literal,$args:literal)=>{unsafe {
        asm!("bt {carry}, 0",concat!($op," ",$args),"pushfq","pop {flags}",
             inout("eax") a,in("ecx") b,carry=in(reg) carry as u64,flags=lateout(reg) flags);
    }}}
    match (bits, subtract) {
        (8, false) => op!("adc", "al, cl"),
        (8, true) => op!("sbb", "al, cl"),
        (16, false) => op!("adc", "ax, cx"),
        (16, true) => op!("sbb", "ax, cx"),
        (32, false) => op!("adc", "eax, ecx"),
        (32, true) => op!("sbb", "eax, ecx"),
        _ => unreachable!(),
    }
    (a & (u32::MAX >> (32 - bits)), flags as u32 & ARITH_FLAGS)
}

#[test]
fn carry_borrow_exhaustive_byte_and_wide_boundaries() {
    for bits in [8, 16, 32] {
        let values: Vec<u32> = if bits == 8 {
            (0..256).collect()
        } else {
            vec![
                0,
                1,
                2,
                0xf,
                0x10,
                0x7f,
                0x80,
                0xff,
                0x100,
                0x7fff,
                0x8000,
                0xffff,
                0x10000,
                0x7fffffff,
                0x80000000,
                0xfffffffe,
                u32::MAX,
            ]
        };
        for &a in &values {
            for &b in &values {
                for carry in [false, true] {
                    for sub in [false, true] {
                        let mut f = Flags::new(RAW, 0, 0, carry as u32, 32);
                        let r = f.alu_carry(a, b, bits, sub);
                        assert_eq!(
                            (r, f.snapshot().0 & ARITH_FLAGS),
                            carry_cpu(a, b, carry, bits, sub),
                            "a={a:x} b={b:x} bits={bits} carry={carry} sub={sub}"
                        );
                    }
                }
            }
        }
    }
}

fn multiply_cpu(mut a: u32, b: u32, bits: u32, signed: bool) -> (u32, u32, u32) {
    let mut hi: u32;
    let flags: u64;
    macro_rules! op {($op:literal,$arg:literal)=>{unsafe {
        asm!(concat!($op," ",$arg),"pushfq","pop {flags}",inout("eax") a,in("ecx") b,
             lateout("edx") hi,flags=lateout(reg) flags);
    }}}
    match (bits, signed) {
        (8, false) => op!("mul", "cl"),
        (8, true) => op!("imul", "cl"),
        (16, false) => op!("mul", "cx"),
        (16, true) => op!("imul", "cx"),
        (32, false) => op!("mul", "ecx"),
        (32, true) => op!("imul", "ecx"),
        _ => unreachable!(),
    }
    if bits == 8 {
        hi = a >> 8;
    }
    let mask = u32::MAX >> (32 - bits);
    (a & mask, hi & mask, flags as u32 & 0x801)
}

#[test]
fn products_match_hardware_and_undefined_flags_trap() {
    for bits in [8, 16, 32] {
        let values: Vec<u32> = if bits == 8 {
            (0..256).collect()
        } else {
            vec![0, 1, 2, 0x7fff, 0x8000, 0xffff, 0x7fffffff, 0x80000000, u32::MAX]
        };
        for &a in &values {
            for &b in &values {
                for signed in [false, true] {
                    let mut f = Flags::default();
                    let (lo, hi) = f.multiply(a, b, bits, signed);
                    assert_eq!((lo, hi, f.snapshot().0 & 0x801), multiply_cpu(a, b, bits, signed));
                }
            }
        }
    }
    let mut f = Flags::default();
    f.multiply(0, 0, 32, false);
    assert!(std::panic::catch_unwind(|| f.zf()).is_err());
    f.sahf(0x45);
    assert!(f.cf() && f.zf());
    assert!(!f.of());
    f = Flags::partial(0, 0);
    f.sahf(0);
    assert!(std::panic::catch_unwind(|| f.of()).is_err());
}

#[test]
fn division_boundaries_and_traps() {
    for bits in [8, 16, 32] {
        let mask = u32::MAX >> (32 - bits);
        assert_eq!(divide(mask, 0, 1, bits, false), Some((mask, 0)));
        assert_eq!(divide(0, 1, 1, bits, false), None);
        assert_eq!(divide(0, 0, 0, bits, false), None);
        assert_eq!(divide(0, 1 << (bits - 1), mask, bits, true), None);
        for &(lo, hi, d) in &[
            (7, 0, 3),
            (u32::MAX, u32::MAX, 3),
            (0x80000000, 0, 0x80000000),
            (0x12345678, 0, 123),
        ] {
            for signed in [false, true] {
                if let Some(want) = divide(lo, hi, d, bits, signed) {
                    let mut a = lo;
                    let mut h = hi;
                    if bits == 8 {
                        a = (lo & 255) | ((hi & 255) << 8);
                    }
                    macro_rules! op {($op:literal,$arg:literal)=>{unsafe{asm!(concat!($op," ",$arg),inout("eax") a,inout("edx") h,in("ecx") d);}}}
                    match (bits, signed) {
                        (8, false) => op!("div", "cl"),
                        (8, true) => op!("idiv", "cl"),
                        (16, false) => op!("div", "cx"),
                        (16, true) => op!("idiv", "cx"),
                        (32, false) => op!("div", "ecx"),
                        (32, true) => op!("idiv", "ecx"),
                        _ => unreachable!(),
                    }
                    assert_eq!(
                        want,
                        (a & mask, if bits == 8 { (a >> 8) & 255 } else { h & mask })
                    );
                }
            }
        }
    }
}

#[test]
fn rotations_double_shifts_and_bit_operations() {
    for bits in [16, 32] {
        for a in [0u32, 1, 0x8000, 0x7fffffff, 0x80000000, u32::MAX] {
            for count in 0..32u32 {
                for right in [false, true] {
                    let mut f = Flags::new(RAW, 0, 0, 0x8d5, 32);
                    let got = f.rotate(a, count, bits, right);
                    let mut value = a;
                    let flags: u64;
                    macro_rules! op {($op:literal,$args:literal)=>{unsafe{
                asm!("push {initial}","popfq",concat!($op," ",$args),"pushfq","pop {flags}",
                     inout("eax")value,in("ecx")count,initial=in(reg)0xad7u64,flags=lateout(reg)flags);
            }}}
                    match (bits, right) {
                        (16, false) => op!("rol", "ax, cl"),
                        (16, true) => op!("ror", "ax, cl"),
                        (32, false) => op!("rol", "eax, cl"),
                        (32, true) => op!("ror", "eax, cl"),
                        _ => unreachable!(),
                    }
                    let (v, valid) = f.snapshot();
                    assert_eq!(got, value & (u32::MAX >> (32 - bits)));
                    assert_eq!(v & valid, flags as u32 & valid, "rotate {bits} {count}");
                    if count > bits {
                        continue;
                    }
                    let b = 0xabcdef01u32;
                    let mut f = Flags::new(RAW, 0, 0, 0x8d5, 32);
                    let got = f.double_shift(a, b, count, bits, right);
                    let mut value = a;
                    let flags: u64;
                    macro_rules! op {($op:literal,$args:literal)=>{unsafe{
                asm!("push {initial}","popfq",concat!($op," ",$args),"pushfq","pop {flags}",
                     inout("eax")value,in("edx")b,in("ecx")count,initial=in(reg)0xad7u64,flags=lateout(reg)flags);
            }}}
                    match (bits, right) {
                        (16, false) => op!("shld", "ax, dx, cl"),
                        (16, true) => op!("shrd", "ax, dx, cl"),
                        (32, false) => op!("shld", "eax, edx, cl"),
                        (32, true) => op!("shrd", "eax, edx, cl"),
                        _ => unreachable!(),
                    }
                    let (v, valid) = f.snapshot();
                    assert_eq!(got, value & (u32::MAX >> (32 - bits)));
                    assert_eq!(v & valid, flags as u32 & valid, "double shift {bits} {count}");
                }
            }
        }
    }
    let mut f = Flags::new(RAW, 0, 0, 0x40, 32);
    assert_eq!(f.bit_test(0, 33, 32, 1), 2);
    assert!(!f.cf());
    assert!(f.zf());
    assert_eq!(f.bit_scan(0x8000, 16, true), 15);
    assert!(!f.zf());
}

#[test]
fn sse_lanes_nan_order_rounding_and_host_state() {
    let a = [1f32.to_bits(), 2f32.to_bits(), 3f32.to_bits(), 4f32.to_bits()];
    assert_eq!(
        addps(a, a),
        [2f32.to_bits(), 4f32.to_bits(), 6f32.to_bits(), 8f32.to_bits()]
    );
    assert_eq!(
        minps(
            [0, 0x80000000, 0x7fc00001, 0x7fa00002],
            [0x80000000, 0, 0x7fc00003, 0x7fc00004]
        ),
        [0x80000000, 0, 0x7fc00003, 0x7fc00004]
    );
    assert_eq!(unpcklps(a, [5, 6, 7, 8]), [a[0], 5, a[1], 6]);
    assert_eq!(shuffle_d::<0x1b>([0; 4], a), [a[3], a[2], a[1], a[0]]);
    assert_eq!(
        cvttps2dq(
            [
                1.9f32.to_bits(),
                (-1.9f32).to_bits(),
                f32::INFINITY.to_bits(),
                f32::NAN.to_bits()
            ],
            [0; 4]
        ),
        [0; 4]
    );
    let src = [
        1.9f32.to_bits(),
        (-1.9f32).to_bits(),
        f32::INFINITY.to_bits(),
        f32::NAN.to_bits(),
    ];
    assert_eq!(cvttps2dq([0; 4], src), [1, u32::MAX, 0x80000000, 0x80000000]);
    let mut old = 0u32;
    let changed = 0x3f80u32;
    let mut after = 0u32;
    unsafe {
        asm!("stmxcsr [{old}]","ldmxcsr [{changed}]",old=in(reg)&mut old,changed=in(reg)&changed,options(nostack));
    }
    let r = cvtps2dq([0; 4], [1.5f32.to_bits(); 4]);
    unsafe {
        asm!("stmxcsr [{after}]","ldmxcsr [{old}]",after=in(reg)&mut after,old=in(reg)&old,options(nostack));
    }
    assert_eq!(r, [2; 4]);
    assert_eq!(after, changed);
    let r = cvtss2sd([11, 22, 33, 44], [1.5f32.to_bits(), 55, 66, 77]);
    assert_eq!(&r[2..], &[33, 44]);
    assert_eq!(r[0] as u64 | ((r[1] as u64) << 32), 1.5f64.to_bits());
}

#[test]
fn double_comparison_matches_cpu_without_changing_host_state() {
    use std::arch::x86_64::__m128i;
    let values = [
        0u64,
        1,
        0x8000000000000000,
        1f64.to_bits(),
        (-1f64).to_bits(),
        f64::INFINITY.to_bits(),
        f64::NEG_INFINITY.to_bits(),
        0x7ff8000000000001,
        0xfff0000000000001,
    ];
    for x in values {
        for y in values {
            let a = [x as u32, (x >> 32) as u32, 0, 0];
            let b = [y as u32, (y >> 32) as u32, 0, 0];
            let av: __m128i = unsafe { std::mem::transmute(a) };
            let bv: __m128i = unsafe { std::mem::transmute(b) };
            let mut old = 0u32;
            let default = 0x1f80u32;
            let flags: u64;
            unsafe {
                asm!("stmxcsr [{old}]","ldmxcsr [{default}]","ucomisd {a}, {b}",
            "pushfq","pop {flags}","ldmxcsr [{old}]",a=in(xmm_reg)av,b=in(xmm_reg)bv,
            old=in(reg)&mut old,default=in(reg)&default,flags=out(reg)flags);
            }
            assert_eq!(
                comisd(a, b).snapshot().0 & ARITH_FLAGS,
                flags as u32 & ARITH_FLAGS
            );
        }
    }
}

#[test]
fn string_df_overlap_and_zero_count() {
    use ddnative_base::mem::{Mem, Sparse};
    let mut m = Sparse::default();
    for k in 0..16 {
        m.set_u8(100 + k, k as u8);
    }
    let mut cpu = Cpu::default();
    cpu.esi = 100;
    cpu.edi = 101;
    cpu.ecx = 4;
    string(&mut m, &mut cpu, 1, true, true);
    assert_eq!((0..5).map(|k| m.u8(100 + k)).collect::<Vec<_>>(), vec![0; 5]);
    cpu.esi = 115;
    cpu.edi = 120;
    cpu.ecx = 3;
    cpu.df = true;
    string(&mut m, &mut cpu, 1, true, true);
    assert_eq!((m.u8(120), m.u8(119), m.u8(118)), (15, 14, 13));
    assert_eq!((cpu.esi, cpu.edi, cpu.ecx), (112, 117, 0));
    string(&mut m, &mut cpu, 4, false, true);
    assert_eq!(cpu.edi, 117);
}

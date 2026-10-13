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
    // L22: helpers run under the live (guest) MXCSR: round down, and the
    // inexact conversion sets PE.
    assert_eq!(r, [1; 4]);
    assert_eq!(after, changed | 0x20);
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

fn rotate_carry_cpu(mut a: u32, n: u32, carry: bool, bits: u32, right: bool) -> (u32, u32) {
    let flags: u64;
    macro_rules! op {($op:literal)=>{unsafe {
        asm!("bt {carry}, 0",$op,"pushfq","pop {flags}",
             inout("eax") a,in("ecx") n,carry=in(reg) carry as u64,flags=lateout(reg) flags);
    }}}
    match (bits, right) {
        (8, false) => op!("rcl al, cl"),
        (8, true) => op!("rcr al, cl"),
        (16, false) => op!("rcl ax, cl"),
        (16, true) => op!("rcr ax, cl"),
        (32, false) => op!("rcl eax, cl"),
        (32, true) => op!("rcr eax, cl"),
        _ => unreachable!(),
    }
    (a & (u32::MAX >> (32 - bits)), flags as u32)
}

#[test]
fn rotate_through_carry_matches_cpu() {
    // L22: RCR/RCL (the CRT's 64-bit divide helpers use rcr r32, 1).
    for bits in [8, 16, 32] {
        for a in [0u32, 1, 0x80, 0x81, 0x7fff, 0x8000, 0x8001, 0x7fffffff, 0x80000000, 0xdeadbeef, u32::MAX] {
            for n in 0..40 {
                for carry in [false, true] {
                    for right in [false, true] {
                        let mut f = Flags::new(RAW, 0, 0, carry as u32 | 0x40, 32);
                        let r = f.rotate_carry(a, n, bits, right);
                        let (want, flags) = rotate_carry_cpu(a, n, carry, bits, right);
                        let (got, valid) = f.snapshot();
                        let check = if n & 31 == 1 { 0x801 } else { 1 };
                        assert_eq!(r, want, "a={a:x} n={n} bits={bits} cf={carry} right={right}");
                        assert_eq!(got & check, flags & check, "flags a={a:x} n={n} bits={bits} cf={carry} right={right}");
                        assert_eq!(valid & check, check);
                        assert!(got & 0x40 != 0, "ZF is kept");
                    }
                }
            }
        }
    }
}

#[test]
fn scan_matches_cpu() {
    use ddnative_base::mem::{Mem, Sparse};
    let data: Vec<u8> = b"abcxabcy\0zz".to_vec();
    for (needle, repeat, count, df) in [(b'x', 2u8, 11u32, false), (b'\0', 2, 11, false), (b'q', 2, 11, false),
                                        (b'a', 1, 11, false), (b'a', 2, 0, false), (b'c', 2, 3, true), (b'y', 0, 5, false)] {
        let mut m = Sparse::default();
        for (k, b) in data.iter().enumerate() {
            m.set_u8(0x1000 + k as u32, *b);
        }
        let start = if df { 0x1000 + 6 } else { 0x1000 };
        let mut cpu = Cpu::default();
        cpu.fl = Flags::new(RAW, 0, 0, 0x44, 32);
        cpu.eax = 0x12345600 | needle as u32;
        cpu.edi = start;
        cpu.ecx = count;
        cpu.df = df;
        scan(&mut m, &mut cpu, 1, repeat);
        // the host, over the same bytes
        let base = data.as_ptr() as u64;
        let mut rdi = base + (start - 0x1000) as u64;
        let mut rcx = count as u64;
        let flags: u64;
        unsafe {
            macro_rules! op {($op:literal)=>{asm!("xor {f:e}, {f:e}", "cmp {f:e}, 0", // ZF set as in the model
                 "test {df}, 1", "jz 2f", "std", "2:", $op, "pushfq", "pop {f}", "cld",
                 df=in(reg) df as u64, f=out(reg) flags, inout("rdi") rdi, inout("rcx") rcx, in("al") needle)}}
            match repeat { 0 => op!("scasb"), 1 => op!("repe scasb"), _ => op!("repne scasb") }
        }
        let host_edi = start.wrapping_add((rdi as i64 - (base + (start - 0x1000) as u64) as i64) as u32);
        assert_eq!((cpu.edi, cpu.ecx), (host_edi, if repeat == 0 { count } else { rcx as u32 }), "{needle} {repeat} {count} {df}");
        assert_eq!(cpu.fl.snapshot().0 & ARITH_FLAGS, flags as u32 & ARITH_FLAGS, "{needle} {repeat} {count} {df}");
    }
}

#[test]
fn word_max_min_and_byte_shifts_match_cpu() {
    use std::arch::x86_64::*;
    let a = [0x8000_7fffu32, 0x0001_ffff, 0x1234_5678, 0xffff_0000];
    let b = [0x7fff_8000u32, 0xffff_0001, 0x5678_1234, 0x0000_ffff];
    let (av, bv): (__m128i, __m128i) = unsafe { (std::mem::transmute(a), std::mem::transmute(b)) };
    let host = |v: __m128i| -> [u32; 4] { unsafe { std::mem::transmute(v) } };
    unsafe {
        assert_eq!(pmaxsw(a, b), host(_mm_max_epi16(av, bv)));
        assert_eq!(pminsw(a, b), host(_mm_min_epi16(av, bv)));
        assert_eq!(pslldq::<8>(a), host(_mm_slli_si128::<8>(av)));
        assert_eq!(psrldq::<3>(a), host(_mm_srli_si128::<3>(av)));
        assert_eq!(pslldq::<16>(a), [0; 4]);
    }
}

#[test]
fn added_x87_forms_match_cpu() {
    ddnative_host::install();
    use ddnative_base::x87::X87;
    // fldpi; fld1; fpatan; fstp m64  vs the host
    let mut st = X87::default();
    let mut data = [0u8; 10];
    st.register::<0xebd9>(); // fldpi
    st.register::<0xe8d9>(); // fld1
    st.register::<0xf3d9>(); // fpatan: atan(st1 / st0)
    st.exec::<0x18dd>(&mut data); // fstp m64
    let mut host = 0f64;
    unsafe { asm!("fldpi", "fld1", "fpatan", "fstp qword ptr [{p}]", p = in(reg) &mut host); }
    assert_eq!(f64::from_le_bytes(data[..8].try_into().unwrap()).to_bits(), host.to_bits());
    assert_eq!(st.depth(), 0);
    // fptan pushes 1.0 above tan(x); fxam and fnclex keep the depth
    let mut st = X87::default();
    st.register::<0xebd9>();
    st.register::<0xf2d9>();
    assert_eq!(st.depth(), 2);
    st.register::<0xe5d9>();
    st.register::<0xe2db>();
    assert_eq!(st.status() & 0x3f, 0);
    st.exec::<0x18dd>(&mut data);
    assert_eq!(f64::from_le_bytes(data[..8].try_into().unwrap()), 1.0);
}

#[test]
fn x87_fnsave_frstor_match_cpu() {
    ddnative_host::install();
    // L46: the guest FNSAVE image against the host's own FNSAVE of the same
    // state (control/status/tag words, reserved words, registers), the
    // FNINIT state it leaves, and FRSTOR's tag recomputation. The WOW64
    // selectors and pointer rules come from research/runtime/x87_state_oracle.c.
    use ddnative_base::mem::Image;
    use ddnative_base::x87::X87;
    let mut m = Image { base: 0x1000, bytes: vec![0; 0x200] };
    m.bytes[0x100..0x108].copy_from_slice(&2.5f64.to_le_bytes());
    let mut st = X87::default();
    st.register::<0xe8d9>(); // fld1
    st.register::<0xebd9>(); // fldpi
    st.memory::<0x00dd, 8, false>(&mut m, 0x1100); // fld m64
    st.fnsave(&mut m, 0x1000, 0x012F_1DA8, 0x55c, 0x0019_FF08);
    let mut native = [0u8; 108];
    let mut host = [0u8; 108];
    let value = 2.5f64;
    unsafe {
        asm!("fnsave [{h}]", "fninit", "fld1", "fldpi", "fld qword ptr [{v}]", "fnsave [{n}]", "frstor [{h}]",
             h = in(reg) host.as_mut_ptr(), n = in(reg) native.as_mut_ptr(), v = in(reg) &value);
    }
    let guest = &m.bytes[..108];
    assert_eq!(guest[..12], native[..12], "control, status, tag and reserved words");
    assert_eq!(guest[28..], native[28..], "registers");
    assert_eq!(guest[12..28], [0xa8, 0x1d, 0x2f, 0x01, 0x23, 0x00, 0x5c, 0x05,
                               0x08, 0xff, 0x19, 0x00, 0x2b, 0x00, 0xff, 0xff]);
    assert_eq!((st.control(), st.status(), st.depth()), (0x037f, 0, 0), "FNINIT state");
    // FRSTOR keeps empty tags and recomputes the others (2.0 tagged "zero")
    let mut crafted = [0u8; 108];
    crafted[0..2].copy_from_slice(&0x027fu16.to_le_bytes());
    crafted[4..6].copy_from_slice(&0x3000u16.to_le_bytes()); // top 6
    crafted[8..10].copy_from_slice(&0x4fffu16.to_le_bytes()); // phys6 valid, phys7 zero
    crafted[28 + 7..28 + 10].copy_from_slice(&[0x80, 0xff, 0x3f]); // st0 = 1.0
    crafted[38 + 7..38 + 10].copy_from_slice(&[0x80, 0x00, 0x40]); // st1 = 2.0
    m.bytes[..108].copy_from_slice(&crafted);
    st.frstor(&m, 0x1000);
    assert_eq!((st.control(), st.depth()), (0x027f, 2));
    st.fnsave(&mut m, 0x1000, 0, 0, 0);
    assert_eq!(m.bytes[8..10], 0x0fffu16.to_le_bytes(), "tags recomputed as on the hardware (E5)");
    assert_eq!(m.bytes[28..48], crafted[28..48]);
}

#[test]
fn inc_dec_carry_an_undefined_cf_over() {
    // L46: _qsort 0x012EEBC6 runs `div ebx; inc eax; cmp eax, 8; ja`.
    // DIV leaves the arithmetic flags undefined; INC keeps CF, which the
    // CMP rewrites before any read.
    let mut fl = Flags::partial(0, 0);
    fl.inc_dec(INC, 0xa89, 0xa8a, 32);
    assert_eq!(fl.snapshot(), (0, ARITH_FLAGS & !1)); // 0xA8A: OF/SF/ZF/AF/PF clear, CF undefined
    assert!(!fl.zf() && !fl.sf() && !fl.of());
    fl = Flags::new(SUB, 0xa8a, 8, 0xa82, 32); // cmp eax, 8
    assert!(fl.cond("a"));
    // a defined CF is carried exactly as before (dec keeps CF = 1)
    let mut fl = Flags::new(SUB, 0, 1, u32::MAX, 32);
    fl.inc_dec(DEC, 5, 4, 32);
    assert!(fl.cf() && !fl.zf());
}

#[test]
#[should_panic(expected = "read of undefined x86 flag 0x1")]
fn inc_dec_keep_cf_undefined() {
    let mut fl = Flags::partial(0, 0);
    fl.inc_dec(DEC, 1, 0, 32);
    let _ = fl.cf();
}

#[test]
fn pushfd_popfd_follow_cpl3() {
    // L22: measured in a 32-bit Windows process (research/native/probe_machine.rs):
    // IF and bit 1 read as set; POPFD changes the arithmetic flags, DF, NT, AC, ID only.
    let mut cpu = Cpu::default();
    cpu.fl = Flags::new(RAW, 0, 0, 0x44, 32); // ZF, PF
    assert_eq!(pushfd_image(&cpu), 0x246);
    popfd(&mut cpu, 0x0020_0000 | 0x4_0000 | 0x4000 | 0x400 | 0x3000 | 0x8d5);
    assert_eq!(pushfd_image(&cpu), 0x0020_0000 | 0x4_0000 | 0x4000 | 0x400 | 0x202 | 0x8d5, "IOPL is kept");
    assert!(cpu.df);
    assert_eq!(cpu.fl.snapshot().0 & ARITH_FLAGS, 0x8d5);
    assert_eq!(Cpu::default().seg, Segments { es: 0x2b, cs: 0x23, ss: 0x2b, ds: 0x2b, fs: 0x53, gs: 0x2b });
}

#[test]
#[should_panic(expected = "undefined x86 flags")]
fn pushfd_of_undefined_flags_traps() {
    let mut cpu = Cpu::default();
    cpu.fl = Flags::partial(0, 0x40);
    pushfd_image(&cpu);
}

#[test]
fn machine_answers_cpuid_and_rdtsc() {
    use ddnative_base::platform::{self, Machine};
    struct Fixed;
    impl Machine for Fixed {
        fn cpuid(&self, leaf: u32, subleaf: u32) -> [u32; 4] { [leaf, subleaf, 3, 4] }
        fn rdtsc(&self) -> u64 { 0x1122_3344_5566_7788 }
    }
    platform::set_machine(Some(std::rc::Rc::new(Fixed)));
    let mut cpu = Cpu::default();
    (cpu.eax, cpu.ecx) = (7, 1);
    platform::cpuid(&mut cpu);
    assert_eq!((cpu.eax, cpu.ebx, cpu.ecx, cpu.edx), (7, 1, 3, 4));
    platform::rdtsc(&mut cpu);
    assert_eq!((cpu.eax, cpu.edx), (0x5566_7788, 0x1122_3344));
    platform::set_machine(None);
}

#[test]
fn approximations_are_the_host_instructions() {
    use std::arch::x86_64::*;
    let a = [0x4040_0000u32, 0x3f33_3333, 0x42f6_8000, 0x3a83_126f];
    let b = [0x40b0_0000u32, 7, 8, 9];
    let (av, bv): (__m128, __m128) = unsafe { (std::mem::transmute(a), std::mem::transmute(b)) };
    let host = |v: __m128| -> [u32; 4] { unsafe { std::mem::transmute(v) } };
    unsafe {
        assert_eq!(rcpps(b, a), host(_mm_rcp_ps(av)));
        assert_eq!(rsqrtps(b, a), host(_mm_rsqrt_ps(av)));
        assert_eq!(rcpss(b, a), host(_mm_rcp_ss(_mm_move_ss(bv, av))));
        assert_eq!(rsqrtss(a, b), host(_mm_rsqrt_ss(_mm_move_ss(av, bv))));
    }
}

#[test]
fn guest_mxcsr_is_live_in_the_helpers() {
    // L22: rounding control and sticky flags follow the live MXCSR.
    let one_third = |round: u32| {
        ldmxcsr(MXCSR_DEFAULT | round << 13);
        let r = divps([0x3f80_0000; 4], [0x4040_0000; 4])[0]; // 1/3
        let flags = stmxcsr() & 0x3f;
        ldmxcsr(MXCSR_DEFAULT);
        (r, flags)
    };
    // the scalar helpers follow it as well
    ldmxcsr(MXCSR_DEFAULT | 1 << 13);
    assert_eq!(divss(0x3f80_0000, 0x4040_0000), 0x3eaa_aaaa);
    assert_eq!(cvtsi2ss(16_777_217), 0x4b80_0000); // rounds down to 2^24
    assert_eq!(stmxcsr() & 0x3f, 0x20);
    ldmxcsr(MXCSR_DEFAULT);
    assert_eq!(divss(0x3f80_0000, 0x4040_0000), 0x3eaa_aaab);
    let (nearest, pe) = one_third(0);
    let (down, _) = one_third(1);
    assert_eq!(nearest, 0x3eaa_aaab);
    assert_eq!(down, 0x3eaa_aaaa);
    assert_eq!(pe, 0x20, "precision flag");
    ldmxcsr(MXCSR_DEFAULT);
    let _ = divsd([0, 0x3ff0_0000, 0, 0], [0; 4]); // 1.0 / 0.0
    assert_eq!(stmxcsr() & 0x3f, 0x04, "zero-divide flag");
    ldmxcsr(MXCSR_DEFAULT);
    assert_eq!(stmxcsr(), MXCSR_DEFAULT);
    assert_eq!(Cpu::default().mxcsr, Mxcsr(0x1F80));
}

#[test]
fn hardware_compares_set_flags_like_the_cpu() {
    let qnan = 0x7fc0_0000u32;
    ldmxcsr(MXCSR_DEFAULT);
    let f = comiss_hw::<false>(qnan, 0x3f80_0000);
    assert_eq!(stmxcsr() & 1, 0, "ucomiss: no IE for a quiet NaN");
    assert!(f.zf() && f.pf() && f.cf());
    let f = comiss_hw::<true>(qnan, 0x3f80_0000);
    assert_eq!(stmxcsr() & 1, 1, "comiss: IE for any NaN");
    assert!(f.zf() && f.pf() && f.cf());
    ldmxcsr(MXCSR_DEFAULT);
    assert!(comiss_hw::<true>(0x3f80_0000, 0x4000_0000).cf()); // 1 < 2
    assert!(!comiss_hw::<true>(0x4000_0000, 0x3f80_0000).cf());
    assert!(comisd_hw::<false>([0, 0x3ff0_0000, 0, 0], [0, 0x3ff0_0000, 0, 0]).zf());
    assert_eq!(stmxcsr(), MXCSR_DEFAULT);
}

#[test]
#[should_panic(expected = "unmasked")]
fn ldmxcsr_rejects_unmasked_exceptions() {
    ldmxcsr(0x1F00);
}

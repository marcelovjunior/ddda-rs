//! Unit tests of ddnative's machine helpers. They live here, not in
//! `#[cfg(test)]` modules, because the crate sets `[lib] test = false`: a
//! lib unit-test build would compile the ~100 MB of generated code a second
//! time, in parallel, which needs over 20 GB of memory.

#[cfg(target_arch = "x86_64")]
mod x87_hardware {
    use ddnative::x87_copy;

    /// Ground truth from the CPU itself: the emulator used for the
    /// differential vectors does not model this case (research/native).
    fn hardware(bits: u32) -> u32 {
        let src = bits;
        let mut dst = 0u32;
        unsafe {
            std::arch::asm!("fld dword ptr [{s}]", "fstp dword ptr [{d}]",
                s = in(reg) &src, d = in(reg) &mut dst, out("st(0)") _);
        }
        dst
    }

    #[test]
    fn x87_copy_matches_the_cpu() {
        ddnative_host::install();
        let mut cases = vec![0, 0x8000_0000, 0x3F80_0000, 0x7F80_0000, 0xFF80_0000,
            0x7F80_0001, 0xFFA0_0000, 0x7FBF_FFFF, 0x7FC0_0001, 0xFFC0_0000, 0x0000_0001];
        let mut x = 0x1234_5678u32;
        for _ in 0..2000 {
            x ^= x << 13; x ^= x >> 17; x ^= x << 5;
            cases.push(x);
            cases.push(x | 0x7F80_0000); // NaN/inf space
        }
        for bits in cases {
            assert_eq!(x87_copy(f32::from_bits(bits)).to_bits(), hardware(bits), "{bits:#010x}");
        }
    }
}

mod alu {

    use ddnative::x86::*;

    #[test]
    fn compact_binary_alu_matches_literal_templates() {
        ddnative_host::install();
        let ops: [(u8, fn(&mut Flags, u32, u32, u32) -> u32); 5] = [
            (ADD, Flags::alu_add), (SUB, Flags::alu_sub),
            (LOGIC, Flags::alu_and), (LOGIC, Flags::alu_or), (LOGIC, Flags::alu_xor),
        ];
        for bits in [8, 16, 32] {
            let mask = if bits == 32 { u32::MAX } else { (1 << bits) - 1 };
            // Exhaustive byte operands, plus high bits and signed/width
            // boundaries for every width. No executable-derived data.
            let mut values: Vec<u32> = (0..256).collect();
            values.extend([mask, mask >> 1, (mask >> 1) + 1, mask.wrapping_add(1),
                           0x7fffffff, 0x80000000, u32::MAX]);
            for &input_a in &values {
                for &input_b in &values {
                    let (a, b) = (input_a & mask, input_b & mask);
                    for (op, &(kind, helper)) in ops.iter().enumerate() {
                        let r = match op {
                            0 => a.wrapping_add(b), 1 => a.wrapping_sub(b),
                            2 => a & b, 3 => a | b, 4 => a ^ b, _ => unreachable!(),
                        } & mask;
                        let mut fl = Flags { cf_in: true, ..Flags::default() };
                        assert_eq!(helper(&mut fl, input_a, input_b, bits), r);
                        assert_eq!((fl.kind, fl.a, fl.b, fl.r, fl.bits, fl.cf_in),
                                   (kind, a, b, r, bits, false));
                    }
                }
            }
        }
    }
}
#[cfg(target_arch = "x86_64")]
mod x87_exact {
    use ddnative::x87::X87;
    use ddnative::x86::{Flags, ADD};

    fn load_i64(x: &mut X87, v: i64) {
        let mut b = [0; 10]; b[..8].copy_from_slice(&v.to_le_bytes()); x.exec::<0x28df>(&mut b);
    }
    fn control(x: &mut X87, cw: u16) {
        let mut b = [0; 10]; b[..2].copy_from_slice(&cw.to_le_bytes()); x.exec::<0x28d9>(&mut b);
    }
    fn integer(x: &mut X87) -> i64 {
        let mut b = [0; 10]; x.exec::<0x38df>(&mut b); i64::from_le_bytes(b[..8].try_into().unwrap())
    }
    #[test]
    fn extended_values_and_precision_control() {
        ddnative_host::install();
        for (cw, want) in [(0x037f, 1), (0x027f, 0), (0x007f, 0)] {
            let mut x = X87::default(); control(&mut x, cw);
            load_i64(&mut x, 1i64 << 60); x.register::<0xe8d9>(); // fld1
            x.register::<0xc1de>(); // faddp st(1), st(0)
            load_i64(&mut x, 1i64 << 60); x.register::<0xe9de>(); // fsubp st(1), st(0)
            assert_eq!(integer(&mut x), want);
            assert!(x.is_empty()); assert_eq!(x.control(), cw);
        }
    }
    #[test]
    fn rounding_and_integer_indefinite() {
        ddnative_host::install();
        for (rc, positive, negative) in [(0, 2, -2), (1, 1, -2), (2, 2, -1), (3, 1, -1)] {
            let mut x = X87::default(); control(&mut x, 0x037f | rc << 10);
            x.push(1.5f32.to_bits()); assert_eq!(integer(&mut x), positive);
            x.push((-1.5f32).to_bits()); assert_eq!(integer(&mut x), negative);
        }
        let mut x = X87::default(); x.push(f32::INFINITY.to_bits());
        assert_eq!(integer(&mut x), i64::MIN); assert_ne!(x.status() & 1, 0);
    }
    #[test]
    fn compare_status_top_and_flags() {
        ddnative_host::install();
        assert_eq!(Flags::default().lahf(), 2);
        assert_eq!(Flags::new(ADD, 15, 1, 16, 32).lahf() & 0x10, 0x10);
        for (a, b, condition) in [(1., 2., 0x100), (2., 1., 0), (1., 1., 0x4000)] {
            let mut x = X87::default(); x.push((b as f32).to_bits()); x.push((a as f32).to_bits());
            x.register::<0xd9d8>(); // fcomp st(1)
            assert_eq!(x.status() & 0x4500, condition);
            assert_eq!((x.status() >> 11) & 7, 7);
            let mut f = Flags::new(ADD, 0x7fffffff, 1, 0x80000000, 32);
            f.sahf((x.status() >> 8) as u8);
            assert!(f.of()); assert_eq!(f.cf(), condition & 0x100 != 0);
            assert_eq!(f.zf(), condition & 0x4000 != 0);
            assert_eq!(f.lahf() & 0xd5, (x.status() >> 8) as u8 & 0xd5);
        }
    }
    #[test]
    fn qword_registers_sqrt_and_host_state() {
        ddnative_host::install();
        ddnative::x87::park();
        let mut before = 0u16;
        unsafe { std::arch::asm!("fnstcw [{p}]", p=in(reg) &mut before); }
        let mut x = X87::default();
        let mut data = [0; 10]; data[..8].copy_from_slice(&(1.0f64 + f64::EPSILON).to_bits().to_le_bytes());
        x.exec::<0x00dd>(&mut data); x.register::<0xc0d9>(); // fld st(0)
        let mut out = [0; 10]; x.exec::<0x18dd>(&mut out);
        assert_eq!(data[..8], out[..8]);
        x.push(4f32.to_bits()); x.register::<0xfad9>();
        assert_eq!(x.top_f32(), Some(2f32.to_bits()));
        control(&mut x, 0x0f7f); x.register::<0xc9d9>(); // fxch st(1)
        let guest_top = x.top_extended();
        let guest_depth = x.depth();
        // L62 keeps guest x87 state live between instructions. Host x87
        // work belongs after park(), which restores the host control word;
        // it must not push a host value onto the live guest stack.
        ddnative::x87::park();
        let mut after = 0u16; let mut value = 0u32;
        unsafe { std::arch::asm!("fnstcw [{p}]", "fld1", "fstp dword ptr [{v}]", p=in(reg) &mut after, v=in(reg) &mut value); }
        assert_eq!(before, after); assert_eq!(value, 1f32.to_bits());
        assert_eq!(x.control(), 0x0f7f);
        assert_eq!(x.depth(), guest_depth);
        assert_eq!(x.top_extended(), guest_top);
        x.register::<0xc9d9>(); // reacquire the parked guest, fxch st(1)
        assert_eq!(x.top_f32(), Some(2f32.to_bits()));
        x.register::<0xc9d9>();
        assert_eq!(x.top_extended(), guest_top);
        assert_eq!(x.control(), 0x0f7f);
        ddnative::x87::park();
        unsafe { std::arch::asm!("fnstcw [{p}]", p=in(reg) &mut after); }
        assert_eq!(before, after);
    }
    #[test]
    fn arithmetic_flags_compare() {
        ddnative_host::install();
        let mut x = X87::default(); x.push(2f32.to_bits()); x.push(1f32.to_bits());
        let f = x.compare_flags::<0xf1df>(); // fcomip st(0),st(1)
        assert!(f.cf()); assert!(!f.zf()); assert!(!f.of()); assert!(!f.af());
    }
}

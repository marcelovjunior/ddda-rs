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

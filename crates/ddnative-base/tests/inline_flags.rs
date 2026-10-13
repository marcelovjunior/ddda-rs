use ddnative_base::x86::Flags;

type Alu = fn(&mut Flags, u32, u32, u32) -> u32;

fn check(a: u32, b: u32, bits: u32) {
    let methods: [(Alu, Alu); 5] = [
        (Flags::alu_add, Flags::alu_add_inline),
        (Flags::alu_sub, Flags::alu_sub_inline),
        (Flags::alu_and, Flags::alu_and_inline),
        (Flags::alu_or, Flags::alu_or_inline),
        (Flags::alu_xor, Flags::alu_xor_inline),
    ];
    for (baseline, candidate) in methods {
        let mut old = Flags { kind: 255, a: 123, b: 456, r: 789, bits: 7, cf_in: true };
        let mut new = old;
        assert_eq!(baseline(&mut old, a, b, bits), candidate(&mut new, a, b, bits));
        assert_eq!((old.kind, old.a, old.b, old.r, old.bits, old.cf_in),
                   (new.kind, new.a, new.b, new.r, new.bits, new.cf_in));
        for cc in ["e", "ne", "b", "be", "a", "ae", "l", "le", "g", "ge", "s", "ns", "o", "no", "p", "np"] {
            assert_eq!(old.cond(cc), new.cond(cc));
        }
        assert_eq!(old.lahf(), new.lahf());
    }
}

#[test]
fn exhaustive_byte_operands_with_dirty_upper_bits() {
    for a in 0..256 { for b in 0..256 { check(a | 0xdead_0000, b | 0xbeef_0000, 8); } }
}

#[test]
fn wide_overflow_carry_sign_and_random_operands() {
    let boundaries = [0, 1, 0x7f, 0x80, 0xff, 0x100, 0x7fff, 0x8000,
        0xffff, 0x10000, 0x7fff_ffff, 0x8000_0000, 0xffff_ffff];
    let mut state = 0x9e37_79b9u32;
    for bits in [16, 32] {
        for a in boundaries { for b in boundaries { check(a, b, bits); } }
        for _ in 0..10_000 {
            state ^= state << 13; state ^= state >> 17; state ^= state << 5;
            let a = state;
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            check(a, state, bits);
        }
    }
}

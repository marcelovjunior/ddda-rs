//! L179: the lifted float compares (x86::comiss_hw / comisd_hw) read the
//! hardware's ZF/PF/CF with SETcc; their flags must be what COMISS/UCOMISS
//! define: unordered 1/1/1, less 0/0/1, equal 1/0/0, greater 0/0/0.
use ddnative_base::x86;

fn expect(a: f64, b: f64) -> (bool, bool, bool) {
    if a.is_nan() || b.is_nan() {
        (true, true, true)
    } else if a < b {
        (false, false, true)
    } else if a == b {
        (true, false, false)
    } else {
        (false, false, false)
    }
}

#[test]
fn float_compares_set_the_architectural_flags() {
    let specials: [u32; 12] = [0, 0x8000_0000, 1, 0x8000_0001, 0x3f80_0000, 0xbf80_0000, 0x7f80_0000, 0xff80_0000,
        0x7fc0_0000, 0x7f80_0001, 0x7f7f_ffff, 0x0080_0000];
    let mut seed = 0x9e37_79b9u32;
    let mut values: Vec<u32> = specials.to_vec();
    for _ in 0..2000 {
        seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5;
        values.push(seed);
    }
    for &a in &values {
        for &b in values.iter().step_by(37) {
            let want = expect(f32::from_bits(a) as f64, f32::from_bits(b) as f64);
            for f in [x86::comiss_hw::<true>(a, b), x86::comiss_hw::<false>(a, b)] {
                assert_eq!((f.zf(), f.pf(), f.cf()), want, "comiss {a:#x} {b:#x}");
            }
            let (da, db) = ((f32::from_bits(a) as f64).to_bits(), (f32::from_bits(b) as f64).to_bits());
            let (wa, wb) = ([da as u32, (da >> 32) as u32, 0, 0], [db as u32, (db >> 32) as u32, 0, 0]);
            for f in [x86::comisd_hw::<true>(wa, wb), x86::comisd_hw::<false>(wa, wb)] {
                assert_eq!((f.zf(), f.pf(), f.cf()), want, "comisd {a:#x} {b:#x}");
            }
        }
    }
}

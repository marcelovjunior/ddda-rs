//! L190: the scalar SSE helpers of the mechanically translated ports (`semantic_rt`) return the NaN the
//! hardware returns: with two NaN operands the first source operand (quieted), whatever the other is
//! (the helpers returned a signalling second operand before 2026-10-14).
use ddnative_base::x86;


#[test]
fn rt_nan_rules_match_the_hardware() {
    ddnative_host::install();
    let vals = [0x7fc0_0001u32, 0x7fa0_0002, 0xffc0_0003, 0xffa0_0004, 0x3f80_0000, 0x0000_0000, 0x7f80_0000, 0xff80_0000, 0x7fc1_2345, 0x7fb1_2345];
    let mut bad = 0;
    for &a in &vals {
        for &b in &vals {
            for (name, hw, rtf) in [
                ("mulss", x86::mulss as fn(u32, u32) -> u32, mtcore::semantic_rt::mulss as fn(f32, f32) -> f32),
                ("addss", x86::addss, mtcore::semantic_rt::addss),
                ("subss", x86::subss, mtcore::semantic_rt::subss),
                ("divss", x86::divss, mtcore::semantic_rt::divss),
            ] {
                let (h, r) = (hw(a, b), rtf(f32::from_bits(a), f32::from_bits(b)).to_bits());
                if h != r {
                    bad += 1;
                    eprintln!("{name} {a:08x} {b:08x}: hardware {h:08x} rt {r:08x}");
                }
            }
        }
    }
    assert_eq!(bad, 0);
}

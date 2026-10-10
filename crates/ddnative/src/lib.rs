//! One-to-one translations of DDDA.exe functions (the systematic rebuild,
//! docs/METHODOLOGY.md). Each function here:
//!
//! - cites the original address it translates, and translates the whole
//!   function: every branch, in the original's order;
//! - keeps the original's state as named fields at the original's
//!   offsets (documented per field), not a reshaped model;
//! - reaches every callee it cannot yet own through an explicit trait,
//!   never a skipped branch or a stand-in value;
//! - is checked by a differential test against the original instructions
//!   (`research/native/`, vectors replayed by `tests/`).
//!
//! Platform-layer functions (allocators, the CRT, threads, file and device
//! I/O) are not translated here: Rust's own facilities replace them.

pub mod action;
pub mod character;
pub mod enemy;
pub mod model;
pub mod particle;
pub mod player_action;

/// An `fld dword` / `fstp dword` pair: the value is kept, except that a
/// signalling NaN becomes quiet (the x87 load raises the masked invalid
/// exception and sets the quiet bit).
pub fn x87_copy(v: f32) -> f32 {
    let bits = v.to_bits();
    if v.is_nan() && bits & 0x0040_0000 == 0 {
        f32::from_bits(bits | 0x0040_0000)
    } else {
        v
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod x87_hardware {
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
            assert_eq!(super::x87_copy(f32::from_bits(bits)).to_bits(), hardware(bits), "{bits:#010x}");
        }
    }
}

//! Helpers of the mechanically translated semantic ports (L184, s4;
//! research/native/semport.py): x86 flag arithmetic and SSE operations
//! with the hardware's exact semantics.

fn mask(bits: u32) -> u32 {
    if bits == 32 {
        u32::MAX
    } else {
        (1 << bits) - 1
    }
}

fn parity_even(value: u32) -> bool {
    (value as u8).count_ones() % 2 == 0
}

fn sign(value: u32, bits: u32) -> bool {
    value >> (bits - 1) & 1 != 0
}

/// Flags (cf, zf, sf, of, pf) of `a - b` at `bits` width.
pub fn sub_flags(a: u32, b: u32, bits: u32) -> (bool, bool, bool, bool, bool) {
    let (a, b) = (a & mask(bits), b & mask(bits));
    let r = a.wrapping_sub(b) & mask(bits);
    let of = (sign(a, bits) != sign(b, bits)) && (sign(a, bits) != sign(r, bits));
    (a < b, r == 0, sign(r, bits), of, parity_even(r))
}

/// Flags of `a + b` at `bits` width.
pub fn add_flags(a: u32, b: u32, bits: u32) -> (bool, bool, bool, bool, bool) {
    let (a, b) = (a & mask(bits), b & mask(bits));
    let wide = a as u64 + b as u64;
    let r = (wide as u32) & mask(bits);
    let of = (sign(a, bits) == sign(b, bits)) && (sign(a, bits) != sign(r, bits));
    (wide >> bits != 0, r == 0, sign(r, bits), of, parity_even(r))
}

/// Flags of a logic operation with result `r`.
pub fn logic_flags(r: u32, bits: u32) -> (bool, bool, bool, bool, bool) {
    let r = r & mask(bits);
    (false, r == 0, sign(r, bits), false, parity_even(r))
}

/// `SHL a, count` (count 1..=31): result and flags (cf, zf, sf, of, pf). The
/// overflow flag is defined for count 1 only; it is computed the same way
/// for any count here.
pub fn shl(a: u32, count: u32, bits: u32) -> (u32, bool, bool, bool, bool, bool) {
    let a = a & mask(bits);
    let r = ((a as u64) << count) as u32 & mask(bits);
    let cf = count <= bits && (a as u64) >> (bits - count) & 1 != 0;
    (r, cf, r == 0, sign(r, bits), sign(r, bits) != cf, parity_even(r))
}

/// Flags (cf, zf, pf) `COMISS`/`UCOMISS a, b` sets.
pub fn comiss(a: f32, b: f32) -> (bool, bool, bool) {
    if a.is_nan() || b.is_nan() {
        (true, true, true)
    } else if a > b {
        (false, false, false)
    } else if a < b {
        (true, false, false)
    } else {
        (false, true, false)
    }
}

/// `SHUFPS dst, src, imm`.
pub fn shufps(dst: [f32; 4], src: [f32; 4], imm: u32) -> [f32; 4] {
    [dst[(imm & 3) as usize], dst[(imm >> 2 & 3) as usize], src[(imm >> 4 & 3) as usize], src[(imm >> 6 & 3) as usize]]
}

/// `CVTPS2PD` of the two low floats into two doubles (as four lanes of bits).
pub fn cvtps2pd(low: f32, high: f32) -> [f32; 4] {
    let a = (low as f64).to_bits();
    let b = (high as f64).to_bits();
    [f32::from_bits(a as u32), f32::from_bits((a >> 32) as u32), f32::from_bits(b as u32), f32::from_bits((b >> 32) as u32)]
}

/// `CVTTSS2SI`: truncate toward zero; out of range or NaN gives 0x80000000.
pub fn cvttss2si(value: f32) -> u32 {
    if value.is_nan() || value >= 2147483648.0 || value < -2147483648.0 {
        0x8000_0000
    } else {
        value as i32 as u32
    }
}

/// The NaN an SSE arithmetic instruction returns when an operand is a NaN:
/// the first (destination) operand if it is one, else the second; quieted. Plain Rust arithmetic may swap the
/// operands of commutative operations, which would pick the other NaN.
fn pick_nan(a: u32, b: u32) -> Option<u32> {
    let is_nan = |v: u32| v & 0x7fff_ffff > 0x7f80_0000;
    let chosen = match (is_nan(a), is_nan(b)) {
        (false, false) => return None,
        (true, false) => a,
        (false, true) => b,
        // both NaN: the first source operand, whatever the other is (measured on
        // the hardware; the x87 rule that a quiet NaN beats a signalling one does
        // not apply to SSE)
        (true, true) => a,
    };
    Some(chosen | 0x0040_0000)
}

#[cold]
#[inline(never)]
fn nan_result(a: u32, b: u32, r: u32) -> u32 {
    pick_nan(a, b).unwrap_or(r)
}

macro_rules! scalar_op {
    ($name:ident, $op:tt) => {
        /// The scalar single-precision instruction (x86 NaN rules).
        #[inline(always)]
        pub fn $name(a: f32, b: f32) -> f32 {
            let r = a $op b;
            if r != r {
                return f32::from_bits(nan_result(a.to_bits(), b.to_bits(), r.to_bits()));
            }
            r
        }
    };
}
scalar_op!(mulss, *);
scalar_op!(addss, +);
scalar_op!(subss, -);
scalar_op!(divss, /);

/// FLD dword / FSTP dword of a float on the hardware: the same value, a
/// signalling NaN quieted.
#[inline]
pub fn x87_copy(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

#[cfg(feature = "cover")]
thread_local! {
    static HITS: std::cell::RefCell<std::collections::BTreeSet<u32>> = Default::default();
}
/// A translated block was entered (feature `cover`).
#[cfg(feature = "cover")]
pub fn hit(pc: u32) {
    HITS.with(|h| h.borrow_mut().insert(pc));
}
/// The blocks entered so far on this thread (feature `cover`).
#[cfg(feature = "cover")]
pub fn hits() -> Vec<u32> {
    HITS.with(|h| h.borrow().iter().copied().collect())
}

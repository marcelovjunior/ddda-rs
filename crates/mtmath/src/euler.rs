//! MtMatrix to Euler angles: six functions, one per rotation order, that
//! share one shape and differ in which elements they read and where each
//! angle goes. Each is a row of [`ORDERS`].
//!
//! For a rotation matrix, one element (the pivot) is plus or minus the sine
//! of the middle rotation. Away from the poles the middle angle is
//! `-asin(±pivot)` and the outer two come from `atan2` of element pairs;
//! at a pole (pivot `<= -1`, or `>= 1` or NaN) the middle angle is
//! `±π/2`, one outer angle comes from another pair and the other is 0. The
//! fourth output word is set to 0. Results are in radians.
//!
//! `atan2` is the CRT's double-precision routine (0x012F1109, single
//! inputs widened, result rounded to float) and `asin` its float routine
//! (0x012F0D4C); neither is ported. Elements are numbered in memory order
//! (`m[r][c]` is `4 r + c`).

use crate::scalar::F32;

/// π/2 (0x3FC90FDB), read from 0x01606360; -π/2 from 0x0161C88C.
pub const HALF_PI: F32 = F32::from_bits(0x3FC9_0FDB);

/// One `atan2(±m[y], ±m[x])`, its sign and the output slot it goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Atan2 {
    pub slot: usize,
    pub y: usize,
    pub negate_y: bool,
    pub x: usize,
    pub negate_x: bool,
    pub negate_result: bool,
}

/// The pole branch: one `atan2` into `atan2.slot`, `±π/2` into the middle
/// slot, 0 into the remaining one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pole {
    pub atan2: Atan2,
    /// Whether the middle angle is `+π/2` (else `-π/2`).
    pub middle_positive: bool,
}

/// One rotation order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EulerOrder {
    /// The original's address.
    pub address: u32,
    /// The element holding `±sin` of the middle angle.
    pub pivot: usize,
    /// The first outer angle (computed first).
    pub first: Atan2,
    /// The middle angle's slot; it is `-asin(pivot)`, or `-asin(-pivot)`
    /// when `negate_pivot`.
    pub middle: usize,
    pub negate_pivot: bool,
    /// The last outer angle (the slot left zero at the poles).
    pub last: Atan2,
    /// Pivot `<= -1`.
    pub low: Pole,
    /// Pivot `>= 1`, or NaN.
    pub high: Pole,
}

const fn t(slot: usize, y: usize, negate_y: bool, x: usize, negate_x: bool, negate_result: bool) -> Atan2 {
    Atan2 { slot, y, negate_y, x, negate_x, negate_result }
}

const fn pole(atan2: Atan2, middle_positive: bool) -> Pole {
    Pole { atan2, middle_positive }
}

/// The six orders, from the code at each address (DDDA.exe 2017-12-13 Steam).
pub const ORDERS: [EulerOrder; 6] = [
    EulerOrder {
        address: 0x00CF_C9E0, pivot: 2,
        first: t(0, 6, true, 10, false, true), middle: 1, negate_pivot: false, last: t(2, 1, true, 0, false, true),
        low: pole(t(0, 4, false, 5, false, false), true), high: pole(t(0, 4, false, 5, false, true), false),
    },
    EulerOrder {
        address: 0x00CF_CB00, pivot: 1,
        first: t(0, 9, false, 5, false, true), middle: 2, negate_pivot: true, last: t(1, 2, false, 0, false, true),
        low: pole(t(0, 8, false, 10, false, true), false), high: pole(t(0, 8, true, 10, false, true), true),
    },
    EulerOrder {
        address: 0x00CF_CC20, pivot: 6,
        first: t(1, 2, false, 10, false, true), middle: 0, negate_pivot: true, last: t(2, 4, false, 5, false, true),
        low: pole(t(1, 1, false, 0, false, true), false), high: pole(t(1, 1, true, 0, false, true), true),
    },
    EulerOrder {
        address: 0x00CF_CD40, pivot: 4,
        first: t(1, 8, true, 0, false, true), middle: 2, negate_pivot: false, last: t(0, 6, true, 5, false, true),
        low: pole(t(1, 9, false, 10, false, false), true), high: pole(t(1, 9, false, 10, false, true), false),
    },
    EulerOrder {
        address: 0x00CF_CE60, pivot: 8,
        first: t(2, 4, false, 0, false, true), middle: 1, negate_pivot: true, last: t(0, 9, false, 10, false, true),
        low: pole(t(2, 1, false, 2, false, false), false), high: pole(t(2, 1, true, 2, true, true), true),
    },
    EulerOrder {
        address: 0x00CF_CF80, pivot: 9,
        first: t(2, 1, true, 5, false, true), middle: 0, negate_pivot: false, last: t(1, 8, true, 10, false, true),
        low: pole(t(2, 8, false, 0, false, false), true), high: pole(t(2, 2, false, 0, false, true), false),
    },
];

/// Which way [`to_euler`] went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EulerBranch {
    Main,
    Low,
    High,
}

/// Where a conversion reads the matrix and the CRT routines it calls, in
/// the original's order.
pub trait EulerInput {
    /// Element `i` of the matrix (read only when the branch needs it).
    fn elem(&mut self, i: usize) -> F32;
    /// `atan2(y, x)` on the widened inputs (`y`, `x` already signed).
    fn atan2(&mut self, term: &Atan2, y: F32, x: F32) -> f64;
    /// `asinf(v)`.
    fn asin(&mut self, v: F32) -> F32;
}

fn signed(v: F32, negate: bool) -> F32 {
    if negate { -v } else { v }
}

fn angle(input: &mut impl EulerInput, term: &Atan2) -> F32 {
    let y = signed(input.elem(term.y), term.negate_y);
    let x = signed(input.elem(term.x), term.negate_x);
    signed(F32(input.atan2(term, y, x) as f32), term.negate_result)
}

/// The Euler angles of a rotation matrix for one order (see the module
/// comment). Only the elements the branch needs are read, as in the
/// original.
// Translated from 0x00CFC9E0, 0x00CFCB00, 0x00CFCC20, 0x00CFCD40, 0x00CFCE60, 0x00CFCF80 (DDDA.exe 2017-12-13 Steam), MtMatrix to Euler angles.
pub fn to_euler(order: &EulerOrder, input: &mut impl EulerInput) -> ([F32; 4], EulerBranch) {
    let mut out = [F32::ZERO; 4];
    let p = input.elem(order.pivot);
    let (pole, branch) = if !(F32::ONE > p) {
        (order.high, EulerBranch::High)
    } else if !(p > F32(-1.0)) {
        (order.low, EulerBranch::Low)
    } else {
        out[order.first.slot] = angle(input, &order.first);
        out[order.middle] = -input.asin(signed(p, order.negate_pivot));
        out[order.last.slot] = angle(input, &order.last);
        return (out, EulerBranch::Main);
    };
    out[pole.atan2.slot] = angle(input, &pole.atan2);
    out[order.middle] = if pole.middle_positive { HALF_PI } else { -HALF_PI };
    (out, branch)
}

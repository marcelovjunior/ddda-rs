//! L184 s7: semantic port of 0x012F05C0, the CRT's float-to-int
//! conversion `_ftol2_sse` that the compiler calls for every `(int)float`
//! cast.
//!
//! It takes the value in ST0, pops it and returns it truncated toward zero
//! in EAX. With SSE2 present (the dword at 0x019AA078 is non-zero, always
//! on the machines this runs on) the original stores ST0 as a double
//! (`fstp qword`, rounding the extended value to double) and truncates it
//! with `cvttsd2si`, which gives 0x80000000 for NaN and out-of-range
//! values. Without SSE2 it takes a 64-bit x87 path at 0x012F05F6, which
//! stays a guest call.
//!
//! Translated from 0x012F05C0 (DDDA.exe 2017-12-13 Steam).
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

const SSE2_FLAG: u32 = 0x019a_a078;
const NO_SSE2_PATH: u32 = 0x012f_05f6;

/// `cvttsd2si`: truncation toward zero, 0x80000000 ("integer indefinite")
/// for NaN or a value outside the 32-bit range.
#[inline(always)]
fn truncate_to_i32(v: f64) -> u32 {
    if v < 2147483648.0 && v > -2147483649.0 {
        v as i32 as u32
    } else {
        0x8000_0000
    }
}

/// 0x012F05C0 (cdecl, no arguments; ST0 in, EAX out).
pub fn ftol2_sse(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    if pg.u32(m, SSE2_FLAG) == 0 {
        return crate::semantic::call_guest(m, c, NO_SSE2_PATH);
    }
    let entry = c.esp;
    // the original's 8-byte aligned slot below its frame
    let slot = entry.wrapping_sub(12) & !7;
    c.st.real_memory::<0x18dd, 8, true>(m, slot); // fstp qword ptr [slot]
    let bits = pg.u32(m, slot) as u64 | (pg.u32(m, slot.wrapping_add(4)) as u64) << 32;
    c.eax = truncate_to_i32(f64::from_bits(bits));
    c.esp = entry.wrapping_add(4);
}

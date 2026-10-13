//! L184 s7: semantic port of 0x00F2FFC0, a distance-falloff integral.
//!
//! `float10 integrate_falloff(this, pos, velocity, total_time, radius,
//! scale, _)` (thiscall, `ret 0x18`, result in ST0): walks a point from
//! `pos` along `velocity` in `this->steps` (the dword at `this + 0x3ac`)
//! equal time slices of `total_time / steps`. At each step the point's
//! distance from the origin, less `radius`, is turned into an attenuation
//! `exp(-(distance - radius) / scale)` (1.0 when the point is inside the
//! radius); the trapezoid rule over the steps is accumulated and the
//! result is `slice * sum`.
//!
//! The per-step arithmetic is single-precision SSE (the CRT `expf` is
//! the bit-exact port in [`crate::semantic_s7_math`]); the time slice and
//! the final scaling are x87, issued through `cpu.st` exactly as the
//! lifted code does, so ST0 is the original's extended value.
//!
//! Translated from 0x00F2FFC0 (DDDA.exe 2017-12-13 Steam).
use crate::semantic_s7::F;
use crate::semantic_s7_math::expf;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

const STEPS: u32 = 0x3ac;
const ONE: F = F(1.0);
const HALF: F = F(0.5);
/// Distances at or below this (the float at 0x01561738) count as inside.
const FLOOR_ADDRESS: u32 = 0x0156_1738;
/// 2^32 as a float, added for a step count with the sign bit set.
const UNSIGNED_BIAS_ADDRESS: u32 = 0x0161_c220;

#[inline(always)]
fn load(pg: Pages, m: &dyn Mem, address: u32) -> F {
    F::from_bits(pg.u32(m, address))
}

/// 0x00F2FFC0: see the module docs. Stack arguments: `[esp+4]` start
/// position (two floats), `[esp+8]` velocity (two floats), `[esp+0xc]`
/// total time, `[esp+0x10]` radius, `[esp+0x14]` unused, `[esp+0x18]` scale
/// (the original then reuses that slot as scratch).
pub fn integrate_falloff(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let inverse_scale = ONE / load(pg, m, entry.wrapping_add(0x18));
    let steps = pg.u32(m, this.wrapping_add(STEPS));
    // x87: time slice = total_time / steps (in extended precision), stored
    // as a float; the per-step displacement is velocity * the unrounded slice
    pg.set_u32(m, entry.wrapping_add(0x18), steps);
    c.st.memory::<0x00db, 4, false>(m, entry.wrapping_add(0x18)); // fild
    if (steps as i32) < 0 {
        c.st.real_memory::<0x00d8, 4, false>(m, UNSIGNED_BIAS_ADDRESS); // fadd
    }
    c.st.memory::<0x38d8, 4, false>(m, entry.wrapping_add(0xc)); // fdivr total_time
    c.st.real_memory::<0x10d9, 4, true>(m, entry.wrapping_add(0xc)); // fst: slice as a float
    let velocity = pg.u32(m, entry.wrapping_add(8));
    c.st.real_memory::<0x00d9, 4, false>(m, velocity); // fld vx
    c.st.register::<0xc9d8>(); // fmul st1
    c.st.real_memory::<0x18d9, 4, true>(m, entry.wrapping_sub(8)); // fstp step_x
    c.st.real_memory::<0x08d8, 4, false>(m, velocity.wrapping_add(4)); // fmul vy
    c.st.real_memory::<0x18d9, 4, true>(m, entry.wrapping_sub(4)); // fstp step_y
    let step_x = load(pg, m, entry.wrapping_sub(8));
    let step_y = load(pg, m, entry.wrapping_sub(4));

    let position = pg.u32(m, entry.wrapping_add(4));
    let mut y = load(pg, m, position.wrapping_add(4));
    let mut x = load(pg, m, position);
    let radius = load(pg, m, entry.wrapping_add(0x10));
    let mut previous = F(expf(&*m, pg, (-(((y * y + x * x).sqrt() - radius) * inverse_scale)).0));
    let mut sum = F(0.0);
    let mut remaining = steps;
    while remaining != 0 {
        x = x + step_x;
        y = y + step_y;
        let distance = (y * y + x * x).sqrt() - radius;
        let floor = load(pg, m, FLOOR_ADDRESS);
        let current = if distance.0 > floor.0 {
            F(expf(&*m, pg, (-(distance * inverse_scale)).0))
        } else {
            ONE
        };
        remaining -= 1;
        sum = ((current + previous) * HALF) + sum;
        previous = current;
    }
    pg.set_u32(m, entry.wrapping_sub(0x18), sum.to_bits());
    c.st.real_memory::<0x00d9, 4, false>(m, entry.wrapping_add(0xc)); // fld slice
    c.st.real_memory::<0x08d8, 4, false>(m, entry.wrapping_sub(0x18)); // fmul sum
    c.esp = entry.wrapping_add(4 + 0x18);
}

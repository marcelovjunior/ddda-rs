//! L184 s5: render-state helpers.
//!
//! 0x00FE4F30 keeps one floating-point shader constant of the draw state as
//! a byte: the constant lives in a per-frame buffer (pointer at `state +
//! 0x11C8`, 16-byte aligned, low bits are flags) and holds `byte / 255`. A
//! call with the byte it already holds does nothing; otherwise the old
//! buffer is recorded in the state's change log (when the log flag is set),
//! a fresh 16-byte-granular slot is taken from the frame allocator and the
//! new constant stored in it.
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{call_ext, Cpu};

/// 255.0 (0x0161C298) and 1/255 (0x0161C29C).
const BYTE_SCALE: f32 = f32::from_bits(0x437f_0000);
const BYTE_RECIPROCAL: f32 = f32::from_bits(0x3b80_8081);

/// The constant's current byte: the value at the buffer times 255, truncated
/// toward zero by the x87 `FISTP` (round-to-zero control word) and cut to its
/// low 32 bits. NaN, infinities and overflow give the "integer indefinite"
/// (low word 0). The product is exact in the x87's double precision.
fn constant_byte(value: f32) -> u32 {
    let product = value as f64 * BYTE_SCALE as f64;
    if product.is_nan() || product.abs() >= 9.223_372_036_854_775_8e18 {
        0
    } else {
        (product.trunc() as i64) as u32
    }
}

/// 0x00FE4F30 `state->set_byte_constant(byte)` (stdcall: stack arguments
/// `state`, `byte`; `ret 8`).
// Translated from 0x00FE4F30 (DDDA.exe 2017-12-13 Steam)
pub fn set_byte_constant(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let state = pg.u32(m, entry.wrapping_add(4));
    // the original's ESP at its call of the frame allocator (two saved registers, 8 bytes of locals, EDI)
    let frame = entry.wrapping_sub(8 + 4 + 4 + 4);
    let current_buffer = pg.u32(m, state + 0x11c8) & 0xffff_fff0;
    let current = if current_buffer == 0 { 0 } else { constant_byte(f32::from_bits(pg.u32(m, current_buffer))) };
    let byte = pg.u8(m, entry.wrapping_add(8)) as u32;
    if current != byte {
        let flags = pg.u32(m, state + 0x5c) | 2;
        pg.set_u32(m, state + 0x5c, flags);
        if (pg.u32(m, state + 0x5c) as u8) & 0x80 != 0 {
            // record the old constant in the change log
            let record = |index: u32| state + 0x30c7c + index.wrapping_mul(12);
            let index = pg.u32(m, state + 0x6c);
            let head = pg.u32(m, record(index));
            pg.set_u32(m, record(index), (head & 0x8000_01fa) | 0x1fa);
            let index = pg.u32(m, state + 0x6c);
            let head = pg.u32(m, record(index));
            pg.set_u32(m, record(index), head & 0x7fff_ffff);
            let old_low = pg.u32(m, state + 0x11c8);
            let index = pg.u32(m, state + 0x6c);
            pg.set_u32(m, record(index) + 4, old_low);
            let old_high = pg.u32(m, state + 0x11cc);
            pg.set_u32(m, record(index) + 8, old_high);
            let count = pg.u32(m, state + 0x6c).wrapping_add(1);
            pg.set_u32(m, state + 0x6c, count);
        }
        let owner = pg.u32(m, state + 0x18);
        let description = pg.u32(m, owner + 0x7e8);
        let used = pg.u32(m, state + 0x24);
        let size = ((pg.u16(m, description + 0x18) as u32).wrapping_mul(4).wrapping_add(0xf)) & 0xffff_fff0;
        if used.wrapping_add(size) > pg.u32(m, state + 0x28) {
            // frame allocator 0x00FED890 (thiscall, one stack argument)
            let saved = (c.esi, c.edi, c.ebp);
            c.esp = frame;
            c.esp = c.esp.wrapping_sub(4);
            pg.set_u32(m, c.esp, size);
            c.esp = c.esp.wrapping_sub(4);
            pg.set_u32(m, c.esp, 0x00fe_500f);
            (c.eax, c.ecx, c.esi, c.edi, c.ebp) = (description, state, state, size, byte);
            call_ext(m, c, 0x00fe_d890);
            (c.esi, c.edi, c.ebp) = saved;
        }
        let slot = pg.u32(m, state + 0x24);
        pg.set_u32(m, state + 0x24, slot.wrapping_add(size));
        pg.set_u32(m, state + 0x11c8, slot);
        let flags = pg.u32(m, state + 0x154) | 1;
        pg.set_u32(m, state + 0x154, flags);
        if slot != 0 {
            // FILD of the byte, times 1/255 in double precision, stored as a float
            let value = (byte as f64 * BYTE_RECIPROCAL as f64) as f32;
            pg.set_u32(m, slot, value.to_bits());
        }
        let flags = pg.u32(m, state + 0x5c) & 0xffff_fffd;
        pg.set_u32(m, state + 0x5c, flags);
    }
    c.esp = entry.wrapping_add(4 + 8);
}

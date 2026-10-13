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

// ---------------------------------------------------------------------
// 0x00DEFA60: release the render context's pending resources

use crate::semantic::call_from;

/// One kind of deferred release of the render context 0x00DEFA60 empties.
struct Pending {
    /// dword counting the elements of all 25 buckets of this kind
    total: u32,
    /// first of the 25 buckets (16 bytes each: element count, ?, element array pointer, ?)
    buckets: u32,
    /// how an element is addressed
    element: Element,
    /// the release function (thiscall, one stack argument, `ret 4`)
    release: u32,
    /// its return address in 0x00DEFA60
    back: u32,
    /// the object it is called on: the context itself or its embedded pool (`+ 0x1a0`)
    on_pool: bool,
}

enum Element {
    /// records of `size` bytes; the release gets the record's address
    Record(u32),
    /// pointers; the release gets the pointer stored in the array
    Pointer,
}

const PENDING: [Pending; 6] = [
    Pending { total: 0x10e10, buckets: 0x10c84, element: Element::Record(0x50), release: 0x0113_4a90, back: 0x00de_faa1, on_pool: true },
    Pending { total: 0x11460, buckets: 0x112d4, element: Element::Record(0x50), release: 0x0113_4ca0, back: 0x00de_faf1, on_pool: true },
    Pending { total: 0x10fa4, buckets: 0x10e18, element: Element::Record(0xc), release: 0x0113_4bd0, back: 0x00de_fb41, on_pool: true },
    Pending { total: 0x115f4, buckets: 0x11468, element: Element::Record(8), release: 0x0113_4e40, back: 0x00de_fb92, on_pool: true },
    Pending { total: 0x112cc, buckets: 0x11140, element: Element::Pointer, release: 0x00fb_4bf0, back: 0x00de_fbde, on_pool: false },
    Pending { total: 0x11138, buckets: 0x10fac, element: Element::Record(0x14), release: 0x0113_1290, back: 0x00de_fc31, on_pool: true },
];

/// 0x00DEFA60 `context->release_pending()` (fastcall: ECX = context, plain
/// `ret`): for each of six kinds of deferred releases that has entries
/// (its total is positive), call the kind's release function once per element
/// of each of the 25 buckets, clear the bucket's count, then clear the total.
/// The element array pointer is read again before every call (a release
/// may change it); the element count once per bucket.
// Translated from 0x00DEFA60 (DDDA.exe 2017-12-13 Steam)
pub fn release_pending(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let context = c.ecx;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // the original's ESP at its calls: 8 bytes of locals and four saved registers
    let frame = entry.wrapping_sub(8 + 16);
    for kind in &PENDING {
        if (pg.u32(m, context + kind.total) as i32) <= 0 {
            continue;
        }
        for bucket in 0..25u32 {
            let at = context + kind.buckets + 16 * bucket;
            let count = pg.u32(m, at);
            for index in 0..count {
                let data = pg.u32(m, at + 8);
                let argument = match kind.element {
                    Element::Record(size) => data.wrapping_add(index.wrapping_mul(size)),
                    Element::Pointer => pg.u32(m, data.wrapping_add(4 * index)),
                };
                let this = if kind.on_pool { context + 0x1a0 } else { context };
                let release = kind.release;
                call_from(m, c, pg, frame, this, &[argument], kind.back, |m, c| ddnative_base::x86::call_ext(m, c, release));
            }
            pg.set_u32(m, at, 0);
        }
        pg.set_u32(m, context + kind.total, 0);
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4);
}

// ---------------------------------------------------------------------
// 0x0076F570: copy the controller's pad bindings

/// 0x0076F570 `unit->copy_input_bindings()` (stdcall: stack argument `unit`,
/// `ret 4`): when the unit has a controller object (`unit + 0xCD0`) whose
/// class chain (virtual method +0x10, then the `+0x10` base links) reaches
/// the class id kept at 0x018D2D10, and the byte `unit + 0xD99` is set, copy
/// its eight 16-slot binding tables: for each slot whose bit is set in the
/// table's mask (dword `+0x10`) the unit gets a 1 byte in its flag array
/// (`unit + 0x72`, 4 bytes per slot, 0x40 per table) and the slot's 16-bit
/// binding (0 when the table has no array) in its binding array (`unit +
/// 4 * (0x1C + 16 * table + slot)`); a clear bit leaves a 0 flag byte and the
/// binding untouched. Tables are 0x140 bytes apart from controller + 0x480.
// Translated from 0x0076F570 (DDDA.exe 2017-12-13 Steam)
pub fn copy_input_bindings(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let unit = pg.u32(m, entry.wrapping_add(4));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let controller = pg.u32(m, unit + 0xcd0);
    if controller != 0 {
        // controller->virtual +0x10() returns its class descriptor (frame: two locals words, EBP, EDI saved)
        let vtable = pg.u32(m, controller);
        let method = pg.u32(m, vtable + 0x10);
        let frame = entry.wrapping_sub(8 + 4 + 4);
        let descriptor = call_from(m, c, pg, frame, controller, &[], 0x0076_f590, |m, c| {
            c.edx = method;
            c.eax = vtable;
            ddnative_base::x86::call_ext(m, c, method)
        });
        let wanted = pg.u32(m, 0x018d_2d10);
        let mut class = descriptor;
        let mut found = pg.u32(m, class + 4) == wanted;
        while !found {
            class = pg.u32(m, class + 0x10);
            if class == 0 {
                (c.ebx, c.ebp, c.esi, c.edi) = saved;
                c.esp = entry.wrapping_add(4 + 4);
                return;
            }
            found = pg.u32(m, class + 4) == wanted;
        }
        if pg.u8(m, unit + 0xd99) != 0 {
            let mut table = controller.wrapping_add(0x480);
            for group in 0..8u32 {
                let first = 0x1c + 0x10 * group;
                if table.wrapping_sub(0x100) != 0 {
                    let mask = pg.u32(m, table + 0x10);
                    for slot in 0..16u32 {
                        let flag = unit + 0x72 + 0x40 * group + 4 * slot;
                        pg.set_u8(m, flag, 0);
                        if mask & (1 << slot) != 0 {
                            pg.set_u8(m, flag, 1);
                            let bindings = pg.u32(m, table);
                            let value = if bindings != 0 { pg.u16(m, bindings + 2 * slot) } else { 0 };
                            pg.set_u16(m, unit + (first + slot) * 4, value);
                        }
                    }
                }
                table = table.wrapping_add(0x140);
            }
        }
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4 + 4);
}

//! Semantic ports (L184, subagent s4): native rewrites of hot game
//! functions that keep the ABI-visible result of the original (return
//! value, callee-saved registers, ESP and every guest memory write above
//! the returned stack). See `semantic.rs` for the pattern. Caller-saved
//! ECX/EDX, the flags and popped stack scratch are not reproduced.
use crate::semantic::call_from;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

/// What the x87 `FLD m32` / `FSTP m32` pair does to a float's bits: a
/// signalling NaN comes back quiet, everything else is unchanged.
fn through_x87(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

// Translated from 0x00e70710 (DDDA.exe 2017-12-13 Steam).
/// 0x00E70710, `actor->update()` (thiscall, void, vf09 of the actor class
/// reached from job 0x005D8D70): one frame step of an animated actor
/// (`this`, ESI in the original).
///
/// With the "linked" flag (byte +0x11B bit 0) clear, or when its link
/// (+0x11C, +0x1D0) is in an unusable state, the step is only the base
/// class's 0x00FDF600 (a tail call). Otherwise it follows the master at
/// +0x1D0 (copying its blend flag, position +0x60..+0x68 with +0x6C
/// cleared, +0x100 and +0x1B0), runs the vtable hooks (+0x54, +0x74/+0x68/
/// +0x78, +0x60, +0x88), 0x00E6BCA0, then advances the per-frame counters
/// (+0x1B4, +0x20C) and the tick loop (0x00E6D0C0, 0x00E6D070, 0x00E6EFD0,
/// ...) once for every elapsed tick (u16 at +0x110), and finally the
/// timer at +0x206 (0x00E6E230 or a plain subtract).
pub fn actor_update(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let this = c.ecx;
    let entry = c.esp;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // the original's frame after `push ebx; push esi`: its calls are made here
    let frame = entry.wrapping_sub(8);
    c.ebx = 1;
    c.esi = this;
    let guest = |target: u32| move |m: &mut dyn Mem, c: &mut Cpu| crate::semantic::call_guest(m, c, target);
    // a call of a (non-virtual) guest function on `this`
    macro_rules! call {
        ($back:expr, $target:expr) => {
            call_from(m, c, pg, frame, this, &[], $back, guest($target))
        };
    }
    // a call of `slot` of the vtable of `this`, which the original reads
    // right before the call
    macro_rules! vcall {
        ($slot:expr, $back:expr) => {{
            let method = pg.u32(m, pg.u32(m, this).wrapping_add($slot));
            call_from(m, c, pg, frame, this, &[], $back, guest(method))
        }};
    }
    // `pop esi; pop ebx; jmp 0x00FDF600`: the callee returns to our caller
    macro_rules! base_step_and_return {
        () => {{
            c.esp = entry;
            c.ecx = this;
            crate::semantic::call_guest(m, c, 0x00fd_f600);
            (c.ebx, c.ebp, c.esi, c.edi) = saved;
            return;
        }};
    }
    macro_rules! finish {
        () => {{
            (c.ebx, c.ebp, c.esi, c.edi) = saved;
            c.esp = entry.wrapping_add(4);
            return;
        }};
    }

    if pg.u8(m, this.wrapping_add(0x11b)) & 1 == 0 {
        if vcall!(0x9c, 0x00e7_072b) & 0xff != 0 {
            finish!();
        }
        base_step_and_return!();
    }
    let link = pg.u32(m, this.wrapping_add(0x11c));
    if link != 0 {
        let kind = pg.u32(m, link.wrapping_add(4)) & 7;
        if kind != 2 && kind != 1 {
            pg.set_u32(m, this.wrapping_add(0x11c), 0);
            base_step_and_return!();
        }
    }
    let master = pg.u32(m, this.wrapping_add(0x1d0));
    if master != 0 {
        let kind = pg.u32(m, master.wrapping_add(4)) & 7;
        if (kind != 2 && kind != 1)
            || pg.u32(m, master.wrapping_add(0x1d4)) != pg.u32(m, this.wrapping_add(0x1d4))
            || pg.u16(m, master.wrapping_add(0x1e0)) == 0
        {
            base_step_and_return!();
        }
        let master_flags = pg.u32(m, master.wrapping_add(4));
        let flags = pg.u32(m, this.wrapping_add(4));
        if master_flags & 7 == 2 && master_flags & 0x4800 == 0x4800 {
            pg.set_u32(m, this.wrapping_add(4), flags | 0x800);
        } else {
            pg.set_u32(m, this.wrapping_add(4), flags & !0x800);
        }
        // `push edi` makes the next two calls one word deeper
        let own_vtable = pg.u32(m, this);
        let deep = frame.wrapping_sub(4);
        c.edi = own_vtable;
        let method = pg.u32(m, pg.u32(m, master).wrapping_add(0x60));
        let low = call_from(m, c, pg, deep, master, &[], 0x00e7_07cf, guest(method)) & 0xff;
        let method = pg.u32(m, own_vtable.wrapping_add(0x64));
        call_from(m, c, pg, deep, this, &[low], 0x00e7_07da, guest(method));
        c.edi = saved.3;
        let master = pg.u32(m, this.wrapping_add(0x1d0));
        for offset in [0x100u32, 0x60, 0x64] {
            let v = through_x87(pg.u32(m, master.wrapping_add(offset)));
            pg.set_u32(m, this.wrapping_add(offset), v);
        }
        let v = through_x87(pg.u32(m, master.wrapping_add(0x68)));
        pg.set_u32(m, this.wrapping_add(0x6c), 0);
        pg.set_u32(m, this.wrapping_add(0x68), v);
        let master = pg.u32(m, this.wrapping_add(0x1d0));
        let v = through_x87(pg.u32(m, master.wrapping_add(0x1b0)));
        pg.set_u32(m, this.wrapping_add(0x1b0), v);
    }
    // 0x00E70819
    vcall!(0x54, 0x00e7_0822);
    if pg.u8(m, this.wrapping_add(0xf0)) & 0x20 != 0 {
        base_step_and_return!();
    }
    let state = call!(0x00e7_0839, 0x00fe_3600);
    if state == 3 {
        base_step_and_return!();
    }
    if state == 2 {
        vcall!(0x74, 0x00e7_0854);
    }
    if vcall!(0x68, 0x00e7_085d) & 0xff != 0 {
        vcall!(0x78, 0x00e7_086a);
    }
    if vcall!(0x60, 0x00e7_0873) & 0xff != 0 {
        finish!();
    }
    vcall!(0x88, 0x00e7_0887);
    call!(0x00e7_088e, 0x00e6_bca0);
    // the pending-ticks counter
    if pg.u16(m, this.wrapping_add(0x1b6)) != 0 {
        let pending = pg.u16(m, this.wrapping_add(0x1b4));
        let ticks = pg.u16(m, this.wrapping_add(0x110));
        if pending <= ticks {
            pg.set_u16(m, this.wrapping_add(0x1b4), 0);
            if call!(0x00e7_0939, 0x00fd_f670) & 0xff != 0 {
                base_step_and_return!();
            }
        } else {
            let ticks = pg.u16(m, this.wrapping_add(0x110));
            let v = pg.u16(m, this.wrapping_add(0x1b4)).wrapping_sub(ticks);
            pg.set_u16(m, this.wrapping_add(0x1b4), v);
        }
    }
    if pg.u32(m, this.wrapping_add(0xf0)) & 0x8_0000 != 0 {
        vcall!(0xa4, 0x00e7_08d4);
        let ticks = pg.u16(m, this.wrapping_add(0x110));
        pg.set_u16(m, this.wrapping_add(0x20c), 0);
        if 0 < ticks {
            loop {
                call!(0x00e7_08f7, 0x00e6_d0c0);
                let child = pg.u32(m, this.wrapping_add(0x1f0));
                let method = pg.u32(m, pg.u32(m, child).wrapping_add(0x44));
                call_from(m, c, pg, frame, child, &[], 0x00e7_0904, guest(method));
                call!(0x00e7_090b, 0x00e6_efd0);
                let v = pg.u16(m, this.wrapping_add(0x20c)).wrapping_add(1);
                pg.set_u16(m, this.wrapping_add(0x20c), v);
                if pg.u16(m, this.wrapping_add(0x20c)) >= pg.u16(m, this.wrapping_add(0x110)) {
                    break;
                }
            }
        }
    } else {
        vcall!(0xa8, 0x00e7_0954);
        if pg.u32(m, this.wrapping_add(0x1d0)) != 0 && pg.u32(m, this.wrapping_add(0x1f8)) != 0 {
            call!(0x00e7_096d, 0x00e6_ea50);
        }
        let ticks = pg.u16(m, this.wrapping_add(0x110));
        pg.set_u16(m, this.wrapping_add(0x20c), 0);
        if 0 < ticks {
            loop {
                call!(0x00e7_098b, 0x00e6_d0c0);
                call!(0x00e7_0992, 0x00e6_d070);
                call!(0x00e7_0999, 0x00e6_efd0);
                let v = pg.u16(m, this.wrapping_add(0x20c)).wrapping_add(1);
                pg.set_u16(m, this.wrapping_add(0x20c), v);
                if pg.u16(m, this.wrapping_add(0x20c)) >= pg.u16(m, this.wrapping_add(0x110)) {
                    break;
                }
            }
        }
        if pg.u16(m, this.wrapping_add(0x110)) == 0 {
            call!(0x00e7_09c4, 0x00e6_d1c0);
            call!(0x00e7_09cb, 0x00e6_d2e0);
        }
        call!(0x00e7_09d2, 0x00e6_d330);
    }
    // 0x00E709D2
    if pg.u16(m, this.wrapping_add(0x1e2)) == 0 {
        call!(0x00e7_09e3, 0x00fd_f600);
    }
    let ticks = pg.u32(m, this.wrapping_add(0x110)) & 0xffff;
    if ticks != 0 {
        let v = pg.u32(m, this.wrapping_add(0xf0)) & 0xfffe_7fff;
        pg.set_u32(m, this.wrapping_add(0xf0), v);
    }
    if (pg.u32(m, this.wrapping_add(0xf0)) >> 6) & 1 == 0 {
        let v = pg.u32(m, this.wrapping_add(0x10c)).wrapping_add(ticks);
        pg.set_u32(m, this.wrapping_add(0x10c), v);
    }
    if pg.u32(m, this.wrapping_add(0x204)) & 0xf == 3 {
        let timer = pg.u16(m, this.wrapping_add(0x206)) as u32;
        if timer > ticks {
            let v = pg.u16(m, this.wrapping_add(0x206)).wrapping_sub(pg.u16(m, this.wrapping_add(0x110)));
            pg.set_u16(m, this.wrapping_add(0x206), v);
        } else {
            let arg = ticks.wrapping_sub(timer).wrapping_add(1);
            call_from(m, c, pg, frame, this, &[arg], 0x00e7_0a45, guest(0x00e6_e230));
        }
    }
    finish!();
}

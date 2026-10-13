//! Semantic ports, subagent s2 (L184): native rewrites of hot game
//! functions that keep the ABI-visible result of the original (return
//! value, callee-saved registers, ESP and every guest write above the
//! returned stack) instead of replaying its instructions. See
//! `semantic.rs` for the conventions.
use crate::semantic::call_from;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

fn guest(target: u32) -> impl FnOnce(&mut dyn Mem, &mut Cpu) {
    move |m, c| ddnative_base::x86::call_ext(m, c, target)
}

fn f(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// Guest addresses used by [`probe_segment`].
const LINE_SEGMENT_VTABLE: u32 = 0x0142_bc80;
const AABB_VTABLE: u32 = 0x0159_4890;

// Translated from 0x00fac6f0 (DDDA.exe 2017-12-13 Steam).
/// 0x00FAC6F0, `world->probe_segment(segment, query) -> hit` (thiscall,
/// `ret 8`): test the line segment `segment` (start xyz at +0, end xyz at
/// +0x10, 16-byte padded vectors) against the world's collision through a
/// collision query `query`; returns 1 if the collision test (0x01166C00)
/// finds something, else 0.
///
/// The original builds two MtGeomLineSegment shapes on its stack and runs
/// 0x011C2810 over them into a stack result buffer (0x011C3A30 destroys
/// it), then chooses the shape handed to the test: a segment shorter than
/// the squared threshold at `this + 0x115F8` becomes a bounding box (the
/// segment's min/max, grown by the margins at 0x01629510/14/18 after the
/// `this->vf11` call on it), a longer one is tested as the segment itself.
/// The query's +0x94 is pointed at the stack result buffer (its +0x98 is
/// saved into the buffer's +0xE4); on a hit the buffer's inner object
/// (+0x84) is released by 0x00DE46F0.
///
/// The shapes and the result buffer live in the original's stack frame and
/// are read by the callees, so the frame is rebuilt at the original's
/// addresses (S = the 16-aligned frame minus 0x250); only the query's
/// +0x94 (pointing at S + 0xD0) is visible above the returned stack.
pub fn probe_segment(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let this = c.ecx;
    let entry = c.esp;
    let segment = pg.u32(m, entry.wrapping_add(4));
    let query = pg.u32(m, entry.wrapping_add(8));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // push ebp; and esp,-16; sub esp,0x244; push ebx/esi/edi
    let s = (entry.wrapping_sub(4) & !15).wrapping_sub(0x244 + 0xc);
    let at = |off: u32| s.wrapping_add(off);
    let seg = |m: &dyn Mem, off: u32| pg.u32(m, segment.wrapping_add(off));
    c.ebp = entry.wrapping_sub(4);
    c.edi = this;
    c.esi = segment;
    c.ebx = 2;

    // the two line-segment shapes (MtGeomLineSegment) at S+0x70 and S+0xa0
    let epsilon = pg.u32(m, 0x0162_adb0);
    let (x0, y0, z0) = (seg(m, 0), seg(m, 4), seg(m, 8));
    let (x1, y1, z1) = (seg(m, 0x10), seg(m, 0x14), seg(m, 0x18));
    for base in [0x70u32, 0xa0] {
        for (off, v) in [(0, LINE_SEGMENT_VTABLE), (4, 2), (8, epsilon), (0x10, x0), (0x14, y0), (0x18, z0), (0x1c, 0),
                         (0x20, x1), (0x24, y1), (0x28, z1), (0x2c, 0)] {
            pg.set_u32(m, at(base + off), v);
        }
    }
    let buffer = at(0xd0);
    call_from(m, c, pg, s, buffer, &[0x018c_fe80, 0x018c_fe80, at(0xa0), at(0x70)], 0x00fa_c814, guest(0x011c_2810));

    // the shape tested: an AABB (S+0x10) around a short segment, or the
    // segment itself (S+0x40). Only the shape handed to the test is built:
    // the other is never reached by the callee.
    let epsilon = pg.u32(m, 0x0162_adb0);
    let (x1, y1, z1) = (seg(m, 0x10), seg(m, 0x14), seg(m, 0x18));
    let (x0, y0, z0) = (seg(m, 0), seg(m, 4), seg(m, 8));
    let squared = (f(x1) - f(x0)) * (f(x1) - f(x0)) + (f(y1) - f(y0)) * (f(y1) - f(y0)) + (f(z1) - f(z0)) * (f(z1) - f(z0));
    let threshold = f(pg.u32(m, this.wrapping_add(0x115f8)));
    // the original takes the long (segment) branch unless threshold > squared
    let boxed = threshold > squared;
    if !boxed {
        c.ebx = 0;
        for (off, v) in [(0, LINE_SEGMENT_VTABLE), (4, 2), (8, epsilon), (0x10, x0), (0x14, y0), (0x18, z0), (0x1c, 0),
                         (0x20, x1), (0x24, y1), (0x28, z1), (0x2c, 0)] {
            pg.set_u32(m, at(0x40 + off), v);
        }
    } else {
        c.ebx = 1;
        let lower = |a: f32, b: f32| if a > b { b } else { a }; // comiss a,b; ja: keep b
        let upper = |a: f32, b: f32| if a > b { a } else { b };
        // sNetwork::vf11 (0x00B16AF0, a bare `ret 0xc`) does nothing, so the
        // margins are applied straight to the bounds.
        let margin_x = f(pg.u32(m, 0x0162_9510));
        let margin_y = f(pg.u32(m, 0x0162_9514));
        let margin_z = f(pg.u32(m, 0x0162_9518));
        let min = [lower(f(x1), f(x0)) - margin_x, lower(f(y1), f(y0)) - margin_y, lower(f(z1), f(z0)) - margin_z];
        let max = [upper(f(x0), f(x1)) + margin_x, upper(f(y0), f(y1)) + margin_y, upper(f(z0), f(z1)) + margin_z];
        for (off, v) in [(0, AABB_VTABLE), (4, 7), (8, epsilon), (0x10, min[0].to_bits()), (0x14, min[1].to_bits()),
                         (0x18, min[2].to_bits()), (0x1c, 0), (0x20, max[0].to_bits()), (0x24, max[1].to_bits()),
                         (0x28, max[2].to_bits()), (0x2c, 0)] {
            pg.set_u32(m, at(0x10 + off), v);
        }
    }
    // sNetwork::vf11(segment, net, 1) is likewise a no-op: no call.
    c.esi = query;
    let shape = if boxed { at(0x10) } else { at(0x40) };
    let saved_extra = pg.u32(m, query.wrapping_add(0x98));
    pg.set_u32(m, at(0x1b4), saved_extra);
    pg.set_u32(m, query.wrapping_add(0x94), buffer);
    let hit = call_from(m, c, pg, s, this, &[shape, query], 0x00fa_c941, guest(0x0116_6c00));
    let result;
    if hit != 0 {
        let owner = pg.u32(m, query.wrapping_add(0x98));
        call_from(m, c, pg, s, owner, &[at(0x154)], 0x00fa_ca88, guest(0x00de_46f0));
        result = 1u32;
        call_from(m, c, pg, s, buffer, &[], 0x00fa_caa1, guest(0x011c_3a30));
    } else {
        result = 0;
        call_from(m, c, pg, s, buffer, &[], 0x00fa_c962, guest(0x011c_3a30));
    }
    c.eax = (c.eax & !0xff) | result;
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4 + 8);
}

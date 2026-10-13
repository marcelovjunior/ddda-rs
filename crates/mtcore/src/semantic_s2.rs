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

// ---------------------------------------------------------------------
// 0x0079E200: character sweep step
// ---------------------------------------------------------------------

/// An x87 `fld dword; fstp dword` round trip: a copy that turns a
/// signalling NaN into the quiet one (the only value that changes).
fn x87_copy(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

/// The collision parameter block (`sCollision::Param`, 0xBC bytes) the
/// sweep builds on its stack. Offsets are the original's; the methods are
/// the trivial callees of 0x0079E200 done inline (the recorder runs their
/// original code, so the objects they leave are checked at every guest call).
#[derive(Clone, Copy)]
struct CollisionParam(u32);

impl CollisionParam {
    const VTABLE: u32 = 0x0157_d150;
    const DESTROYED_VTABLE: u32 = 0x0155_8134;

    /// 0x00DE45C0 `ctor(0x7fffffff, 0x3fffffff, 0, 0, 10, 0, 1, 1, 0, 1)`:
    /// all-layers query parameters with the engine defaults.
    fn construct(self, m: &mut dyn Mem, pg: Pages) {
        let p = self.0;
        pg.set_u32(m, p, Self::VTABLE);
        for off in [0x7c, 0x8c, 0xac] {
            pg.set_u32(m, p + off, 0);
        }
        pg.set_u32(m, p + 0xc, 0x3fff_ffff);
        pg.set_u32(m, p + 4, 0x7fff_ffff);
        pg.set_u32(m, p + 0x40, 0);
        pg.set_u32(m, p + 8, 1);
        pg.set_u32(m, p + 0x44, 10);
        pg.set_u32(m, p + 0x14, 0xf);
        for off in (0x18..=0x38).step_by(4) {
            pg.set_u32(m, p + off, 0);
        }
        pg.set_u8(m, p + 0x3c, 0);
        pg.set_u32(m, p + 0x48, 3);
        for (off, global) in [(0x4cu32, 0x0142_bc50u32), (0x58, 0x0140_8e58), (0x5c, 0x0142_bac8), (0x60, 0x0140_712c)] {
            let v = pg.u32(m, global);
            pg.set_u32(m, p + off, v);
        }
        let v = pg.u32(m, 0x0142_af90);
        pg.set_u32(m, p + 0x50, 0);
        pg.set_u32(m, p + 0x54, 0);
        pg.set_u32(m, p + 0x64, v);
        pg.set_u32(m, p + 0x6c, 0);
        for k in 0..3u32 {
            let v = pg.u32(m, 0x018c_fe80 + 4 * k);
            pg.set_u32(m, p + 0xa0 + 4 * k, v);
        }
        pg.set_u32(m, p + 0xac, 0);
        pg.set_u16(m, p + 0xb0, 0);
        pg.set_u8(m, p + 0xb2, 0);
        pg.set_u8(m, p + 0xb3, 1);
        pg.set_u32(m, p + 0xb4, 0x0100_0000);
        pg.set_u32(m, p + 0xb8, 0x0100_0101);
    }

    /// 0x004038F0: the destructor only resets the vtable.
    fn destroy(self, m: &mut dyn Mem, pg: Pages) {
        pg.set_u32(m, self.0, Self::DESTROYED_VTABLE);
    }

    /// 0x00DE4920: forget the cast shape.
    fn clear_shape(self, m: &mut dyn Mem, pg: Pages) {
        pg.set_u32(m, self.0 + 0x6c, 0);
    }

    /// 0x012BB7A0 `set_query(q)`.
    fn set_query(self, m: &mut dyn Mem, pg: Pages, query: u32) {
        pg.set_u32(m, self.0 + 0x10, query);
    }

    /// 0x00DE4710 `set_masks(high, low, third, _)` (the 4th argument is unused).
    fn set_masks(self, m: &mut dyn Mem, pg: Pages, high: u32, low: u32, third: u32) {
        pg.set_u32(m, self.0 + 4, high);
        pg.set_u32(m, self.0 + 0xc, low);
        pg.set_u32(m, self.0 + 8, third);
        pg.set_u8(m, self.0 + 0xb8, 1);
    }

    /// 0x00DE4730 `set_flags(flags, keep_top)`.
    fn set_flags(self, m: &mut dyn Mem, pg: Pages, flags: u32, keep_top: u8) {
        let current = pg.u32(m, self.0 + 0x40);
        let keep = keep_top != 0 && current & 0x8000_0000 != 0;
        pg.set_u32(m, self.0 + 0x40, if keep { flags | 0x8000_0000 } else { flags });
    }

    /// 0x00DE4B70 `finish(flag, margin)`.
    fn finish(self, m: &mut dyn Mem, pg: Pages, flag: u8, margin: u32) {
        let p = self.0;
        let flags = pg.u32(m, p + 0x40) | 0x8000_0000;
        pg.set_u32(m, p + 0x40, flags);
        pg.set_u8(m, p + 0xb5, 0);
        pg.set_u32(m, p + 0x5c, 0);
        if flag != 0 {
            pg.set_u32(m, p + 0x40, flags | 0x4000_0000);
            pg.set_u8(m, p + 0xb4, 0);
        } else {
            pg.set_u8(m, p + 0xb4, 1);
        }
        let v = pg.u32(m, 0x0142_bc50);
        pg.set_u32(m, p + 0x4c, v);
        pg.set_u32(m, p + 0x68, margin);
    }

    /// 0x00DE48B0 `set_ray(from, direction, 0, flag)`: cast a ray.
    fn set_ray(self, m: &mut dyn Mem, pg: Pages, from: u32, direction: u32, flag: u8) {
        let p = self.0;
        pg.set_u32(m, p + 0x6c, 2);
        for k in 0..4u32 {
            let v = pg.u32(m, from + 4 * k);
            pg.set_u32(m, p + 0x90 + 4 * k, v);
        }
        for k in 0..3u32 {
            let v = x87_copy(pg.u32(m, direction + 4 * k));
            pg.set_u32(m, p + 0xa0 + 4 * k, v);
        }
        pg.set_u32(m, p + 0xac, 0);
        pg.set_u8(m, p + 0xb0, 0);
        pg.set_u8(m, p + 0xb1, flag);
    }

    /// 0x00DE4780 `set_capsule(out, centre, axis, drop, reach, 0, flag)`:
    /// the capsule's end points `centre + axis * reach` and
    /// `centre - axis * drop`, written to `out` (0x10-byte vectors at +0 and
    /// +0x10) and copied into the block.
    #[allow(clippy::too_many_arguments)]
    fn set_capsule(self, m: &mut dyn Mem, pg: Pages, out: u32, centre: u32, axis: u32, drop: f32, reach: f32, flag: u8) {
        let p = self.0;
        let rd = |m: &dyn Mem, a: u32| f32::from_bits(pg.u32(m, a));
        let (ax, ay, az) = (rd(m, axis), rd(m, axis + 4), rd(m, axis + 8));
        let (cx, cy, cz) = (rd(m, centre), rd(m, centre + 4), rd(m, centre + 8));
        let top = [cx + ax * reach, cy + ay * reach, cz + az * reach];
        let bottom = [cx - ax * drop, cy - ay * drop, cz - az * drop];
        pg.set_u32(m, out + 8, top[2].to_bits());
        pg.set_u32(m, out, top[0].to_bits());
        pg.set_u32(m, out + 4, top[1].to_bits());
        pg.set_u32(m, out + 0xc, 0);
        pg.set_u32(m, out + 0x10, bottom[0].to_bits());
        pg.set_u32(m, out + 0x1c, 0);
        pg.set_u32(m, out + 0x14, bottom[1].to_bits());
        pg.set_u32(m, out + 0x18, bottom[2].to_bits());
        pg.set_u32(m, p + 0x6c, 1);
        for (src, dst) in [(0u32, 0x70u32), (4, 0x74), (8, 0x78), (0x10, 0x80), (0x14, 0x84), (0x18, 0x88)] {
            let v = x87_copy(pg.u32(m, out + src));
            pg.set_u32(m, p + dst, v);
        }
        pg.set_u32(m, p + 0x7c, 0);
        pg.set_u32(m, p + 0x8c, 0);
        pg.set_u8(m, p + 0xb0, 0);
        pg.set_u8(m, p + 0xb1, flag);
    }
}

// Translated from 0x0079e200 (DDDA.exe 2017-12-13 Steam).
/// 0x0079E200, `sweep->step(pos = [esp+4], next = [esp+8], anchor =
/// [esp+0xc], query = [esp+0x10], reanchor = byte [esp+0x14])` (`this` in
/// ESI, `ret 0x14`, void): one frame of a character's collision sweep.
///
/// The sweep object keeps a capsule (end points at +0xD0 and +0xE0, radius
/// at +0xF0, both end points first moved by the offset at +0x100), the
/// position it came from (+0x110) and the one it wants (+0x120, shifted by
/// the anchor's movement when `reanchor`). Unless the sweep is disabled
/// (byte +0x1AC, or the world flag at `[0x018FA4E4] + 0x9C448`) it builds a
/// collision parameter block on its stack, selects the cast shape by the
/// mode at +0x1A0 (1: capsule around the wanted position, 2: ray from the
/// capsule, otherwise none), lifts the start by a stepping height (+0x190
/// scaled by `query[0x14]`, or +0x194) when byte +0x1AD is set, and runs the
/// collision cast (0x010B4710 / 0x010AF350 for a short move, 0x010B4960 /
/// 0x010B4560 for a long one, the plain variants when the filter at
/// +0x28/+0x2C/+0x42/+0x44 is empty). The result flags go to +0x1A8 and the
/// resolved position to +0x140 and into `pos`; a hit (bit 3) with byte
/// +0x1AE set re-casts from `pos` (0x010FEA80), and byte +0x1AF adds a
/// ground probe below the final position (0x0111B570; before the cast it
/// also runs 0x0111B2B0 on the start). A disabled sweep just copies `pos`.
///
/// The original calls ten tiny methods of the parameter block; they are
/// done inline here ([`CollisionParam`]). Only the seven collision entry
/// points stay guest calls. The stack objects are rebuilt at the original's
/// addresses because those callees read them.
pub fn sweep_step(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let this = c.esi;
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let (pos, next, anchor, query) = (arg(m, 0), arg(m, 1), arg(m, 2), arg(m, 3));
    let reanchor = pg.u8(m, entry.wrapping_add(0x14));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // push ebp; and esp,-16; sub esp,0x1d8; push ebx; push edi
    let s = (entry.wrapping_sub(4) & !15).wrapping_sub(0x1d8 + 8);
    let at = |off: u32| s.wrapping_add(off);
    let edi = this.wrapping_add(0xd0);
    (c.ebx, c.ebp, c.edi) = (pos, entry.wrapping_sub(4), edi);
    let t = |off: u32| this.wrapping_add(off);
    let rd = |m: &dyn Mem, a: u32| f32::from_bits(pg.u32(m, a));
    let wr = |m: &mut dyn Mem, a: u32, v: f32| pg.set_u32(m, a, v.to_bits());
    let guest = |target: u32| move |m: &mut dyn Mem, c: &mut Cpu| ddnative_base::x86::call_ext(m, c, target);

    let param = CollisionParam(at(0x60));
    pg.set_u32(m, at(0x1c), 0);
    pg.set_u32(m, at(0x3c), 0);
    param.construct(m, pg);

    // x87 round trips of the capsule fields, then the offset added to both end points
    for off in [0xd0u32, 0xd4, 0xd8, 0xe0, 0xe4, 0xe8, 0xf0] {
        let v = x87_copy(pg.u32(m, t(off)));
        pg.set_u32(m, t(off), v);
    }
    pg.set_u32(m, t(0xdc), 0);
    pg.set_u32(m, t(0xec), 0);
    let v = rd(m, t(0x100)) + rd(m, t(0xd0));
    wr(m, t(0xd0), v);
    let v = rd(m, t(0xd4)) + rd(m, t(0x104));
    wr(m, t(0xd4), v);
    let v = rd(m, t(0xd8)) + rd(m, t(0x108));
    wr(m, t(0xd8), v);
    let v = rd(m, t(0x100)) + rd(m, t(0xe0));
    wr(m, t(0xe0), v);
    let v = rd(m, t(0x104)) + rd(m, t(0xe4));
    wr(m, t(0xe4), v);
    let v = rd(m, t(0xe8)) + rd(m, t(0x108));
    wr(m, t(0xe8), v);
    // the previous and wanted positions
    for (src, dst) in [(pos, 0x110u32), (next, 0x120)] {
        for k in 0..3u32 {
            let v = x87_copy(pg.u32(m, src + 4 * k));
            pg.set_u32(m, t(dst + 4 * k), v);
        }
        pg.set_u32(m, t(dst + 0xc), 0);
    }
    if reanchor != 0 {
        let delta = [rd(m, anchor) - rd(m, edi), rd(m, anchor + 4) - rd(m, edi + 4), rd(m, anchor + 8) - rd(m, edi + 8)];
        let wanted = [rd(m, t(0x120)) + delta[0], rd(m, t(0x124)) + delta[1], rd(m, t(0x128)) + delta[2]];
        wr(m, t(0x124), wanted[1]);
        wr(m, t(0x120), wanted[0]);
        wr(m, t(0x128), wanted[2]);
    }

    let finish = |m: &mut dyn Mem, c: &mut Cpu| {
        param.destroy(m, pg);
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(4 + 0x14);
    };
    // a disabled sweep passes `pos` through
    if pg.u8(m, t(0x1ac)) != 0 || pg.u8(m, pg.u32(m, 0x018f_a4e4).wrapping_add(0x9c448)) == 0 {
        pg.set_u32(m, t(0x1a8), 1);
        for base in [0x130u32, 0x140] {
            for k in 0..3u32 {
                let v = x87_copy(pg.u32(m, pos + 4 * k));
                pg.set_u32(m, t(base + 4 * k), v);
            }
            pg.set_u32(m, t(base + 0xc), 0);
        }
        return finish(m, c);
    }

    // the start (S+0x10) and wanted (S+0x30) positions, raw
    let stepping = pg.u8(m, t(0x1ad));
    for k in 0..3u32 {
        let v = pg.u32(m, t(0x110 + 4 * k));
        pg.set_u32(m, at(0x10 + 4 * k), v);
        let v = pg.u32(m, t(0x120 + 4 * k));
        pg.set_u32(m, at(0x30 + 4 * k), v);
    }
    pg.set_u8(m, at(0xf), stepping);
    pg.set_u32(m, at(0x1c), 0);
    pg.set_u32(m, at(0x3c), 0);
    if stepping != 0 {
        let epsilon = rd(m, 0x0150_a350);
        let height = rd(m, query + 0x14);
        let y = rd(m, at(0x14));
        let lifted = if epsilon > height { rd(m, t(0x194)) + y } else { rd(m, t(0x190)) * height + y };
        wr(m, at(0x14), lifted);
    }
    // a second parameter block (S+0x120) for the ground probes
    let probe = CollisionParam(at(0x120));
    let world = |m: &dyn Mem| pg.u32(m, 0x018d_0dd0);
    if pg.u8(m, t(0x1af)) != 0 {
        let r = pg.u32(m, t(0xf0));
        pg.set_u32(m, at(0x20), 0);
        pg.set_u32(m, at(0x24), r);
        pg.set_u32(m, at(0x28), 0);
        pg.set_u32(m, at(0x2c), r);
        probe.construct(m, pg);
        probe.set_query(m, pg, query);
        let masks = (pg.u32(m, t(0x19c)), pg.u32(m, t(0x198)));
        probe.set_masks(m, pg, masks.0, masks.1, 1);
        let ecx = world(m);
        call_from(m, c, pg, s, ecx, &[at(0x10), at(0x20), t(0x10), probe.0], 0x0079_e51e, guest(0x0111_b2b0));
        probe.destroy(m, pg);
    }
    for (src, dst) in [(0x10u32, 0x130u32), (0x14, 0x134), (0x18, 0x138)] {
        let v = pg.u32(m, at(src));
        pg.set_u32(m, t(dst), v);
    }
    pg.set_u32(m, t(0x13c), 0);

    // the cast shape
    let mode = pg.u32(m, t(0x1a0));
    if mode == 2 {
        let flag_b = pg.u8(m, t(0x1b0));
        let axis = [pg.u32(m, 0x0162_9540), pg.u32(m, 0x0162_9544), pg.u32(m, 0x0162_9548)];
        let radius = rd(m, t(0xf0));
        let k = f64::from_bits(u64::from(pg.u32(m, 0x0161_c9f0)) | u64::from(pg.u32(m, 0x0161_c9f4)) << 32);
        let mut reach = (f64::from(radius) * k) as f32;
        if stepping == 0 {
            reach = (rd(m, t(0x124)) - rd(m, t(0x134))) + reach;
        }
        let from = if rd(m, t(0xd4)) > rd(m, t(0xe4)) { 0xe0u32 } else { 0xd0 };
        let (x, y, z) = (rd(m, t(from)) + rd(m, at(0x30)), rd(m, t(from + 4)) + rd(m, at(0x34)), rd(m, t(from + 8)) + rd(m, at(0x38)));
        wr(m, at(0x20), x);
        let w = pg.u32(m, t(0x1a4));
        pg.set_u32(m, at(0x2c), w);
        wr(m, at(0x24), y);
        pg.set_u32(m, t(0x160), x.to_bits());
        pg.set_u32(m, t(0x164), y.to_bits());
        wr(m, at(0x28), z);
        pg.set_u32(m, t(0x168), z.to_bits());
        pg.set_u32(m, t(0x16c), w);
        let sign = pg.u32(m, 0x0161_c280);
        for (k, bits) in axis.into_iter().enumerate() {
            wr(m, t(0x150 + 4 * k as u32), f32::from_bits(bits ^ sign) * reach);
        }
        pg.set_u32(m, t(0x15c), 0);
        param.clear_shape(m, pg);
        param.set_ray(m, pg, t(0x160), t(0x150), flag_b);
    } else if mode == 1 {
        let flag_b = pg.u8(m, t(0x1b0));
        param.clear_shape(m, pg);
        let radius = rd(m, t(0xf0));
        let drop = radius * rd(m, 0x014f_0e9c);
        param.set_capsule(m, pg, t(0x170), at(0x30), 0x0162_9540, drop, radius, flag_b);
    } else {
        param.clear_shape(m, pg);
        param.clear_shape(m, pg);
    }
    let masks = (pg.u32(m, t(0x19c)), pg.u32(m, t(0x198)));
    param.set_masks(m, pg, masks.0, masks.1, 1);
    let flag = u8::from(pg.u8(m, at(0xf)) == 0);
    let margin = pg.u32(m, 0x014e_ae5c);
    param.finish(m, pg, flag, margin);
    if pg.u8(m, t(0x1b1)) != 0 {
        param.set_flags(m, pg, 1, 1);
    }

    // the cast itself
    let (x, y, z) = (rd(m, edi), rd(m, edi + 4), rd(m, edi + 8));
    let (dx, dy, dz) = (x - rd(m, t(0xe0)), y - rd(m, t(0xe4)), z - rd(m, t(0xe8)));
    let length = (dx * dx + dy * dy + dz * dz).sqrt();
    pg.set_u32(m, at(0x4c), t(0x10));
    let plain = pg.u32(m, t(0x44)) == 0 && pg.u32(m, t(0x28)) == 0 && pg.u32(m, t(0x2c)) == u32::MAX && pg.u8(m, t(0x42)) == 0;
    let ecx = world(m);
    let result = if rd(m, 0x0150_a350) > length {
        let r = pg.u32(m, t(0xf0));
        pg.set_u32(m, at(0x2c), r);
        pg.set_u32(m, at(0x20), x.to_bits());
        pg.set_u32(m, at(0x24), y.to_bits());
        pg.set_u32(m, at(0x28), z.to_bits());
        let (target, back) = if plain { (0x010b_4710, 0x0079_e83d) } else { (0x010a_f350, 0x0079_e81c) };
        call_from(m, c, pg, s, ecx, &[at(0x10), at(0x30), at(0x20), param.0], back, guest(target))
    } else {
        let (target, back) = if plain { (0x010b_4960, 0x0079_e88f) } else { (0x010b_4560, 0x0079_e872) };
        call_from(m, c, pg, s, ecx, &[at(0x10), at(0x30), edi, param.0], back, guest(target))
    };
    pg.set_u32(m, t(0x1a8), result);
    let store_resolved = |m: &mut dyn Mem| {
        let resolved = [pg.u32(m, at(0x10)), pg.u32(m, at(0x14)), pg.u32(m, at(0x18))];
        for (k, v) in resolved.into_iter().enumerate() {
            pg.set_u32(m, t(0x140 + 4 * k as u32), v);
        }
        pg.set_u32(m, t(0x14c), 0);
        resolved
    };
    let mut resolved = store_resolved(m);
    // a blocked cast re-casts from the original position
    if result != 0 && result & 8 != 0 && pg.u8(m, t(0x1ae)) != 0 {
        for k in 0..3u32 {
            let v = pg.u32(m, pos + 4 * k);
            pg.set_u32(m, at(0x10 + 4 * k), v);
        }
        pg.set_u32(m, at(0x1c), 0);
        let ecx = world(m);
        call_from(m, c, pg, s, ecx, &[at(0x10), at(0x30), edi, param.0], 0x0079_e926, guest(0x010f_ea80));
        resolved = store_resolved(m);
    }
    for (k, v) in resolved.into_iter().enumerate() {
        pg.set_u32(m, pos + 4 * k as u32, v);
    }
    pg.set_u32(m, pos + 0xc, 0);

    // the ground probe below the resolved position
    if pg.u8(m, t(0x1af)) != 0 {
        let r = pg.u32(m, t(0xf0));
        let below = f32::from_bits(resolved[1]) - rd(m, 0x0150_a088);
        pg.set_u32(m, at(0x20), 0);
        pg.set_u32(m, at(0x24), r);
        pg.set_u32(m, at(0x28), 0);
        pg.set_u32(m, at(0x2c), r);
        pg.set_u32(m, at(0x50), resolved[0]);
        wr(m, at(0x54), below);
        pg.set_u32(m, at(0x58), resolved[2]);
        pg.set_u32(m, at(0x5c), 0);
        probe.construct(m, pg);
        let masks = (pg.u32(m, t(0x19c)), pg.u32(m, t(0x198)));
        probe.set_masks(m, pg, masks.0, masks.1, 1);
        probe.set_query(m, pg, query);
        let filter = pg.u32(m, at(0x4c));
        let ecx = world(m);
        call_from(m, c, pg, s, ecx, &[at(0x50), pos, at(0x20), filter, probe.0], 0x0079_ea38, guest(0x0111_b570));
        probe.destroy(m, pg);
    }
    finish(m, c);
}

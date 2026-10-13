//! Semantic port (L176) of sSceneExt::vf16 0x0049B1A0, the light setup of
//! the model-draw path: the scene's lights that reach a draw, ordered, and
//! written into the draw context's light constant block. It keeps the
//! original's ABI-visible result (EAX, the callee-saved registers, ESP,
//! every memory write outside the popped stack scratch); ECX, EDX, the
//! flags and the scratch are not reproduced. Checked against the original's
//! recorded vectors in tests/material.rs; opt-in through crates/ddports
//! (`DDRUNTIME_LIGHTS=1`).
//!
//! The original builds the block's 0x190 bytes in its stack frame and
//! copies them whole, entries it did not fill included (what the stack held
//! before). The port therefore keeps its locals in guest memory at the
//! original's frame addresses (the frame aligned to 16 bytes as the
//! original's), so the block carries the same bytes.
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{self, Cpu};

/// The default light terms when no light is asked for (four floats).
const DEFAULTS: u32 = 0x018C_FE70;
/// The overlap test's lane mask and all-ones (16 bytes each).
const LANE_MASK: u32 = 0x0162_AC60;
const ALL_ONES: u32 = 0x0162_AC30;
/// 2^32 as a float (the unsigned correction of fild).
const TWO_32: u32 = 0x0161_C220;
/// The context's arena refill (thiscall, the size; a guest call).
const REFILL: u32 = 0x00FE_D890;

/// A float copied through the x87 stack (fld/fstp, 32-bit both ways):
/// unchanged but for a signalling NaN, which the load quiets.
pub(crate) fn through_x87(v: u32) -> u32 {
    if v & 0x7f80_0000 == 0x7f80_0000 && v & 0x007f_ffff != 0 {
        v | 0x0040_0000
    } else {
        v
    }
}

/// fild of a 32-bit word, +2^32 (the image's constant) when it is negative,
/// fstp to 32 bits: the word as an unsigned count, rounded to nearest.
fn unsigned_to_float(m: &dyn Mem, pg: Pages, v: u32) -> u32 {
    let mut x = v as i32 as f64;
    if (v as i32) < 0 {
        x += f32::from_bits(pg.u32(m, TWO_32)) as f64;
    }
    (x as f32).to_bits()
}

fn push(m: &mut dyn Mem, pg: Pages, c: &mut Cpu, v: u32) {
    c.esp = c.esp.wrapping_sub(4);
    pg.set_u32(m, c.esp, v);
}

/// A guest call from the original's frame `s`: one argument, the return
/// address the original pushes, ECX; the callee pops its argument.
fn call(m: &mut dyn Mem, c: &mut Cpu, s: u32, ecx: u32, arg: u32, resume: u32, target: u32) -> u32 {
    let pg = Pages::of(m);
    c.esp = s;
    push(m, pg, c, arg);
    push(m, pg, c, resume);
    c.ecx = ecx;
    x86::call_ext(m, c, target);
    c.esp = s;
    c.eax
}

/// Append a record of a cache change when the context records (bit 0x80
/// of +0x5C): the tag, then the cache's previous two words.
fn record(m: &mut dyn Mem, pg: Pages, ctx: u32, tag: u32, first: u32, step: u32) {
    let at = |o: u32| ctx.wrapping_add(o);
    let slot = |m: &dyn Mem| ctx.wrapping_add(pg.u32(m, at(0x6c)).wrapping_mul(12)).wrapping_add(0x30c7c);
    let r = slot(m);
    let w = pg.u32(m, r);
    pg.set_u32(m, r, (w & (0x8000_0000 | tag)) | tag);
    let r = slot(m);
    let w = pg.u32(m, r);
    pg.set_u32(m, r, w & 0x7fff_ffff);
    let a = pg.u32(m, at(first));
    let r = slot(m);
    pg.set_u32(m, r.wrapping_add(4), a);
    let b = pg.u32(m, at(first.wrapping_add(4)));
    pg.set_u32(m, r.wrapping_add(8), b);
    let n = pg.u32(m, at(0x6c));
    pg.set_u32(m, at(0x6c), n.wrapping_add(step));
}

// Translated from 0x0049B1A0 (DDDA.exe 2017-12-13 Steam).
/// 0x0049B1A0, sSceneExt::vf16 `this->lights(context, most, bounds, kinds,
/// groups, y_scale, near_scale, far_scale)` (thiscall, `ret 0x20`).
///
/// Unless `most`, `kinds` or `groups` is zero: the scene's first list of
/// lights (`[this+0x6C]`, `[this+0x70]` entries of `[this+0x20]`) gives up
/// to `most` lights of type 1 that are enabled (state bits) and match
/// `kinds` and `groups`; the second list (`+0x64`, `+0x68`) adds the
/// matching lights whose bounds (virtual +0x4C) overlap `bounds`, in
/// descending priority (`[light+0x30]`, insertion order kept for equal
/// priorities, duplicates skipped), up to `most`. Of those, type-2 lights
/// (at most `[this+0xB78]`) follow the first list's; the others fill the
/// list from its end. Each listed light gives three terms: its position
/// words (+0x5D4, +0x5D8 scaled by `y_scale`, +0x5E0, 0), and two vectors
/// from its virtual +0x50 and +0x54 (copied through the x87 stack); the
/// first terms' x are scaled by `near_scale` for the first list's lights,
/// by `far_scale` for the rest. With no lights asked for, the head terms
/// are the defaults at 0x018CFE70.
///
/// Then four of the context's cached states take the scene's (each change
/// recorded when the context records), and the context's light constants
/// (0x190 bytes: the counts, then the three terms of 8 lights) go to a new
/// block of its arena (0x00FED890 when it runs out, a guest call).
pub fn scene_lights(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let e = c.esp;
    let arg = |k: u32| e.wrapping_add(4 + 4 * k);
    // the frame: ebp = e - 4, aligned to 16, 0x224 bytes of locals, three
    // saved registers
    let s = ((e.wrapping_sub(4) & !0xf).wrapping_sub(0x224)).wrapping_sub(12);
    let l = |o: u32| s.wrapping_add(o);
    let this = c.ecx;
    pg.set_u32(m, l(0x18), this);
    let mut ebx: u32 = 0;

    let asked = pg.u32(m, arg(1)) != 0 && pg.u32(m, arg(3)) != 0 && pg.u32(m, arg(4)) != 0;
    if asked {
        // the first list: enabled type-1 lights of the asked kinds and groups
        let mut i = 0u32;
        if pg.u32(m, this.wrapping_add(0x70)) > 0 {
            loop {
                if ebx >= pg.u32(m, arg(1)) {
                    break;
                }
                let first = pg.u32(m, this.wrapping_add(0x6c));
                let table = pg.u32(m, this.wrapping_add(0x20));
                let light = pg.u32(m, table.wrapping_add(first.wrapping_add(i).wrapping_mul(4)));
                let state = pg.u32(m, light.wrapping_add(4));
                if state & 7 == 2 && state & 0x4800 == 0x4800 {
                    let kind = pg.u32(m, light.wrapping_add(0x5d0));
                    if pg.u32(m, arg(3)) & kind != 0 && pg.u32(m, light.wrapping_add(0x5e4)) & pg.u32(m, arg(4)) != 0 && kind == 1 {
                        pg.set_u32(m, l(0x7c).wrapping_add(ebx.wrapping_mul(4)), light);
                        ebx = ebx.wrapping_add(1);
                    }
                }
                i = i.wrapping_add(1);
                if i >= pg.u32(m, this.wrapping_add(0x70)) {
                    break;
                }
            }
        }
        pg.set_u32(m, l(0x14), ebx);
        pg.set_u32(m, l(0x10), 0);
        // the second list: by bounds and priority
        if pg.u32(m, this.wrapping_add(0x68)) > 0 {
            loop {
                let j = pg.u32(m, l(0x10));
                let scene = pg.u32(m, l(0x18));
                let first = pg.u32(m, scene.wrapping_add(0x64));
                let table = pg.u32(m, scene.wrapping_add(0x20));
                let light = pg.u32(m, table.wrapping_add(first.wrapping_add(j).wrapping_mul(4)));
                pg.set_u32(m, l(0x3c), 0);
                pg.set_u32(m, l(0x4c), 0);
                let bounds_fn = pg.u32(m, pg.u32(m, light).wrapping_add(0x4c));
                call(m, c, s, light, l(0x30), 0x0049_B26E, bounds_fn);
                'next: {
                    let state = pg.u32(m, light.wrapping_add(4));
                    if !(state & 7 == 2 && state & 0x4800 == 0x4800) {
                        break 'next;
                    }
                    if pg.u32(m, light.wrapping_add(0x5d0)) & pg.u32(m, arg(3)) == 0 {
                        break 'next;
                    }
                    if pg.u32(m, light.wrapping_add(0x5e4)) & pg.u32(m, arg(4)) == 0 {
                        break 'next;
                    }
                    if !overlaps(m, pg, pg.u32(m, arg(2)), l(0x30)) {
                        break 'next;
                    }
                    let priority = pg.u32(m, light.wrapping_add(0x30)); // 0x007261D0
                    pg.set_u32(m, l(0x1c), priority);
                    let list = |k: u32| l(0x58).wrapping_add(k.wrapping_mul(4));
                    let mut k = pg.u32(m, l(0x14));
                    if k < ebx {
                        loop {
                            let other = pg.u32(m, list(k));
                            if other == light {
                                break 'next;
                            }
                            if pg.u32(m, other.wrapping_add(0x30)) < pg.u32(m, l(0x1c)) {
                                // insert before it
                                let mut a = ebx;
                                while a > k {
                                    let v = pg.u32(m, list(a).wrapping_sub(4));
                                    pg.set_u32(m, list(a), v);
                                    a = a.wrapping_sub(1);
                                }
                                pg.set_u32(m, list(k), light);
                                if ebx < pg.u32(m, arg(1)) {
                                    ebx = ebx.wrapping_add(1);
                                }
                                break 'next;
                            }
                            k = k.wrapping_add(1);
                            if k >= ebx {
                                break;
                            }
                        }
                    }
                    if ebx < pg.u32(m, arg(1)) {
                        pg.set_u32(m, list(ebx), light);
                        ebx = ebx.wrapping_add(1);
                    }
                }
                let j = pg.u32(m, l(0x10)).wrapping_add(1);
                pg.set_u32(m, l(0x10), j);
                if j >= pg.u32(m, pg.u32(m, l(0x18)).wrapping_add(0x68)) {
                    break;
                }
            }
        }

        // type-2 lights after the first list's, the others from the end
        let k0 = pg.u32(m, l(0x14));
        let mut front = k0;
        pg.set_u32(m, l(0x10), k0);
        if k0 < ebx {
            let mut back = l(0x7c).wrapping_add(ebx.wrapping_mul(4));
            pg.set_u32(m, l(0x10), back);
            let mut typed = 0u32;
            let mut k = k0;
            loop {
                let light = pg.u32(m, l(0x58).wrapping_add(k.wrapping_mul(4)));
                if pg.u32(m, light.wrapping_add(0x5d0)) == 2 {
                    let scene = pg.u32(m, l(0x18));
                    if typed >= pg.u32(m, scene.wrapping_add(0xb78)) {
                        break;
                    }
                    pg.set_u32(m, l(0x7c).wrapping_add(front.wrapping_mul(4)), light);
                    front = front.wrapping_add(1);
                    typed = typed.wrapping_add(1);
                } else {
                    back = pg.u32(m, l(0x10)).wrapping_sub(4);
                    pg.set_u32(m, l(0x10), back);
                    pg.set_u32(m, back, light);
                }
                k = k.wrapping_add(1);
                if k >= ebx {
                    break;
                }
            }
            pg.set_u32(m, l(0x10), front);
        }

        // the head terms: the counts as floats
        let k0 = pg.u32(m, l(0x14));
        let k0f = unsigned_to_float(m, pg, pg.u32(m, l(0x14)));
        pg.set_u32(m, l(0x20), k0f);
        let count = pg.u32(m, l(0x10));
        pg.set_u32(m, l(0x1c), count);
        let countf = unsigned_to_float(m, pg, pg.u32(m, l(0x1c)));
        pg.set_u32(m, l(0x24), countf);
        pg.set_u32(m, l(0x28), countf);

        // each listed light's three terms
        for i in 0..count {
            let light = pg.u32(m, l(0x7c).wrapping_add(i.wrapping_mul(4)));
            let o = i.wrapping_mul(16);
            let t = |base: u32, k: u32| l(base).wrapping_add(o).wrapping_add(4 * k);
            let x = pg.u32(m, light.wrapping_add(0x5d4));
            let vtable = pg.u32(m, light);
            let y = pg.u32(m, light.wrapping_add(0x5d8));
            let z = pg.u32(m, light.wrapping_add(0x5e0));
            let second_fn = pg.u32(m, vtable.wrapping_add(0x50));
            pg.set_u32(m, t(0xa0, 0), x);
            pg.set_u32(m, t(0xa0, 1), y);
            pg.set_u32(m, t(0xa0, 2), z);
            pg.set_u32(m, t(0xa0, 3), 0);
            let r = call(m, c, s, light, l(0x120), 0x0049_B454, second_fn);
            for k in 0..4 {
                let v = through_x87(pg.u32(m, r.wrapping_add(4 * k)));
                pg.set_u32(m, t(0x130, k), v);
            }
            let third_fn = pg.u32(m, pg.u32(m, light).wrapping_add(0x54));
            let r = call(m, c, s, light, l(0x30), 0x0049_B489, third_fn);
            let v = through_x87(pg.u32(m, r));
            let y = pg.u32(m, t(0xa0, 1));
            pg.set_u32(m, t(0x1b0, 0), v);
            let y = x86::mulss(y, pg.u32(m, arg(5)));
            let v = through_x87(pg.u32(m, r.wrapping_add(4)));
            pg.set_u32(m, t(0x1b0, 1), v);
            pg.set_u32(m, t(0xa0, 1), y);
            for k in 2..4 {
                let v = through_x87(pg.u32(m, r.wrapping_add(4 * k)));
                pg.set_u32(m, t(0x1b0, k), v);
            }
        }

        // the first terms' x: near scale for the first list's lights, far
        // scale for the rest (the original's operand order, which decides a
        // NaN's payload, by unrolled position)
        let x_of = |i: u32| l(0xa0).wrapping_add(i.wrapping_mul(16));
        let scale = |m: &mut dyn Mem, i: u32, s: u32, s_first: bool| {
            let x = pg.u32(m, x_of(i));
            pg.set_u32(m, x_of(i), if s_first { x86::mulss(s, x) } else { x86::mulss(x, s) });
        };
        let near = pg.u32(m, arg(6));
        let mut i = 0u32;
        if (k0 as i32) >= 4 {
            let groups = (k0.wrapping_sub(4) >> 2).wrapping_add(1);
            for g in 0..groups {
                for (p, s_first) in [(0, false), (1, false), (2, true), (3, false)] {
                    scale(m, 4 * g + p, near, s_first);
                }
            }
            i = groups.wrapping_mul(4);
        }
        while i < k0 {
            scale(m, i, near, false);
            i = i.wrapping_add(1);
        }
        let count = pg.u32(m, l(0x10));
        if k0 < count {
            let far = pg.u32(m, arg(7));
            let mut i = k0;
            if (count.wrapping_sub(k0) as i32) >= 4 {
                let groups = (count.wrapping_sub(k0).wrapping_sub(4) >> 2).wrapping_add(1);
                for g in 0..groups {
                    for (p, s_first) in [(0, true), (1, true), (2, false), (3, false)] {
                        scale(m, k0 + 4 * g + p, far, s_first);
                    }
                }
                i = k0.wrapping_add(groups.wrapping_mul(4));
            }
            while i < count {
                scale(m, i, far, false);
                i = i.wrapping_add(1);
            }
        }
    } else {
        for k in 0..4 {
            let v = pg.u32(m, DEFAULTS + 4 * k);
            pg.set_u32(m, l(0x20 + 4 * k), v);
        }
    }

    // the cached states the scene sets (tag, cache, the scene's word, the
    // value the cache's second word takes)
    let ctx = pg.u32(m, arg(0));
    let at = |o: u32| ctx.wrapping_add(o);
    for (tag, cache, source, value) in [
        (0x467u32, 0x2530u32, 0x2320u32, 0x007D_8464u32),
        (0x468, 0x2538, 0x2328, 0x3E64_8465),
        (0x469, 0x2540, 0x2348, 0x469),
        (0x46b, 0x2550, 0x2358, 0x46b),
    ] {
        if pg.u32(m, at(cache)) != pg.u32(m, pg.u32(m, at(0x20)).wrapping_add(source)) {
            if pg.u8(m, at(0x5c)) & 0x80 != 0 {
                record(m, pg, ctx, tag, cache, 1);
            }
            let v = pg.u32(m, pg.u32(m, at(0x20)).wrapping_add(source));
            pg.set_u32(m, at(cache), v);
            pg.set_u32(m, at(cache + 4), value);
            let f = pg.u32(m, at(0x154));
            pg.set_u32(m, at(0x154), f | 3);
        }
    }
    let f = pg.u32(m, at(0x5c));
    pg.set_u32(m, at(0x5c), f | 2);
    if pg.u8(m, at(0x5c)) & 0x80 != 0 {
        record(m, pg, ctx, 0x1ed, 0x1160, 1);
    }

    // the light constants: a new block of the arena
    let pass = pg.u32(m, pg.u32(m, at(0x18)).wrapping_add(0x7b4));
    let size = (pg.u16(m, pass.wrapping_add(0x18)) as u32).wrapping_mul(4).wrapping_add(0xf) & !0xf;
    if pg.u32(m, at(0x24)).wrapping_add(size) > pg.u32(m, at(0x28)) {
        call(m, c, s, ctx, size, 0x0049_B933, REFILL);
    }
    let block = pg.u32(m, at(0x24));
    pg.set_u32(m, at(0x24), block.wrapping_add(size));
    pg.set_u32(m, at(0x1160), block);
    let f = pg.u32(m, at(0x154));
    pg.set_u32(m, at(0x154), f | 1);
    c.eax = if block == 0 {
        size
    } else {
        for (to, from, words) in [(0u32, 0x20u32, 4u32), (0x10, 0xa0, 32), (0x90, 0x130, 32), (0x110, 0x1b0, 32)] {
            for k in 0..words {
                let v = pg.u32(m, l(from + 4 * k));
                pg.set_u32(m, block.wrapping_add(to + 4 * k), v);
            }
        }
        let f = pg.u32(m, at(0x5c));
        pg.set_u32(m, at(0x5c), f & !2);
        l(0x230)
    };
    c.esp = e.wrapping_add(0x24);
}

/// The original's packed overlap test of `bounds` (min at 0, max at 0x10)
/// with the light's bounds at `light` (min at 0, max at 0x10): every lane
/// `light.min <= bounds.max` and `bounds.min <= light.max` (ordered), the
/// lanes the image's mask selects forced to the all-ones constant's.
fn overlaps(m: &dyn Mem, pg: Pages, bounds: u32, light: u32) -> bool {
    let w = |a: u32, k: u32| pg.u32(m, a.wrapping_add(4 * k));
    let le = |a: u32, b: u32| if f32::from_bits(a) <= f32::from_bits(b) { u32::MAX } else { 0 };
    (0..4).all(|k| {
        let both = le(w(light, k), w(bounds, 4 + k)) & le(w(bounds, k), w(light, 4 + k));
        let mask = w(LANE_MASK, k);
        let ones = w(ALL_ONES, k);
        ((!mask & both) | (mask & ones)) == ones
    })
}

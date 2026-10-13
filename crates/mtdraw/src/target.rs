//! Semantic port (L176) of the draw context's render-target setter
//! 0x00FEFB90, on the shadow and main-pass model-draw path (from
//! 0x00FF1090, the pass's target selection). It keeps the original's
//! ABI-visible result (EAX, the callee-saved registers, ESP, every memory
//! write outside the popped stack scratch); ECX, EDX, the flags and the
//! scratch are not reproduced. Checked against the original's recorded
//! vectors in tests/target.rs; opt-in through crates/ddports
//! (`DDRUNTIME_MATERIAL=1`).
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{self, Cpu};

/// The frame serial objects are stamped with (a counter).
const SERIAL: u32 = 0x018D_2CE8;
/// The constant-buffer arena's allocator object (a pointer), and its
/// allocation (thiscall, one argument).
const ALLOCATOR: u32 = 0x018D_08F4;
const ALLOCATE: u32 = 0x00DA_7D70;
/// The CRT memcpy (cdecl), a platform model.
const MEMCPY: u32 = 0x012E_FEA0;
/// (slot, source) u16 pairs, zero-terminated: the target's state slots.
const SLOT_TABLE: u32 = 0x0144_7B7C;
/// Floats in the image: one, the viewport scale, the sign mask.
const ONE: u32 = 0x0140_6CE0;
const VIEW_SCALE: u32 = 0x0140_6CF0;
const SIGN: u32 = 0x0161_C280;

// Translated from 0x00FEFB90 (DDDA.exe 2017-12-13 Steam).
/// 0x00FEFB90, `context->set_target(target, depth, flags, unused)`
/// (thiscall, `ret 0x10`).
///
/// Unless the context already has this target, depth surface and flags
/// (and its state's low bits read 1): binds them, stamps them with the
/// frame serial, records the target's format (marking a change in the
/// context's flags at +0x154), copies a new target's state slots (the
/// slot table at 0x01447B7C; through its source index when the target's
/// bit 31 is set), then allocates the next viewport constant block from
/// the context's arena (0x00DA7D70 when it runs out, a guest call),
/// copies the previous block (same size) or the pass's template (new
/// size) into it and writes the target size, its reciprocal and, for a
/// new size, the viewport's offset and scale. EAX: `flags` on the early
/// return, the block size when the arena is exhausted on a new size,
/// otherwise memcpy's (the block).
///
/// The floats use the original's SSE steps through the same hardware
/// helpers as the lifted code; the reads and writes keep its order.
pub fn set_render_target(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let ctx = c.ecx;
    let at = |o: u32| ctx.wrapping_add(o);
    let flags = pg.u32(m, entry.wrapping_add(0xc));
    let target = pg.u32(m, entry.wrapping_add(4));
    let depth = pg.u32(m, entry.wrapping_add(8));
    let done = |c: &mut Cpu, eax: u32| {
        c.eax = eax;
        c.esp = entry.wrapping_add(0x14);
    };
    if pg.u32(m, at(0xb4)) == target && pg.u32(m, at(0xc4)) == depth {
        let state = pg.u32(m, at(0xc8));
        if (state >> 4) & 0xfff == flags && state & 7 == 1 {
            return done(c, flags);
        }
    }
    if depth != 0 {
        if flags & 1 != 0 {
            pg.set_u32(m, at(0x1d8), target);
            pg.set_u32(m, at(0x1dc), depth);
        }
        if flags & 0x43 != 0 && pg.u8(m, depth.wrapping_add(0x20)) & 0x80 != 0 {
            let owner = pg.u32(m, pg.u32(m, pg.u32(m, depth.wrapping_add(0x10)).wrapping_add(0x44)));
            pg.set_u32(m, at(0x1d8), owner);
            pg.set_u32(m, at(0x1dc), depth);
            if owner != 0 {
                let serial = pg.u32(m, SERIAL).wrapping_add(1);
                pg.set_u32(m, owner.wrapping_add(4), serial);
            }
        }
    }
    let state = pg.u32(m, at(0xc8));
    pg.set_u32(m, at(0xc4), depth);
    pg.set_u32(m, at(0xc8), ((flags & 0xfff) << 4) | (state & 0xffff_0009) | 1);
    let format = if target != 0 {
        let serial = pg.u32(m, SERIAL).wrapping_add(1);
        let bits = pg.u32(m, target.wrapping_add(0x20));
        pg.set_u32(m, target.wrapping_add(4), serial);
        (bits >> 20) & 0xf
    } else {
        0xd
    };
    if pg.u32(m, at(0x170)) != format {
        let f = pg.u32(m, at(0x154));
        pg.set_u32(m, at(0x154), f | 3);
        pg.set_u32(m, at(0x170), format);
    }
    if depth != 0 {
        let serial = pg.u32(m, SERIAL).wrapping_add(1);
        pg.set_u32(m, depth.wrapping_add(4), serial);
    }
    if pg.u32(m, at(0xb4)) != target {
        pg.set_u32(m, at(0xb4), target);
        if target != 0 {
            let indirect = pg.u32(m, target.wrapping_add(0x20)) & 0x8000_0000 != 0;
            let mut entry = SLOT_TABLE;
            let mut slot = pg.u32(m, entry) & 0xffff;
            while slot != 0 {
                let source = if indirect { pg.u16(m, entry.wrapping_add(2)) as u32 } else { slot };
                let table = pg.u32(m, at(0x20)).wrapping_add(source.wrapping_mul(8));
                let to = ctx.wrapping_add(slot.wrapping_mul(8));
                let first = pg.u32(m, table);
                pg.set_u32(m, to.wrapping_add(0x1f8), first);
                let second = pg.u32(m, table.wrapping_add(4));
                entry = entry.wrapping_add(4);
                pg.set_u32(m, to.wrapping_add(0x1fc), second);
                slot = pg.u32(m, entry) & 0xffff;
            }
        }
    }
    pg.set_u32(m, at(0xb8), 0);
    pg.set_u32(m, at(0xbc), 0);
    pg.set_u32(m, at(0xc0), 0);
    let s = pg.u32(m, at(0xc8));
    pg.set_u32(m, at(0xc8), s | 8);
    let state = pg.u32(m, at(0xc8));
    let (w, h) = if target != 0 {
        let size = pg.u32(m, target.wrapping_add(0x18));
        (size & 0xffff, (size >> 16) & 0x7fff)
    } else if depth != 0 {
        let width = pg.u32(m, depth.wrapping_add(0x18)) & 0xffff;
        (width, pg.u16(m, depth.wrapping_add(0x1a)) as u32)
    } else {
        (1, 1)
    };
    // the original's frame: three saved registers below 0x2C bytes of locals
    let frame = entry.wrapping_sub(0x38);
    let float = |v: u32| x86::cvtdq2ps([v, 0, 0, 0], [v, 0, 0, 0])[0];

    if pg.u16(m, at(0x10c)) as u32 == w && pg.u16(m, at(0x10e)) as u32 == h {
        // the same size: the previous block again, with the size terms
        let pass = pg.u32(m, pg.u32(m, at(0x18)).wrapping_add(0x54c));
        let size = (pg.u16(m, pass.wrapping_add(0x18)) as u32).wrapping_mul(4).wrapping_add(0xf) & !0xf;
        let cursor = pg.u32(m, at(0x24));
        reserve(m, c, ctx, cursor, size, frame, 0x00FE_FDDA);
        let block = pg.u32(m, at(0x24));
        pg.set_u32(m, at(0x24), block.wrapping_add(size));
        let n = (pg.u16(m, pass.wrapping_add(0x18)) as u32).wrapping_mul(4);
        let previous = pg.u32(m, at(0xc90));
        let eax = memcpy(m, c, frame, block, previous, n, 0x00FE_FE12);
        pg.set_u32(m, at(0xc90), block);
        let f = pg.u32(m, at(0x154));
        pg.set_u32(m, at(0x154), f | 1);
        if block != 0 {
            let (fw, fh) = (float(w), float(h));
            let one = pg.u32(m, ONE);
            pg.set_u32(m, block.wrapping_add(0x10), fw);
            pg.set_u32(m, block.wrapping_add(0x14), fh);
            pg.set_u32(m, block.wrapping_add(0x18), x86::divss(one, fw));
            pg.set_u32(m, block.wrapping_add(0x1c), x86::divss(one, fh));
        }
        return done(c, eax);
    }

    // a new size: the viewport from the context's rectangle
    pg.set_u16(m, at(0x10c), w as u16);
    pg.set_u16(m, at(0x10e), h as u16);
    let width = pg.u32(m, at(0x10c)) & 0xffff;
    let height = h & 0xffff;
    let rect = if state & 0x200 != 0 { 0xdc } else { 0xcc };
    let [x0, y0, x1, y1] = [0, 4, 8, 12].map(|k| pg.u32(m, at(rect + k)));
    let rw = x86::cvtsi2ss(x1.wrapping_sub(x0));
    let pass_owner = pg.u32(m, at(0x18));
    let rh_int = y1.wrapping_sub(y0);
    let fx0 = x86::cvtsi2ss(x0);
    let cursor = pg.u32(m, at(0x24));
    let fw = x86::cvtsi2ss(width);
    let scale = pg.u32(m, VIEW_SCALE);
    let sx = x86::mulss(x86::divss(rw, fw), scale);
    let rh = x86::cvtsi2ss(rh_int);
    let pass = pg.u32(m, pass_owner.wrapping_add(0x54c));
    let tx = x86::divss(x86::divss(fx0, fw), sx);
    let fy0 = x86::cvtsi2ss(y0);
    let size = (pg.u16(m, pass.wrapping_add(0x18)) as u32).wrapping_mul(4).wrapping_add(0xf) & !0xf;
    let fh = float(height);
    let sy = x86::mulss(x86::divss(rh, fh), scale);
    let ty = x86::divss(x86::divss(fy0, fh), sy);
    reserve(m, c, ctx, cursor, size, frame, 0x00FE_FFAF);
    let block = pg.u32(m, at(0x24));
    pg.set_u32(m, at(0x24), block.wrapping_add(size));
    pg.set_u32(m, at(0xc90), block);
    let f = pg.u32(m, at(0x154));
    pg.set_u32(m, at(0x154), f | 1);
    if block == 0 {
        return done(c, size);
    }
    let n = (pg.u16(m, pass.wrapping_add(0x18)) as u32).wrapping_mul(4);
    let template = pg.u32(m, pass.wrapping_add(0x24));
    let eax = memcpy(m, c, frame, block, template, n, 0x00FE_FFF9);
    let sign = pg.u32(m, SIGN);
    let one = pg.u32(m, ONE);
    pg.set_u32(m, block, x86::addss(tx, one));
    pg.set_u32(m, block.wrapping_add(4), x86::addss(ty, one) ^ sign);
    pg.set_u32(m, block.wrapping_add(8), sx);
    pg.set_u32(m, block.wrapping_add(0xc), sy ^ sign);
    pg.set_u32(m, block.wrapping_add(0x20), x86::divss(one, rw));
    pg.set_u32(m, block.wrapping_add(0x24), x86::divss(one, rh));
    let (fw, fh) = (float(w), float(h));
    pg.set_u32(m, block.wrapping_add(0x10), fw);
    pg.set_u32(m, block.wrapping_add(0x14), fh);
    pg.set_u32(m, block.wrapping_add(0x18), x86::divss(one, fw));
    pg.set_u32(m, block.wrapping_add(0x1c), x86::divss(one, fh));
    done(c, eax)
}

/// The arena check: when `cursor + size` passes the context's limit
/// (+0x28), allocate the size rounded up to 4 KiB (0x00DA7D70, a guest
/// call from the original's frame) as the new arena.
fn reserve(m: &mut dyn Mem, c: &mut Cpu, ctx: u32, cursor: u32, size: u32, frame: u32, resume: u32) {
    let pg = Pages::of(m);
    if cursor.wrapping_add(size) <= pg.u32(m, ctx.wrapping_add(0x28)) {
        return;
    }
    let chunk = size.wrapping_add(0xfff) & !0xfff;
    c.ecx = pg.u32(m, ALLOCATOR);
    c.esp = frame;
    push(m, pg, c, chunk);
    push(m, pg, c, resume);
    x86::call_ext(m, c, ALLOCATE);
    let arena = c.eax;
    pg.set_u32(m, ctx.wrapping_add(0x24), arena);
    pg.set_u32(m, ctx.wrapping_add(0x28), if arena != 0 { arena.wrapping_add(chunk) } else { 0 });
}

/// memcpy(dst, src, n) from the original's frame, as its call site makes
/// it (the platform model, overridable like the lifted code's call): EAX.
fn memcpy(m: &mut dyn Mem, c: &mut Cpu, frame: u32, dst: u32, src: u32, n: u32, resume: u32) -> u32 {
    let pg = Pages::of(m);
    c.esp = frame;
    push(m, pg, c, n);
    push(m, pg, c, src);
    push(m, pg, c, dst);
    push(m, pg, c, resume);
    x86::call_local(m, c, MEMCPY, ddnative_base::platform::memcpy);
    c.esp = frame;
    c.eax
}

fn push(m: &mut dyn Mem, pg: Pages, c: &mut Cpu, v: u32) {
    c.esp = c.esp.wrapping_sub(4);
    pg.set_u32(m, c.esp, v);
}

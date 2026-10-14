//! Semantic ports (L176) of the nDraw material's parameter lookup, on the
//! shadow and main-pass model-draw path (under uBaseModel::vf26
//! 0x00EA5320). They keep the original's ABI-visible result (EAX, the
//! callee-saved registers, ESP, every memory write above the returned
//! stack) rather than its instructions; ECX, EDX, the flags and the popped
//! stack scratch are not reproduced (MSVC callers never read them after a
//! call). Checked against the original's recorded vectors in
//! tests/material.rs; opt-in through crates/ddports
//! (`DDRUNTIME_MATERIAL=1`).
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

/// The binding table the found parameter is filled from (a pointer).
const BINDINGS: u32 = 0x018D_12CC;

// Translated from 0x01066590 (DDDA.exe 2017-12-13 Steam).
/// 0x01066590, `this->find(slot, key) -> entry` (thiscall, `ret 8`): the
/// 12-byte parameter entry of `slot` whose key (bits 20..31 of its first
/// word) is `key & 0xfff`, or 0.
pub fn find_parameter(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let slot = pg.u32(m, c.esp.wrapping_add(4));
    c.eax = find(m, pg, c.ecx, slot, Key::At(c.esp.wrapping_add(8)));
    c.esp = c.esp.wrapping_add(12);
}

/// The key searched for: read from its argument slot only once the count
/// is non-zero, as the original does, or already read by the caller.
#[derive(Clone, Copy)]
enum Key {
    At(u32),
    Value(u32),
}

/// The search itself. `this + 0x78 + 4*slot` points at `count` (at
/// `this + 0x88 + 4*slot`) entry indices; entries are 12 bytes from
/// `this + 0x30`. Up to two indices are searched in order; more by a
/// binary search over the indices' keys. All arithmetic wraps as the
/// original's: when the upper bound passes below index 0 the midpoint
/// becomes 0x7FFFFFFF and the search reads the word before the list.
fn find(m: &dyn Mem, pg: Pages, this: u32, slot: u32, key: Key) -> u32 {
    let at = this.wrapping_add(slot.wrapping_mul(4));
    let count = pg.u32(m, at.wrapping_add(0x88));
    if count == 0 {
        return 0;
    }
    let list = pg.u32(m, at.wrapping_add(0x78));
    let key = match key {
        Key::At(a) => pg.u32(m, a),
        Key::Value(v) => v,
    } & 0xfff;
    let entries = pg.u32(m, this.wrapping_add(0x30));
    // the i-th listed entry and its key
    let entry = |i: u32| {
        let index = pg.u32(m, list.wrapping_add(i.wrapping_mul(4)));
        let e = entries.wrapping_add(index.wrapping_mul(12));
        (e, pg.u32(m, e) >> 20)
    };
    if count <= 2 {
        for i in 0..count {
            let (e, k) = entry(i);
            if k == key {
                return e;
            }
        }
        return 0;
    }
    let (mut lo, mut hi) = (0u32, count - 1);
    loop {
        let mid = hi.wrapping_add(lo) >> 1;
        let (e, k) = entry(mid);
        if k == key {
            return e;
        }
        if lo >= hi {
            return 0;
        }
        if k >= key {
            hi = mid.wrapping_sub(1);
        } else {
            lo = mid.wrapping_add(1);
        }
    }
}

// Translated from 0x01066B10 (DDDA.exe 2017-12-13 Steam).
/// 0x01066B10, `this->bind(key, value) -> found` (thiscall, `ret 8`):
/// find `key` in slot 0; if found, set the entry's second word from the
/// binding table (`[0x018D12CC] + 8*(value & 0xfff) + 0x3c`) and its third
/// to `value`'s 12-bit index under `key`'s high bits. AL is 1 when found
/// (EAX keeps the entry's upper bytes, as the original's `mov al, 1`), 0
/// otherwise.
pub fn bind_parameter(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let key = pg.u32(m, c.esp.wrapping_add(4));
    let e = find(m, pg, c.ecx, 0, Key::Value(key));
    c.eax = if e == 0 {
        0
    } else {
        let value = pg.u32(m, c.esp.wrapping_add(8));
        let table = pg.u32(m, BINDINGS);
        let index = value & 0xfff;
        let bound = pg.u32(m, table.wrapping_add(index.wrapping_mul(8)).wrapping_add(0x3c));
        pg.set_u32(m, e.wrapping_add(4), bound);
        pg.set_u32(m, e.wrapping_add(8), ((key ^ value) & 0xffff_f000) | index);
        (e & !0xff) | 1
    };
    c.esp = c.esp.wrapping_add(12);
}

/// The frame serial the material's apply stamps (a counter).
const SERIAL: u32 = 0x018D_2CE8;
/// The colour scale's threshold and the scale (floats in the image).
const THRESHOLD: u32 = 0x0140_6CE0;
const SCALE: u32 = 0x0161_C298;
/// The shader-binding cache's tag.
const SHADER_TAG: u32 = 0x10FD_CA55;
/// 0x01062F20, the context update the apply ends with (a guest call).
const UPDATE: u32 = 0x0106_2F20;

// Translated from 0x010655A0 (DDDA.exe 2017-12-13 Steam).
/// 0x010655A0, `this->apply(context, value) -> update(context)` (thiscall,
/// `ret 8`): stamp the material with the frame serial, write its states
/// into the context's caches (each group only when it differs, marking
/// what changed in the context's flags at +0x154 and +0x114), flush a
/// pending record, copy the material's source entries into the context's
/// slot table, then call 0x01062F20 (`this->update(context)`, a guest
/// call) and return its EAX.
///
/// The reads and writes keep the original's order (the context and the
/// material may alias), including its reloads of `context + 0x6C` and of
/// the source count. The four colour bytes use the original's SSE steps
/// (comiss against the threshold, mulss, cvttss2si) through the same
/// hardware helpers as the lifted code.
pub fn apply_material_state(m: &mut dyn Mem, c: &mut Cpu) {
    use ddnative_base::x86;
    let pg = Pages::of(m);
    let this = c.ecx;
    let entry_esp = c.esp;
    let entry_edi = c.edi;
    let serial = pg.u32(m, SERIAL).wrapping_add(1);
    let value = pg.u32(m, entry_esp.wrapping_add(8));
    let shader = pg.u32(m, this.wrapping_add(0x10));
    pg.set_u32(m, this.wrapping_add(4), serial);
    let ctx = pg.u32(m, entry_esp.wrapping_add(4));
    let at = |o: u32| ctx.wrapping_add(o);

    // the shader binding: shader, value byte (compared as 32 bits), tag
    if !(pg.u32(m, at(0x1cc)) == shader && pg.u8(m, at(0x169)) as u32 == value && pg.u32(m, at(0x1d0)) == SHADER_TAG) {
        pg.set_u32(m, at(0x1cc), shader);
        let f = pg.u32(m, at(0x154));
        pg.set_u32(m, at(0x154), f | 3);
        pg.set_u8(m, at(0x169), value as u8);
        pg.set_u32(m, at(0x1d0), SHADER_TAG);
        pg.set_u16(m, at(0x158), (shader & 0xfff) as u16);
        pg.set_u16(m, at(0x15a), value as u16);
    }

    // the blend state and its mode byte
    let blend = pg.u32(m, this.wrapping_add(0x28));
    let mode = pg.u8(m, this.wrapping_add(0x14));
    if !(pg.u32(m, at(0x11c)) == blend && pg.u8(m, at(0x114)) == mode) {
        pg.set_u32(m, at(0x11c), blend);
        pg.set_u8(m, at(0x114), mode);
        let f = pg.u32(m, at(0x114));
        pg.set_u32(m, at(0x114), (f & 0xfff) | 0x1000);
    }

    // the colour: four bytes, each the component scaled (or the scale
    // itself above the threshold), truncated
    let first = pg.u32(m, this.wrapping_add(0x40));
    let threshold = pg.u32(m, THRESHOLD);
    let scale = pg.u32(m, SCALE);
    let above = |a: u32| {
        let f = x86::comiss_hw::<true>(a, threshold);
        !f.cf() && !f.zf()
    };
    let byte = |a: u32| -> u32 {
        let v = if above(a) { scale } else { x86::mulss(a, scale) };
        x86::cvttss2si(v) & 0xff
    };
    let b0 = byte(first);
    let b1 = byte(pg.u32(m, this.wrapping_add(0x44)));
    let b2 = byte(pg.u32(m, this.wrapping_add(0x48)));
    let fourth = pg.u32(m, this.wrapping_add(0x4c));
    // (mulss scale, component: the operands the other way round)
    let b3 = x86::cvttss2si(if above(fourth) { scale } else { x86::mulss(scale, fourth) }) & 0xff;
    let colour = b0 | b1 << 8 | b2 << 16 | b3 << 24;

    // the texture of the material's slot, and the colour
    let slot = pg.u32(m, this.wrapping_add(0x2c)) & 0xfff;
    let texture = pg.u32(m, ctx.wrapping_add(slot.wrapping_mul(8)).wrapping_add(0x1f8));
    if !(pg.u32(m, at(0x118)) == texture && colour == pg.u32(m, at(0x110))) {
        pg.set_u32(m, at(0x110), colour);
        let f = pg.u32(m, at(0x114));
        pg.set_u32(m, at(0x118), texture);
        pg.set_u32(m, at(0x114), (f & 0xfff) | 0x1000);
    }

    let extra = pg.u32(m, this.wrapping_add(0x24));
    if pg.u32(m, at(0x120)) != extra {
        pg.set_u32(m, at(0x120), extra);
        let f = pg.u32(m, at(0x114));
        pg.set_u32(m, at(0x114), (f & 0xfff) | 0x1000);
    }

    // the material's flag bits into the context's flags word
    let flags = pg.u32(m, this.wrapping_add(0x14));
    let bit13 = (flags >> 13) & 1;
    let bit8 = (flags >> 8) & 1;
    let high = (flags >> 14) & 0xff;
    let nibble = (flags >> 9) & 0xf;
    let cached = pg.u32(m, at(0x154));
    if !((cached >> 20) & 1 == bit8 && (cached >> 3) & 0xff == nibble && (cached >> 11) & 0xff == high && (cached >> 19) & 1 == bit13) {
        let packed = ((bit8 << 1 | bit13) << 16 | nibble) << 3;
        let kept = ((high << 11) | (cached & 0xffe8_0007)) & 0xfff7_ffff;
        pg.set_u32(m, at(0x154), packed | kept | 1);
    }

    // a pending record: appended to the context's record list when
    // recording (bit 0x80 of +0x5C), then cleared
    if pg.u32(m, at(0x1c30)) != 0 {
        if pg.u8(m, at(0x5c)) & 0x80 != 0 {
            let record = |m: &dyn Mem| ctx.wrapping_add(pg.u32(m, at(0x6c)).wrapping_mul(12)).wrapping_add(0x30c7c);
            let r = record(m);
            let w = pg.u32(m, r);
            pg.set_u32(m, r, (w & 0x8000_0347) | 0x347);
            let r = record(m);
            let w = pg.u32(m, r);
            pg.set_u32(m, r, w & 0x7fff_ffff);
            let pending = pg.u32(m, at(0x1c30));
            let r = record(m);
            pg.set_u32(m, r.wrapping_add(4), pending);
            let second = pg.u32(m, at(0x1c34));
            pg.set_u32(m, r.wrapping_add(8), second);
            let n = pg.u32(m, at(0x6c));
            pg.set_u32(m, at(0x6c), n.wrapping_add(1));
        }
        pg.set_u32(m, at(0x1c30), 0);
        if pg.u32(m, at(0x1c34)) != 0 {
            pg.set_u32(m, at(0x1c34), 0);
            let f = pg.u32(m, at(0x154));
            pg.set_u32(m, at(0x154), f | 2);
        }
        let f = pg.u32(m, at(0x154));
        pg.set_u32(m, at(0x154), f | 1);
    }

    // the material's source entries into the context's slot table (the
    // count reread every step, as the original)
    let mut source = pg.u32(m, this.wrapping_add(0x30));
    if pg.u32(m, this.wrapping_add(0x18)) & 0x1ff != 0 {
        let mut i = 0u32;
        loop {
            let key = pg.u32(m, source) >> 20;
            let slot = ctx.wrapping_add(key.wrapping_mul(8));
            let w1 = pg.u32(m, source.wrapping_add(4));
            pg.set_u32(m, slot.wrapping_add(0x1f8), w1);
            let w2 = pg.u32(m, source.wrapping_add(8));
            pg.set_u32(m, slot.wrapping_add(0x1fc), w2);
            i = i.wrapping_add(1);
            source = source.wrapping_add(12);
            if i >= pg.u32(m, this.wrapping_add(0x18)) & 0x1ff {
                break;
            }
        }
    }
    let f = pg.u32(m, at(0x154));
    pg.set_u32(m, at(0x154), f | 3);

    // this->update(context): the original's frame (three saved registers)
    // and its pushes, so the callee sees the same stack. The lifted tail
    // sets ECX = EDI = this and pops the caller's EDI after the call; the
    // callee reads EDI (0x01062F20's own lifted code).
    c.esp = entry_esp.wrapping_sub(12);
    push(m, pg, c, ctx);
    push(m, pg, c, 0x0106_589C);
    c.ecx = this;
    c.edi = this;
    x86::call_ext(m, c, UPDATE);
    c.edi = entry_edi;
    c.esp = entry_esp.wrapping_add(12);
}

fn push(m: &mut dyn Mem, pg: Pages, c: &mut Cpu, v: u32) {
    c.esp = c.esp.wrapping_sub(4);
    pg.set_u32(m, c.esp, v);
}

/// The material's default colour (four floats in the image).
const DEFAULT_COLOUR: u32 = 0x0162_9470;
/// The colour parameter's key (slot 1).
const COLOUR_KEY: u32 = 0x6C80_11EE;

// Translated from 0x01067790 (DDDA.exe 2017-12-13 Steam).
/// 0x01067790, nDraw::Material::vf09 `this->colour(out) -> out` (thiscall,
/// `ret 4`): the default colour, replaced by the colour parameter's value
/// (slot 1, key 0x6C8011EE: the vector its entry's second word points at,
/// 16-byte aligned) when it has one. The fourth component goes through
/// the x87 stack (fld/fstp), so a signalling NaN arrives quieted.
pub fn material_colour(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let out = pg.u32(m, c.esp.wrapping_add(4));
    for k in 0..4 {
        let v = pg.u32(m, DEFAULT_COLOUR + 4 * k);
        pg.set_u32(m, out.wrapping_add(4 * k), v);
    }
    let entry = find(m, pg, c.ecx, 1, Key::Value(COLOUR_KEY));
    if entry != 0 {
        let v = pg.u32(m, entry.wrapping_add(4)) & !0xf;
        if v != 0 {
            let (x, y) = (pg.u32(m, v), pg.u32(m, v.wrapping_add(4)));
            let w = quiet(pg.u32(m, v.wrapping_add(0xc)));
            let z = pg.u32(m, v.wrapping_add(8));
            pg.set_u32(m, out.wrapping_add(0xc), w);
            pg.set_u32(m, out, x);
            pg.set_u32(m, out.wrapping_add(4), y);
            pg.set_u32(m, out.wrapping_add(8), z);
        }
    }
    c.eax = out;
    c.esp = c.esp.wrapping_add(8);
}

/// A float through fld/fstp (32-bit both ways): unchanged but for a
/// signalling NaN, which the load quiets (the quiet bit set).
fn quiet(v: u32) -> u32 {
    if v & 0x7f80_0000 == 0x7f80_0000 && v & 0x007f_ffff != 0 {
        v | 0x0040_0000
    } else {
        v
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    /// `quiet` is what the CPU's fld/fstp (32-bit both ways) does to a
    /// float, signalling NaNs included (the recorded vectors leave those
    /// out: Unicorn does not quiet them).
    #[test]
    fn quiet_matches_the_cpus_fld_fstp() {
        let through_x87 = |v: u32| -> u32 {
            let (input, mut output) = (v, 0u32);
            unsafe {
                std::arch::asm!("fld dword ptr [{i}]", "fstp dword ptr [{o}]",
                    i = in(reg) &input, o = in(reg) &mut output, options(nostack));
            }
            output
        };
        for v in [0u32, 0x8000_0000, 1, 0x3f80_0000, 0x7f80_0000, 0xff80_0000, 0x7f80_0001, 0xff80_0001,
                  0x7fbf_ffff, 0x7fc0_0000, 0xffc0_1234, 0x7f81_2345, 0x0000_0001, 0x807f_ffff] {
            assert_eq!(super::quiet(v), through_x87(v), "{v:#010x}");
        }
    }
}

/// The context's arena refill (thiscall, the size; a guest call).
const REFILL: u32 = 0x00FE_D890;
/// The CRT memcpy (cdecl), a platform model.
const MEMCPY: u32 = 0x012E_FEA0;

// Translated from 0x01062F20 (DDDA.exe 2017-12-13 Steam).
/// 0x01062F20, `this->update(context)` (thiscall, `ret 4`), the end of
/// the material apply (0x010655A0): for each of the material's four
/// parameter slots (signed 16-bit indices at `this + 0x50`, 8 bytes apart;
/// negative: none) whose group (`[[this + 0x74] + 0x10]`, entry
/// `index + 1`) lists keys (count in bits 18..31 of its second word), each
/// key's constants move to a fresh block of the context's arena (the
/// arena refilled by 0x00FED890 when it runs out, a guest call): the
/// previous block (`context + 8*key + 0x1F8`) copied in, the slot pointed
/// at the new one, the context marked (+0x154 bit 0). EAX is what the
/// original's last step leaves: the last slot's index when negative, its
/// group when it lists no keys, otherwise the key count.
pub fn update_material(m: &mut dyn Mem, c: &mut Cpu) {
    use ddnative_base::x86;
    let pg = Pages::of(m);
    let this = c.ecx;
    let entry = c.esp;
    let ctx = pg.u32(m, entry.wrapping_add(4));
    // the original's frame: 0x14 bytes of locals, four saved registers
    let frame = entry.wrapping_sub(0x24);
    let mut eax = c.eax;
    let mut slot = this.wrapping_add(0x50);
    for _ in 0..4 {
        let index = pg.u16(m, slot);
        eax = index as u32;
        if (index as i16) >= 0 {
            let table = pg.u32(m, pg.u32(m, this.wrapping_add(0x74)).wrapping_add(0x10));
            let group = pg.u32(m, table.wrapping_add((index as i16 as i32 as u32).wrapping_mul(4)).wrapping_add(4));
            eax = group;
            if pg.u32(m, group.wrapping_add(4)) & 0xfffc_0000 != 0 {
                let mut k = 0u32;
                loop {
                    let keys = pg.u32(m, group.wrapping_add(8));
                    let key = pg.u32(m, keys.wrapping_add(k.wrapping_mul(4))) & 0xfff;
                    let pass = pg.u32(m, pg.u32(m, ctx.wrapping_add(0x18)).wrapping_add(key.wrapping_mul(4)));
                    let size = (pg.u16(m, pass.wrapping_add(0x18)) as u32).wrapping_mul(4).wrapping_add(0xf) & !0xf;
                    if pg.u32(m, ctx.wrapping_add(0x24)).wrapping_add(size) > pg.u32(m, ctx.wrapping_add(0x28)) {
                        c.esp = frame;
                        push(m, pg, c, size);
                        push(m, pg, c, 0x0106_2FBE);
                        c.ecx = ctx;
                        x86::call_ext(m, c, REFILL);
                    }
                    let block = pg.u32(m, ctx.wrapping_add(0x24));
                    pg.set_u32(m, ctx.wrapping_add(0x24), size.wrapping_add(block));
                    let n = (pg.u16(m, pass.wrapping_add(0x18)) as u32).wrapping_mul(4);
                    let at = ctx.wrapping_add(key.wrapping_mul(8)).wrapping_add(0x1f8);
                    let previous = pg.u32(m, at);
                    c.esp = frame;
                    for v in [n, previous, block, 0x0106_2FE2] {
                        push(m, pg, c, v);
                    }
                    x86::call_local(m, c, MEMCPY, ddnative_base::platform::memcpy);
                    c.esp = frame;
                    pg.set_u32(m, at, block);
                    let f = pg.u32(m, ctx.wrapping_add(0x154));
                    pg.set_u32(m, ctx.wrapping_add(0x154), f | 1);
                    k = k.wrapping_add(1);
                    eax = k;
                    if k >= pg.u32(m, group.wrapping_add(4)) >> 18 {
                        break;
                    }
                }
            }
        }
        slot = slot.wrapping_add(8);
    }
    c.eax = eax;
    c.esp = entry.wrapping_add(8);
}

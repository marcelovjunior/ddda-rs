//! Semantic ports (L171): native rewrites of hot game functions that keep
//! the ABI-visible result of the original (return value, callee-saved
//! registers, ESP, and every guest memory write above the returned stack)
//! instead of replaying its instructions. Caller-saved registers (ECX,
//! EDX), the arithmetic flags and the popped stack scratch are not
//! reproduced: MSVC-compiled callers never read them after a call.
//!
//! Each port is opt-in (`DDRUNTIME_SEMANTIC=1`, crates/ddports) and is
//! checked against the original's recorded vectors at that ABI level
//! (crates/mtcore/tests/semantic.rs).
use ddnative_base::mem::Pages;
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

/// 0x00E0EC50, `this->expand(packed, values, out) -> out_end` (thiscall,
/// `ret 0xc`): expand a packed dependency into `out`.
///
/// `this + 0x28` points at the descriptor bank: 8-byte descriptors whose
/// first word holds the recursive-child count in bits 20..25 and the
/// terminal count in bits 26..31, and whose second word points at the u16
/// indices of the children followed by the terminals. `values` is a bank
/// of 8-byte entries whose second word is a packed value: bits 0..11 a
/// descriptor index, bits 12..31 an optional output value. A packed value
/// emits its high part when non-zero, then its recursive children
/// (expanded in turn) and its non-zero terminal values. The original is
/// this recursion unrolled five levels deep, calling itself below that.
pub fn expand_dependencies(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let packed = pg.u32(m, c.esp.wrapping_add(4));
    let values = pg.u32(m, c.esp.wrapping_add(8));
    let out = pg.u32(m, c.esp.wrapping_add(0xc));
    let bank = pg.u32(m, c.ecx.wrapping_add(0x28));
    c.eax = expand(m, pg, bank, values, packed, out);
    c.esp = c.esp.wrapping_add(4 + 0xc);
}

fn expand(m: &mut dyn Mem, pg: Pages, bank: u32, values: u32, packed: u32, mut out: u32) -> u32 {
    let high = packed & 0xffff_f000;
    if high != 0 {
        pg.set_u32(m, out, high);
        out = out.wrapping_add(4);
    }
    let descriptor = bank.wrapping_add((packed & 0xfff).wrapping_mul(8));
    let mut index = pg.u32(m, descriptor.wrapping_add(4));
    // The original reloads the descriptor word for every loop test; keep
    // that, so a descriptor rewritten through `out` behaves the same.
    let mut i = 0u32;
    while i < (pg.u32(m, descriptor) >> 20) & 0x3f {
        let child = pg.u16(m, index) as u32;
        index = index.wrapping_add(2);
        let value = pg.u32(m, values.wrapping_add(child.wrapping_mul(8)).wrapping_add(4));
        out = expand(m, pg, bank, values, value, out);
        i += 1;
    }
    let mut i = 0u32;
    while i < pg.u32(m, descriptor) >> 26 {
        let child = pg.u16(m, index) as u32;
        index = index.wrapping_add(2);
        let value = pg.u32(m, values.wrapping_add(child.wrapping_mul(8)).wrapping_add(4));
        if value != 0 {
            pg.set_u32(m, out, value);
            out = out.wrapping_add(4);
        }
        i += 1;
    }
    out
}

/// 0x00E10FC0, `this->intern_keys(owner, values, keys, extra, crc_out) ->
/// interned` (thiscall, `ret 0x14`): build a key sequence in a 1 KiB stack
/// buffer (at the caller's ESP - 0x400, as the original: the interning
/// callee reads it there), optionally store its CRC, and intern it.
///
/// The sequence: the four `keys` words tagged with their slot in the low
/// bits (zero words skipped), `extra`, then the expansions of the six
/// `owner` words in the order 0, 3, 4, 2, 1, 5 (zero words skipped). The
/// CRC is the table CRC of the words' bytes, high byte first, at
/// 0x014075C0. The interning (0x00E0E5E0, FNV basis 0x811C9DC5) stays a
/// guest call, so its own port and every caller see the same table.
pub fn intern_dependency_keys(m: &mut dyn Mem, c: &mut Cpu) {
    intern_dependency_keys_with(m, c, native_expand)
}

/// One expansion of `packed` into `out` for [`intern_dependency_keys`]:
/// the native recursion (production) or a guest call to 0x00E0EC50.
pub type Expander = fn(&mut dyn Mem, &mut Cpu, Pages, u32, u32, u32, u32) -> u32;

fn native_expand(m: &mut dyn Mem, _: &mut Cpu, pg: Pages, this: u32, values: u32, packed: u32, out: u32) -> u32 {
    let bank = pg.u32(m, this.wrapping_add(0x28));
    expand(m, pg, bank, values, packed, out)
}

/// The original's own call to 0x00E0EC50 (for tests against vectors that
/// record those calls as callees): `push out; push values; push packed;
/// mov ecx, this; call` with ESP at the original's frame.
pub fn guest_expand(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, values: u32, packed: u32, out: u32) -> u32 {
    for v in [out, values, packed, RETURN_FROM_EXPAND.with(|r| r.get())] {
        c.esp = c.esp.wrapping_sub(4);
        pg.set_u32(m, c.esp, v);
    }
    c.ecx = this;
    ddnative_base::x86::call_ext(m, c, 0x00e0_ec50);
    c.eax
}

thread_local! {
    /// The return address of the expansion call being made (each of the
    /// six call sites has its own), for [`guest_expand`].
    static RETURN_FROM_EXPAND: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// [`intern_dependency_keys`] with its expansion step chosen by the caller.
pub fn intern_dependency_keys_with(m: &mut dyn Mem, c: &mut Cpu, expander: Expander) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let owner = arg(m, 0);
    let values = arg(m, 1);
    let keys = arg(m, 2);
    let extra = arg(m, 3);
    let crc_out = arg(m, 4);
    let this = c.ecx;
    let buffer = entry.wrapping_sub(0x400);
    let mut out = buffer;
    for slot in 0..4u32 {
        let k = pg.u32(m, keys.wrapping_add(4 * slot));
        if k != 0 {
            pg.set_u32(m, out, (k & 0xffff_f000) | slot);
            out = out.wrapping_add(4);
        }
    }
    pg.set_u32(m, out, extra);
    out = out.wrapping_add(4);
    // The original's frame during the expansions: ESP = entry - 0x414.
    c.esp = entry.wrapping_sub(0x414);
    for (field, resume) in [(0u32, 0x00e1_1058u32), (3, 0x00e1_1069), (4, 0x00e1_107a), (2, 0x00e1_108b), (1, 0x00e1_109c), (5, 0x00e1_10ad)] {
        let packed = pg.u32(m, owner.wrapping_add(4 * field));
        if packed != 0 {
            RETURN_FROM_EXPAND.with(|r| r.set(resume));
            out = expander(m, c, pg, this, values, packed, out);
        }
    }
    let count = out.wrapping_sub(buffer) >> 2;
    if crc_out != 0 {
        let mut crc = u32::MAX;
        for i in 0..(count & 0x3fff_ffff) {
            let word = pg.u32(m, buffer.wrapping_add(4 * i));
            for shift in [24u32, 16, 8, 0] {
                let index = ((word >> shift) ^ crc) & 0xff;
                crc = (crc >> 8) ^ pg.u32(m, 0x0140_75c0u32.wrapping_add(index * 4));
            }
        }
        pg.set_u32(m, crc_out, crc);
    }
    // The original's frame at the call: ESP = entry - 0x414 (0x404 locals,
    // four pushes), then three arguments and the return address.
    c.esp = entry.wrapping_sub(0x414);
    for v in [0x811c_9dc5u32, count.wrapping_mul(4), buffer, 0x00e1_118f] {
        c.esp = c.esp.wrapping_sub(4);
        pg.set_u32(m, c.esp, v);
    }
    c.ecx = this;
    ddnative_base::x86::call_ext(m, c, 0x00e0_e5e0);
    // EAX = the interned sequence; `ret 0x14` from the entry frame
    c.esp = entry.wrapping_add(4 + 0x14);
}

/// The original body of 0x00E12A20, set by the runtime when it installs
/// [`shader_lookup`] (the rare branch runs it unchanged).
pub static E12A20_ORIGINAL: std::sync::OnceLock<ddnative_base::x86::LiftFn> = std::sync::OnceLock::new();

/// A guest call from the frame `frame` (the caller's ESP at its call
/// sites): push `args` (last first) and `back`, set ECX and run `body`.
/// Returns EAX; ESP is `frame` again after the callee's own `ret n`.
pub(crate) fn call_from(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, this: u32, args: &[u32], back: u32, body: impl FnOnce(&mut dyn Mem, &mut Cpu)) -> u32 {
    c.esp = frame;
    for &v in args.iter().rev() {
        c.esp = c.esp.wrapping_sub(4);
        pg.set_u32(m, c.esp, v);
    }
    c.esp = c.esp.wrapping_sub(4);
    pg.set_u32(m, c.esp, back);
    c.ecx = this;
    body(m, c);
    c.eax
}

/// 0x00E12A20, `this->lookup(a, bank, slot, keys_owner_values, keys,
/// extra, other_bank) -> entry` (thiscall, `ret 0x1c`): the shader/
/// technique lookup of every model primitive draw. The common branch
/// (byte `this + 0x39` clear) interns the dependency keys of
/// `bucket(bank) + slot * 0x28 + 0x1c` and looks them up (0x00E0F3C0),
/// then again with their CRC (0x00E0F4B0), then for `other_bank`; the rare
/// branch (shader creation) runs the original body. Only reads happen
/// before the branch, so the fallback is exact.
pub fn shader_lookup(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let this = c.ecx;
    if pg.u8(m, this.wrapping_add(0x39)) != 0 {
        let original = E12A20_ORIGINAL.get().expect("0x00E12A20 original body");
        return original(m, c);
    }
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let (bank, slot, owner_values, keys, extra, other) = (arg(m, 1), arg(m, 2), arg(m, 3), arg(m, 4), arg(m, 5), arg(m, 6));
    let table = pg.u32(m, this.wrapping_add(0x24));
    let bucket = pg.u32(m, table.wrapping_add((bank & 0xfff).wrapping_mul(4)));
    let owner = bucket.wrapping_add(slot.wrapping_mul(0x28)).wrapping_add(0x1c);
    // The original's frame after its prologue (0x8c locals, four pushes);
    // its CRC local is at frame + 0x10.
    let frame = entry.wrapping_sub(0x9c);
    let crc = frame.wrapping_add(0x10);
    let ext = |target: u32| move |m: &mut dyn Mem, c: &mut Cpu| ddnative_base::x86::call_ext(m, c, target);
    let finish = |c: &mut Cpu, r: u32| {
        c.eax = r;
        c.esp = entry.wrapping_add(4 + 0x1c);
    };
    let key = call_from(m, c, pg, frame, this, &[owner, owner_values, keys, extra, 0], 0x00e1_3208, intern_dependency_keys);
    let r = call_from(m, c, pg, frame, this, &[key], 0x00e1_3210, ext(0x00e0_f3c0));
    if r != 0 {
        return finish(c, r);
    }
    let key = call_from(m, c, pg, frame, this, &[owner, owner_values, keys, extra, crc], 0x00e1_322f, intern_dependency_keys);
    let sum = pg.u32(m, crc);
    let r = call_from(m, c, pg, frame, this, &[key, sum], 0x00e1_3244, ext(0x00e0_f4b0));
    if r != 0 || pg.u8(m, this.wrapping_add(0xab229)) != 0 || other == 0 {
        return finish(c, r);
    }
    let table = pg.u32(m, this.wrapping_add(0x24));
    let owner = pg.u32(m, table.wrapping_add((other & 0xfff).wrapping_mul(4))).wrapping_add(0x1c);
    let key = call_from(m, c, pg, frame, this, &[owner, owner_values, keys, extra, crc], 0x00e1_3280, intern_dependency_keys);
    let sum = pg.u32(m, crc);
    let r = call_from(m, c, pg, frame, this, &[key, sum], 0x00e1_3293, ext(0x00e0_f4b0));
    finish(c, r)
}

/// 0x01054700, `model->update_joints(indices, count, flags)` (thiscall,
/// `ret 0xc`): run the model's joint-update routine over a list of joint
/// indices. The routine is chosen once from the model's flags at +0x378
/// (none, 0x0104EF10, 0x01051B60, 0x010533A0 or 0x01052330; some choices
/// also set bit 6 of +0x108); joints are 0xA0-byte records from +0x364.
/// A joint flagged 0x20 at +0x90 is skipped, and with `flags & 1` only
/// joints with bit 0 of +0x96 are updated. Unless `flags & 2`, joints
/// with an attachment (+4) also get its virtual +0x14 call when model
/// byte +0x37B bit 0 is set. A pending reset (byte +0xE19) runs
/// 0x01054690 first.
pub fn update_joints(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let model = c.ecx;
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    // The index list is read only inside the loop, as the original does.
    let count = arg(m, 1);
    let flags = pg.u8(m, entry.wrapping_add(0xc)); // the original reads the byte only
    let notify = if flags & 2 != 0 { false } else { pg.u8(m, model + 0x37b) & 1 != 0 };
    let visible_only = flags & 1 != 0;
    let guest = |target: u32| move |m: &mut dyn Mem, c: &mut Cpu| ddnative_base::x86::call_ext(m, c, target);
    // The original's frames: four pushes, then ESI pushed for the loop.
    let frame = entry.wrapping_sub(16);
    let loop_frame = entry.wrapping_sub(20);
    if pg.u8(m, model + 0xe19) != 0 {
        call_from(m, c, pg, frame, model, &[], 0x0105_4738, guest(0x0105_4690));
        pg.set_u8(m, model + 0xe19, 0);
    }
    let mode = pg.u32(m, model + 0x378);
    let routine = if mode & 0x0800_0000 != 0 {
        0
    } else if mode & 0x0400_0000 != 0 {
        0x0104_ef10
    } else if mode & 0x40 == 0 {
        0x0105_1b60
    } else {
        let routine = if mode & 0x10_0000 != 0 {
            0x0105_1b60
        } else if mode & 0x4000 != 0 {
            0x0105_33a0
        } else {
            0x0105_2330
        };
        let marks = pg.u32(m, model + 0x108);
        pg.set_u32(m, model + 0x108, marks | 0x40);
        routine
    };
    let joints = model + 0x364;
    for i in 0..count {
        let indices = arg(m, 0);
        let index = pg.u8(m, indices.wrapping_add(i)) as u32;
        let joint = pg.u32(m, joints).wrapping_add(index * 0xa0);
        if pg.u8(m, joint + 0x90) & 0x20 != 0 {
            continue;
        }
        if visible_only && pg.u8(m, joint + 0x96) & 1 == 0 {
            continue;
        }
        if routine != 0 {
            call_from(m, c, pg, loop_frame, model, &[joint], 0x0105_47d6, guest(routine));
        }
        if notify {
            let attachment = pg.u32(m, joint + 4);
            if attachment != 0 {
                let method = pg.u32(m, pg.u32(m, attachment) + 0x14);
                call_from(m, c, pg, loop_frame, attachment, &[joint, model], 0x0105_47ef, guest(method));
            }
        }
    }
    c.esp = entry.wrapping_add(4 + 0xc);
}

//! L184 semantic ports s10: native rewrites (ABI level, see `semantic.rs`)
//! of three functions on the shadow job's critical path. Object offsets and
//! control flow come from the 2017-12-13 Steam executable; the heavy
//! callees stay guest calls.
use crate::semantic::call_from;
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu},
};

/// Runs the guest function at `target` (the lifted body behind the runtime's
/// call handler).
fn guest(target: u32) -> impl FnOnce(&mut dyn Mem, &mut Cpu) {
    move |m, c| x86::call_ext(m, c, target)
}

/// The descriptor a trivial `mov eax, imm32; ret` getter returns, read
/// straight from its code, or `None` for any other method.
#[inline]
fn constant_getter(m: &dyn Mem, pg: Pages, method: u32) -> Option<u32> {
    if pg.u8(m, method) == 0xb8 && pg.u8(m, method.wrapping_add(5)) == 0xc3 {
        Some(pg.u32(m, method.wrapping_add(1)))
    } else {
        None
    }
}

/// Where a class-descriptor chain (`dti`, each link's parent at +0x10, its
/// id at +4) contains `id`.
#[inline]
fn chain_has(m: &dyn Mem, pg: Pages, mut dti: u32, id: u32) -> bool {
    while dti != 0 {
        if pg.u32(m, dti.wrapping_add(4)) == id {
            return true;
        }
        dti = pg.u32(m, dti.wrapping_add(0x10));
    }
    false
}

/// Class-id globals tested by [`apply_param_block`].
const CLASS_SKIPPED: u32 = 0x019a_6cc0;
const CLASS_TARGETED: u32 = 0x019a_6ce0;

// Translated from 0x00c85a00 (DDDA.exe 2017-12-13 Steam).
/// 0x00C85A00, `apply_param_block(block, target, extra)` (stdcall, `ret
/// 0xc`; void): apply a parameter block to a scene object. Objects of the
/// class at 0x019A6CC0 (or derived from it) are left alone; objects not
/// derived from the class at 0x019A6CE0 are left alone too. For the
/// others, the block's three base helpers run (0x00C86390 (EAX target, ECX
/// block), 0x00C86420 (EAX extra, ECX block, stack target), 0x00C864D0 (EAX
/// target, ECX block)), then one helper per section selected by the block's
/// flag word (+4): bit 3 with mode +0x18 = 1 / 2 (0x00C879D0 / 0x00C87A70),
/// bit 4 with mode +0x1c = 1 (0x00C87B70), bit 5 with mode +0x20 = 0 / 1 /
/// 2 (0x00C87C30 / 0x00C87CA0). The block's fields are read again after
/// every helper, as the original, since a helper may change them.
///
/// The object's class descriptor comes from its virtual method +0x10,
/// called twice by the original; a plain `mov eax, imm; ret` method is
/// read from its code instead (it has no effects), any other is called.
pub fn apply_param_block(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let block = pg.u32(m, entry.wrapping_add(4));
    let target = pg.u32(m, entry.wrapping_add(8));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // The original's frame: EBP = entry - 4, ESP aligned to 8, then four pushes.
    let ebp = entry.wrapping_sub(4);
    let frame = (ebp & !7).wrapping_sub(16);
    let entry_edi = c.edi;
    (c.ebx, c.ebp, c.esi, c.edi) = (block, ebp, target, entry_edi);

    // The class descriptor of `target`: one virtual call (resume 0x00C85A19
    // for the first, 0x00C85A49 for the second).
    let vtable = pg.u32(m, target);
    let method = pg.u32(m, vtable.wrapping_add(0x10));
    let constant = constant_getter(m, pg, method);
    let descriptor = |m: &mut dyn Mem, c: &mut Cpu, resume: u32| -> u32 {
        if let Some(dti) = constant {
            return dti;
        }
        let vtable = pg.u32(m, target);
        let method = pg.u32(m, vtable.wrapping_add(0x10));
        c.eax = vtable;
        c.edx = method;
        call_from(m, c, pg, frame, target, &[], resume, guest(method))
    };
    let dti = descriptor(m, c, 0x00c8_5a19);
    let skipped = pg.u32(m, CLASS_SKIPPED);
    let leave = |c: &mut Cpu| {
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(4 + 0xc);
    };
    if chain_has(m, pg, dti, skipped) {
        return leave(c);
    }
    let dti = descriptor(m, c, 0x00c8_5a49);
    let targeted = pg.u32(m, CLASS_TARGETED);
    if !chain_has(m, pg, dti, targeted) {
        return leave(c);
    }

    // helper(target in EAX, block in ECX), ESI = target
    let run = |m: &mut dyn Mem, c: &mut Cpu, helper: u32, resume: u32, eax: u32, ecx: u32, args: &[u32], esi: u32, edi: u32| {
        (c.ebx, c.ebp, c.esi, c.edi) = (block, ebp, esi, edi);
        c.eax = eax;
        call_from(m, c, pg, frame, ecx, args, resume, guest(helper));
    };
    let mut edi = entry_edi;
    run(m, c, 0x00c8_6390, 0x00c8_5a7e, target, block, &[], target, edi);
    let extra = pg.u32(m, entry.wrapping_add(0xc));
    run(m, c, 0x00c8_6420, 0x00c8_5a89, extra, block, &[target], target, edi);
    run(m, c, 0x00c8_64d0, 0x00c8_5a92, target, block, &[], target, edi);
    if pg.u8(m, block.wrapping_add(4)) & 8 != 0 {
        match pg.u32(m, block.wrapping_add(0x18)) {
            1 => run(m, c, 0x00c8_79d0, 0x00c8_5abc, block, target, &[], target, edi),
            2 => {
                // ESI = block, EDI = target for this helper; EDI stays target
                edi = target;
                run(m, c, 0x00c8_7a70, 0x00c8_5aaf, 0, 0, &[], block, target);
            }
            _ => {}
        }
    }
    if pg.u8(m, block.wrapping_add(4)) & 0x10 != 0 && pg.u32(m, block.wrapping_add(0x1c)) == 1 {
        edi = block;
        run(m, c, 0x00c8_7b70, 0x00c8_5ad4, 0, 0, &[], target, block);
    }
    if pg.u8(m, block.wrapping_add(4)) & 0x20 != 0 {
        match pg.u32(m, block.wrapping_add(0x20)) {
            0 => run(m, c, 0x00c8_7c30, 0x00c8_5b01, 0, 0, &[], target, edi),
            1 | 2 => run(m, c, 0x00c8_7ca0, 0x00c8_5af3, target, block, &[], target, edi),
            _ => {}
        }
    }
    leave(c);
}

/// Generation counter stamped into a bound resource when the context first
/// binds it (`resource[1] = counter + 1`).
const BIND_GENERATION: u32 = 0x018d_2ce8;

// Translated from 0x007ffcc0 (DDDA.exe 2017-12-13 Steam).
/// 0x007FFCC0, `submit_pass_records(visible, tables, layers, ordinal,
/// pass_bits, use_overrides)` (custom convention: ESI = render context,
/// EDI = pass sequencer; six stack arguments, `ret 0x18`; returns the
/// result of the sequencer's final virtual call): submit one render pass's
/// draw records to the context.
///
/// The sequencer (EDI) holds the pass's resource binding (+0x0c), stage
/// binding (+0x18), an optional table of override resources (+0x20), and
/// `count` (+0x54) 0x30-byte records at +0x50. Each record is
/// `{layer mask u32 @0, packed @4: slot (bits 0..11), group (12..23),
/// layer (24..31); flags @8 (bit 0 enabled, bit 2 special); u8 @9, u8 @10,
/// key @0x10}`. A record is drawn when its slot is set in `visible`
/// (bitmap at +0x110), it is enabled, its layer is in `layers`, and its
/// layer mask shares a bit with the context's mask (u16 at +0x16a).
///
/// Records are grouped: when the group changes, the previous group is
/// closed (0x00FF14D0 on the context), the sequencer's virtual +0x14 opens
/// the new one, and the group's resource (`tables[group]`, or the override
/// table's entry when `use_overrides` and it is set) is bound with
/// 0x00FED190. Per record the context learns the record
/// (0x00EA23F0, ECX = `visible`), its resource binding when changed, and
/// its pass word (+0x164 bits 5..) from `pass_bits`, from `ordinal / 10` and
/// the record's byte, or, for special records of two resource kinds, the
/// sequencer's virtual +0x20; the sequencer then draws it (virtual +0x1c)
/// and re-opens the stage (+0x14). Finally the last group is closed and
/// the sequencer's virtual +0x18 runs. Every field is read at the point the
/// original reads it, since the guest calls change them.
pub fn submit_pass_records(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let (context, pass) = (c.esi, c.edi);
    let saved = (c.ebx, c.ebp);
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    // The original's frames: 0x20 bytes of locals, EBX, and EBP in the loop.
    let frame_loop = entry.wrapping_sub(0x28);
    let frame_end = entry.wrapping_sub(0x24);
    let call = |m: &mut dyn Mem, c: &mut Cpu, frame: u32, this: u32, target: u32, args: &[u32], resume: u32| {
        call_from(m, c, pg, frame, this, args, resume, guest(target))
    };
    // virtual method `slot` of the sequencer
    let call_virtual = |m: &mut dyn Mem, c: &mut Cpu, frame: u32, slot: u32, args: &[u32], resume: u32| {
        let vtable = pg.u32(m, pass);
        let method = pg.u32(m, vtable.wrapping_add(slot));
        c.edx = method;
        call_from(m, c, pg, frame, pass, args, resume, guest(method))
    };

    let binding = pg.u32(m, pass.wrapping_add(0xc));
    let records = pg.u32(m, pass.wrapping_add(0x50));
    let count = pg.u32(m, pass.wrapping_add(0x54));
    let context_layers = u32::from(pg.u16(m, context.wrapping_add(0x16a)));
    let stage = pg.u32(m, pass.wrapping_add(0x18));
    if pg.u32(m, context.wrapping_add(0x14c)) != stage {
        let generation = pg.u32(m, BIND_GENERATION).wrapping_add(1);
        pg.set_u32(m, stage.wrapping_add(4), generation);
        pg.set_u32(m, context.wrapping_add(0x14c), stage);
        pg.set_u32(m, context.wrapping_add(0x150), 1);
    }
    // without an override table the original clears its `use_overrides` argument
    let overrides = pg.u32(m, pass.wrapping_add(0x20));
    let ordinal_base = if pg.u8(m, pass.wrapping_add(0x570)) != 0 { 10 } else { 0 };
    let mut open_group = u32::MAX;
    let mut index = 0u32;
    // `rec` is the original's EBP: record + 4
    let mut rec = records.wrapping_add(4);
    while index < count {
        let packed = pg.u32(m, rec);
        let slot = packed & 0xfff;
        let visible = arg(m, 0);
        let drawn = pg.u32(m, visible.wrapping_add(0x110).wrapping_add((slot >> 5) * 4)) & (1 << (slot & 0x1f)) != 0
            && pg.u8(m, rec.wrapping_add(4)) & 1 != 0
            && pg.u8(m, entry.wrapping_add(0xc)) & ((packed >> 24) as u8) != 0
            && (pg.u32(m, rec.wrapping_sub(4)) & context_layers) & 0xffff != 0;
        if drawn {
            let group = (pg.u32(m, rec) >> 12) & 0xfff;
            let resource = pg.u32(m, arg(m, 1).wrapping_add(group * 4));
            if open_group != group {
                if (open_group as i32) >= 0 {
                    call(m, c, frame_loop, context, 0x00ff_14d0, &[], 0x007f_fdc6);
                }
                open_group = (pg.u32(m, rec) >> 12) & 0xfff;
                call_virtual(m, c, frame_loop, 0x14, &[context, 0, 2, 0x100], 0x007f_fde8);
                let mut bound = resource;
                if overrides != 0 && pg.u8(m, entry.wrapping_add(0x18)) != 0 {
                    let g = (pg.u32(m, rec) >> 12) & 0xfff;
                    let over = pg.u32(m, overrides.wrapping_add(g * 4));
                    if over != 0 {
                        bound = over;
                    }
                }
                call(m, c, frame_loop, context, 0x00fe_d190, &[bound, 0x18], 0x007f_fe18);
            }
            let visible = arg(m, 0);
            call(m, c, frame_loop, visible, 0x00ea_23f0, &[context, rec.wrapping_sub(4)], 0x007f_fe26);
            let key = pg.u32(m, rec.wrapping_add(0xc));
            let variant = u32::from(pg.u8(m, rec.wrapping_add(6)));
            if pg.u32(m, context.wrapping_add(0x138)) != key
                || pg.u32(m, context.wrapping_add(0x124)) != binding
                || u32::from(pg.u8(m, context.wrapping_add(0x134))) != variant
            {
                if binding != 0 {
                    let generation = pg.u32(m, BIND_GENERATION).wrapping_add(1);
                    pg.set_u32(m, binding.wrapping_add(4), generation);
                }
                pg.set_u32(m, context.wrapping_add(0x124), binding);
                pg.set_u8(m, context.wrapping_add(0x134), variant as u8);
                pg.set_u32(m, context.wrapping_add(0x138), key);
                pg.set_u32(m, context.wrapping_add(0x150), 1);
            }
            let drawn_result = call_virtual(m, c, frame_loop, 0x14, &[context, index, 1, 0x100], 0x007f_fe8c);
            let kind = (pg.u32(m, resource.wrapping_add(0x14)) >> 0x1a) & 0x1f;
            if kind == 0x10 || kind == 0xe {
                if pg.u8(m, rec.wrapping_add(4)) & 4 != 0 {
                    let visible = arg(m, 0);
                    call_virtual(m, c, frame_loop, 0x20, &[context, rec.wrapping_sub(4), visible], 0x007f_fed8);
                } else {
                    let step = u32::from(pg.u8(m, rec.wrapping_add(5))).wrapping_add(ordinal_base);
                    let tens = arg(m, 3) / 10;
                    let word = pg.u32(m, context.wrapping_add(0x164));
                    pg.set_u32(m, context.wrapping_add(0x164), (tens.wrapping_add(step) << 5) | (word & 0x1f));
                }
            } else {
                let word = pg.u32(m, context.wrapping_add(0x164));
                let pass_bits = arg(m, 4);
                pg.set_u32(m, context.wrapping_add(0x164), (word & 0x1f) | (pass_bits << 5));
            }
            call_virtual(m, c, frame_loop, 0x1c, &[index, context, rec.wrapping_sub(4), u32::from(drawn_result != 0)], 0x007f_ff20);
            call_virtual(m, c, frame_loop, 0x14, &[context, 0, 2, 0x100], 0x007f_ff33);
        }
        index += 1;
        rec = rec.wrapping_add(0x30);
    }
    if (open_group as i32) >= 0 {
        call(m, c, frame_end, context, 0x00ff_14d0, &[], 0x007f_ff58);
    }
    call_virtual(m, c, frame_end, 0x18, &[context], 0x007f_ff62);
    (c.ebx, c.ebp) = saved;
    c.esp = entry.wrapping_add(4 + 0x18);
}

/// Render-state helpers of `uPlayerBase::vf25`: the model's state slots are
/// {value, tag} pairs; changing one while the model records an undo log
/// (bit 7 of the byte at +0x5c) first pushes the old pair on the log.
struct StateSlots {
    pg: Pages,
    model: u32,
}

/// Dirty bits of the model's state (+0x154): 1 = a slot changed, 2 = a
/// slot's tag changed.
const DIRTY: u32 = 0x154;
/// The undo log: a count at +0x6c and 12-byte entries {code, old value,
/// old tag} from +0x30c7c.
const UNDO_COUNT: u32 = 0x6c;
const UNDO_LOG: u32 = 0x30c7c;

impl StateSlots {
    fn field(&self, m: &dyn Mem, offset: u32) -> u32 {
        self.pg.u32(m, self.model.wrapping_add(offset))
    }
    fn set_field(&self, m: &mut dyn Mem, offset: u32, value: u32) {
        self.pg.set_u32(m, self.model.wrapping_add(offset), value);
    }
    fn mark_dirty(&self, m: &mut dyn Mem, bits: u32) {
        let dirty = self.field(m, DIRTY);
        self.set_field(m, DIRTY, dirty | bits);
    }
    /// Push {code, slot value, slot tag} on the undo log when it is on.
    fn log_old(&self, m: &mut dyn Mem, slot: u32, code: u32) {
        if self.pg.u8(m, self.model.wrapping_add(0x5c)) & 0x80 == 0 {
            return;
        }
        // The original's read-modify-writes of the entry's first word end at
        // exactly `code` (bit 31 cleared).
        let n = self.field(m, UNDO_COUNT);
        let entry = n.wrapping_mul(12).wrapping_add(UNDO_LOG);
        self.set_field(m, entry, code);
        let value = self.field(m, slot);
        self.set_field(m, entry.wrapping_add(4), value);
        let tag = self.field(m, slot.wrapping_add(4));
        self.set_field(m, entry.wrapping_add(8), tag);
        self.set_field(m, UNDO_COUNT, n.wrapping_add(1));
    }
    /// A plain value slot: when `slot` differs from `[[model + 0x20] +
    /// table_offset]` (the device's current value), log, store it with `tag`
    /// and mark everything dirty.
    fn set_value(&self, m: &mut dyn Mem, slot: u32, table_offset: u32, code: u32, tag: u32) {
        let table = self.field(m, 0x20);
        let current = self.field(m, slot);
        let new = self.pg.u32(m, table.wrapping_add(table_offset));
        if current == new {
            return;
        }
        self.log_old(m, slot, code);
        self.set_field(m, slot, new);
        self.set_field(m, slot.wrapping_add(4), tag);
        self.mark_dirty(m, 3);
    }
    /// A resource slot: when `slot` differs from `new`, log, store it (its
    /// use generation is stamped) with the tag `resource.id | flavour`.
    fn set_resource(&self, m: &mut dyn Mem, slot: u32, new: u32, code: u32, flavour: u32) {
        if self.field(m, slot) == new {
            return;
        }
        self.log_old(m, slot, code);
        let mut tag = 0;
        if new == 0 {
            self.set_field(m, slot, 0);
        } else {
            let generation = self.pg.u32(m, BIND_GENERATION).wrapping_add(1);
            self.pg.set_u32(m, new.wrapping_add(4), generation);
            self.set_field(m, slot, new);
            tag = self.pg.u32(m, new.wrapping_add(0xc)) | flavour;
        }
        if self.field(m, slot.wrapping_add(4)) != tag {
            self.set_field(m, slot.wrapping_add(4), tag);
            self.mark_dirty(m, 2);
        }
        self.mark_dirty(m, 1);
    }
}

/// x87 FLD/FSTP of a float on the hardware: the same value, a signalling
/// NaN quieted (the Unicorn oracle's x87 copies it unchanged, so its
/// vectors hold no signalling NaN there).
#[inline]
fn x87_copy_f32(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

/// `dest = rows * joint`: output `4 * column + row` is the row-th row of
/// the 4x4 float matrix `rows` (at +0..+0x3c) dotted with the joint's
/// column `column` (floats at +0x10 / 0x20 / 0x30 / 0x40 + 4 * column),
/// summed left to right as the original's SSE code does.
fn transform_joint(m: &dyn Mem, pg: Pages, joint: u32, rows: u32) -> [f32; 12] {
    let f = |a: u32| f32::from_bits(pg.u32(m, a));
    let mut out = [0.0f32; 12];
    let mut matrix = [0.0f32; 16];
    for (k, v) in matrix.iter_mut().enumerate() {
        *v = f(rows.wrapping_add(4 * k as u32));
    }
    let mut columns = [[0.0f32; 4]; 3];
    for (c, column) in columns.iter_mut().enumerate() {
        for (k, v) in column.iter_mut().enumerate() {
            *v = f(joint.wrapping_add(0x10 * (k as u32 + 1) + 4 * c as u32));
        }
    }
    for c in 0..3 {
        for r in 0..4 {
            let row = &matrix[4 * r..4 * r + 4];
            let column = &columns[c];
            out[4 * c + r] = ((row[0] * column[0] + row[1] * column[1]) + row[2] * column[2]) + row[3] * column[3];
        }
    }
    out
}

// Translated from 0x00b8a7f0 (DDDA.exe 2017-12-13 Steam).
/// 0x00B8A7F0, `uPlayerBase::vf25(this, model)` (thiscall, `ret 4`; void):
/// prepare the player's model for drawing.
///
/// Without bit 2 of the byte at `this + 0x56d0` it only runs the plain
/// model preparation (0x00FFC9F0) and, unless the model's mask (u16 at
/// +0x16a) has a bit of 0x30c, the virtual draw (`this` vtable +0x198).
/// With the bit, after the base preparation (0x00EA5B50) it binds the
/// player's resources into the model's render state:
///
/// - a player with a skin resource (`this[0x3c]`, whose +0x220 is set) and
///   a model not of kind 9 (byte +0x168): the three skin resources
///   `this[0xe34 / 0xe3c / 0xe38]` and the pass constant (device table
///   +0x2ed8) are bound and the function ends;
/// - otherwise, when the player has joints (`this[0x4d34]` > 0): the render
///   state for the joint count (device table +0x2648 / +0x2668, +0x2650),
///   the joint matrix palette (each joint, selected by the byte list at
///   +0x4d40, times its 0x40-byte matrix from `this[0x5700]`, into the
///   0x30-byte slots of the consumer found by 0x00FE46C0) and the
///   consumer's bookkeeping (0x00BFC7D0); then 0x00FE4780 for
///   the model, the lighting resource (the +0xe2c table when bit 29 of
///   `this[0x108]` is set, else the plain constant) and, last, the model's
///   constants block: a copy of the model's skeleton descriptor words plus
///   the player's four floats (+0xe1c..) in the model's linear allocator
///   (0x00FED890 grows it), and the virtual draw.
///
/// The state slots {value, tag} change through [`StateSlots`], logging the
/// old pair when the model's undo log is on. The skeleton copy (CRT memcpy
/// 0x012EFEA0 in the original) is done natively.
pub fn player_prepare_draw(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let model = pg.u32(m, entry.wrapping_add(4));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // The original's frame: EBP = entry - 4, ESP aligned to 8, 0x4c locals,
    // then EBX, ESI, EDI pushed.
    let locals = ((entry.wrapping_sub(4)) & !7).wrapping_sub(0x4c);
    let frame = locals.wrapping_sub(12);
    let call = |m: &mut dyn Mem, c: &mut Cpu, this: u32, target: u32, args: &[u32], resume: u32| {
        call_from(m, c, pg, frame, this, args, resume, guest(target))
    };
    let draw = |m: &mut dyn Mem, c: &mut Cpu, resume: u32| {
        let vtable = pg.u32(m, this);
        let method = pg.u32(m, vtable.wrapping_add(0x198));
        c.edx = vtable;
        call_from(m, c, pg, frame, this, &[model], resume, guest(method));
    };
    let finish = |c: &mut Cpu| {
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(8);
    };
    let slots = StateSlots { pg, model };

    if pg.u8(m, this.wrapping_add(0x56d0)) & 4 == 0 {
        call(m, c, this, 0x00ff_c9f0, &[model], 0x00b8_a814);
        if pg.u16(m, model.wrapping_add(0x16a)) & 0x30c == 0 {
            draw(m, c, 0x00b8_a833);
        }
        return finish(c);
    }
    call(m, c, this, 0x00ea_5b50, &[model], 0x00b8_a841);

    let has_skin = |m: &dyn Mem| {
        let skin = pg.u32(m, this.wrapping_add(0xf0));
        skin != 0 && pg.u32(m, skin.wrapping_add(0x220)) != 0
    };
    if pg.u8(m, model.wrapping_add(0x168)) == 9 || !has_skin(m) {
        // no usable skin resource: the joint matrix palette path
        let joints = pg.u32(m, this.wrapping_add(0x4d34));
        if joints == 0 {
            return finish(c);
        }
        let device = pg.u32(m, 0x018d_08f4);
        let busy = pg.u32(m, device.wrapping_add(0xfc));
        let table = pg.u32(m, model.wrapping_add(0x20));
        let current = pg.u32(m, model.wrapping_add(0x2840));
        // (unsigned `joints <= busy` picks the second constant)
        let (table_offset, tag) = if joints <= busy { (0x2668, 0xf139_f4cd) } else { (0x2648, 0x4c9) };
        if current != pg.u32(m, table.wrapping_add(table_offset)) {
            slots.log_old(m, 0x2840, 0x4c9);
            let table = slots.field(m, 0x20);
            let new = pg.u32(m, table.wrapping_add(table_offset));
            slots.set_field(m, 0x2840, new);
            slots.set_field(m, 0x2844, tag);
            slots.mark_dirty(m, 3);
        }
        let table = slots.field(m, 0x20);
        let current = slots.field(m, 0x2848);
        if current != pg.u32(m, table.wrapping_add(0x2650)) {
            slots.log_old(m, 0x2848, 0x4ca);
            let table = slots.field(m, 0x20);
            let new = pg.u32(m, table.wrapping_add(0x2650));
            slots.set_field(m, 0x2848, new);
            slots.set_field(m, 0x284c, 0x4ca);
            slots.mark_dirty(m, 3);
        }
        let consumer_id = pg.u32(m, this.wrapping_add(0xd8c));
        let matrices = pg.u32(m, this.wrapping_add(0x5700));
        let consumers = pg.u32(m, 0x018d_2c80);
        let slot_base = call(m, c, consumers, 0x00fe_46c0, &[consumer_id], 0x00b8_ad11);
        if slot_base != 0 {
            let mut out = slot_base.wrapping_add(0x18);
            let mut joint_index = 0u32;
            if pg.u32(m, this.wrapping_add(0x4d34)) != 0 {
                loop {
                    let palette = u32::from(pg.u8(m, this.wrapping_add(joint_index).wrapping_add(0x4d40)));
                    let joint = palette.wrapping_mul(0xa0).wrapping_add(pg.u32(m, this.wrapping_add(0x364)));
                    let rows = palette.wrapping_shl(6).wrapping_add(matrices);
                    let result = transform_joint(m, pg, joint, rows);
                    // the original's store order
                    for i in [2usize, 3, 0, 1, 6, 7, 4, 5, 9, 8, 10, 11] {
                        pg.set_u32(m, out.wrapping_sub(0x18).wrapping_add(4 * i as u32), result[i].to_bits());
                    }
                    joint_index = joint_index.wrapping_add(1);
                    out = out.wrapping_add(0x30);
                    if joint_index >= pg.u32(m, this.wrapping_add(0x4d34)) {
                        break;
                    }
                }
            }
            let countdown = pg.u32(m, this.wrapping_add(0xd80));
            if countdown != 0 {
                pg.set_u32(m, this.wrapping_add(0xd80), countdown.wrapping_sub(1));
            }
            let consumer_id = pg.u32(m, this.wrapping_add(0xd8c));
            let consumers = pg.u32(m, 0x018d_2c80);
            call(m, c, consumers, 0x00bf_c7d0, &[consumer_id], 0x00b8_b144);
        }
    } else {
        // skinned players bind their three skin resources and the pass constant
        slots.set_value(m, 0x30a0, 0x2ed8, 0x5d5, 0xd04f_95db);
        let resource = pg.u32(m, this.wrapping_add(0xe34));
        slots.set_resource(m, 0x1ad8, resource, 0x31c, 0x298d_e000);
        let resource = pg.u32(m, this.wrapping_add(0xe3c));
        slots.set_resource(m, 0x1af0, resource, 0x31f, 0x1523_2000);
        let resource = pg.u32(m, this.wrapping_add(0xe38));
        slots.set_resource(m, 0x1ae8, resource, 0x31e, 0x282e_9000);
        return finish(c);
    }
    let consumers = pg.u32(m, 0x018d_2c80);
    let consumer_id = pg.u32(m, this.wrapping_add(0xd8c));
    call(m, c, consumers, 0x00fe_4780, &[model, consumer_id], 0x00b8_b157);

    let lighting = if (pg.u32(m, this.wrapping_add(0x108)) >> 0x1d) & 1 != 0 {
        let device = pg.u32(m, 0x018d_08f4);
        let index = pg.u32(m, device.wrapping_add(0x33db8));
        let resource = pg.u32(m, this.wrapping_add(index.wrapping_mul(4)).wrapping_add(0xe2c));
        (resource != 0).then_some(resource)
    } else {
        None
    };
    match lighting {
        Some(_) => {
            slots.set_value(m, 0x30d8, 0x2ee8, 0x5dc, 0xf322_75dd);
            let device = pg.u32(m, 0x018d_08f4);
            let index = pg.u32(m, device.wrapping_add(0x33db8));
            let resource = pg.u32(m, this.wrapping_add(index.wrapping_mul(4)).wrapping_add(0xe2c));
            slots.set_resource(m, 0x1d18, resource, 0x364, 0x1c96_7000);
        }
        None => slots.set_value(m, 0x30d8, 0x2ee0, 0x5dc, 0x5dc),
    }

    // the model's constants block
    let skeleton = pg.u32(m, pg.u32(m, model.wrapping_add(0x18)).wrapping_add(0x8c4));
    let words = u32::from(pg.u16(m, skeleton.wrapping_add(0x18)));
    let size = (words.wrapping_mul(4).wrapping_add(0xf)) & !0xf;
    let cursor = slots.field(m, 0x24);
    if cursor.wrapping_add(size) > slots.field(m, 0x28) {
        call(m, c, model, 0x00fe_d890, &[size], 0x00b8_b39c);
    }
    let block = slots.field(m, 0x24);
    slots.set_field(m, 0x24, block.wrapping_add(size));
    slots.set_field(m, 0x1380, block);
    slots.mark_dirty(m, 1);
    if block == 0 {
        draw(m, c, 0x00b8_b41a);
        return finish(c);
    }
    let words = u32::from(pg.u16(m, skeleton.wrapping_add(0x18)));
    let source = pg.u32(m, skeleton.wrapping_add(0x24));
    copy_words(m, pg, block, source, words);
    for k in 0..4u32 {
        let bits = x87_copy_f32(pg.u32(m, this.wrapping_add(0xe1c + 4 * k)));
        pg.set_u32(m, block.wrapping_add(4 * k), bits);
    }
    draw(m, c, 0x00b8_b402);
    finish(c);
}

/// memcpy of `words` 32-bit words (the CRT routine is overlap safe).
fn copy_words(m: &mut dyn Mem, pg: Pages, dst: u32, src: u32, words: u32) {
    let bytes = words.wrapping_mul(4);
    if dst.wrapping_sub(src) < bytes && dst != src {
        let copy: Vec<u32> = (0..words).map(|i| pg.u32(m, src.wrapping_add(4 * i))).collect();
        for (i, v) in copy.into_iter().enumerate() {
            pg.set_u32(m, dst.wrapping_add(4 * i as u32), v);
        }
    } else {
        for i in 0..words {
            let v = pg.u32(m, src.wrapping_add(4 * i));
            pg.set_u32(m, dst.wrapping_add(4 * i), v);
        }
    }
}

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
pub(crate) fn copy_words(m: &mut dyn Mem, pg: Pages, dst: u32, src: u32, words: u32) {
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

/// Constants of 0x00FE4780's transform block: the multipliers of the
/// packed word's cell (x 12) and slot parts (the game's own data).
const CELL_SCALE: u32 = 0x0143_a04c;
const SLOT_SCALE: u32 = 0x0143_43a8;

// Translated from 0x00fe4780 (DDDA.exe 2017-12-13 Steam).
/// 0x00FE4780, `consumer->bind_to_model(model, packed)` (thiscall, `ret 8`;
/// void): bind a consumer's (the model-draw object found by 0x00FE46C0)
/// resources and buffer ranges into a model's render state for the packed
/// word `packed` = {slot (bits 0..11), cell (12..23), count (24..31)}.
///
/// 1. The consumer's resources (+0x8c, +0x90) go into the model's slots
///    0x1b80 / 0x1b88 ([`StateSlots::set_resource`], undo codes 0x331 /
///    0x332).
/// 2. The model's allocator (+0x24 cursor, +0x28 limit; 0x00FED890 grows
///    it) hands out a block sized for the skeleton at `model[0x18][0x7d4]`
///    (words u16 at +0x18, x 4 rounded up to 16) which becomes the model's
///    constants at +0x11a0 (the undo log records the old pair, code 0x1f5);
///    when allocated it holds `{CELL_SCALE, cell * 12 * CELL_SCALE, slot *
///    SLOT_SCALE}` as floats (the original's x87 multiply of the exact
///    integer by the game's constant, rounded once to float).
/// 3. With `n = (count * 36 + 15) / 16` below 16, each of the consumer's
///    buffer ranges (+0x2a0 -> model slots 0x11a8 and 0x11b8, +0x2a4 ->
///    0x11b0; address `base + slot * stride (+0x2a8) + cell * 576`, or the
///    device table's default at +0xfb0 / +0xfc0 / +0xfb8 when that is 0)
///    is bound as `address | n` unless the slot already holds it (low four
///    bits ignored) with a zero tag, logging the old pair (codes 0x1f6 /
///    0x1f8 / 0x1f7).
///
/// Bit 1 of the model's flag word (+0x5c) is set during the constants
/// block's allocation, as the original does.
pub fn bind_consumer_to_model(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let consumer = c.ecx;
    let model = pg.u32(m, entry.wrapping_add(4));
    // The original's frame: ECX, EBX, EBP, ESI, EDI pushed.
    let frame = entry.wrapping_sub(20);
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let slots = StateSlots { pg, model };

    let resource = pg.u32(m, consumer.wrapping_add(0x8c));
    slots.set_resource(m, 0x1b80, resource, 0x331, 0xe06e_a000);
    let resource = pg.u32(m, consumer.wrapping_add(0x90));
    slots.set_resource(m, 0x1b88, resource, 0x332, 0x7c77_0000);
    let flags = slots.field(m, 0x5c);
    slots.set_field(m, 0x5c, flags | 2);
    slots.log_old(m, 0x11a0, 0x1f5);

    let skeleton = pg.u32(m, pg.u32(m, model.wrapping_add(0x18)).wrapping_add(0x7d4));
    let size = (u32::from(pg.u16(m, skeleton.wrapping_add(0x18))).wrapping_mul(4).wrapping_add(0xf)) & !0xf;
    if slots.field(m, 0x24).wrapping_add(size) > slots.field(m, 0x28) {
        call_from(m, c, pg, frame, model, &[size], 0x00fe_4989, guest(0x00fe_d890));
    }
    let block = slots.field(m, 0x24);
    slots.set_field(m, 0x24, block.wrapping_add(size));
    let packed = pg.u32(m, entry.wrapping_add(8));
    slots.set_field(m, 0x11a0, block);
    slots.mark_dirty(m, 1);
    if block != 0 {
        let scale = |m: &dyn Mem, address: u32, whole: u32| {
            // FILD of an exact integer, FMUL by the float constant, FSTP to float
            (f64::from(whole) * f64::from(f32::from_bits(pg.u32(m, address)))) as f32
        };
        let cell = scale(m, CELL_SCALE, ((packed >> 12) & 0xfff) * 12);
        let slot = scale(m, SLOT_SCALE, packed & 0xfff);
        pg.set_u32(m, block.wrapping_add(4), cell.to_bits());
        pg.set_u32(m, block.wrapping_add(8), slot.to_bits());
        let first = pg.u32(m, CELL_SCALE);
        pg.set_u32(m, block, first);
    }
    let flags = slots.field(m, 0x5c) & !2;
    slots.set_field(m, 0x5c, flags);
    let count_units = ((packed >> 24).wrapping_mul(36).wrapping_add(0xf)) >> 4;
    if count_units <= 0xf {
        let cell_offset = ((packed >> 12) & 0xfff).wrapping_mul(9) << 6;
        let slot_part = |m: &dyn Mem| (packed & 0xfff).wrapping_mul(pg.u32(m, consumer.wrapping_add(0x2a8)));
        let device_default = |m: &dyn Mem, offset: u32| pg.u32(m, slots.field(m, 0x20).wrapping_add(offset));
        // `bind_range`: the model's {value, tag} range slot at `slot`
        let bind_range = |m: &mut dyn Mem, slot: u32, address: u32, code: u32, stale_flags: Option<u32>| {
            let current = slots.field(m, slot);
            if current & !0xf == address && slots.field(m, slot + 4) == 0 {
                return;
            }
            // the first range checks the flags word read before the allocation's
            // end (a stale copy), the others read it again
            let logging = match stale_flags {
                Some(flags) => flags as u8 & 0x80 != 0,
                None => pg.u8(m, model.wrapping_add(0x5c)) & 0x80 != 0,
            };
            if logging {
                slots.log_old(m, slot, code);
            }
            slots.set_field(m, slot, address | count_units);
            if slots.field(m, slot + 4) != 0 {
                slots.set_field(m, slot + 4, 0);
                slots.mark_dirty(m, 2);
            }
            slots.mark_dirty(m, 1);
        };
        let first = pg.u32(m, consumer.wrapping_add(0x2a0));
        if first != 0 {
            let base = slot_part(m).wrapping_add(first).wrapping_add(cell_offset);
            let address = if base == 0 { device_default(m, 0xfb0) } else { base };
            bind_range(m, 0x11a8, address, 0x1f6, Some(flags));
            let address = if base == 0 { device_default(m, 0xfc0) } else { base };
            bind_range(m, 0x11b8, address, 0x1f8, None);
        }
        let second = pg.u32(m, consumer.wrapping_add(0x2a4));
        if second != 0 {
            let base = slot_part(m).wrapping_add(second).wrapping_add(cell_offset);
            let address = if base == 0 { device_default(m, 0xfb8) } else { base };
            bind_range(m, 0x11b0, address, 0x1f7, None);
        }
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4 + 8);
}

/// The original body of 0x0049E920, set by the runtime when it installs
/// [`find_nearest_object`]; only used for the pathological case of more
/// than 256 hits (the original keeps 256 and then sorts past its array).
pub static FIND_NEAREST_ORIGINAL: std::sync::OnceLock<ddnative_base::x86::LiftFn> = std::sync::OnceLock::new();

/// Class-id globals of 0x0049E920: objects of the first class are range
/// tested, objects of the second or third class are skipped, any other
/// object ends the search with no result.
const CLASS_RANGED: u32 = 0x019a_5e30;
const CLASS_SKIPPED_B: u32 = 0x019a_60b0;
const CLASS_SKIPPED_C: u32 = 0x019a_60d0;

// Translated from 0x0049e920 (DDDA.exe 2017-12-13 Steam).
/// 0x0049E920, `find_nearest_object(list, exclude, owner_id, position,
/// range_scale, max_dy, param)` (stdcall, `ret 0x1c`): the list's object
/// (count +0x24, pointers +0x30) of the first class (descriptor chain
/// containing the id at 0x019A5E30) nearest to `position` (three floats),
/// or 0.
///
/// An object is a candidate when it is not `exclude`, its flags (+4) have
/// kind 2 (low three bits) and bit 0x400, it is of the first class with
/// `owner_id` at +0x2574 (and `param` at +0x2534 when `param` is set), the
/// planar distance (x, z at +0x40 / +0x48) is within `range_scale *
/// [0x014F9E04]` (and `range_scale > 0`) and the height difference (y at
/// +0x44, absolute value through the mask at 0x0161C330) does not exceed
/// `max_dy`. An object of neither the first, the second (0x019A60B0) nor
/// the third (0x019A60D0) class makes the whole search return 0 at once.
///
/// The original bubble-sorts the hits by distance and returns the first;
/// that is the first hit of the smallest distance, found here by a scan
/// (the sort itself when a distance is a NaN, whose order the sort defines).
/// The class descriptor comes from virtual +0x10, called three times per
/// object by the original; a plain `mov eax, imm; ret` getter is read from
/// its code once instead, any other is called three times.
pub fn find_nearest_object(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // The original's frame: EBP = entry - 4, ESP aligned to 8, 0x80c locals,
    // EBX, ESI, EDI pushed.
    let ebp = entry.wrapping_sub(4);
    let frame = (ebp & !7).wrapping_sub(0x80c).wrapping_sub(12);
    let list = arg(m, 0);
    let total = pg.u32(m, list.wrapping_add(0x24));
    let mut hits: Vec<(u32, f32)> = Vec::new();
    let finish = |c: &mut Cpu, result: u32| {
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.eax = result;
        c.esp = entry.wrapping_add(4 + 0x1c);
    };
    let mut index = 0u32;
    while index < total {
        let object = pg.u32(m, pg.u32(m, arg(m, 0).wrapping_add(0x30)).wrapping_add(index.wrapping_mul(4)));
        index += 1;
        if object == arg(m, 1) {
            continue;
        }
        let flags = pg.u32(m, object.wrapping_add(4));
        if flags & 7 != 2 || flags & 0x400 == 0 {
            continue;
        }
        // the object's class descriptor (virtual +0x10, resume addresses of its three calls)
        let vtable = pg.u32(m, object);
        let method = pg.u32(m, vtable.wrapping_add(0x10));
        let constant = constant_getter(m, pg, method);
        c.esi = object;
        let descriptor = |m: &mut dyn Mem, c: &mut Cpu, resume: u32| -> u32 {
            if let Some(dti) = constant {
                return dti;
            }
            let vtable = pg.u32(m, object);
            let method = pg.u32(m, vtable.wrapping_add(0x10));
            c.eax = method;
            c.edx = vtable;
            call_from(m, c, pg, frame, object, &[], resume, guest(method))
        };
        let dti = descriptor(m, c, 0x0049_e9a1);
        let class = pg.u32(m, CLASS_RANGED);
        let ranged = chain_has(m, pg, dti, class);
        let dti = descriptor(m, c, 0x0049_e9d2);
        let class = pg.u32(m, CLASS_SKIPPED_B);
        let skipped_b = chain_has(m, pg, dti, class);
        let dti = descriptor(m, c, 0x0049_ea0b);
        let class = pg.u32(m, CLASS_SKIPPED_C);
        let skipped_c = chain_has(m, pg, dti, class);
        if !ranged {
            if skipped_b {
                continue;
            }
            if !skipped_c {
                return finish(c, 0);
            }
            continue;
        }
        if pg.u32(m, object.wrapping_add(0x2574)) != arg(m, 2) {
            continue;
        }
        let param = arg(m, 6);
        if param != 0 && pg.u32(m, object.wrapping_add(0x2534)) != param {
            continue;
        }
        let f = |m: &dyn Mem, a: u32| f32::from_bits(pg.u32(m, a));
        let position = arg(m, 3);
        let dx = f(m, position) - f(m, object.wrapping_add(0x40));
        let dz = f(m, position.wrapping_add(8)) - f(m, object.wrapping_add(0x48));
        let distance = (dz * dz + dx * dx).sqrt();
        let scale = f32::from_bits(arg(m, 4));
        if 0.0 >= scale {
            continue;
        }
        let limit = scale * f32::from_bits(pg.u32(m, 0x014f_9e04));
        if distance > limit {
            continue;
        }
        let dy = f(m, position.wrapping_add(4)) - f(m, object.wrapping_add(0x44));
        let dy = f32::from_bits(dy.to_bits() & pg.u32(m, 0x0161_c330));
        if dy > f32::from_bits(arg(m, 5)) {
            continue;
        }
        if hits.len() >= 0x100 {
            // more hits than the original's array: it sorts past it
            let original = FIND_NEAREST_ORIGINAL.get().expect("0x0049E920 original body");
            (c.ebx, c.ebp, c.esi, c.edi) = saved;
            c.esp = entry;
            return original(m, c);
        }
        hits.push((object, distance));
    }
    // the first hit of the smallest distance; with a NaN distance the
    // original's bubble sort decides
    let nearest = if hits.iter().any(|h| h.1.is_nan()) {
        let mut sorted = hits;
        let mut passes = sorted.len().saturating_sub(1);
        while passes > 0 {
            for j in 0..passes {
                if sorted[j].1 > sorted[j + 1].1 {
                    sorted.swap(j, j + 1);
                }
            }
            passes -= 1;
        }
        sorted.first().map_or(0, |h| h.0)
    } else {
        let mut best: Option<(u32, f32)> = None;
        for &(object, distance) in &hits {
            if best.map_or(true, |b| distance < b.1) {
                best = Some((object, distance));
            }
        }
        best.map_or(0, |b| b.0)
    };
    finish(c, nearest);
}

/// Words in the lighting-probe accumulator (a spherical-harmonics style
/// vector of 36 floats) and the bytes of the original's frame the port
/// models (offsets from the frame's ESP).
const PROBE_WORDS: usize = 36;
const ACCUMULATOR: u32 = 0x20;
const CONTRIBUTION: u32 = 0xd0;

/// Reads a guest f32 pair as the 64-bit value `MOVQ` / `SQRTSD` load.
#[inline]
fn read_u64(m: &dyn Mem, pg: Pages, address: u32) -> u64 {
    u64::from(pg.u32(m, address)) | u64::from(pg.u32(m, address.wrapping_add(4))) << 32
}

// Translated from 0x00fe66a0 (DDDA.exe 2017-12-13 Steam).
/// 0x00FE66A0, `scene->gather_ambient_probe(model, light_mask, flags,
/// position)` (thiscall, `ret 0x10`; void): gather the lighting around
/// `position` for `model` into the model's per-draw lighting constants
/// (block at +0x11c0, 0x90 bytes).
///
/// The scene (`this`) holds light lists in `this[0x20]` as ranges
/// (`this[0x3c]` + `this[0x40]`, `this[0x34]` + `this[0x38]`, `this[0x2c]`
/// + `this[0x30]`, `this[0x44]` + `this[0x48]`). An object takes part when
/// its kind word (+4) has the model's class bit (byte +0x168 of the
/// model) in bits 16..25, kind 2 in the low three bits and 0x4800 set.
///
/// - flags bit 1: the first range's objects (that pass the test of
///   0x00D13600 against `position`) report a float through their virtual
///   +0x4c (x87 `ST0`); the largest per kind (word +0x32 zero or not) goes
///   to two float slots.
/// - flags bit 0: the second and third ranges' objects (mask `light_mask`
///   against +0x34, bit 1 of +0x30; the third also needs 0x00D13600 on its
///   +0x210) fill 36 floats through virtual +0x4c(model, position,
///   buffer) -> bool; the contributions are summed. With `this[0xb5c]` and
///   `this[0xb60]` both set, the nested search 0x011787C0 gathers them
///   instead (callback 0x00FE5190 on a sphere around `position`).
/// - the model's state slot 0x28f0 is bound to one of two device
///   constants by whether anything contributed; the fourth range's first
///   matching object supplies a direction, intensity and resource (flags
///   bit 2 picks that resource over the default; slot 0x1cb8 follows).
/// - finally the constants block is allocated in the model's allocator and
///   filled with the direction, the intensities and the probe converted
///   to its band form: the sum's 36 floats scaled by the normalisation
///   constants (sqrt of the constants at 0x0161D290 / 0x0161CA58 /
///   0x0143A4E0 / 0x01408CA8 divided by their multipliers), 28 of them
///   copied to the block.
///
/// The frame is modelled in a local array at the original's offsets, and
/// synced with guest memory around the three calls that see it (the
/// virtual buffer fill and the nested search).
pub fn gather_ambient_probe(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let model = pg.u32(m, entry.wrapping_add(4));
    let slots = StateSlots { pg, model };
    // The original's frame: EBP = entry - 4, ESP aligned to 16, 0x1b4
    // locals, EBX, ESI, EDI pushed. `frame` is its ESP.
    let frame = ((entry.wrapping_sub(4)) & !0xf).wrapping_sub(0x1b4).wrapping_sub(12);
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let rd = |m: &dyn Mem, address: u32| pg.u32(m, address);
    let rd64 = |m: &dyn Mem, address: u32| read_u64(m, pg, address);
    let guest_call = |m: &mut dyn Mem, c: &mut Cpu, this: u32, target: u32, args: &[u32], resume: u32| {
        call_from(m, c, pg, frame, this, args, resume, guest(target))
    };
    // frame words at the original's offsets
    let mut l = [0u32; 0x1c0 / 4];
    let f = |bits: u32| f32::from_bits(bits);

    l[0xbc / 4] = 1u32 << (pg.u8(m, model.wrapping_add(0x168)) & 0x1f);
    let flags = arg(m, 2);
    l[0xcc / 4] = rd(m, 0x018c_fe5c);
    l[0xc8 / 4] = rd(m, 0x018c_fe58);
    let class_bit = l[0xbc / 4];
    let takes_part = |word: u32| (word >> 16) & class_bit & 0x3ff != 0 && word & 7 == 2 && word & 0x4800 == 0x4800;
    let buffer_address = frame.wrapping_add(CONTRIBUTION);

    // 1. floats reported by the first range
    if flags & 2 != 0 && pg.u32(m, this.wrapping_add(0x40)) != 0 {
        let mut i = 0u32;
        loop {
            let index = pg.u32(m, this.wrapping_add(0x3c)).wrapping_add(i);
            let list = pg.u32(m, this.wrapping_add(0x20));
            let object = pg.u32(m, list.wrapping_add(index.wrapping_mul(4)));
            if takes_part(pg.u32(m, object.wrapping_add(4))) {
                let position = arg(m, 3);
                guest_call(m, c, object, 0x00d1_3600, &[position, object.wrapping_add(0x60)], 0x00fe_674f);
                if c.eax & 0xff != 0 {
                    let position = arg(m, 3);
                    let method = pg.u32(m, pg.u32(m, object).wrapping_add(0x4c));
                    call_from(m, c, pg, frame, object, &[position], 0x00fe_6763, guest(method));
                    // FSTP float: the callee's ST0
                    let reported = match c.st.top_f32() {
                        Some(bits) => {
                            c.st.pop();
                            bits
                        }
                        None => 0xffc0_0000,
                    };
                    l[0x1c / 4] = reported;
                    let slot = if pg.u16(m, object.wrapping_add(0x32)) == 0 { 0xc8 / 4 } else { 0xcc / 4 };
                    if f(reported) > f(l[slot]) {
                        l[slot] = reported;
                    }
                }
            }
            i = i.wrapping_add(1);
            if i >= pg.u32(m, this.wrapping_add(0x40)) {
                break;
            }
        }
    }

    // 2. contributions of the second and third ranges
    let want_probe = flags & 1 != 0;
    l[0xc4 / 4] = flags & 1;
    let mut hit = false;
    // sum one contribution (the buffer the virtual call filled)
    let mut accumulate = |m: &dyn Mem, l: &mut [u32; 0x1c0 / 4], hit: &mut bool| {
        let base = (ACCUMULATOR / 4) as usize;
        let buffer = (CONTRIBUTION / 4) as usize;
        for k in 0..PROBE_WORDS {
            let contribution = pg.u32(m, buffer_address.wrapping_add(4 * k as u32));
            l[buffer + k] = contribution;
            l[base + k] = if *hit { (f(contribution) + f(l[base + k])).to_bits() } else { contribution };
        }
        *hit = true;
    };
    if want_probe && pg.u32(m, this.wrapping_add(0x38)) != 0 {
        let mut j = 0u32;
        loop {
            let index = pg.u32(m, this.wrapping_add(0x34)).wrapping_add(j);
            let list = pg.u32(m, this.wrapping_add(0x20));
            let object = pg.u32(m, list.wrapping_add(index.wrapping_mul(4)));
            let light_mask = arg(m, 1);
            if pg.u32(m, object.wrapping_add(0x34)) & light_mask != 0
                && takes_part(pg.u32(m, object.wrapping_add(4)))
                && (pg.u32(m, object.wrapping_add(0x30)) >> 1) & 1 != 0
            {
                let method = pg.u32(m, pg.u32(m, object).wrapping_add(0x4c));
                let position = arg(m, 3);
                call_from(m, c, pg, frame, object, &[model, position, buffer_address], 0x00fe_6861, guest(method));
                if c.eax & 0xff != 0 {
                    accumulate(m, &mut l, &mut hit);
                }
            }
            j = j.wrapping_add(1);
            if j >= pg.u32(m, this.wrapping_add(0x38)) {
                break;
            }
        }
    }
    if want_probe {
        if pg.u8(m, this.wrapping_add(0xb5c)) == 0 || pg.u32(m, this.wrapping_add(0xb60)) == 0 {
            if pg.u32(m, this.wrapping_add(0x30)) != 0 {
                let mut k = 0u32;
                loop {
                    let index = pg.u32(m, this.wrapping_add(0x2c)).wrapping_add(k);
                    let list = pg.u32(m, this.wrapping_add(0x20));
                    let object = pg.u32(m, list.wrapping_add(index.wrapping_mul(4)));
                    let light_mask = arg(m, 1);
                    if pg.u32(m, object.wrapping_add(0x34)) & light_mask != 0
                        && takes_part(pg.u32(m, object.wrapping_add(4)))
                        && (pg.u32(m, object.wrapping_add(0x30)) >> 1) & 1 != 0
                    {
                        let position = arg(m, 3);
                        guest_call(m, c, object, 0x00d1_3600, &[object.wrapping_add(0x210), position], 0x00fe_6f4f);
                        if c.eax & 0xff != 0 {
                            let method = pg.u32(m, pg.u32(m, object).wrapping_add(0x4c));
                            let position = arg(m, 3);
                            call_from(m, c, pg, frame, object, &[model, position, buffer_address], 0x00fe_6f70, guest(method));
                            if c.eax & 0xff != 0 {
                                accumulate(m, &mut l, &mut hit);
                            }
                        }
                    }
                    k = k.wrapping_add(1);
                    if k >= pg.u32(m, this.wrapping_add(0x30)) {
                        break;
                    }
                }
            }
        } else {
            // the nested search: a sphere around `position` and a callback that
            // sums into the accumulator, which therefore lives in guest memory
            // for the call
            l[0x188 / 4] = rd(m, 0x0162_adb0);
            let position = arg(m, 3);
            l[0x190 / 4] = pg.u32(m, position);
            l[0x194 / 4] = pg.u32(m, position.wrapping_add(4));
            let high = rd64(m, position.wrapping_add(8));
            l[0x198 / 4] = high as u32;
            l[0x19c / 4] = (high >> 32) as u32;
            let light_mask = arg(m, 1);
            l[0x160 / 4] = light_mask;
            let old = l[0x168 / 4];
            l[0x168 / 4] = (old & !0xff) | u32::from(hit);
            l[0x164 / 4] = class_bit;
            l[0x16c / 4] = model;
            l[0x170 / 4] = frame.wrapping_add(ACCUMULATOR);
            l[0x180 / 4] = 0x0157_92e8;
            l[0x184 / 4] = 5;
            for (offset, word) in [
                (0x160u32, l[0x160 / 4]),
                (0x164, l[0x164 / 4]),
                (0x168, l[0x168 / 4] & 0xff),
                (0x16c, l[0x16c / 4]),
                (0x170, l[0x170 / 4]),
                (0x180, l[0x180 / 4]),
                (0x184, l[0x184 / 4]),
                (0x188, l[0x188 / 4]),
                (0x190, l[0x190 / 4]),
                (0x194, l[0x194 / 4]),
                (0x198, l[0x198 / 4]),
                (0x19c, l[0x19c / 4]),
            ] {
                if offset == 0x168 {
                    pg.set_u8(m, frame.wrapping_add(offset), word as u8);
                } else {
                    pg.set_u32(m, frame.wrapping_add(offset), word);
                }
            }
            for k in 0..PROBE_WORDS {
                pg.set_u32(m, frame.wrapping_add(ACCUMULATOR + 4 * k as u32), l[(ACCUMULATOR / 4) as usize + k]);
            }
            let ecx = c.ecx;
            guest_call(
                m,
                c,
                ecx,
                0x0117_87c0,
                &[frame.wrapping_add(0x180), this, 0x00fe_5190, frame.wrapping_add(0x160), 0, 0xffff_ffff],
                0x00fe_6eb6,
            );
            hit = pg.u8(m, frame.wrapping_add(0x168)) != 0;
            for k in 0..PROBE_WORDS {
                l[(ACCUMULATOR / 4) as usize + k] = pg.u32(m, frame.wrapping_add(ACCUMULATOR + 4 * k as u32));
            }
        }
    }

    // 3. the model's state slot 0x28f0 follows whether anything contributed
    if hit {
        slots.set_value(m, 0x28f0, 0x26f8, 0x4df, 0x4df);
    } else {
        slots.set_value(m, 0x28f0, 0x2700, 0x4df, 0xbeaa_b4e0);
    }

    // 4. the fourth range's first matching object: direction, intensity, resource
    let lights = pg.u32(m, this.wrapping_add(0x48));
    let one = rd(m, 0x0140_6ce0);
    l[0x160 / 4] = rd(m, 0x018c_fe80);
    l[0x164 / 4] = rd(m, 0x018c_fe84);
    l[0x168 / 4] = rd(m, 0x018c_fe88);
    l[0x1bc / 4] = one;
    l[0xc4 / 4] = one;
    l[0xc0 / 4] = lights;
    l[0x1c / 4] = 0;
    if lights != 0 {
        let first = pg.u32(m, this.wrapping_add(0x44));
        let list = pg.u32(m, this.wrapping_add(0x20));
        let mut pointer = list.wrapping_add(first.wrapping_mul(4));
        let mut i = 0u32;
        while i < lights {
            let object = pg.u32(m, pointer);
            let light_mask = arg(m, 1);
            if pg.u32(m, object.wrapping_add(0x34)) & light_mask != 0 && takes_part(pg.u32(m, object.wrapping_add(4))) {
                let source = pg.u32(m, object.wrapping_add(0x238));
                let resource = if source != 0 { pg.u32(m, source.wrapping_add(0xcc)) } else { 0 };
                l[0x1bc / 4] = pg.u32(m, object.wrapping_add(0x230));
                l[0x160 / 4] = pg.u32(m, object.wrapping_add(0x50));
                l[0x164 / 4] = pg.u32(m, object.wrapping_add(0x54));
                l[0x168 / 4] = pg.u32(m, object.wrapping_add(0x58));
                l[0x1c / 4] = resource;
                l[0xc4 / 4] = pg.u32(m, object.wrapping_add(0x234));
                break;
            }
            pointer = pointer.wrapping_add(4);
            i += 1;
        }
    }
    if arg(m, 2) & 4 == 0 {
        slots.set_resource(m, 0x1cb8, 0, 0x358, 0xf1d7_e000);
    } else {
        let mut resource = l[0x1c / 4];
        if resource == 0 {
            resource = pg.u32(m, pg.u32(m, 0x018d_2c80).wrapping_add(0x2c0));
        }
        slots.set_resource(m, 0x1cb8, resource, 0x358, 0xf1d7_e000);
    }

    // 5. the constants block
    let flags_word = slots.field(m, 0x5c);
    slots.set_field(m, 0x5c, flags_word | 2);
    slots.log_old(m, 0x11c0, 0x1f9);
    let skeleton = pg.u32(m, pg.u32(m, model.wrapping_add(0x18)).wrapping_add(0x7e4));
    let size = (u32::from(pg.u16(m, skeleton.wrapping_add(0x18))).wrapping_mul(4).wrapping_add(0xf)) & !0xf;
    if slots.field(m, 0x24).wrapping_add(size) > slots.field(m, 0x28) {
        call_from(m, c, pg, frame, model, &[size], 0x00fe_7935, guest(0x00fe_d890));
    }
    let block = slots.field(m, 0x24);
    slots.set_field(m, 0x24, block.wrapping_add(size));
    slots.set_field(m, 0x11c0, block);
    slots.mark_dirty(m, 1);
    if block != 0 {
        let one = rd(m, 0x0140_6ce0);
        let direction = [l[0x160 / 4], l[0x164 / 4], l[0x168 / 4]];
        for (k, word) in [direction[0], direction[1], direction[2], l[0x1bc / 4], l[0xc4 / 4], l[0xc8 / 4], l[0xcc / 4]].into_iter().enumerate() {
            pg.set_u32(m, block.wrapping_add(4 * k as u32), word);
        }
        let device = pg.u32(m, 0x018d_08f4);
        let word = x87_copy_f32(pg.u32(m, device.wrapping_add(0x11c)));
        pg.set_u32(m, block.wrapping_add(0x1c), word);
        // x: the original's XMM registers (low 64 bits)
        let mut x = [0u64; 8];
        x[0] = u64::from(direction[0]);
        x[1] = u64::from(direction[1]);
        x[2] = u64::from(direction[2]);
        x[5] = u64::from(one);
        if hit {
            x[6] = u64::from(l[0x2c / 4]);
            x[7] = u64::from(l[0x24 / 4]);
        } else {
            l[0x20 / 4] = x[0] as u32;
            l[0x50 / 4] = x[1] as u32;
            l[0x80 / 4] = x[2] as u32;
            for offset in [0x28u32, 0x30, 0x34, 0x38, 0x3c, 0x40, 0x54, 0x58, 0x5c, 0x60, 0x64, 0x68, 0x6c, 0x70, 0x84, 0x88, 0x8c, 0x90, 0x94, 0x98, 0x9c, 0xa0] {
                l[(offset / 4) as usize] = 0;
            }
            x[0] = 0;
            x[6] = 0;
            x[7] = 0;
        }
        {
            x[0] = f64::from_bits(rd64(m, 0x0161d290)).sqrt().to_bits();
            x[2] ^= x[2];
            x[2] = (x[2] & !0xffff_ffff) | u64::from((f64::from_bits(x[0]) as f32).to_bits());
            x[0] = x[2];
            x[0] = (x[0] & !0xffff_ffff) | u64::from((f32::from_bits((x[0] as u32)) * f32::from_bits(rd(m, 0x01406cec))).to_bits());
            x[5] = (x[5] & !0xffff_ffff) | u64::from((f32::from_bits((x[5] as u32)) / f32::from_bits((x[0] as u32))).to_bits());
            x[0] = f64::from_bits(rd64(m, 0x0161ca58)).sqrt().to_bits();
            x[1] ^= x[1];
            x[1] = (x[1] & !0xffff_ffff) | u64::from((f64::from_bits(x[0]) as f32).to_bits());
            x[0] = x[2];
            x[0] = (x[0] & !0xffff_ffff) | u64::from((f32::from_bits((x[0] as u32)) * f32::from_bits(rd(m, 0x01406cf4))).to_bits());
            x[1] = (x[1] & !0xffff_ffff) | u64::from((f32::from_bits((x[1] as u32)) / f32::from_bits((x[0] as u32))).to_bits());
            x[0] = f64::from_bits(rd64(m, 0x0143a4e0)).sqrt().to_bits();
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) * f32::from_bits((x[1] as u32))).to_bits());
            x[3] = x[2];
            x[3] = (x[3] & !0xffff_ffff) | u64::from((f32::from_bits((x[3] as u32)) * f32::from_bits(rd(m, 0x01434024))).to_bits());
            x[2] = (x[2] & !0xffff_ffff) | u64::from((f32::from_bits((x[2] as u32)) * f32::from_bits(rd(m, 0x0142a8f0))).to_bits());
            x[0] = (x[0] & !0xffff_ffff) | u64::from((f64::from_bits(x[0]) as f32).to_bits());
            x[0] = (x[0] & !0xffff_ffff) | u64::from((f32::from_bits((x[0] as u32)) / f32::from_bits((x[3] as u32))).to_bits());
            x[3] = f64::from_bits(rd64(m, 0x01408ca8)).sqrt().to_bits();
            x[7] = (x[7] & !0xffff_ffff) | u64::from((f32::from_bits((x[7] as u32)) * f32::from_bits((x[1] as u32))).to_bits());
            x[3] = u64::from((f64::from_bits(x[3]) as f32).to_bits());
            x[3] = (x[3] & !0xffff_ffff) | u64::from((f32::from_bits((x[3] as u32)) / f32::from_bits((x[2] as u32))).to_bits());
            x[2] = u64::from(rd(m, 0x0161c280));
            x[6] ^= x[2];
            l[96] = x[6] as u32;
            x[6] = u64::from(l[10]);
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) * f32::from_bits((x[1] as u32))).to_bits());
            l[98] = x[6] as u32;
            x[6] = u64::from(l[8]);
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) * f32::from_bits((x[5] as u32))).to_bits());
            l[48] = x[6] as u32;
            x[7] ^= x[2];
            l[97] = x[7] as u32;
            x[7] = x[3];
            x[7] = (x[7] & !0xffff_ffff) | u64::from((f32::from_bits((x[7] as u32)) * f32::from_bits(l[14])).to_bits());
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) - f32::from_bits((x[7] as u32))).to_bits());
            l[99] = x[6] as u32;
            x[6] = u64::from(l[23]);
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) * f32::from_bits((x[1] as u32))).to_bits());
            x[6] ^= x[2];
            l[100] = x[6] as u32;
            x[6] = u64::from(l[21]);
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) * f32::from_bits((x[1] as u32))).to_bits());
            x[6] ^= x[2];
            l[101] = x[6] as u32;
            x[6] = u64::from(l[22]);
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) * f32::from_bits((x[1] as u32))).to_bits());
            l[102] = x[6] as u32;
            x[6] = u64::from(l[20]);
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) * f32::from_bits((x[5] as u32))).to_bits());
            l[48] = x[6] as u32;
            x[7] = u64::from(l[48]);
            x[6] = x[3];
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) * f32::from_bits(l[26])).to_bits());
            x[7] = (x[7] & !0xffff_ffff) | u64::from((f32::from_bits((x[7] as u32)) - f32::from_bits((x[6] as u32))).to_bits());
            x[6] = u64::from(l[35]);
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) * f32::from_bits((x[1] as u32))).to_bits());
            x[6] ^= x[2];
            l[104] = x[6] as u32;
            x[6] = u64::from(l[33]);
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) * f32::from_bits((x[1] as u32))).to_bits());
            x[6] ^= x[2];
            l[105] = x[6] as u32;
            x[6] = u64::from(l[34]);
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) * f32::from_bits((x[1] as u32))).to_bits());
            x[1] = u64::from(l[32]);
            x[1] = (x[1] & !0xffff_ffff) | u64::from((f32::from_bits((x[1] as u32)) * f32::from_bits((x[5] as u32))).to_bits());
            x[5] = x[3];
            x[5] = (x[5] & !0xffff_ffff) | u64::from((f32::from_bits((x[5] as u32)) * f32::from_bits(l[38])).to_bits());
            x[1] = (x[1] & !0xffff_ffff) | u64::from((f32::from_bits((x[1] as u32)) - f32::from_bits((x[5] as u32))).to_bits());
            x[5] = u64::from(l[96]);
            l[52] = x[5] as u32;
            x[5] = u64::from(l[97]);
            l[53] = x[5] as u32;
            x[5] = u64::from(l[98]);
            l[54] = x[5] as u32;
            x[5] = u64::from(l[99]);
            x[4] = x[0];
            x[4] = (x[4] & !0xffff_ffff) | u64::from((f32::from_bits((x[4] as u32)) * f32::from_bits(rd(m, 0x01406cf0))).to_bits());
            l[55] = x[5] as u32;
            x[5] = u64::from(l[100]);
            x[3] = (x[3] & !0xffff_ffff) | u64::from((f32::from_bits((x[3] as u32)) * f32::from_bits(rd(m, 0x01406cf4))).to_bits());
            l[56] = x[5] as u32;
            x[5] = u64::from(l[101]);
            l[63] = x[1] as u32;
            l[57] = x[5] as u32;
            x[5] = u64::from(l[102]);
            l[58] = x[5] as u32;
            x[5] = u64::from(l[104]);
            l[59] = x[7] as u32;
            x[1] = x[0];
            x[1] = (x[1] & !0xffff_ffff) | u64::from((f32::from_bits((x[1] as u32)) * f32::from_bits(l[12])).to_bits());
            l[96] = x[1] as u32;
            l[60] = x[5] as u32;
            x[5] = u64::from(l[105]);
            x[1] = x[0];
            x[1] = (x[1] & !0xffff_ffff) | u64::from((f32::from_bits((x[1] as u32)) * f32::from_bits(l[13])).to_bits());
            x[1] ^= x[2];
            l[61] = x[5] as u32;
            l[65] = x[1] as u32;
            l[62] = x[6] as u32;
            x[6] = x[0];
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) * f32::from_bits(l[24])).to_bits());
            x[7] = x[3];
            x[7] = (x[7] & !0xffff_ffff) | u64::from((f32::from_bits((x[7] as u32)) * f32::from_bits(l[26])).to_bits());
            l[102] = x[7] as u32;
            x[7] = x[0];
            x[7] = (x[7] & !0xffff_ffff) | u64::from((f32::from_bits((x[7] as u32)) * f32::from_bits(l[27])).to_bits());
            x[7] ^= x[2];
            x[5] = x[3];
            x[5] = (x[5] & !0xffff_ffff) | u64::from((f32::from_bits((x[5] as u32)) * f32::from_bits(l[14])).to_bits());
            x[3] = (x[3] & !0xffff_ffff) | u64::from((f32::from_bits((x[3] as u32)) * f32::from_bits(l[38])).to_bits());
            l[98] = x[5] as u32;
            x[1] = u64::from(l[98]);
            l[66] = x[1] as u32;
            l[103] = x[7] as u32;
            l[100] = x[6] as u32;
            x[1] = u64::from(l[100]);
            x[7] = x[0];
            x[7] = (x[7] & !0xffff_ffff) | u64::from((f32::from_bits((x[7] as u32)) * f32::from_bits(l[36])).to_bits());
            l[68] = x[1] as u32;
            x[1] = u64::from(l[102]);
            x[5] = x[0];
            x[5] = (x[5] & !0xffff_ffff) | u64::from((f32::from_bits((x[5] as u32)) * f32::from_bits(l[15])).to_bits());
            x[6] = x[0];
            x[6] = (x[6] & !0xffff_ffff) | u64::from((f32::from_bits((x[6] as u32)) * f32::from_bits(l[25])).to_bits());
            l[70] = x[1] as u32;
            x[1] = u64::from(l[103]);
            l[104] = x[7] as u32;
            x[7] = x[0];
            x[0] = (x[0] & !0xffff_ffff) | u64::from((f32::from_bits((x[0] as u32)) * f32::from_bits(l[39])).to_bits());
            x[7] = (x[7] & !0xffff_ffff) | u64::from((f32::from_bits((x[7] as u32)) * f32::from_bits(l[37])).to_bits());
            x[0] ^= x[2];
            l[71] = x[1] as u32;
            x[1] = u64::from(l[104]);
            x[7] ^= x[2];
            l[72] = x[1] as u32;
            l[75] = x[0] as u32;
            x[1] = x[7];
            x[0] = x[4];
            x[0] = (x[0] & !0xffff_ffff) | u64::from((f32::from_bits((x[0] as u32)) * f32::from_bits(l[16])).to_bits());
            l[73] = x[1] as u32;
            x[5] ^= x[2];
            x[6] ^= x[2];
            x[2] = u64::from(l[96]);
            x[1] = x[4];
            x[1] = (x[1] & !0xffff_ffff) | u64::from((f32::from_bits((x[1] as u32)) * f32::from_bits(l[28])).to_bits());
            x[4] = (x[4] & !0xffff_ffff) | u64::from((f32::from_bits((x[4] as u32)) * f32::from_bits(l[40])).to_bits());
            l[76] = x[0] as u32;
            x[0] = u64::from(rd(m, 0x01406ce0));
            l[105] = x[7] as u32;
            l[64] = x[2] as u32;
            l[67] = x[5] as u32;
            l[69] = x[6] as u32;
            l[74] = x[3] as u32;
            l[77] = x[1] as u32;
            l[78] = x[4] as u32;
            l[79] = x[0] as u32;
        }
        for k in 0..28u32 {
            let word = l[(CONTRIBUTION / 4 + k) as usize];
            pg.set_u32(m, block.wrapping_add(0x20 + 4 * k), word);
        }
    }
    let flags_word = slots.field(m, 0x5c);
    slots.set_field(m, 0x5c, flags_word & !2);
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4 + 0x10);
}

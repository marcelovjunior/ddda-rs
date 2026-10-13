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

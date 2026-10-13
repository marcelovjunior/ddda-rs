//! L172 P2c: ports of the per-unit model draw path (the work the shadow
//! job's units run). Same ABI-level conventions as `semantic.rs`.
use crate::semantic::call_from;
use crate::semantic_s10::{x87_copy_f32, StateSlots};
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu},
};

fn guest(target: u32) -> impl FnOnce(&mut dyn Mem, &mut Cpu) {
    move |m, c| x86::call_ext(m, c, target)
}

// Translated from 0x00ea3130 (DDDA.exe 2017-12-13 Steam), uBaseModel::vf11.
/// 0x00EA3130, `model->draw_unit(context)` (thiscall, `ret 4`; void): draw one
/// model unit into a render context.
///
/// Nothing happens without a mesh (`this[0xf0]`) or when the model's
/// intensity (`this[0x158]`) equals the constant at 0x01561738. Otherwise:
/// 1. the visibility mask (0x00EA1EE0, folded in): `this[0x108] & 0x3f`
///    zero means all (0x3f); else 0x00FF0780 / 0x00FF0AD0 on the context
///    narrow it, and a negative result ends the draw;
/// 2. the model's bounds (0x00EA1F40, folded in): the position floats at
///    +0x160..+0x168 (w = 0), replaced by the virtual +0x30 result of the
///    last link of the parent chain (+0x30) when it reports success;
/// 3. virtual +0x64(context) prepares, 0x00EA1FD0 chooses the shadow state
///    (a guest call), and virtual +0x68(context, mesh, +0xf8, bounds, mask,
///    state) draws;
/// 4. the context's state slot 0x2e08 is bound to the device constant at
///    +0x2c10 (undo-logged); a resource at 0x1d90 is cleared; and for
///    models with bit 30 of `this[0x108]` the slot 0x2eb0 is bound to
///    +0x2cc8 and a fresh constants block (descriptor from the context's
///    layout +0x89c, copied, third float cleared) becomes the context's
///    +0x1330 block.
pub fn base_model_draw_unit(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let context = pg.u32(m, entry.wrapping_add(4));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // The original's frame: EBP = entry - 4, ESP aligned to 16, 0x14 locals,
    // EBX, ESI, EDI pushed.
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x14).wrapping_sub(12);
    let finish = |c: &mut Cpu| {
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(8);
    };
    c.esi = context;
    c.edi = this;
    if pg.u32(m, this.wrapping_add(0xf0)) == 0 {
        return finish(c);
    }
    let intensity = f32::from_bits(pg.u32(m, this.wrapping_add(0x158)));
    if intensity == f32::from_bits(pg.u32(m, 0x0156_1738)) {
        return finish(c);
    }
    let mask = visibility_mask(m, c, pg, frame, this, context);
    if mask < 0 {
        return finish(c);
    }
    let bounds = frame.wrapping_add(0x10);
    model_bounds(m, c, pg, frame, this, bounds);
    let virtual_call = |m: &mut dyn Mem, c: &mut Cpu, slot: u32, args: &[u32], resume: u32| {
        let method = pg.u32(m, pg.u32(m, this).wrapping_add(slot));
        call_from(m, c, pg, frame, this, args, resume, guest(method))
    };
    virtual_call(m, c, 0x64, &[context], 0x00ea_318d);
    let state = call_from(m, c, pg, frame, this, &[context], 0x00ea_3195, guest(0x00ea_1fd0));
    let mesh_bank = pg.u32(m, this.wrapping_add(0xf8));
    let mesh = pg.u32(m, this.wrapping_add(0xf0));
    virtual_call(m, c, 0x68, &[context, mesh, mesh_bank, bounds, mask as u32, state], 0x00ea_31b4);

    let slots = StateSlots { pg, model: context };
    slots.set_value(m, 0x2e08, 0x2c10, 0x582, 0x582);
    slots.set_resource(m, 0x1d90, 0, 0x373, 0);
    if (pg.u32(m, this.wrapping_add(0x108)) >> 0x1e) & 1 != 0 {
        slots.set_value(m, 0x2eb0, 0x2cc8, 0x597, 0x950a_6599);
        fresh_parameter_block(m, c, pg, frame, &slots);
    }
    finish(c);
}

/// 0x00EA1EE0 folded: the model's visibility mask for a context, or a
/// negative value for "not visible". `frame` is the caller's ESP.
fn visibility_mask(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, this: u32, context: u32) -> i32 {
    if pg.u8(m, this.wrapping_add(0x108)) & 0x3f == 0 {
        return 0x3f;
    }
    // 0x00EA1EE0's frame: its entry is the caller's frame less the argument
    // and return address; ESI and EDI are pushed
    let inner = frame.wrapping_sub(16);
    let mut mask = call_from(m, c, pg, inner, context, &[this.wrapping_add(0x160)], 0x00ea_1f08, guest(0x00ff_0780)) as i32;
    if mask > 0 {
        if mask != 0x3f {
            mask = call_from(m, c, pg, inner, context, &[this.wrapping_add(0x170), mask as u32], 0x00ea_1f20, guest(0x00ff_0ad0)) as i32;
        }
        if mask > 0 && pg.u8(m, this.wrapping_add(0x108)) & 0x3f == 1 {
            mask = 0x3f;
        }
    }
    mask
}

/// 0x00EA1F40 folded: write the model's 4-float position record to `out`.
fn model_bounds(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, this: u32, out: u32) {
    for (k, offset) in [0x160u32, 0x164, 0x168].into_iter().enumerate() {
        let bits = x87_copy_f32(pg.u32(m, this.wrapping_add(offset)));
        pg.set_u32(m, out.wrapping_add(4 * k as u32), bits);
    }
    pg.set_u32(m, out.wrapping_add(12), 0);
    let mut link = pg.u32(m, this.wrapping_add(0x30));
    if link == 0 {
        return;
    }
    loop {
        let next = pg.u32(m, link.wrapping_add(0x30));
        if next == 0 {
            break;
        }
        link = next;
    }
    // 0x00EA1F40's own frame: its entry is the caller's frame less the
    // argument and return address; EBP, the 16-byte alignment, 0x1c locals
    // and ESI follow
    let entry = frame.wrapping_sub(8);
    let inner = (entry.wrapping_sub(4) & !15).wrapping_sub(0x1c).wrapping_sub(4);
    let local = inner.wrapping_add(0x10);
    let method = pg.u32(m, pg.u32(m, link).wrapping_add(0x30));
    call_from(m, c, pg, inner, link, &[local], 0x00ea_1f8d, guest(method));
    if c.eax & 0xff != 0 {
        for k in 0..3u32 {
            let bits = pg.u32(m, local.wrapping_add(4 * k));
            pg.set_u32(m, out.wrapping_add(4 * k), bits);
        }
        pg.set_u32(m, out.wrapping_add(12), 0);
    }
}

/// The context's fresh parameter block (+0x1330): a copy of the layout's
/// descriptor (+0x89c) block with its third float cleared.
fn fresh_parameter_block(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, context: &StateSlots) {
    let flags = context.field(m, 0x5c);
    context.set_field(m, 0x5c, flags | 2);
    context.log_old(m, 0x1330, 0x227);
    let descriptor = pg.u32(m, context.field(m, 0x18).wrapping_add(0x89c));
    let size = (u32::from(pg.u16(m, descriptor.wrapping_add(0x18))).wrapping_mul(4).wrapping_add(0xf)) & !0xf;
    if context.field(m, 0x24).wrapping_add(size) > context.field(m, 0x28) {
        call_from(m, c, pg, frame, context.model, &[size], 0x00ea_342f, guest(0x00fe_d890));
    }
    let block = context.field(m, 0x24);
    context.set_field(m, 0x24, block.wrapping_add(size));
    context.set_field(m, 0x1330, block);
    context.mark_dirty(m, 1);
    if block != 0 {
        let words = u32::from(pg.u16(m, descriptor.wrapping_add(0x18)));
        let source = pg.u32(m, descriptor.wrapping_add(0x24));
        crate::semantic_s10::copy_words(m, pg, block, source, words);
    }
    pg.set_u32(m, block.wrapping_add(8), 0);
    let flags = context.field(m, 0x5c);
    context.set_field(m, 0x5c, flags & !2);
}

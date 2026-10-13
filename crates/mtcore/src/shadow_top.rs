//! Semantic ports of the shadow job's top levels (L172 P2d): the pass setup
//! 0x00EFD240, the cascade loop 0x00EFDC70 (also reached through the
//! `uStableShadow::vf24` thunk 0x00EFEF80), the shadow vf11 0x00EE2C90 and the
//! per-unit job wrapper 0x00DB7620. They keep the ABI-visible result of the
//! originals (callee-saved registers, ESP and every guest memory write above
//! the returned stack, in particular the draw context and the arguments the
//! callees read) and make each guest call the original makes, through
//! [`call_guest`] (a registered native port is entered directly).
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{self, Cpu};

/// A call from a native port to the guest function at `target`: straight
/// into the native port registered as an override for it, else `call_ext`.
#[inline(always)]
pub fn call_guest(m: &mut dyn Mem, c: &mut Cpu, target: u32) {
    if x86::may_be_overridden(target) {
        return call_overridden(m, c, target);
    }
    x86::call_ext(m, c, target)
}

#[inline(never)]
fn call_overridden(m: &mut dyn Mem, c: &mut Cpu, target: u32) {
    match x86::override_of(target) {
        Some(f) => f(m, c),
        None => x86::call_ext(m, c, target),
    }
}

/// A guest call from the original's frame `frame` (its ESP at the call
/// sites): push `args` (last first) and `back`, set ECX and call `target`.
/// Returns EAX; ESP is `frame` again after the callee's own `ret n`.
fn guest_call(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, this: u32, args: &[u32], back: u32, target: u32) -> u32 {
    c.esp = frame;
    for &v in args.iter().rev() {
        c.esp = c.esp.wrapping_sub(4);
        pg.set_u32(m, c.esp, v);
    }
    c.esp = c.esp.wrapping_sub(4);
    pg.set_u32(m, c.esp, back);
    c.ecx = this;
    call_guest(m, c, target);
    c.eax
}

/// The draw context the shadow pass writes into (the same structure as the
/// material code's): a word at `offset`, the dirty bits at +0x154 and the
/// undo journal.
struct DrawContext {
    pg: Pages,
    ctx: u32,
}

impl DrawContext {
    fn word(&self, m: &dyn Mem, offset: u32) -> u32 {
        self.pg.u32(m, self.ctx.wrapping_add(offset))
    }

    fn set_word(&self, m: &mut dyn Mem, offset: u32, v: u32) {
        self.pg.set_u32(m, self.ctx.wrapping_add(offset), v);
    }

    /// `ctx[0x154] |= bits`.
    fn mark_dirty(&self, m: &mut dyn Mem, bits: u32) {
        let v = self.word(m, 0x154) | bits;
        self.set_word(m, 0x154, v);
    }

    /// When recording (`ctx[0x5c] & 0x80`), pushes the (value, tag) pair of
    /// the binding at `slot` onto the undo journal (`ctx + 0x30C7C`, 12-byte
    /// entries, count at +0x6C) under `code`.
    fn journal(&self, m: &mut dyn Mem, code: u32, slot: u32) {
        if self.pg.u8(m, self.ctx.wrapping_add(0x5c)) & 0x80 == 0 {
            return;
        }
        let count = self.word(m, 0x6c);
        let entry = 0x30c7c_u32.wrapping_add(count.wrapping_mul(12));
        self.set_word(m, entry, code);
        let (value, tag) = (self.word(m, slot), self.word(m, slot + 4));
        self.set_word(m, entry + 4, value);
        self.set_word(m, entry + 8, tag);
        let count = self.word(m, 0x6c);
        self.set_word(m, 0x6c, count.wrapping_add(1));
    }

    /// Takes room for the constant block `[[ctx + 0x18] + descriptor]` from
    /// the context's bump allocator (`ctx[0x24]`, limit `ctx[0x28]`, grown
    /// by 0x00FED890 `ctx->grow(bytes)`), stores it in `ctx[destination]`
    /// and marks dirty bit 1. Returns the block.
    fn reserve(&self, m: &mut dyn Mem, c: &mut Cpu, frame: u32, descriptor: u32, destination: u32, grow_back: u32) -> u32 {
        let pg = self.pg;
        let block = pg.u32(m, self.word(m, 0x18).wrapping_add(descriptor));
        let bytes = ((pg.u16(m, block.wrapping_add(0x18)) as u32) * 4 + 0xf) & 0xffff_fff0;
        if self.word(m, 0x24).wrapping_add(bytes) > self.word(m, 0x28) {
            guest_call(m, c, pg, frame, self.ctx, &[bytes], grow_back, 0x00fe_d890);
        }
        let at = self.word(m, 0x24);
        self.set_word(m, 0x24, at.wrapping_add(bytes));
        self.set_word(m, destination, at);
        self.mark_dirty(m, 1);
        at
    }
}

/// `fld dword` / `fstp dword` round trip of a float's bits: identical
/// except that a signalling NaN comes back quiet.
fn through_x87(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

// Translated from 0x00efd240 (DDDA.exe 2017-12-13 Steam).
/// 0x00EFD240, `shadow->setup_pass(ctx, cascade, index)` (thiscall, `ret 0xC`):
/// sets up one shadow-map pass in the draw context `ctx` and draws it.
/// `cascade` is the per-pass record built by the cascade loop (the two
/// matrices at +0x10 / +0x50, the viewport block at +0x90, ...).
///
/// In order: 0x00FF1090 `ctx->set_pass(0, 0)`; the pass bits `ctx[0x164]`;
/// 0x00FED8F0 `ctx->saved_state()` (a 3-float eye position, kept for later);
/// the shadow's virtual method 0x74 `(ctx, cascade)`; the bump-allocated
/// block 0x914 (3 floats of `this[0xB4/0xB8/0xC4]`, journalled as 0x245);
/// the depth shader binding at `ctx + 0x3848` (key 0xED98E6CB, or
/// 0xA97086CC when `this[0xB4] <= 0.0`, journalled as 0x6CA); methods 0x64
/// and 0x00EDE880 (the pass draw); the texture stamps (`[0x018D2CE8] + 1`)
/// of the three textures `this[0xDC / 0xE0 / 0xE4]` and the block 0x910
/// holding them (journalled as 0x244); methods 0x70 and the global's 0x40;
/// for pass types other than 4/1 the extra 0x00EEA130 and method 0x68; and
/// 0x00FEDC00 `ctx->restore_recording_state()`.
pub fn setup_pass(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let ctx = pg.u32(m, entry.wrapping_add(4));
    let cascade = pg.u32(m, entry.wrapping_add(8));
    let index = pg.u32(m, entry.wrapping_add(12));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // the original's frame after its prologue and three pushes
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x14 + 12);
    (c.ebx, c.ebp, c.esi, c.edi) = (cascade, entry.wrapping_sub(4), ctx, this);
    let dc = DrawContext { pg, ctx };
    let method = |m: &dyn Mem, slot: u32| pg.u32(m, pg.u32(m, this).wrapping_add(slot));

    guest_call(m, c, pg, frame, ctx, &[0, 0], 0x00ef_d25c, 0x00ff_1090);
    let bits = (dc.word(m, 0x164) & 0x1f) | 0x20;
    dc.set_word(m, 0x164, bits);
    let state = guest_call(m, c, pg, frame, ctx, &[], 0x00ef_d275, 0x00fe_d8f0);
    let eye = [pg.u32(m, state), pg.u32(m, state.wrapping_add(4)), pg.u32(m, state.wrapping_add(8))];
    let target = method(m, 0x74);
    guest_call(m, c, pg, frame, this, &[ctx, cascade], 0x00ef_d2a3, target);

    // the shadow block: three floats of the shadow
    let v = dc.word(m, 0x5c) | 2;
    dc.set_word(m, 0x5c, v);
    dc.journal(m, 0x245, 0x1420);
    let block = dc.reserve(m, c, frame, 0x914, 0x1420, 0x00ef_d338);
    if block != 0 {
        for (k, offset) in [0xb4u32, 0xb8, 0xc4].into_iter().enumerate() {
            let bits = through_x87(pg.u32(m, this.wrapping_add(offset)));
            pg.set_u32(m, block.wrapping_add(4 * k as u32), bits);
        }
    }
    let v = dc.word(m, 0x5c) & !2;
    dc.set_word(m, 0x5c, v);

    // the depth shader binding
    let key = if f32::from_bits(pg.u32(m, this.wrapping_add(0xb4))) > f32::from_bits(pg.u32(m, 0x0156_1738)) {
        0xed98_e6cb_u32
    } else {
        0xa970_86cc
    };
    let slot = key & 0xfff;
    if dc.word(m, 0x3848) != pg.u32(m, dc.word(m, 0x20).wrapping_add(slot * 8)) {
        dc.journal(m, 0x6ca, 0x3848);
        let shader = pg.u32(m, dc.word(m, 0x20).wrapping_add(slot * 8));
        dc.set_word(m, 0x3848, shader);
        dc.set_word(m, 0x384c, ((key ^ 0x4d0b_2000) & 0xffff_f000) | slot);
        dc.mark_dirty(m, 3);
    }

    let target = method(m, 0x64);
    guest_call(m, c, pg, frame, this, &[ctx], 0x00ef_d43b, target);
    let (a, b) = (pg.u32(m, cascade), pg.u32(m, cascade.wrapping_add(4)));
    let flag = pg.u8(m, cascade.wrapping_add(0xa0)) as u32;
    guest_call(
        m,
        c,
        pg,
        frame,
        this,
        &[ctx, a, b, cascade.wrapping_add(0x10), cascade.wrapping_add(0x50), cascade.wrapping_add(0x90), flag],
        0x00ef_d461,
        0x00ed_e880,
    );

    // the texture stamps and the texture block
    for offset in [0xdcu32, 0xe0, 0xe4] {
        let stamp = pg.u32(m, 0x018d_2ce8).wrapping_add(1);
        let texture = pg.u32(m, this.wrapping_add(offset));
        pg.set_u32(m, texture.wrapping_add(4), stamp);
    }
    let v = dc.word(m, 0x5c) | 2;
    dc.set_word(m, 0x5c, v);
    dc.journal(m, 0x244, 0x1418);
    let block = dc.reserve(m, c, frame, 0x910, 0x1418, 0x00ef_d524);
    if block != 0 {
        for (k, offset) in [0xdcu32, 0xe0, 0xe4].into_iter().enumerate() {
            let v = pg.u32(m, this.wrapping_add(offset));
            pg.set_u32(m, block.wrapping_add(4 * k as u32), v);
        }
    }
    let v = dc.word(m, 0x5c) & !2;
    dc.set_word(m, 0x5c, v);

    let target = method(m, 0x70);
    let argument = pg.u32(m, cascade.wrapping_add(0xa4));
    guest_call(m, c, pg, frame, this, &[ctx, argument], 0x00ef_d570, target);
    if pg.u32(m, cascade.wrapping_add(0xa8)) != 1 {
        for (k, v) in eye.into_iter().enumerate() {
            dc.set_word(m, 0x184 + 4 * k as u32, v);
        }
    }
    let base = pg.u32(m, this.wrapping_add(0x150)).wrapping_add(index);
    dc.set_word(m, 0x180, base);
    let global = pg.u32(m, 0x018d_09e0);
    let target = pg.u32(m, pg.u32(m, global).wrapping_add(0x40));
    guest_call(m, c, pg, frame, global, &[ctx, 0], 0x00ef_d5c2, target);
    dc.set_word(m, 0x180, 0);
    if pg.u32(m, this.wrapping_add(0x64)) != 4 && pg.u32(m, this.wrapping_add(0x68)) == 1 && pg.u32(m, cascade.wrapping_add(0xa8)) != 1 {
        let a = pg.u32(m, cascade);
        guest_call(m, c, pg, frame, this, &[ctx, a, cascade.wrapping_add(0x90)], 0x00ef_d5f6, 0x00ee_a130);
        let (a, b) = (pg.u32(m, cascade), pg.u32(m, this.wrapping_add(0x34)));
        let target = method(m, 0x68);
        guest_call(m, c, pg, frame, this, &[ctx, a, b], 0x00ef_d607, target);
    }
    guest_call(m, c, pg, frame, ctx, &[], 0x00ef_d60e, 0x00fe_dc00);
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(16);
}

// Translated from 0x00efdc70 (DDDA.exe 2017-12-13 Steam), uStableShadow::vf24
// (0x00EFEF80 is a `jmp` to it).
/// 0x00EFDC70, `shadow->draw_cascades(ctx)` (thiscall, `ret 4`): draws the
/// stable shadow's cascades in order. Does nothing without a light
/// (`this[0x30]`) or when `this[0x7B]` is clear. The cascade count is 1, or
/// `this[0x174]` when `this[0x170]` is set. For each cascade `k` it asks
/// 0x00EE5CE0 `this->build_cascade(&view, &projection, &extra, ctx, k,
/// count)` (its x87 result is dropped) for the cascade's two 4x4 matrices,
/// copies them into the 0x1C0-byte pass record on its stack together with
/// the light, the per-cascade range `[k * n, (k + 1) * n)` of the `n =
/// this[0x74]` units and a viewport block (zeroed once), and runs 0x00EFD240
/// [`setup_pass`] `(ctx, &record, k)`. Then 0x00EEA950 `this->finish(ctx, 1)`.
pub fn draw_cascades(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let ctx = pg.u32(m, entry.wrapping_add(4));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let finish = |c: &mut Cpu| {
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(8);
    };
    let light = pg.u32(m, this.wrapping_add(0x30));
    if light == 0 || pg.u8(m, this.wrapping_add(0x7b)) == 0 {
        return finish(c);
    }
    // the original's frame after its prologue and three pushes
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x1b4 + 12);
    (c.ebp, c.edi) = (entry.wrapping_sub(4), this);
    let at = |offset: u32| frame.wrapping_add(offset);
    let count = if pg.u8(m, this.wrapping_add(0x170)) == 0 { 1 } else { pg.u32(m, this.wrapping_add(0x174)) };
    // the pass record at frame + 0x90, with its fixed parts
    let record = at(0x90);
    pg.set_u32(m, at(0x120), 0);
    pg.set_u32(m, at(0x124), 0);
    pg.set_u32(m, at(0x128), 0);
    pg.set_u32(m, at(0x12c), 0);
    pg.set_u32(m, record, light);
    let tail = pg.u32(m, this.wrapping_add(0x3c));
    pg.set_u32(m, at(0x94), tail);
    pg.set_u8(m, at(0x130), 1);
    pg.set_u32(m, at(0x138), 0);
    for offset in [0x14cu32, 0x15c, 0x16c, 0x17c, 0x18c, 0x19c, 0x1ac, 0x1bc] {
        pg.set_u32(m, at(offset), 0);
    }
    for k in 0..count {
        let (view, projection, extra) = (at(0x50), at(0x10), at(0x140));
        guest_call(m, c, pg, frame, this, &[view, projection, extra, ctx, k, count], 0x00ef_dd69, 0x00ee_5ce0);
        c.st.register::<0xd8dd>(); // fstp st(0): the result is dropped
        for i in 0..16u32 {
            let v = pg.u32(m, at(0x50 + 4 * i));
            pg.set_u32(m, at(0xa0 + 4 * i), v);
        }
        for i in 0..16u32 {
            let v = pg.u32(m, at(0x10 + 4 * i));
            pg.set_u32(m, at(0xe0 + 4 * i), v);
        }
        let n = pg.u32(m, this.wrapping_add(0x74));
        pg.set_u32(m, at(0x124), n.wrapping_mul(k));
        pg.set_u32(m, at(0x128), n);
        pg.set_u32(m, at(0x12c), k.wrapping_add(1).wrapping_mul(n));
        pg.set_u32(m, at(0x120), 0);
        pg.set_u32(m, at(0x134), extra);
        guest_call(m, c, pg, frame, this, &[ctx, record, k], 0x00ef_dfa9, 0x00ef_d240);
    }
    guest_call(m, c, pg, frame, this, &[ctx, 1], 0x00ef_dfc1, 0x00ee_a950);
    finish(c)
}

// Translated from 0x00ee2c90 (DDDA.exe 2017-12-13 Steam), uShadow::vf11.
/// 0x00EE2C90, `shadow->draw(ctx)` (thiscall, `ret 4`): for a draw context
/// whose pass flags (`ctx[0x16A] & 1`) ask for shadows, and a shadow that is
/// not disabled (`this[0x78] == 0`), runs 0x00EDE9C0 `this->prepare(ctx)`
/// and then the shadow's virtual method 0x60 `(ctx)`.
pub fn draw_shadow(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let ctx = pg.u32(m, entry.wrapping_add(4));
    let saved = (c.esi, c.edi);
    // the original's frame after its two pushes
    let frame = entry.wrapping_sub(8);
    (c.esi, c.edi) = (this, ctx);
    if pg.u8(m, ctx.wrapping_add(0x16a)) & 1 != 0 && pg.u8(m, this.wrapping_add(0x78)) == 0 {
        guest_call(m, c, pg, frame, this, &[ctx], 0x00ee_2cad, 0x00ed_e9c0);
        let target = pg.u32(m, pg.u32(m, this).wrapping_add(0x60));
        guest_call(m, c, pg, frame, this, &[ctx], 0x00ee_2cb7, target);
    }
    (c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(8);
}

// Translated from 0x00db7620 (DDDA.exe 2017-12-13 Steam).
/// 0x00DB7620, `unit->run_job(arg)` (thiscall, `ret 4`): the per-unit job the
/// shadow traversal queues. When 0x00DA7DB0 `scheduler->nested()` (ECX =
/// `[0x018D08F4]`) says no, it runs the unit's virtual method 0x2C `(arg)`;
/// otherwise it tail-jumps to 0x00BFC7D0 with ECX = the scheduler, leaving
/// the stack (return address and `arg`) to that function.
pub fn run_unit_job(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let scheduler = pg.u32(m, 0x018d_08f4);
    let saved = c.esi;
    c.esi = this;
    // the original's frame after its push of ESI
    let frame = entry.wrapping_sub(4);
    let nested = guest_call(m, c, pg, frame, scheduler, &[], 0x00db_762e, 0x00da_7db0) & 0xff != 0;
    if !nested {
        let arg = pg.u32(m, entry.wrapping_add(4));
        let target = pg.u32(m, pg.u32(m, this).wrapping_add(0x2c));
        guest_call(m, c, pg, frame, this, &[arg], 0x00db_7640, target);
        c.esi = saved;
        c.esp = entry.wrapping_add(8);
    } else {
        // jmp: the stack is as the caller left it
        c.esi = saved;
        c.esp = entry;
        c.ecx = pg.u32(m, 0x018d_08f4);
        call_guest(m, c, 0x00bf_c7d0);
    }
}

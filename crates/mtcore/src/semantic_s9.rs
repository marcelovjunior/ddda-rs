//! L184 semantic ports (s9): the render-state stack pop (0x00FED5D0) and
//! the per-mesh model draw (0x00EA5B50). See crate::semantic for the
//! ABI-level contract.
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

/// Copy `len` bytes from `src` to `dst` like the CRT memcpy, which behaves
/// as memmove (overlap-safe): runs inside a page are moved in bulk, the
/// rest byte by byte through the memory trait. When the destination starts
/// inside the source the copy runs back to front.
pub fn copy_guest(m: &mut dyn Mem, pg: Pages, dst: u32, src: u32, len: u32) {
    let backward = dst.wrapping_sub(src) < len;
    let mut done = 0u32;
    while done < len {
        // the next piece: [offset, offset + chunk) of the buffers
        let left = len - done;
        let (offset, chunk) = if backward {
            let end = left;
            // bytes from the previous page boundary up to the piece's end
            let since_boundary = |a: u32| match a.wrapping_add(end) & 0xfff {
                0 => 0x1000,
                n => n,
            };
            let chunk = end.min(since_boundary(src)).min(since_boundary(dst));
            (end - chunk, chunk)
        } else {
            let room = |a: u32| 0x1000 - (a.wrapping_add(done) & 0xfff);
            (done, left.min(room(src)).min(room(dst)))
        };
        let (from, to) = (src.wrapping_add(offset), dst.wrapping_add(offset));
        if let (Some(f), Some(t)) = (pg.bytes(from, chunk as usize, 1), pg.bytes(to, chunk as usize, 2)) {
            // SAFETY: `bytes` returned `chunk` bytes inside backed pages.
            unsafe { std::ptr::copy(f, t, chunk as usize) };
        } else if backward {
            for k in (0..chunk).rev() {
                let b = pg.u8(m, from.wrapping_add(k));
                pg.set_u8(m, to.wrapping_add(k), b);
            }
        } else {
            for k in 0..chunk {
                let b = pg.u8(m, from.wrapping_add(k));
                pg.set_u8(m, to.wrapping_add(k), b);
            }
        }
        done += chunk;
    }
}

/// 0x00FED5D0, `render_state->pop(full)` (thiscall, `ret 4`): pop the top
/// saved render state off the state stack and make it current.
///
/// The stack lives inside the render-state object: `depth` at +0x30874,
/// saved states of 0x8144 bytes from +0x81F8. The current state is the
/// 0x144-byte block at +0xB4 (plus 8 bytes per `slot_count` at +0x14 when
/// `full` is set, which restores everything with one memcpy and returns
/// its destination, +0xB4). Without `full` only the base block is
/// restored, and six saved words (entry +0x12C..+0x140) are written back
/// into the slot table at +0x1F8, 8 bytes per slot, at the slot numbers
/// currently held in the six globals 0x01447B44..0x01447B58 (low 12 bits).
pub fn pop_render_state(m: &mut dyn Mem, c: &mut Cpu) {
    const STATE_SIZE: u32 = 0x8144;
    const BASE_BLOCK: u32 = 0x144;
    const SLOT_GLOBALS: u32 = 0x0144_7b44;
    let pg = Pages::of(m);
    let entry = c.esp;
    let full = pg.u8(m, entry.wrapping_add(4)) != 0;
    let this = c.ecx;
    c.esp = entry.wrapping_add(8);
    let depth = pg.u32(m, this.wrapping_add(0x30874)).wrapping_sub(1);
    pg.set_u32(m, this.wrapping_add(0x30874), depth);
    let saved = this
        .wrapping_add(depth.wrapping_mul(STATE_SIZE))
        .wrapping_add(0x81f8);
    let current = this.wrapping_add(0xb4);
    if full {
        let slot_count = pg.u32(m, this.wrapping_add(0x14));
        copy_guest(
            m,
            pg,
            current,
            saved,
            slot_count.wrapping_mul(8).wrapping_add(BASE_BLOCK),
        );
        c.eax = current;
        return;
    }
    copy_guest(m, pg, current, saved, BASE_BLOCK);
    for i in 0..6u32 {
        let slot = pg.u32(m, SLOT_GLOBALS + 4 * i) & 0xfff;
        let word = pg.u32(m, saved.wrapping_add(0x12c + 4 * i));
        pg.set_u32(m, this.wrapping_add(0x1f8 + slot * 8), word);
    }
    c.eax = this;
}

/// Offsets of the draw context (`ctx`) fields 0x00EA5B50 works with.
mod ctx {
    /// Dirty flags the next primitive flush acts on (bit 0: a slot moved,
    /// bit 1: a slot's tag moved).
    pub const DIRTY: u32 = 0x154;
    /// Mode flags: bit 7 turns the undo log on, bit 1 is set while a
    /// per-model constant block is being filled.
    pub const MODE: u32 = 0x5c;
    /// Number of undo-log entries.
    pub const LOG_COUNT: u32 = 0x6c;
    /// The undo log: 12-byte entries (code, old value, old tag).
    pub const LOG: u32 = 0x30c7c;
    /// The context's default state table (read-only to this function).
    pub const DEFAULTS: u32 = 0x20;
    /// The context's constant-block arena cursor and limit.
    pub const ARENA_CURSOR: u32 = 0x24;
    pub const ARENA_LIMIT: u32 = 0x28;
}

/// The frame counter the resources' use stamps are taken from.
const TICK: u32 = 0x018d_2ce8;
/// The global listener object (virtual method 0x34 = model bounds report).
const LISTENER: u32 = 0x018d_2c80;

struct Upload<'a> {
    m: &'a mut dyn Mem,
    c: &'a mut Cpu,
    pg: Pages,
    /// The original's ESP at its call sites (arguments are pushed below).
    frame: u32,
    ctx: u32,
    model: u32,
}

impl Upload<'_> {
    fn get(&self, offset: u32) -> u32 {
        self.pg.u32(self.m, self.ctx.wrapping_add(offset))
    }
    fn set(&mut self, offset: u32, v: u32) {
        self.pg.set_u32(self.m, self.ctx.wrapping_add(offset), v);
    }
    fn model(&self, offset: u32) -> u32 {
        self.pg.u32(self.m, self.model.wrapping_add(offset))
    }
    fn word(&self, address: u32) -> u32 {
        self.pg.u32(self.m, address)
    }
    fn defaults(&self, offset: u32) -> u32 {
        self.word(self.get(ctx::DEFAULTS).wrapping_add(offset))
    }
    fn add_dirty(&mut self, bits: u32) {
        let v = self.get(ctx::DIRTY) | bits;
        self.set(ctx::DIRTY, v);
    }
    fn logging(&self) -> bool {
        self.pg.u8(self.m, self.ctx.wrapping_add(ctx::MODE)) & 0x80 != 0
    }
    /// Append `code` and the slot's current value and tag to the undo log
    /// (when logging is on).
    fn log(&mut self, code: u32, slot: u32) {
        if !self.logging() {
            return;
        }
        let count = self.get(ctx::LOG_COUNT);
        let entry = ctx::LOG.wrapping_add(count.wrapping_mul(12));
        self.set(entry, code);
        let (value, tag) = (self.get(slot), self.get(slot + 4));
        self.set(entry + 4, value);
        self.set(entry + 8, tag);
        self.set(ctx::LOG_COUNT, count.wrapping_add(1));
    }
    /// A plain state slot (`slot`: value, `slot + 4`: tag): when the
    /// context's value differs from `new`, log it, store `new` with `tag`
    /// and mark everything dirty (bits 0 and 1).
    fn set_state(&mut self, slot: u32, new: u32, tag: u32, code: u32) {
        if self.get(slot) == new {
            return;
        }
        self.log(code, slot);
        self.set(slot, new);
        self.set(slot + 4, tag);
        self.add_dirty(3);
    }
    /// A resource slot: `obj` (0 for none) is bound into the slot with the
    /// tag `obj.tag | tag_bits`; binding touches the resource's use stamp
    /// (`obj + 4`, the frame tick + 1). Dirty bit 0 for a new binding, bit
    /// 1 as well when the tag changed.
    fn bind_resource(&mut self, slot: u32, obj: u32, tag_bits: u32, code: u32) {
        if self.get(slot) == obj {
            return;
        }
        self.log(code, slot);
        let tag = if obj != 0 {
            let stamp = self.word(TICK).wrapping_add(1);
            self.pg.set_u32(self.m, obj.wrapping_add(4), stamp);
            self.set(slot, obj);
            self.word(obj.wrapping_add(0xc)) | tag_bits
        } else {
            self.set(slot, 0);
            0
        };
        if self.get(slot + 4) != tag {
            self.set(slot + 4, tag);
            self.add_dirty(2);
        }
        self.add_dirty(1);
    }
    fn call(&mut self, target: u32, this: u32, args: &[u32], back: u32) -> u32 {
        let (pg, frame) = (self.pg, self.frame);
        crate::semantic::call_from(
            self.m,
            self.c,
            pg,
            frame,
            this,
            args,
            back,
            move |m: &mut dyn Mem, c: &mut Cpu| ddnative_base::x86::call_ext(m, c, target),
        )
    }
    /// Take `size` bytes from the context's constant arena, growing it
    /// (0x00FED890) when full.
    fn arena_alloc(&mut self, size: u32, back: u32) -> u32 {
        if self.get(ctx::ARENA_CURSOR).wrapping_add(size) > self.get(ctx::ARENA_LIMIT) {
            let this = self.ctx;
            self.call(0x00fe_d890, this, &[size], back);
        }
        let block = self.get(ctx::ARENA_CURSOR);
        self.set(ctx::ARENA_CURSOR, block.wrapping_add(size));
        block
    }
    /// The descriptor `[[ctx + 0x18] + offset]` and the 16-byte aligned
    /// size of its `u16` count (x4) block.
    fn descriptor(&self, offset: u32) -> (u32, u32) {
        let d = self.word(self.word(self.ctx.wrapping_add(0x18)).wrapping_add(offset));
        let size = ((self.pg.u16(self.m, d.wrapping_add(0x18)) as u32) * 4 + 0xf) & !0xf;
        (d, size)
    }
    /// Fill a freshly allocated constant block with the descriptor's data;
    /// `block + 8` is then set to `w8` (also when the arena gave no block,
    /// as the original does).
    fn fill_block(&mut self, block: u32, d: u32, w8: u32) {
        if block != 0 {
            let bytes = (self.pg.u16(self.m, d.wrapping_add(0x18)) as u32) * 4;
            let source = self.word(d.wrapping_add(0x24));
            copy_guest(self.m, self.pg, block, source, bytes);
        }
        self.pg.set_u32(self.m, block.wrapping_add(8), w8);
    }
    fn mode_and(&mut self, mask: u32) {
        let v = self.get(ctx::MODE) & mask;
        self.set(ctx::MODE, v);
    }
    fn mode_or(&mut self, bits: u32) {
        let v = self.get(ctx::MODE) | bits;
        self.set(ctx::MODE, v);
    }

    /// The model's 4x4 matrix (rows at +0xB0..+0xF0) written transposed,
    /// three columns (12 floats), into a new constant block that becomes
    /// the context's world-matrix block (+0xCA8).
    fn upload_world_matrix(&mut self) {
        self.mode_or(2);
        self.log(0x156, 0xca8);
        let (_, size) = self.descriptor(0x558);
        let block = self.arena_alloc(size, 0x00ea_5d32);
        self.set(0xca8, block);
        self.add_dirty(1);
        if block != 0 {
            for column in 0..3u32 {
                for row in 0..4u32 {
                    let v = self.model(0xb0 + 0x10 * row + 4 * column);
                    self.pg
                        .set_u32(self.m, block.wrapping_add(16 * column + 4 * row), v);
                }
            }
        }
        self.mode_and(!2);
    }

    /// Report the model's 16-byte bounding vector to the listener
    /// (0x00FE66A0, then its virtual method 0x34) and hand the context the
    /// model's fade value (0x00FF1310).
    fn report_visibility(&mut self) {
        if self.pg.u16(self.m, self.ctx.wrapping_add(0x16a)) & 0x813 != 0 {
            let flags = self.model(0x104);
            let mut source = self.model;
            if flags & 2 != 0 {
                let parent = self.model(0x30);
                if parent != 0 {
                    source = parent;
                }
            }
            let vector = self.frame.wrapping_add(0x20);
            for k in 0..4u32 {
                let v = self.word(source.wrapping_add(0x1c0 + 4 * k));
                self.pg.set_u32(self.m, vector + 4 * k, v);
            }
            let kind = if flags & 4 != 0 { 7 } else { 5 };
            let (ctx, id) = (self.ctx, self.model(0x100));
            let listener = self.word(LISTENER);
            self.call(0x00fe_66a0, listener, &[ctx, id, kind, vector], 0x00ea_60d4);
            let listener = self.word(LISTENER);
            let method = self.word(self.word(listener).wrapping_add(0x34));
            let group = (self.model(0x108) >> 25) & 0xf;
            let id = self.model(0x100);
            let bounds = self.model.wrapping_add(0x160);
            self.call(method, listener, &[ctx, id, group, bounds], 0x00ea_60fd);
        }
        let (ctx, fade) = (self.ctx, self.model(0x158));
        self.call(0x00ff_1310, ctx, &[fade], 0x00ea_6112);
    }

    /// The per-model constant block in the arena (+0x1330), filled with the
    /// model's constants and word 2 cleared; and, when the model asks for a
    /// second pass (bit 30 of +0x108) that the pass mask allows, a second
    /// block with word 2 taken from a global, followed by a re-check of the
    /// two slots that pass depends on.
    fn upload_constant_blocks(&mut self) {
        self.mode_or(2);
        self.log(0x227, 0x1330);
        let (d, size) = self.descriptor(0x89c);
        let block = self.arena_alloc(size, 0x00ea_6468);
        self.set(0x1330, block);
        self.add_dirty(1);
        self.fill_block(block, d, 0);
        self.mode_and(!2);
        let second_pass = (self.model(0x108) >> 30) & 1 != 0
            && self.pg.u16(self.m, self.ctx.wrapping_add(0x16a)) & 0x208 == 0;
        if !second_pass {
            return;
        }
        self.mode_or(2);
        self.log(0x227, 0x1330);
        let (d, size) = self.descriptor(0x89c);
        let block = self.arena_alloc(size, 0x00ea_6566);
        self.set(0x1330, block);
        self.add_dirty(1);
        let constant = self.word(0x0140_6ce0);
        self.fill_block(block, d, constant);
        self.mode_and(!2);
        self.bind_global_state_h();
        self.bind_listener_resource();
    }

    fn bind_global_state_h(&mut self) {
        let new = self.defaults(0x2cc8);
        self.set_state(0x2eb0, new, 0x950a_6599, 0x597);
    }
    fn bind_listener_resource(&mut self) {
        let listener = self.word(LISTENER);
        let obj = self.word(listener.wrapping_add(0x2cc));
        self.bind_resource(0x1cf0, obj, 0x45fb_b000, 0x35f);
    }
    /// The bound resource `[ctx + 0x1d4]`'s member `offset`, or 0.
    fn bound_resource(&self, offset: u32) -> u32 {
        match self.get(0x1d4) {
            0 => 0,
            object => self.word(object.wrapping_add(offset)),
        }
    }
}

/// 0x00EA5B50, `model->upload_state(ctx)` (thiscall, `ret 4`, void): bring
/// a draw context's cached render state in line with one model before its
/// primitives are drawn.
///
/// The context caches state in slots (a value and a tag word each, e.g.
/// `+0x2868/+0x286C`); a slot is rewritten only when its value differs
/// from the wanted one, the old value is appended to the context's undo log
/// when logging is on (`+0x5C` bit 7), and the dirty flags (`+0x154`) tell
/// the next primitive flush what moved. This sets, in order: the model's
/// shader slot (one of two defaults, chosen by bit 31 of `model + 0x108`;
/// the second also uploads the world matrix block), the default state
/// slots, the two bound-resource slots, the visibility report to the
/// listener (0x018D2C80) and the fade, the global slots, and the per-model
/// constant blocks.
// Translated from 0x00EA5B50 (DDDA.exe 2017-12-13 Steam)
pub fn model_upload_state(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let ctx = pg.u32(m, entry.wrapping_add(4));
    let model = c.ecx;
    // The original's frame: push ebp; and esp, -16; 0x64 locals; 3 pushes.
    let frame = (entry.wrapping_sub(4) & !0xf).wrapping_sub(0x64 + 12);
    let mut u = Upload {
        m,
        c,
        pg,
        frame,
        ctx,
        model,
    };
    if u.model(0x108) & 0x8000_0000 != 0 {
        let new = u.defaults(0x2670);
        u.set_state(0x2868, new, 0x4ce, 0x4ce);
    } else {
        let new = u.defaults(0x2678);
        u.set_state(0x2868, new, 0xd3d3_74cf, 0x4ce);
        u.upload_world_matrix();
    }
    let new = u.defaults(0x2ea8);
    u.set_state(0x30a0, new, 0x5d5, 0x5d5);
    let obj = u.bound_resource(0x210);
    u.bind_resource(0x1c78, obj, 0xc496_e000, 0x350);
    u.report_visibility();
    let obj = u.bound_resource(0x264);
    u.bind_resource(0x1aa0, obj, 0x377b_6000, 0x315);
    u.bind_global_state_h();
    u.bind_listener_resource();
    u.upload_constant_blocks();
    let new = u.defaults(0x2ee0);
    u.set_state(0x30d8, new, 0x5dc, 0x5dc);
    let new = u.defaults(0x2050);
    u.set_state(0x2248, new, 0x40a, 0x40a);
    u.c.esp = entry.wrapping_add(8);
}

/// 0x00FF1310, `ctx->set_fade(value)` (thiscall, `ret 4`, void): set the
/// draw context's fade value (`+0x17C`, compared as a float: only a changed
/// value, or a NaN, counts as a change) and republish the fade block.
///
/// On a change the value is stored and a new constant block (the size of
/// the descriptor `[[ctx + 0x18] + 0x568]`'s `u16` count x4, 16-byte
/// aligned) is taken from the context's arena (`+0x24` cursor, `+0x28`
/// limit; a full arena is refilled from the page allocator 0x00DA7D70, or
/// left empty when that fails), the previous block (`+0xCC8`) is copied
/// into it, the value is written at `block + 0x10`, the block becomes the
/// current one and dirty bit 0 (`+0x154`) is set.
// Translated from 0x00FF1310 (DDDA.exe 2017-12-13 Steam)
pub fn set_fade(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let ctx = c.ecx;
    let value_bits = pg.u32(m, entry.wrapping_add(4));
    let field = |offset: u32| ctx.wrapping_add(offset);
    c.esp = entry.wrapping_add(8);
    if f32::from_bits(pg.u32(m, field(0x17c))) == f32::from_bits(value_bits) {
        return;
    }
    pg.set_u32(m, field(0x17c), value_bits);
    let descriptor = pg.u32(m, pg.u32(m, field(0x18)).wrapping_add(0x568));
    let words = pg.u16(m, descriptor.wrapping_add(0x18)) as u32;
    let size = (words * 4 + 0xf) & !0xf;
    if pg.u32(m, field(0x24)).wrapping_add(size) > pg.u32(m, field(0x28)) {
        let chunk = size.wrapping_add(0xfff) & !0xfff;
        // The original's frame at this call: entry - 16 (four saved registers).
        let page_allocator = pg.u32(m, 0x018d_08f4);
        let frame = entry.wrapping_sub(16);
        let block = crate::semantic::call_from(
            m,
            c,
            pg,
            frame,
            page_allocator,
            &[chunk],
            0x00ff_1372,
            |m: &mut dyn Mem, c: &mut Cpu| ddnative_base::x86::call_ext(m, c, 0x00da_7d70),
        );
        pg.set_u32(m, field(0x24), block);
        pg.set_u32(
            m,
            field(0x28),
            if block == 0 { 0 } else { block.wrapping_add(chunk) },
        );
        c.esp = entry.wrapping_add(8);
    }
    let block = pg.u32(m, field(0x24));
    pg.set_u32(m, field(0x24), block.wrapping_add(size));
    let previous = pg.u32(m, field(0xcc8));
    copy_guest(m, pg, block, previous, words * 4);
    // the original re-reads its argument after the copy
    let value_bits = pg.u32(m, entry.wrapping_add(4));
    pg.set_u32(m, field(0xcc8), block);
    let dirty = pg.u32(m, field(0x154)) | 1;
    pg.set_u32(m, field(0x154), dirty);
    pg.set_u32(m, block.wrapping_add(0x10), value_bits);
}

/// 0x00D13600, `sphere_contains(center, probe)` (cdecl, two pointers to
/// `[x, y, z, radius]`): true when the probe point is within the sum of the
/// two radii of the center (`(r1 + r2)^2 >= |probe - center|^2`; a NaN
/// anywhere makes it false). Folded natively into [`scene_collect_visible`].
fn sphere_contains(m: &dyn Mem, pg: Pages, center: u32, probe: u32) -> bool {
    let f = |a: u32, k: u32| f32::from_bits(pg.u32(m, a.wrapping_add(4 * k)));
    let dx = f(probe, 0) - f(center, 0);
    let dy = f(probe, 1) - f(center, 1);
    let dz = f(probe, 2) - f(center, 2);
    let reach = f(center, 3) + f(probe, 3);
    let reach2 = reach * reach;
    let distance2 = (dx * dx + dy * dy) + dz * dz;
    reach2 >= distance2
}

/// The scene collector's working state (0x00FEB1F0): the guest frame holds
/// the selection array and its count because the virtual callees fill them.
struct Collect<'a> {
    m: &'a mut dyn Mem,
    c: &'a mut Cpu,
    pg: Pages,
    frame: u32,
    scene: u32,
    ctx: u32,
    mask: u32,
    max: u32,
    probe: u32,
}

/// Offsets inside the collector's guest frame.
const COUNT: u32 = 0x14;
const SELECTED: u32 = 0x6c;

/// The scene's object-state slot tables, in the order the passes below
/// walk them (DTI-style ids: the low 12 bits are the slot number).
const SLOT_TABLE_RESET_A: u32 = 0x0143_a61c;
const SLOT_TABLE_CLEAR_B: u32 = 0x0143_a5fc;
const SLOT_TABLE_CLEAR_C: u32 = 0x0142_ec54;
const SLOT_TABLE_RESET_D: u32 = 0x0142_ec34;
const SLOT_TABLE_E: u32 = 0x0143_a5dc;

impl Collect<'_> {
    fn word(&self, address: u32) -> u32 {
        self.pg.u32(self.m, address)
    }
    fn set_word(&mut self, address: u32, v: u32) {
        self.pg.set_u32(self.m, address, v);
    }
    fn scene(&self, offset: u32) -> u32 {
        self.word(self.scene.wrapping_add(offset))
    }
    fn ctx(&self, offset: u32) -> u32 {
        self.word(self.ctx.wrapping_add(offset))
    }
    fn set_ctx(&mut self, offset: u32, v: u32) {
        self.set_word(self.ctx.wrapping_add(offset), v);
    }
    fn count(&self) -> u32 {
        self.word(self.frame.wrapping_add(COUNT))
    }
    fn set_count(&mut self, v: u32) {
        self.set_word(self.frame.wrapping_add(COUNT), v);
    }
    fn selected(&self, i: u32) -> u32 {
        self.word(self.frame.wrapping_add(SELECTED).wrapping_add(4 * i))
    }
    fn set_selected(&mut self, i: u32, v: u32) {
        self.set_word(self.frame.wrapping_add(SELECTED).wrapping_add(4 * i), v);
    }
    fn guest_call(&mut self, target: u32, this: u32, args: &[u32], back: u32) {
        let (pg, frame) = (self.pg, self.frame);
        crate::semantic::call_from(
            self.m,
            self.c,
            pg,
            frame,
            this,
            args,
            back,
            move |m: &mut dyn Mem, c: &mut Cpu| ddnative_base::x86::call_ext(m, c, target),
        );
    }
    /// A call through slot `slot` of `object`'s vtable.
    fn virtual_call(&mut self, object: u32, slot: u32, args: &[u32], back: u32) {
        let method = self.word(self.word(object).wrapping_add(slot));
        self.guest_call(method, object, args, back);
    }
    fn add_dirty(&mut self, bits: u32) {
        let v = self.ctx(0x154) | bits;
        self.set_ctx(0x154, v);
    }

    // The object-state slot table at `ctx + 0x1f8` (8 bytes per slot: value, tag).
    fn value(&self, slot: u32) -> u32 {
        self.ctx(0x1f8 + slot * 8)
    }
    fn tag(&self, slot: u32) -> u32 {
        self.ctx(0x1fc + slot * 8)
    }
    /// The slot's default value, from the table at `[ctx + 0x20]`.
    fn default_value(&self, slot: u32) -> u32 {
        self.word(self.ctx(0x20).wrapping_add(slot * 8))
    }
    /// Append (slot, value, tag) to the undo log when logging is on.
    fn log(&mut self, slot: u32) {
        if self.pg.u8(self.m, self.ctx.wrapping_add(0x5c)) & 0x80 == 0 {
            return;
        }
        let count = self.ctx(0x6c);
        let entry = 0x30c7c_u32.wrapping_add(count.wrapping_mul(12));
        self.set_ctx(entry, slot);
        let (value, tag) = (self.value(slot), self.tag(slot));
        self.set_ctx(entry + 4, value);
        self.set_ctx(entry + 8, tag);
        self.set_ctx(0x6c, count.wrapping_add(1));
    }
    /// Put the slot back to its default value (tagged with its own number).
    fn reset_slot(&mut self, slot: u32) {
        if self.value(slot) == self.default_value(slot) {
            return;
        }
        self.log(slot);
        let default = self.default_value(slot);
        self.set_ctx(0x1f8 + slot * 8, default);
        self.set_ctx(0x1fc + slot * 8, slot);
        self.add_dirty(3);
    }
    /// Clear the slot's value and tag.
    fn clear_slot(&mut self, slot: u32) {
        if self.value(slot) == 0 {
            return;
        }
        self.log(slot);
        self.set_ctx(0x1f8 + slot * 8, 0);
        if self.tag(slot) != 0 {
            self.set_ctx(0x1fc + slot * 8, 0);
            self.add_dirty(2);
        }
        self.add_dirty(1);
    }
    fn slot_of(&self, table: u32, index: u32) -> u32 {
        self.word(table.wrapping_add(4 * index)) & 0xfff
    }

    /// An object passes the scene filter: it is in the mask, drawn in this
    /// pass, of kind 2 and flagged 0x4800 (and has a render block).
    fn passes_filter(&self, object: u32, passbit: u32) -> bool {
        if self.word(object.wrapping_add(0x34)) & self.mask == 0 {
            return false;
        }
        let flags = self.word(object.wrapping_add(4));
        (flags >> 16) & passbit & 0x3ff != 0 && flags & 7 == 2 && flags & 0x4800 == 0x4800
    }

    /// The `n` objects drawn this pass, in the first list of the scene.
    fn select_first_list(&mut self, passbit: u32) {
        let remaining = self.scene(0x38);
        if remaining == 0 {
            return;
        }
        let mut cursor = self.scene(0x20).wrapping_add(self.scene(0x34).wrapping_mul(4));
        let mut selected = 0;
        for _ in 0..remaining {
            let object = self.word(cursor);
            if self.passes_filter(object, passbit)
                && selected < self.max
                && self.word(object.wrapping_add(0x1e0)) != 0
            {
                self.set_selected(selected, object);
                selected += 1;
            }
            cursor = cursor.wrapping_add(4);
        }
        self.set_count(selected);
    }

    /// Insert the objects of the second list that contain the probe point
    /// into the selection, keeping it ordered by descending `u16` at +0x32.
    fn select_second_list(&mut self, passbit: u32) {
        if self.scene(0x30) == 0 {
            return;
        }
        let mut i = 0u32;
        loop {
            let slot = self.scene(0x2c).wrapping_add(i);
            let object = self.word(self.scene(0x20).wrapping_add(slot.wrapping_mul(4)));
            if self.passes_filter(object, passbit)
                && self.word(object.wrapping_add(0x1e0)) != 0
                && sphere_contains(&*self.m, self.pg, object.wrapping_add(0x210), self.probe)
            {
                self.insert_ordered(object);
            }
            i += 1;
            if i >= self.scene(0x30) {
                break;
            }
        }
    }

    /// Put `object` into the selection before the first entry whose `u16`
    /// at +0x32 is smaller, or at the end; the selection never grows past
    /// `max` (an insertion into a full selection pushes the last entry out).
    fn insert_ordered(&mut self, object: u32) {
        let count = self.count();
        let key = self.pg.u16(self.m, object.wrapping_add(0x32));
        let at = (0..count).find(|&at| {
            let other = self.selected(at);
            self.pg.u16(self.m, other.wrapping_add(0x32)) < key
        });
        match at {
            Some(at) => {
                let mut k = count;
                while k > at {
                    let previous = self.selected(k - 1);
                    self.set_selected(k, previous);
                    k -= 1;
                }
                self.set_selected(at, object);
            }
            None if count < self.max => self.set_selected(count, object),
            None => return,
        }
        if count < self.max {
            self.set_count(count + 1);
        }
    }

    /// Selection sort of the chosen objects by the low 12 bits of their
    /// +0x1e0 (stable for equal keys: the first minimum wins).
    fn sort_selected(&mut self, count: u32) {
        let key = |this: &Self, i: u32| this.word(this.selected(i).wrapping_add(0x1e0)) & 0xfff;
        for i in 0..count {
            let mut best = i;
            let mut best_key = key(self, i);
            for j in i + 1..count {
                let k = key(self, j);
                if best_key > k {
                    best_key = k;
                    best = j;
                }
            }
            let (a, b) = (self.selected(i), self.selected(best));
            self.set_selected(i, b);
            self.set_selected(best, a);
        }
    }

    /// For the unused selection positions `count..8`, restore the slots
    /// those positions would have used.
    fn release_unused_positions(&mut self, count: u32) {
        for k in count..8 {
            let slot = self.slot_of(SLOT_TABLE_RESET_A, k);
            self.reset_slot(slot);
            let slot = self.slot_of(SLOT_TABLE_CLEAR_B, k);
            self.clear_slot(slot);
            let slot = self.slot_of(SLOT_TABLE_CLEAR_C, k);
            self.clear_slot(slot);
        }
        for k in count..8 {
            let slot = self.slot_of(SLOT_TABLE_RESET_D, k);
            self.reset_slot(slot);
        }
    }

    /// Merge the slot bindings of neighbouring positions: while the
    /// bindings of positions 0.. agree with the id they were bound as,
    /// carry on; at the first position bound differently, copy that
    /// binding's default over the first position's slot and refresh the
    /// related slots.
    fn merge_bindings(&mut self, count: u32) {
        let positions = count.saturating_sub(1);
        let bound_id = |this: &Self, table_value: u32| {
            let slot = table_value & 0xfff;
            let id = this.tag(slot) & 0xfff;
            let entry = this.word(this.ctx(0x1c).wrapping_add(id * 8));
            (entry << 12) | id
        };
        let mut i = 0;
        while i < positions {
            let wanted = self.word(SLOT_TABLE_RESET_D.wrapping_add(4 * i));
            if bound_id(self, wanted) != wanted {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            if j >= positions {
                return;
            }
            let different = loop {
                let value = self.word(SLOT_TABLE_RESET_D.wrapping_add(4 * j));
                let bound = bound_id(self, value);
                if bound != value {
                    break bound;
                }
                j += 1;
                if j >= positions {
                    return;
                }
            };
            if different == 0 {
                return;
            }
            let wanted_slot = wanted & 0xfff;
            let source = different & 0xfff;
            if self.value(wanted_slot) != self.default_value(source) {
                self.log(wanted_slot);
                let default = self.default_value(source);
                self.set_ctx(0x1f8 + wanted_slot * 8, default);
                self.set_ctx(
                    0x1fc + wanted_slot * 8,
                    ((wanted ^ different) & 0xffff_f000) | source,
                );
                self.add_dirty(3);
            }
            // The related slot of position j, then of position i.
            let other = self.slot_of(SLOT_TABLE_E, j);
            let mut replacement = self.value(other) & 0xffff_fff0;
            let own = self.word(SLOT_TABLE_E.wrapping_add(4 * i)) & 0xfff;
            if replacement == 0 {
                replacement = self.default_value(own);
            }
            if self.value(own) & 0xffff_fff0 != replacement || self.tag(own) != 0 {
                self.log(own);
                let old_tag = self.tag(own);
                self.set_ctx(0x1f8 + own * 8, replacement);
                if old_tag != 0 {
                    self.set_ctx(0x1fc + own * 8, 0);
                    self.add_dirty(2);
                }
                self.add_dirty(1);
            }
            let slot = self.word(SLOT_TABLE_RESET_D.wrapping_add(4 * j)) & 0xfff;
            self.reset_slot(slot);
            i += 1;
        }
    }
}

/// 0x00FEB1F0, `scene->collect_visible(ctx, mask, max, probe)` (thiscall,
/// `ret 0x10`, void; `sScene::vf13`): choose up to `max` objects of the
/// scene for one draw pass, let them prepare their draw state, and bring
/// the draw context's object-state slots in line with that choice.
///
/// 1. From the scene's first object list take those in `mask`, drawn in
///    this pass (`ctx + 0x168` selects the bit), of kind 2, flagged 0x4800
///    and holding a render block (+0x1e0), up to `max`.
/// 2. The scene's selection virtual (+0x64) adds more (mode 0).
/// 3. When the scene has a spatial index (`+0xb5c`, `+0xb60`) the index
///    query 0x011787c0 adds those inside the `probe` sphere; otherwise the
///    second object list is scanned for objects containing the probe
///    (0x00D13600), inserted ordered by descending `u16` at +0x32.
/// 4. The selection virtual again (mode 1), a selection sort by the low 12
///    bits of each object's +0x1e0, and each object's virtual +0x50
///    (`prepare(ctx, index)`).
/// 5. The slots of the unused positions are reset or cleared, adjacent
///    bound positions merged, and the scene's final pass 0x00FE4F30 runs.
// Translated from 0x00FEB1F0 (DDDA.exe 2017-12-13 Steam)
pub fn scene_collect_visible(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let (ctx, mask, max, probe) = (arg(m, 0), arg(m, 1), arg(m, 2), arg(m, 3));
    let scene = c.ecx;
    // The original's frame: push ebp; and esp, -16; 0x84 locals; 3 pushes.
    let frame = (entry.wrapping_sub(4) & !0xf).wrapping_sub(0x84 + 12);
    let passbit = 1u32 << (pg.u8(m, ctx.wrapping_add(0x168)) & 31);
    let mut s = Collect {
        m,
        c,
        pg,
        frame,
        scene,
        ctx,
        mask,
        max,
        probe,
    };
    // The selection array (nine slots) starts empty.
    for i in 0..9 {
        s.set_selected(i, 0);
    }
    s.set_count(0);
    s.select_first_list(passbit);
    let selection = frame.wrapping_add(SELECTED);
    let count_address = frame.wrapping_add(COUNT);
    s.virtual_call(
        scene,
        0x64,
        &[ctx, mask, max, selection, count_address, 0],
        0x00fe_b2da,
    );
    let index = if pg.u8(s.m, scene.wrapping_add(0xb5c)) != 0 {
        s.scene(0xb60)
    } else {
        0
    };
    if pg.u8(s.m, scene.wrapping_add(0xb5c)) != 0 && index != 0 {
        // The index query's argument block lives in this frame.
        let block = frame.wrapping_add(0x2c);
        let query = frame.wrapping_add(0x40);
        let radius_constant = s.word(0x0162_adb0);
        let (p0, p1, p2, p3) = (
            s.word(probe),
            s.word(probe + 4),
            s.word(probe + 8),
            s.word(probe + 12),
        );
        let count = s.count();
        for (offset, v) in [
            (0x48, radius_constant),
            (0x2c, mask),
            (0x38, count),
            (0x50, p0),
            (0x54, p1),
            (0x34, max),
            (0x3c, selection),
            (0x40, 0x0157_92e8),
            (0x44, 5),
            (0x58, p2),
            (0x5c, p3),
            (0x30, passbit),
        ] {
            s.set_word(frame.wrapping_add(offset), v);
        }
        s.guest_call(
            0x0117_87c0,
            index,
            &[query, scene, 0x00fe_5690, block, 0, 0xffff_ffff],
            0x00fe_b362,
        );
        let found = s.word(frame.wrapping_add(0x38));
        s.set_count(found);
    } else {
        s.select_second_list(passbit);
    }
    s.virtual_call(
        scene,
        0x64,
        &[ctx, mask, max, selection, count_address, 1],
        0x00fe_b44d,
    );
    let count = s.count();
    s.sort_selected(count);
    let mut i = 0;
    let mut count = count;
    while i < count {
        let object = s.selected(i);
        s.virtual_call(object, 0x50, &[ctx, i], 0x00fe_b4e3);
        count = s.count();
        i += 1;
    }
    s.release_unused_positions(count);
    s.merge_bindings(count);
    s.guest_call(0x00fe_4f30, scene, &[ctx, mask], 0x00fe_bb10);
    s.c.esp = entry.wrapping_add(4 + 0x10);
}

const EPS: f32 = 1.1920929e-07;

/// The inverse of the product `A * B` of the camera matrices (row-major
/// 4x4), by the game's cofactor expansion; the identity (read from the
/// game's table at 0x016295C0) when the determinant is exactly zero.
#[allow(unused_assignments, unused_variables, clippy::all)]
fn invert_product(p: &[f32; 16], identity: impl Fn() -> [f32; 16]) -> [f32; 16] {
    let [p00, p01, p02, p03, p10, p11, p12, p13, p20, p21, p22, p23, p30, p31, p32, p33] = *p;
    let mut inv = [0f32; 16];
    let mut l298: f32;
    let mut l1e0: f32;
    let mut l268: f32;
    let mut l2ac: f32;
    let mut l1dc: f32;
    let mut l288: f32;
    let mut fv27: f32;
    let mut l274: f32;
    let mut l1d8: f32;
    let mut l26c: f32;
    let mut fv28: f32;
    let mut l2a4: f32;
    let mut l264: f32;
    let mut l270: f32;
    let mut l294: f32;
    let mut fv26: f32;
    l298 = p12 * p23;
    l1e0 = p13 * p21;
    l268 = p13 * p22;
    l2ac = p23 * p32;
    l1dc =
        (((l298 * p31 + ((p11 * p22) * p33) + l1e0 * p32) - l268 * p31) - l2ac * p11) - ((p12 * p33) * p21);
    l288 = p02 * p23;
    fv27 = p01 * p22;
    l274 = p02 * p33;
    l1d8 = -((((p33 * fv27 + p31 * l288 + (p32 * (p03 * p21))) - (p31 * (p03 * p22))) - l2ac * p01)
        - p21 * l274);
    l26c = p02 * p13;
    fv28 = p01 * p12;
    l2a4 = p03 * p11;
    l264 = p03 * p12;
    l270 = (((p33 * fv28 + p31 * l26c + p32 * l2a4) - p31 * l264) - (p01 * (p13 * p32))) - p11 * l274;
    l294 = -((((p23 * fv28 + p21 * l26c + p22 * l2a4) - p21 * l264) - l268 * p01) - p11 * l288);
    inv[15] = l1dc * p00 + l1d8 * p10 + l270 * p20 + l294 * p30;
    if inv[15] == 0.0 {
        let ident = identity();
        inv[0] = ident[0];
        inv[1] = ident[1];
        inv[2] = ident[2];
        inv[3] = ident[3];
        inv[4] = ident[4];
        inv[5] = ident[5];
        inv[6] = ident[6];
        inv[7] = ident[7];
        inv[8] = ident[8];
        inv[9] = ident[9];
        inv[10] = ident[10];
        inv[11] = ident[11];
        inv[12] = ident[12];
        inv[13] = ident[13];
        inv[14] = ident[14];
        inv[15] = ident[15];
    } else {
        inv[15] = 1.0 / inv[15];
        inv[0] = inv[15] * l1dc;
        inv[1] = inv[15] * l1d8;
        inv[3] = inv[15] * l294;
        inv[2] = inv[15] * l270;
        inv[4] = -(((((((p10 * p22) * p33) + l298 * p30 + ((p13 * p20) * p32)) - l268 * p30) - l2ac * p10)
            - ((p12 * p33) * p20))
            * inv[15]);
        inv[5] = ((((((p00 * p22) * p33) + p30 * l288 + ((p03 * p20) * p32)) - (p30 * (p03 * p22)))
            - l2ac * p00)
            - p20 * l274)
            * inv[15];
        l2ac = p03 * p10;
        inv[6] = -(((((((p00 * p12) * p33) + p30 * l26c + l2ac * p32) - p30 * l264) - (p00 * (p13 * p32)))
            - p10 * l274)
            * inv[15]);
        inv[7] = ((((((p00 * p12) * p23) + p20 * l26c + l2ac * p22) - p20 * l264) - l268 * p00) - p10 * l288)
            * inv[15];
        l270 = p10 * p21;
        inv[8] = ((((((p11 * p23) * p30) + l270 * p33 + ((p13 * p20) * p31)) - l1e0 * p30)
            - (p10 * (p23 * p31)))
            - ((p11 * p33) * p20))
            * inv[15];
        l298 = p01 * p23;
        l294 = p00 * p21;
        inv[9] = -(((((l298 * p30 + l294 * p33 + ((p03 * p20) * p31)) - (p30 * (p03 * p21)))
            - (p00 * (p23 * p31)))
            - ((p01 * p33) * p20))
            * inv[15]);
        fv26 = p00 * p11;
        inv[10] = ((((((p01 * p13) * p30) + fv26 * p33 + l2ac * p31) - p30 * l2a4) - ((p13 * p31) * p00))
            - (p10 * (p01 * p33)))
            * inv[15];
        inv[11] = -(((((((p01 * p13) * p20) + fv26 * p23 + l2ac * p21) - p20 * l2a4) - l1e0 * p00)
            - p10 * l298)
            * inv[15]);
        inv[12] = -(((((l270 * p32 + ((p11 * p22) * p30) + ((p12 * p20) * p31)) - ((p12 * p21) * p30))
            - ((p22 * p31) * p10))
            - ((p11 * p32) * p20))
            * inv[15]);
        inv[13] = ((((l294 * p32 + p30 * fv27 + ((p02 * p20) * p31)) - ((p02 * p21) * p30))
            - ((p22 * p31) * p00))
            - ((p01 * p32) * p20))
            * inv[15];
        inv[14] = -(((((fv26 * p32 + p30 * fv28 + ((p02 * p10) * p31)) - ((p02 * p11) * p30))
            - ((p12 * p31) * p00))
            - ((p01 * p32) * p10))
            * inv[15]);
        inv[15] = ((((fv26 * p22 + p20 * fv28 + ((p02 * p10) * p21)) - ((p02 * p11) * p20))
            - ((p12 * p21) * p00))
            - p10 * fv27)
            * inv[15];
    }
    inv
}

/// The inverse of the matrix `B` (row-major 4x4), by the game's cofactor
/// expansion; the identity when the determinant is exactly zero.
#[allow(unused_assignments, unused_variables, clippy::all)]
fn invert_matrix(b: &[f32; 16], identity: impl Fn() -> [f32; 16]) -> [f32; 16] {
    let fv2 = b[0];
    let mut inv = [0f32; 16];
    let mut fv27: f32;
    let mut fv28: f32;
    let mut fv26: f32;
    let mut fv1: f32;
    let mut l2c0: f32;
    let mut l2bc: f32;
    let mut fv3: f32;
    let mut l2d8: f32;
    let mut l2e0: f32;
    let mut l2d4: f32;
    let mut l2e4: f32;
    let mut l2e8: f32;
    let mut l2dc: f32;
    let mut l298: f32;
    let mut l2a4: f32;
    let mut l294: f32;
    let mut fv4: f32;
    fv27 = b[10];
    fv28 = b[14];
    fv26 = b[13];
    fv1 = b[15];
    l2c0 = b[7] * fv27;
    l2bc = b[1];
    inv[0] = (((((b[6] * b[11]) * fv26) + ((b[5] * fv27) * fv1) + ((b[7] * b[9]) * fv28)) - l2c0 * fv26)
        - ((fv28 * b[11]) * b[5]))
        - ((b[6] * fv1) * b[9]);
    fv3 = b[2];
    l2d8 = fv3 * b[11];
    l2e0 = b[3];
    l2d4 = (fv3 * fv1);
    l2e4 = l2e0 * b[5];
    l2e8 = l2bc * b[6];
    l2dc = l2e0 * b[6];
    inv[1] = -((((((l2bc * fv27) * fv1) + l2d8 * fv26 + ((l2e0 * b[9]) * fv28)) - ((l2e0 * fv27) * fv26))
        - (l2bc * (fv28 * b[11])))
        - l2d4 * b[9]);
    inv[2] = (((((fv3 * b[7]) * fv26) + l2e8 * b[15] + l2e4 * fv28) - l2dc * b[13]) - ((fv28 * b[7]) * l2bc))
        - l2d4 * b[5];
    inv[3] = -((((((fv3 * b[7]) * b[9]) + l2e8 * b[11] + l2e4 * b[10]) - l2dc * b[9]) - l2bc * l2c0)
        - l2d8 * b[5]);
    inv[15] = b[4] * inv[1] + inv[0] * fv2 + inv[2] * b[8] + b[12] * inv[3];
    if inv[15] == 0.0 {
        let ident = identity();
        inv[0] = ident[0];
        inv[1] = ident[1];
        inv[2] = ident[2];
        inv[3] = ident[3];
        inv[4] = ident[4];
        inv[5] = ident[5];
        inv[6] = ident[6];
        inv[7] = ident[7];
        inv[8] = ident[8];
        inv[9] = ident[9];
        inv[10] = ident[10];
        inv[11] = ident[11];
        inv[12] = ident[12];
        inv[13] = ident[13];
        inv[14] = ident[14];
        inv[15] = ident[15];
    } else {
        inv[15] = 1.0 / inv[15];
        inv[0] = inv[15] * inv[0];
        inv[1] = inv[15] * inv[1];
        fv27 = b[8];
        inv[2] = inv[15] * inv[2];
        fv28 = b[12];
        inv[3] = inv[15] * inv[3];
        fv26 = b[15];
        inv[4] = -(((((((b[6] * b[11]) * fv28) + ((b[4] * b[10]) * fv26) + (b[14] * (b[7] * fv27)))
            - ((b[7] * b[10]) * fv28))
            - ((b[14] * b[11]) * b[4]))
            - ((b[6] * fv26) * fv27))
            * inv[15]);
        fv1 = b[2];
        l298 = b[3] * fv27;
        inv[5] = ((((((fv2 * b[10]) * fv26) + ((fv1 * b[11]) * fv28) + l298 * b[14])
            - ((b[3] * b[10]) * fv28))
            - (fv2 * (b[14] * b[11])))
            - ((fv1 * fv26) * fv27))
            * inv[15];
        l2e0 = b[3] * b[4];
        fv3 = b[4];
        inv[6] = -(((((((fv1 * b[7]) * fv28) + ((fv2 * b[6]) * fv26) + l2e0 * b[14])
            - ((b[3] * b[6]) * fv28))
            - ((b[14] * b[7]) * fv2))
            - ((fv1 * fv26) * fv3))
            * inv[15]);
        l2a4 = b[9];
        l294 = l2a4 * fv3;
        inv[7] = ((((((fv1 * b[7]) * fv27) + ((fv2 * b[6]) * b[11]) + l2e0 * b[10])
            - ((b[3] * b[6]) * fv27))
            - (fv2 * (b[7] * b[10])))
            - ((fv1 * b[11]) * fv3))
            * inv[15];
        fv26 = b[5];
        fv1 = b[15];
        fv3 = b[1];
        inv[8] = ((((((fv26 * b[11]) * fv28) + l294 * b[15] + (b[13] * (b[7] * fv27)))
            - ((l2a4 * b[7]) * fv28))
            - ((b[13] * b[11]) * b[4]))
            - ((fv26 * fv1) * fv27))
            * inv[15];
        l2c0 = fv3 * b[11];
        inv[9] = -(((((((fv2 * l2a4) * fv1) + l2c0 * fv28 + b[13] * l298) - ((l2a4 * b[3]) * fv28))
            - (fv2 * (b[13] * b[11])))
            - ((fv3 * fv1) * fv27))
            * inv[15]);
        inv[10] = ((((((fv3 * b[7]) * fv28) + ((fv2 * fv26) * b[15]) + b[13] * l2e0)
            - ((fv26 * b[3]) * fv28))
            - ((b[13] * b[7]) * fv2))
            - ((fv3 * fv1) * b[4]))
            * inv[15];
        fv1 = b[4];
        fv4 = b[14];
        inv[11] = -(((((((fv3 * b[7]) * fv27) + ((fv2 * fv26) * b[11]) + l2a4 * l2e0)
            - ((fv26 * b[3]) * fv27))
            - (fv2 * (l2a4 * b[7])))
            - l2c0 * fv1)
            * inv[15]);
        l2e4 = b[2];
        inv[12] = -(((((((fv26 * b[10]) * fv28) + l294 * fv4 + ((fv27 * b[6]) * b[13]))
            - ((l2a4 * b[6]) * b[12]))
            - (fv1 * (b[10] * b[13])))
            - ((b[5] * fv4) * b[8]))
            * inv[15]);
        fv27 = b[1] * b[10];
        fv28 = b[1] * fv4;
        l2d8 = ((((b[9] * fv2) * fv4) + fv27 * b[12] + ((l2e4 * b[8]) * b[13])) - ((l2e4 * b[9]) * b[12]))
            - (fv2 * (b[10] * b[13]));
        inv[13] = (l2d8 - fv28 * b[8]) * inv[15];
        l2e8 = fv2 * b[5];
        l2dc = b[6];
        l2d4 = (l2e4 * b[5]);
        l2e4 = l2e4 * fv1;
        inv[14] = -(((((l2e8 * b[14] + ((l2dc * b[1]) * b[12]) + l2e4 * b[13]) - l2d4 * b[12])
            - ((l2dc * b[13]) * fv2))
            - fv28 * b[4])
            * inv[15]);
        inv[15] = ((((((l2dc * b[1]) * b[8]) + l2e8 * b[10] + l2e4 * b[9]) - l2d4 * b[8])
            - ((l2dc * b[9]) * fv2))
            - fv27 * b[4])
            * inv[15];
    }
    inv
}

/// The six clip-space frustum planes (24 floats: nx, ny, nz, d each) from
/// the eight transformed corners `c` (32 floats, 4 per corner; corner 7 is
/// unused), with the plane windings of the mirrored (`mirrored`) or normal
/// projection.
#[allow(unused_assignments, unused_variables, clippy::all)]
fn frustum_planes(c: &[f32; 32], mirrored: bool) -> [f32; 24] {
    let mut pl = [0f32; 24];
    let mut l1d0: f32;
    let mut fv26: f32;
    let mut fv27: f32;
    let mut fv28: f32;
    let mut l70: f32;
    let mut l6c: f32;
    let mut l2d0: f32;
    let mut fs2cc: f32;
    let mut fs2c8: f32;
    let mut l1cc: f32;
    let mut l2e4: f32;
    let mut l2d8: f32;
    if !mirrored {
        l1d0 = c[4] - c[0];
        fv26 = (c[18] - c[2]) * (c[5] - c[1]) - (c[17] - c[1]) * (c[6] - c[2]);
        fv27 = (c[6] - c[2]) * (c[16] - c[0]) - (c[18] - c[2]) * l1d0;
        pl[2] = (c[17] - c[1]) * l1d0 - (c[5] - c[1]) * (c[16] - c[0]);
        fv28 = (fv27 * fv27 + fv26 * fv26 + pl[2] * pl[2]).sqrt();
        if !(EPS > fv28) {
            fv28 = 1.0 / fv28;
            fv26 = fv28 * fv26;
            fv27 = fv27 * fv28;
            pl[2] = pl[2] * fv28;
        }
        pl[3] = -(fv27 * c[1] + c[0] * fv26 + pl[2] * c[2]);
        pl[0] = fv26;
        pl[1] = fv27;
        pl[6] = (c[9] - c[25]) * (c[12] - c[24]) - (c[13] - c[25]) * (c[8] - c[24]);
        fv28 = (c[14] - c[26]) * (c[8] - c[24]) - (c[10] - c[26]) * (c[12] - c[24]);
        fv27 = (c[10] - c[26]) * (c[13] - c[25]) - (c[9] - c[25]) * (c[14] - c[26]);
        fv26 = (fv28 * fv28 + fv27 * fv27 + pl[6] * pl[6]).sqrt();
        if !(EPS > fv26) {
            fv26 = 1.0 / fv26;
            fv27 = fv26 * fv27;
            fv28 = fv28 * fv26;
            pl[6] = pl[6] * fv26;
        }
        pl[4] = fv27;
        pl[5] = fv28;
        pl[7] = -(fv27 * c[24] + fv28 * c[25] + pl[6] * c[26]);
        l70 = c[4] - c[8];
        pl[10] = (c[1] - c[9]) * l70 - (c[5] - c[9]) * (c[0] - c[8]);
        fv27 = (c[6] - c[10]) * (c[0] - c[8]) - (c[2] - c[10]) * l70;
        fv26 = (c[2] - c[10]) * (c[5] - c[9]) - (c[1] - c[9]) * (c[6] - c[10]);
        fv28 = (fv27 * fv27 + fv26 * fv26 + pl[10] * pl[10]).sqrt();
        if !(EPS > fv28) {
            fv28 = 1.0 / fv28;
            fv26 = fv26 * fv28;
            fv27 = fv27 * fv28;
            pl[10] = pl[10] * fv28;
        }
        pl[8] = fv26;
        pl[9] = fv27;
        pl[11] = -(c[8] * fv26 + fv27 * c[9] + pl[10] * c[10]);
        fv27 = (c[26] - c[18]) * (c[21] - c[17]) - (c[25] - c[17]) * (c[22] - c[18]);
        pl[14] = (c[25] - c[17]) * (c[20] - c[16]) - (c[21] - c[17]) * (c[24] - c[16]);
        fv28 = (c[22] - c[18]) * (c[24] - c[16]) - (c[26] - c[18]) * (c[20] - c[16]);
        fv26 = (fv27 * fv27 + fv28 * fv28 + pl[14] * pl[14]).sqrt();
        if !(EPS > fv26) {
            fv26 = 1.0 / fv26;
            fv27 = fv26 * fv27;
            pl[14] = fv26 * pl[14];
            fv28 = fv28 * fv26;
        }
        pl[12] = fv27;
        pl[13] = fv28;
        pl[15] = -(fv27 * c[16] + c[17] * fv28 + c[18] * pl[14]);
        l6c = c[9] - c[17];
        fv27 = (c[2] - c[18]) * l6c - (c[1] - c[17]) * (c[10] - c[18]);
        pl[18] = (c[8] - c[16]) * (c[1] - c[17]) - (c[0] - c[16]) * l6c;
        fv28 = (c[0] - c[16]) * (c[10] - c[18]) - (c[8] - c[16]) * (c[2] - c[18]);
        fv26 = (fv27 * fv27 + fv28 * fv28 + pl[18] * pl[18]).sqrt();
        if !(EPS > fv26) {
            fv26 = 1.0 / fv26;
            fv27 = fv26 * fv27;
            fv28 = fv28 * fv26;
            pl[18] = pl[18] * fv26;
        }
        pl[16] = fv27;
        pl[17] = fv28;
        pl[19] = -(fv27 * c[16] + c[17] * fv28 + c[18] * pl[18]);
        l2d0 = (c[22] - c[6]) * (c[13] - c[5]) - (c[21] - c[5]) * (c[14] - c[6]);
        fs2cc = (c[20] - c[4]) * (c[14] - c[6]) - (c[12] - c[4]) * (c[22] - c[6]);
        fs2c8 = (c[12] - c[4]) * (c[21] - c[5]) - (c[20] - c[4]) * (c[13] - c[5]);
        fv27 = (l2d0 * l2d0 + fs2cc * fs2cc + fs2c8 * fs2c8).sqrt();
        if !(EPS > fv27) {
            fv27 = 1.0 / fv27;
            l2d0 = l2d0 * fv27;
            fs2cc = fs2cc * fv27;
            fs2c8 = fs2c8 * fv27;
        }
        pl[20] = l2d0;
        pl[21] = fs2cc;
        pl[23] = -(l2d0 * c[4] + c[5] * fs2cc + c[6] * fs2c8);
    } else {
        fv27 = (c[2] - c[18]) * (c[5] - c[17]) - (c[1] - c[17]) * (c[6] - c[18]);
        pl[2] = (c[4] - c[16]) * (c[1] - c[17]) - (c[0] - c[16]) * (c[5] - c[17]);
        fv26 = (c[0] - c[16]) * (c[6] - c[18]) - (c[4] - c[16]) * (c[2] - c[18]);
        fv28 = (fv27 * fv27 + fv26 * fv26 + pl[2] * pl[2]).sqrt();
        if !(EPS > fv28) {
            fv28 = 1.0 / fv28;
            fv27 = fv28 * fv27;
            fv26 = fv28 * fv26;
            pl[2] = fv28 * pl[2];
        }
        pl[3] = -(fv27 * c[16] + fv26 * c[17] + pl[2] * c[18]);
        fv28 = (c[26] - c[10]) * (c[13] - c[9]) - (c[25] - c[9]) * (c[14] - c[10]);
        pl[0] = fv27;
        pl[1] = fv26;
        pl[6] = (c[12] - c[8]) * (c[25] - c[9]) - (c[24] - c[8]) * (c[13] - c[9]);
        fv26 = (c[24] - c[8]) * (c[14] - c[10]) - (c[12] - c[8]) * (c[26] - c[10]);
        fv27 = (fv28 * fv28 + fv26 * fv26 + pl[6] * pl[6]).sqrt();
        if !(EPS > fv27) {
            fv27 = 1.0 / fv27;
            fv28 = fv27 * fv28;
            pl[6] = fv27 * pl[6];
            fv26 = fv26 * fv27;
        }
        l1cc = c[9] - c[1];
        l2e4 = c[10] - c[2];
        pl[7] = -(fv28 * c[8] + c[9] * fv26 + c[10] * pl[6]);
        l2d8 = c[8] - c[0];
        pl[4] = fv28;
        pl[5] = fv26;
        fv28 = l2e4 * (c[5] - c[1]) - l1cc * (c[6] - c[2]);
        pl[10] = (c[4] - c[0]) * l1cc - l2d8 * (c[5] - c[1]);
        fv27 = l2d8 * (c[6] - c[2]) - (c[4] - c[0]) * l2e4;
        fv26 = (fv27 * fv27 + fv28 * fv28 + pl[10] * pl[10]).sqrt();
        if !(EPS > fv26) {
            fv26 = 1.0 / fv26;
            fv28 = fv26 * fv28;
            fv27 = fv27 * fv26;
            pl[10] = pl[10] * fv26;
        }
        pl[11] = -(fv27 * c[1] + c[0] * fv28 + pl[10] * c[2]);
        pl[8] = fv28;
        pl[9] = fv27;
        fv28 = (c[22] - c[26]) * (c[16] - c[24]) - (c[18] - c[26]) * (c[20] - c[24]);
        fv27 = (c[18] - c[26]) * (c[21] - c[25]) - (c[17] - c[25]) * (c[22] - c[26]);
        pl[14] = (c[17] - c[25]) * (c[20] - c[24]) - (c[21] - c[25]) * (c[16] - c[24]);
        fv26 = (fv28 * fv28 + fv27 * fv27 + pl[14] * pl[14]).sqrt();
        if !(EPS > fv26) {
            fv26 = 1.0 / fv26;
            fv27 = fv26 * fv27;
            fv28 = fv28 * fv26;
            pl[14] = pl[14] * fv26;
        }
        pl[12] = fv27;
        pl[13] = fv28;
        l1d0 = c[16] - c[0];
        pl[15] = -(fv27 * c[24] + fv28 * c[25] + pl[14] * c[26]);
        pl[18] = (c[17] - c[1]) * l2d8 - l1cc * l1d0;
        fv27 = l2e4 * l1d0 - (c[18] - c[2]) * l2d8;
        fv28 = (c[18] - c[2]) * l1cc - (c[17] - c[1]) * l2e4;
        fv26 = (fv27 * fv27 + fv28 * fv28 + pl[18] * pl[18]).sqrt();
        if !(EPS > fv26) {
            fv26 = 1.0 / fv26;
            fv28 = fv28 * fv26;
            fv27 = fv27 * fv26;
            pl[18] = pl[18] * fv26;
        }
        pl[16] = fv28;
        pl[17] = fv27;
        pl[19] = -(fv27 * c[1] + c[0] * fv28 + pl[18] * c[2]);
        l2d0 = (c[6] - c[22]) * (c[13] - c[21]) - (c[5] - c[21]) * (c[14] - c[22]);
        fs2cc = (c[14] - c[22]) * (c[4] - c[20]) - (c[6] - c[22]) * (c[12] - c[20]);
        fs2c8 = (c[5] - c[21]) * (c[12] - c[20]) - (c[13] - c[21]) * (c[4] - c[20]);
        fv27 = (fs2cc * fs2cc + l2d0 * l2d0 + fs2c8 * fs2c8).sqrt();
        if !(EPS > fv27) {
            fv27 = 1.0 / fv27;
            l2d0 = l2d0 * fv27;
            fs2cc = fs2cc * fv27;
            fs2c8 = fs2c8 * fv27;
        }
        pl[20] = l2d0;
        pl[21] = fs2cc;
        pl[23] = -(fs2cc * c[21] + c[20] * l2d0 + fs2c8 * c[22]);
    }
    pl[22] = fs2c8;
    pl
}

/// Row-major 4x4 product `a * b`: `p[4i + j] = a[4i]*b[j] + a[4i+1]*b[4+j]
/// + a[4i+2]*b[8+j] + a[4i+3]*b[12+j]`, summed left to right as the
/// game does.
fn multiply_matrices(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut p = [0f32; 16];
    for i in 0..4 {
        for j in 0..4 {
            p[4 * i + j] = ((a[4 * i] * b[j] + a[4 * i + 1] * b[4 + j]) + a[4 * i + 2] * b[8 + j])
                + a[4 * i + 3] * b[12 + j];
        }
    }
    p
}

fn read_floats16(m: &dyn Mem, pg: Pages, address: u32) -> [f32; 16] {
    std::array::from_fn(|i| f32::from_bits(pg.u32(m, address.wrapping_add(4 * i as u32))))
}

/// A 4-corner-vector times a matrix: `x * row0 + y * row1 + z * row2 + w *
/// row3`, per lane, left to right.
fn transform_corner(v: [f32; 4], m: &[f32; 16]) -> [f32; 4] {
    std::array::from_fn(|j| ((v[0] * m[j] + v[1] * m[4 + j]) + v[2] * m[8 + j]) + v[3] * m[12 + j])
}

/// 0x00FF2EE0, `camera->set_view_projection(view, projection, viewport,
/// bias)` (thiscall, `ret 0x10`, void): load a camera's view and projection
/// matrices into the draw context and build the constant blocks derived
/// from them.
///
/// From the view matrix `view` and projection `projection` (row-major 4x4
/// floats) it computes the product, the inverse of the product, the
/// inverse of the projection, the inverse-rotation block and, after the
/// viewport setup 0x00FF04C0, the depth scale (a `log` pair through the
/// CRT's 0x012F45C4 / 0x012F439E: the mirrored-projection flag is
/// `projection[10] < 0`). Then two constant blocks are taken from the
/// context's arena:
///
/// 1. (`this + 0xCA0`) the six frustum planes: the eight clip-space cube
///    corners through the inverse product, divided by their w where it is
///    positive, and the plane through each face (normalized unless the
///    normal is shorter than `FLT_EPSILON`);
/// 2. (`this + 0xC88`) the matrices: the product, view, rotation,
///    projection, inverse projection and inverse product, plus the
///    camera position terms, the projection's depth ratio and the
///    `bias` argument.
///
/// The camera position (-translation in the rotation basis) also lands in
/// `this + 0x184..0x190`.
// Translated from 0x00FF2EE0 (DDDA.exe 2017-12-13 Steam)
pub fn set_camera_matrices(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let (view, projection, viewport) = (arg(m, 0), arg(m, 1), arg(m, 2));
    // The original's frame: push ebp; and esp, -16; 0x2E4 locals; 3 pushes.
    let frame = (entry.wrapping_sub(4) & !0xf).wrapping_sub(0x2e4 + 12);
    let word = |m: &dyn Mem, a: u32| pg.u32(m, a);
    let float = |m: &dyn Mem, a: u32| f32::from_bits(pg.u32(m, a));
    let a = read_floats16(m, pg, view);
    let b = read_floats16(m, pg, projection);

    // The product and its inverse, the projection's inverse, the rotation block.
    let product = multiply_matrices(&a, &b);
    let inverse_product = invert_product(&product, || read_floats16(m, pg, 0x0162_95c0));
    let inverse_projection = invert_matrix(&b, || read_floats16(m, pg, 0x0162_95c0));
    let mut rotation = [0f32; 16];
    rotation[0] = a[0];
    rotation[4] = a[1];
    rotation[8] = a[2];
    rotation[1] = a[4];
    rotation[5] = a[5];
    rotation[9] = a[6];
    rotation[2] = a[8];
    rotation[6] = a[9];
    rotation[10] = a[10];
    rotation[15] = 1.0;
    rotation[12] = -((a[13] * rotation[4] + a[12] * rotation[0]) + a[14] * rotation[8]);
    rotation[13] = -((a[12] * rotation[1] + a[13] * rotation[5]) + a[14] * rotation[9]);
    rotation[14] = -((a[12] * rotation[2] + a[13] * rotation[6]) + a[14] * rotation[10]);

    // The viewport setup (constant block at this + 0xC90), called directly
    // with the original's call frame: argument at frame - 4, return address
    // at frame - 8.
    pg.set_u32(m, frame.wrapping_sub(4), viewport);
    pg.set_u32(m, frame.wrapping_sub(8), 0x00ff_4a67);
    c.esp = frame.wrapping_sub(8);
    c.ecx = this;
    set_viewport_rect(m, c);

    // The depth scale from the projection's z terms.
    let b10 = float(m, projection + 0x28);
    let depth_ratio = float(m, projection + 0x38) / b10;
    let remap = (-(b10 * depth_ratio)) / ((-b10) - 1.0);
    let clamped = if remap > 1.0 { remap } else { 1.0 };
    let natural_log = crt_logf(m, pg, clamped + 1.0);
    let ln2 = crt_log(m, pg, f64::from_bits(read64(m, pg, 0x0161_ca50))) as f32;
    let depth_scale = natural_log / ln2;
    let depth_scale_inverse = 1.0 / depth_scale;
    let mirrored = 0.0 > float(m, projection + 0x28);
    let state = word(m, this + 0x114);
    if (state >> 8) & 1 != mirrored as u32 {
        pg.set_u32(
            m,
            this + 0x114,
            (((mirrored as u32) | 0x10) << 8) | (state & 0xeff),
        );
    }

    // Block 1 (this + 0xCA0): the six frustum planes.
    let descriptor = word(m, word(m, this + 0x18).wrapping_add(0x554));
    let size = ((pg.u16(m, descriptor + 0x18) as u32) * 4 + 0xf) & !0xf;
    let block = arena_take(m, c, pg, frame, this, size, 0x00ff_4b86);
    pg.set_u32(m, this + 0xca0, block);
    let dirty = word(m, this + 0x154) | 1;
    pg.set_u32(m, this + 0x154, dirty);
    if block != 0 {
        let (source, bytes) = (
            word(m, descriptor + 0x24),
            (pg.u16(m, descriptor + 0x18) as u32) * 4,
        );
        copy_guest(m, pg, block, source, bytes);
        let one = float(m, 0x0140_6ce0);
        let minus_one = float(m, 0x0161_c20c);
        // The clip-space cube: x, y in {1, -1}, z in {1, 0}, w = 1.
        let cube: [[f32; 4]; 8] = [
            [one, one, one, one],
            [one, one, 0.0, one],
            [minus_one, one, one, one],
            [minus_one, one, 0.0, one],
            [one, minus_one, one, one],
            [one, minus_one, 0.0, one],
            [minus_one, minus_one, one, one],
            [minus_one, minus_one, 0.0, one],
        ];
        let mut corners = [0f32; 32];
        for (i, corner) in cube.iter().enumerate() {
            let mut t = transform_corner(*corner, &inverse_product);
            if 0.0 < t[3] {
                let reciprocal = one / t[3];
                t = [
                    reciprocal * t[0],
                    reciprocal * t[1],
                    reciprocal * t[2],
                    reciprocal * t[3],
                ];
            }
            corners[4 * i..4 * i + 4].copy_from_slice(&t);
        }
        let planes = frustum_planes(&corners, mirrored);
        for (i, v) in planes.iter().enumerate() {
            pg.set_u32(m, block.wrapping_add(4 * i as u32), v.to_bits());
        }
    }

    // Block 2 (this + 0xC88): the matrices.
    let descriptor = word(m, word(m, this + 0x18).wrapping_add(0x548));
    let size = ((pg.u16(m, descriptor + 0x18) as u32) * 4 + 0xf) & !0xf;
    let block = arena_take(m, c, pg, frame, this, size, 0x00ff_5e6a);
    pg.set_u32(m, this + 0xc88, block);
    let dirty = word(m, this + 0x154) | 1;
    pg.set_u32(m, this + 0x154, dirty);
    if block != 0 {
        let (source, bytes) = (
            word(m, descriptor + 0x24),
            (pg.u16(m, descriptor + 0x18) as u32) * 4,
        );
        copy_guest(m, pg, block, source, bytes);
        let put = |m: &mut dyn Mem, offset: u32, v: u32| pg.set_u32(m, block.wrapping_add(offset), v);
        for (i, v) in product.iter().enumerate() {
            put(m, 4 * i as u32, v.to_bits());
        }
        for i in 0..16u32 {
            let v = word(m, view + 4 * i);
            put(m, 0x70 + 4 * i, v);
        }
        for (i, v) in rotation.iter().enumerate() {
            put(m, 0xb0 + 4 * i as u32, v.to_bits());
        }
        for i in 0..16u32 {
            let v = word(m, projection + 4 * i);
            put(m, 0xf0 + 4 * i, v);
        }
        for (i, v) in inverse_projection.iter().enumerate() {
            put(m, 0x130 + 4 * i as u32, v.to_bits());
        }
        for (i, v) in inverse_product.iter().enumerate() {
            put(m, 0x170 + 4 * i as u32, v.to_bits());
        }
        put(m, 0x40, rotation[12].to_bits());
        put(m, 0x44, rotation[13].to_bits());
        put(m, 0x48, rotation[14].to_bits());
        put(m, 0x50, (-rotation[8]).to_bits());
        put(m, 0x54, (-rotation[9]).to_bits());
        put(m, 0x58, (-rotation[10]).to_bits());
        let bias = arg(m, 3);
        put(m, 0x5c, bias);
        put(m, 0x4c, depth_ratio.to_bits());
        put(m, 0x60, depth_scale.to_bits());
        put(m, 0x64, depth_scale_inverse.to_bits());
    }
    pg.set_u32(m, this + 0x184, rotation[12].to_bits());
    pg.set_u32(m, this + 0x188, rotation[13].to_bits());
    pg.set_u32(m, this + 0x18c, rotation[14].to_bits());
    c.esp = entry.wrapping_add(4 + 0x10);
}

/// Take `size` bytes from the context's arena (`this + 0x24` cursor,
/// `+ 0x28` limit), refilling it from the page allocator 0x00DA7D70 when
/// the cursor would pass the limit (a failed refill leaves it empty).
fn arena_take(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, this: u32, size: u32, back: u32) -> u32 {
    if pg.u32(m, this + 0x24).wrapping_add(size) > pg.u32(m, this + 0x28) {
        let chunk = size.wrapping_add(0xfff) & !0xfff;
        let allocator = pg.u32(m, 0x018d_08f4);
        let page = crate::semantic::call_from(
            m,
            c,
            pg,
            frame,
            allocator,
            &[chunk],
            back,
            |m: &mut dyn Mem, c: &mut Cpu| ddnative_base::x86::call_ext(m, c, 0x00da_7d70),
        );
        pg.set_u32(m, this + 0x24, page);
        pg.set_u32(
            m,
            this + 0x28,
            if page == 0 { 0 } else { page.wrapping_add(chunk) },
        );
    }
    let block = pg.u32(m, this + 0x24);
    pg.set_u32(m, this + 0x24, block.wrapping_add(size));
    block
}

/// 0x00FF04C0, `ctx->set_viewport(rect)` (thiscall, `ret 4`, void): take a
/// new viewport rectangle `[left, top, right, bottom]` (four `i32`) into the
/// draw context and rebuild its viewport constant block.
///
/// Nothing happens when the rectangle equals the stored one (`this + 0xCC`).
/// Otherwise it is stored at `this + 0xCC` and, halved toward zero, at
/// `this + 0xDC` (setting bit 3 of the mode flags at `+0xC8`); then, when the
/// render target size (`u16`s at `+0x10C`/`+0x10E`) is not zero, the flag
/// word selects the halved rectangle (bit 9) or the full one, and a new
/// constant block (the previous one at `+0xC90` copied into fresh arena
/// space) receives the viewport's offset and scale terms:
/// `[ox + 1, -(oy + 1), sx, -sy]` with `sx = width / target_w * 0.5`,
/// `ox = left / target_w / sx` (likewise `y`) and, at `+0x20`/`+0x24`, the
/// reciprocals of the full rectangle's width and height.
// Translated from 0x00FF04C0 (DDDA.exe 2017-12-13 Steam)
pub fn set_viewport_rect(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let rect = pg.u32(m, entry.wrapping_add(4));
    c.esp = entry.wrapping_add(8);
    let field = |offset: u32| this.wrapping_add(offset);
    let word = |m: &dyn Mem, a: u32| pg.u32(m, a);
    let unchanged = word(m, rect + 8) == word(m, field(0xd4))
        && word(m, rect + 4) == word(m, field(0xd0))
        && word(m, rect) == word(m, field(0xcc))
        && word(m, rect + 12) == word(m, field(0xd8));
    if unchanged {
        return;
    }
    for k in 0..4 {
        let v = word(m, rect + 4 * k);
        pg.set_u32(m, field(0xcc + 4 * k), v);
    }
    for k in 0..4 {
        let v = word(m, rect + 4 * k);
        pg.set_u32(m, field(0xdc + 4 * k), v);
    }
    // halve toward zero: (v - (v >> 31)) >> 1
    let halve = |v: i32| (v - (v >> 31)) >> 1;
    for offset in [0xdc, 0xe0] {
        let v = halve(word(m, field(offset)) as i32);
        pg.set_u32(m, field(offset), v as u32);
    }
    let mode = word(m, field(0xc8)) | 8;
    pg.set_u32(m, field(0xc8), mode);
    let mode = word(m, field(0xc8));
    for offset in [0xe4, 0xe8] {
        let v = halve(word(m, field(offset)) as i32);
        pg.set_u32(m, field(offset), v as u32);
    }
    let target_width = word(m, field(0x10c)) & 0xffff;
    if target_width == 0 {
        return;
    }
    let target_height = pg.u16(m, field(0x10e)) as u32;
    if target_height == 0 {
        return;
    }
    let base = if mode & 0x200 != 0 { 0xdc } else { 0xcc };
    let (left, top, right, bottom) = (
        word(m, field(base)) as i32,
        word(m, field(base + 4)) as i32,
        word(m, field(base + 8)) as i32,
        word(m, field(base + 12)) as i32,
    );
    let half = f32::from_bits(word(m, 0x0140_6cf0));
    let scale_x = ((right.wrapping_sub(left)) as f32 / target_width as i32 as f32) * half;
    let offset_x = (left as f32 / target_width as i32 as f32) / scale_x;
    let descriptor = word(m, word(m, field(0x18)).wrapping_add(0x54c));
    let size = ((pg.u16(m, descriptor + 0x18) as u32) * 4 + 0xf) & !0xf;
    let scale_y = ((bottom.wrapping_sub(top)) as f32 / target_height as i32 as f32) * half;
    let offset_y = (top as f32 / target_height as i32 as f32) / scale_y;
    // The original's frame at its allocator call: entry - 0x18 - 3 pushes.
    let block = arena_take(m, c, pg, entry.wrapping_sub(0x24), this, size, 0x00ff_069f);
    let previous = word(m, field(0xc90));
    let bytes = (pg.u16(m, descriptor + 0x18) as u32) * 4;
    copy_guest(m, pg, block, previous, bytes);
    pg.set_u32(m, field(0xc90), block);
    let dirty = word(m, field(0x154)) | 1;
    pg.set_u32(m, field(0x154), dirty);
    if block != 0 {
        let one = f32::from_bits(word(m, 0x0140_6ce0));
        let put =
            |m: &mut dyn Mem, offset: u32, v: f32| pg.set_u32(m, block.wrapping_add(offset), v.to_bits());
        put(m, 0, offset_x + one);
        put(m, 4, -(offset_y + one));
        put(m, 8, scale_x);
        put(m, 0xc, -scale_y);
        let width = word(m, rect + 8).wrapping_sub(word(m, rect)) as i32;
        let height = word(m, rect + 12).wrapping_sub(word(m, rect + 4)) as i32;
        put(m, 0x20, one / width as f32);
        put(m, 0x24, one / height as f32);
    }
    c.esp = entry.wrapping_add(8);
}

fn read64(m: &dyn Mem, pg: Pages, address: u32) -> u64 {
    pg.u32(m, address) as u64 | (pg.u32(m, address.wrapping_add(4)) as u64) << 32
}

fn read_f64(m: &dyn Mem, pg: Pages, address: u32) -> f64 {
    f64::from_bits(read64(m, pg, address))
}

/// A 128-bit table row as two `u64` lanes.
fn read128(m: &dyn Mem, pg: Pages, address: u32) -> [u64; 2] {
    [read64(m, pg, address), read64(m, pg, address.wrapping_add(8))]
}

/// 0x012F45C4, the CRT `logf` (argument in XMM0 as `f32`, result in XMM0):
/// the natural logarithm of a single-precision float, evaluated in double
/// precision from a 64-entry table and a short polynomial.
///
/// The argument's magnitude bits give the table row and the binary
/// exponent; negative numbers and NaNs return NaN, zero returns the CRT's
/// -infinity constant. Denormal arguments are scaled by a constant first.
/// All constants and tables are read from guest memory (0x01454840..),
/// bit for bit as the original.
// Translated from 0x012F45C4 (DDDA.exe 2017-12-13 Steam)
pub fn crt_logf(m: &dyn Mem, pg: Pages, x: f32) -> f32 {
    const ZERO_RESULT: u32 = 0x0145_4ca0;
    let bits = x.to_bits();
    let abs = bits & pg.u32(m, 0x0145_4ca8);
    let mantissa_mask = read64(m, pg, 0x0145_4c80);
    let exponent_bits = read64(m, pg, 0x0145_4c88);
    let mut word = (abs >> 16) & 0xffff;
    let mut shifted = mantissa_mask & ((abs as u64) << 29) | exponent_bits;
    let mut exponent_source = abs;
    let mut bias = 0x3f3f_u32;
    let range = (word.wrapping_sub(0x80)) | (0x7f7f_u32.wrapping_sub(word));
    if range >= 0x8000 {
        if bits < 0x8000_0000 {
            if bits >= 0x7f80_0000 {
                return x + x;
            }
            if bits == 0 {
                return f32::from_bits(pg.u32(m, ZERO_RESULT));
            }
            // positive denormal: scale into the normal range
            let scaled = f32::from_bits(abs) * f32::from_bits(pg.u32(m, 0x0145_4c98));
            exponent_source = scaled.to_bits();
            word = exponent_source >> 16;
            shifted = mantissa_mask & (scaled as f64).to_bits() | exponent_bits;
            bias = 0x5f3f;
        } else {
            if bits == 0x8000_0000 {
                return f32::from_bits(pg.u32(m, ZERO_RESULT));
            }
            if bits & 0x7fff_ffff >= 0x7f80_0001 {
                return x + x;
            }
            return x + f32::from_bits(pg.u32(m, 0x0145_4c9c));
        }
    }
    let row = (((word & 0x7f) + 1) & 0xfe) * 4;
    let base = read_f64(m, pg, 0x0145_4a70);
    let reduced = f64::from_bits(shifted) * read_f64(m, pg, 0x0145_4a70 + row) - base;
    let exponent = ((((exponent_source >> 16) as i32).wrapping_sub(bias as i32)) >> 7) as f64;
    let table_log = read_f64(m, pg, 0x0145_4860 + row);
    let mut p = read_f64(m, pg, 0x0145_4840) * reduced;
    let square = reduced * reduced;
    p += read_f64(m, pg, 0x0145_4848);
    let scaled_exponent = exponent * read_f64(m, pg, 0x0145_4c90);
    let cubic = reduced * square;
    let mut q = read_f64(m, pg, 0x0145_4850) * square;
    q += reduced;
    let cubic = cubic * p;
    let head = table_log + scaled_exponent;
    q += cubic;
    (head + q) as f32
}

/// 0x012F439E, the CRT `log` (argument in XMM0 as `f64`, result in XMM0):
/// the natural logarithm of a double, from a 128-row table pair and a
/// packed polynomial (two lanes: the high lane carries correction terms
/// that are added last).
///
/// Zero returns the CRT's -infinity constant; negative numbers and
/// -infinity return the default NaN; NaN and +infinity are returned as
/// `x + x` / the infinity constant; denormals are scaled by 2^52 and
/// evaluated with the exponent biased by -52. All constants and tables
/// are read from guest memory (0x01453F50..), bit for bit as the original.
// Translated from 0x012F439E (DDDA.exe 2017-12-13 Steam)
pub fn crt_log(m: &dyn Mem, pg: Pages, input: f64) -> f64 {
    let f = f64::from_bits;
    let mut x = input;
    let mut bias_adjust = 0i32;
    loop {
        let bits = x.to_bits();
        let sign_exponent = ((bits >> 52) & 0xffff) as u32;
        let mask = read128(m, pg, 0x0145_3f50);
        let one_bits = read128(m, pg, 0x0145_3fb0);
        let rounder = read128(m, pg, 0x0145_3f60);
        let high_mask = read128(m, pg, 0x0145_3f70);
        // the mantissa with the exponent of 1.0, in both lanes
        let mantissa = [bits & mask[0] | one_bits[0], bits & mask[1] | one_bits[1]];
        let biased = f(rounder[0]) + f(mantissa[0]);
        let row = ((biased.to_bits() & 0xffff) as u32) & 0x7f0;
        let scale = read128(m, pg, 0x0145_4020 + row);
        let log_row = read128(m, pg, 0x0145_4430 + row);
        let head = [high_mask[0] & mantissa[0], high_mask[1] & mantissa[1]];
        let tail = [f(mantissa[0]) - f(head[0]), f(mantissa[1]) - f(head[1])];
        let head_scaled = [
            f(head[0]) * f(scale[0]) - f(one_bits[0]),
            f(head[1]) * f(scale[1]) - f(one_bits[1]),
        ];
        // only the low lane of the table row takes the head term
        let row_low = f(log_row[0]) + head_scaled[0];
        let tail_scaled = [tail[0] * f(scale[0]), tail[1] * f(scale[1])];
        let r = [tail_scaled[0] + head_scaled[0], tail_scaled[1] + head_scaled[1]];
        let ecx = (sign_exponent & 0xfff).wrapping_sub(1);
        if ecx > 0x7fd {
            // zero, denormal, negative, infinity, NaN
            let zero = read128(m, pg, 0x0145_3fc0);
            if f(zero[0]) == x {
                return read_f64(m, pg, 0x0145_3fe0);
            }
            if ecx == u32::MAX {
                x *= read_f64(m, pg, 0x0145_3fd0);
                bias_adjust = -0x34;
                continue;
            }
            if ecx > 0x7fe {
                if (ecx.wrapping_add(1) & 0x7ff) < 0x7ff {
                    return f64::from_bits(0xfff8_0000_0000_0000);
                }
                if (bits as u32) | ((bits >> 32) as u32 & 0xfffff) == 0 {
                    return f64::from_bits(0xfff8_0000_0000_0000);
                }
                return x + x;
            }
            if f(one_bits[0]) == f(bits & mask[0] | one_bits[0]) {
                return read_f64(m, pg, 0x0145_3fe8);
            }
            return x + x;
        }
        let exponent = ecx.wrapping_sub(0x3fe).wrapping_add(bias_adjust as u32) as i32;
        let exponent_f = exponent as f64;
        let row_sum = row.wrapping_add((exponent as u32) << 10);
        let select_offset = if row_sum == 0 { 0x10 } else { 0 };
        let c3 = read128(m, pg, 0x0145_3ff0);
        let c4 = read128(m, pg, 0x0145_4000);
        let c5 = read128(m, pg, 0x0145_4010);
        let c_exp = read128(m, pg, 0x0145_3f80);
        let select = read128(m, pg, 0x0145_3f90 + select_offset);
        let mut square = [r[0] * r[0], r[1] * r[1]];
        let poly = [f(c3[0]) * r[0] + f(c4[0]), f(c3[1]) * r[1] + f(c4[1])];
        square[0] *= square[0]; // MULSD: low lane only
        let exp_part = [exponent_f * f(c_exp[0]), exponent_f * f(c_exp[1])];
        let masked = [
            tail_scaled[0].to_bits() & select[0],
            tail_scaled[1].to_bits() & select[1],
        ];
        let mut acc = [row_low + exp_part[0], f(log_row[1]) + exp_part[1]];
        acc = [acc[0] + f(masked[0]), acc[1] + f(masked[1])];
        let poly = [poly[0] * r[0], poly[1] * r[1]];
        square[0] *= r[0]; // MULSD: low lane only
        let poly = [poly[0] + f(c5[0]), poly[1] + f(c5[1])];
        let high = acc[1];
        let product = [poly[0] * square[0], poly[1] * square[1]];
        return product[0] + product[1] + high + acc[0];
    }
}

/// 0x00F11610, `renderer->draw_with_resources(ctx, group, kind)` (thiscall,
/// `ret 0xc`, void): bind three resources of a draw group into the draw
/// context, commit the primitive state and append one draw packet.
///
/// The renderer counts its draws (`this + 0x198`) and the count, in the
/// high bits, becomes the context's sort key (`ctx + 0x164`). The three
/// resource slots (`ctx + 0x1F40`, `+0x1F48`, `+0x1F58`) are bound from the
/// renderer's tables (`this + 0x80` -> +0x60 for the first, `this + 0xAB4`
/// for the others, row `kind * 0x54`, column `kind * 0x15 + group`) like
/// the model upload does (see `Upload::bind_resource`). The first
/// argument of the material call 0x00FEFB90 comes from the fourth table
/// entry; then 0x00FEEF20 (primitive state flush, called natively) and,
/// unless the context only collects shadows (mode bits 0x50 without 0x100),
/// a 32-byte draw packet and its sort slot are appended; 0x00FF1920
/// submits.
// Translated from 0x00F11610 (DDDA.exe 2017-12-13 Steam)
pub fn draw_with_resources(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let (ctx, group, kind) = (arg(m, 0), arg(m, 1), arg(m, 2));
    // The original's frame at its calls: four saved registers.
    let frame = entry.wrapping_sub(16);
    let count = pg.u32(m, this.wrapping_add(0x198));
    pg.set_u32(m, this.wrapping_add(0x198), count.wrapping_add(1));
    let sort = (count << 5) | (pg.u32(m, ctx.wrapping_add(0x164)) & 0x1f);
    pg.set_u32(m, ctx.wrapping_add(0x164), sort);
    let row = kind.wrapping_mul(0x15);
    let first_table = pg.u32(m, pg.u32(m, this.wrapping_add(0x80)).wrapping_add(0x60));
    let first = pg.u32(
        m,
        first_table
            .wrapping_add(0x10)
            .wrapping_add(row.wrapping_add(group).wrapping_mul(4)),
    );
    let mut u = Upload {
        m,
        c,
        pg,
        frame,
        ctx,
        model: 0,
    };
    u.bind_resource(0x1f40, first, 0x2d4e_f000, 0x3a9);
    let base = kind.wrapping_mul(0x54);
    let table = u.word(this.wrapping_add(0xab4));
    let column = u.word(table.wrapping_add(base).wrapping_add(8)).wrapping_add(row);
    let second = u.word(table.wrapping_add(0x24).wrapping_add(column.wrapping_mul(4)));
    u.bind_resource(0x1f48, second, 0xb855_5000, 0x3aa);
    let table = u.word(this.wrapping_add(0xab4));
    let third = u.word(table.wrapping_add(base).wrapping_add(0x14));
    u.bind_resource(0x1f58, third, 0x8206_0000, 0x3ac);
    let table = u.word(this.wrapping_add(0xab4));
    let column = u
        .word(table.wrapping_add(base).wrapping_add(0xc))
        .wrapping_add(row);
    let object = u.word(table.wrapping_add(0x24).wrapping_add(column.wrapping_mul(4)));
    let material = u.word(u.word(object.wrapping_add(0x44)));
    u.call(0x00fe_fb90, ctx, &[material, 0, 0, 0], 0x00f1_18bb);
    let (m, c) = (u.m, u.c);
    crate::semantic_draw::flush_primitive_state(m, c, pg, frame, ctx);
    let mode = pg.u32(m, ctx.wrapping_add(0x5c));
    if mode & 0x100 != 0 || mode & 0x50 == 0 {
        if pg.u32(m, ctx.wrapping_add(0x24)).wrapping_add(0x20) > pg.u32(m, ctx.wrapping_add(0x28)) {
            crate::semantic::call_from(
                m,
                c,
                pg,
                frame,
                ctx,
                &[0x20],
                0x00f1_18e8,
                |m: &mut dyn Mem, c: &mut Cpu| ddnative_base::x86::call_ext(m, c, 0x00fe_d890),
            );
        }
        let packet = pg.u32(m, ctx.wrapping_add(0x24));
        pg.set_u32(m, ctx.wrapping_add(0x24), packet.wrapping_add(0x20));
        let head = pg.u32(m, packet) & 0xfff0_ffff;
        let serial = pg.u32(m, ctx.wrapping_add(0x74));
        let head = ((serial ^ head) & 0x7fff) ^ head;
        pg.set_u32(m, ctx.wrapping_add(0x74), serial.wrapping_add(1));
        let transparent = (pg.u8(m, ctx.wrapping_add(0x5c)) & 0x50 != 0) as u32;
        let head = head ^ (((transparent << 15) ^ head) & 0x8000);
        let head = (pg.u32(m, ctx.wrapping_add(0x6c)) << 20) | (head & 0xfffff);
        pg.set_u32(m, packet, head);
        let fields = [(8, 0x194), (0xc, 0x198), (0x10, 0x19c), (4, 0x1a0)];
        for (offset, field) in fields {
            let v = pg.u32(m, ctx.wrapping_add(field));
            pg.set_u32(m, packet.wrapping_add(offset), v);
        }
        pg.set_u32(m, packet.wrapping_add(0x14), 4);
        pg.set_u32(m, packet.wrapping_add(0x18), 0);
        let index = pg.u32(m, ctx.wrapping_add(0x48));
        let slot = pg
            .u32(m, ctx.wrapping_add(0x44))
            .wrapping_add(index.wrapping_mul(8));
        pg.set_u32(m, ctx.wrapping_add(0x48), index.wrapping_add(1));
        let key = pg.u32(m, ctx.wrapping_add(0x164));
        let first = (pg.u32(m, slot) & 0x7ff_ffff) | (key << 27);
        pg.set_u32(m, slot, first);
        let order =
            (pg.u32(m, ctx.wrapping_add(0x164)) >> 5).wrapping_add(pg.u32(m, ctx.wrapping_add(0x178)));
        pg.set_u32(m, slot, ((order ^ first) & 0x7ff_ffff) ^ first);
        pg.set_u32(m, slot.wrapping_add(4), packet);
    }
    crate::semantic::call_from(
        m,
        c,
        pg,
        frame,
        ctx,
        &[],
        0x00f1_19b1,
        |m: &mut dyn Mem, c: &mut Cpu| ddnative_base::x86::call_ext(m, c, 0x00ff_1920),
    );
    c.esp = entry.wrapping_add(4 + 0xc);
}

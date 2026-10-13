//! L184 semantic ports (s9): the render-state stack pop (0x00FED5D0) and
//! the per-mesh model draw (0x00EA5B50). See crate::semantic for the
//! ABI-level contract.
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

/// Copy `len` bytes from `src` to `dst` front to back, as the CRT memcpy
/// (and `rep movsd`) does: whole runs inside a page are moved in bulk, the
/// rest dword by dword through the memory trait.
fn copy_guest(m: &mut dyn Mem, pg: Pages, mut dst: u32, mut src: u32, mut len: u32) {
    while len != 0 {
        let room = |a: u32| 0x1000 - (a & 0xfff);
        let chunk = len.min(room(src)).min(room(dst));
        if let (Some(from), Some(to)) = (pg.bytes(src, chunk as usize, 1), pg.bytes(dst, chunk as usize, 2)) {
            // SAFETY: `bytes` returned `chunk` bytes inside backed pages.
            unsafe { std::ptr::copy(from, to, chunk as usize) };
        } else {
            for k in 0..chunk {
                let b = pg.u8(m, src.wrapping_add(k));
                pg.set_u8(m, dst.wrapping_add(k), b);
            }
        }
        src = src.wrapping_add(chunk);
        dst = dst.wrapping_add(chunk);
        len -= chunk;
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
    let saved = this.wrapping_add(depth.wrapping_mul(STATE_SIZE)).wrapping_add(0x81f8);
    let current = this.wrapping_add(0xb4);
    if full {
        let slot_count = pg.u32(m, this.wrapping_add(0x14));
        copy_guest(m, pg, current, saved, slot_count.wrapping_mul(8).wrapping_add(BASE_BLOCK));
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
        crate::semantic::call_from(self.m, self.c, pg, frame, this, args, back, move |m: &mut dyn Mem, c: &mut Cpu| {
            ddnative_base::x86::call_ext(m, c, target)
        })
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
                    self.pg.set_u32(self.m, block.wrapping_add(16 * column + 4 * row), v);
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
        let second_pass = (self.model(0x108) >> 30) & 1 != 0 && self.pg.u16(self.m, self.ctx.wrapping_add(0x16a)) & 0x208 == 0;
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
    let mut u = Upload { m, c, pg, frame, ctx, model };
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

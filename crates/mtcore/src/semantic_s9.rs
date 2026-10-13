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
        let block = crate::semantic::call_from(m, c, pg, frame, page_allocator, &[chunk], 0x00ff_1372, |m: &mut dyn Mem, c: &mut Cpu| {
            ddnative_base::x86::call_ext(m, c, 0x00da_7d70)
        });
        pg.set_u32(m, field(0x24), block);
        pg.set_u32(m, field(0x28), if block == 0 { 0 } else { block.wrapping_add(chunk) });
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
        crate::semantic::call_from(self.m, self.c, pg, frame, this, args, back, move |m: &mut dyn Mem, c: &mut Cpu| {
            ddnative_base::x86::call_ext(m, c, target)
        });
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
            if self.passes_filter(object, passbit) && selected < self.max && self.word(object.wrapping_add(0x1e0)) != 0 {
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
                self.set_ctx(0x1fc + wanted_slot * 8, ((wanted ^ different) & 0xffff_f000) | source);
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
    let mut s = Collect { m, c, pg, frame, scene, ctx, mask, max, probe };
    // The selection array (nine slots) starts empty.
    for i in 0..9 {
        s.set_selected(i, 0);
    }
    s.set_count(0);
    s.select_first_list(passbit);
    let selection = frame.wrapping_add(SELECTED);
    let count_address = frame.wrapping_add(COUNT);
    s.virtual_call(scene, 0x64, &[ctx, mask, max, selection, count_address, 0], 0x00fe_b2da);
    let index = if pg.u8(s.m, scene.wrapping_add(0xb5c)) != 0 { s.scene(0xb60) } else { 0 };
    if pg.u8(s.m, scene.wrapping_add(0xb5c)) != 0 && index != 0 {
        // The index query's argument block lives in this frame.
        let block = frame.wrapping_add(0x2c);
        let query = frame.wrapping_add(0x40);
        let radius_constant = s.word(0x0162_adb0);
        let (p0, p1, p2, p3) = (s.word(probe), s.word(probe + 4), s.word(probe + 8), s.word(probe + 12));
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
        s.guest_call(0x0117_87c0, index, &[query, scene, 0x00fe_5690, block, 0, 0xffff_ffff], 0x00fe_b362);
        let found = s.word(frame.wrapping_add(0x38));
        s.set_count(found);
    } else {
        s.select_second_list(passbit);
    }
    s.virtual_call(scene, 0x64, &[ctx, mask, max, selection, count_address, 1], 0x00fe_b44d);
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

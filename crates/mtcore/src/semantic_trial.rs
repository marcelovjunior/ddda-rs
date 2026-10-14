//! L191 trial port: `uBaseModel::vf25`, the model upload state routine
//! (0x00EA5B50). See crate::semantic for the ABI-level contract.
//!
//! The render state (`this`) keeps eight "channels". Each channel is a pair
//! of words (a key and a tag) that remembers what the GPU side was last told.
//! Every frame the routine compares a channel with the value the owner
//! currently supplies. On a change, the old pair is appended to the record
//! list (12-byte records at +0x30C7C, count at +0x6C) when the record flag
//! (+0x5C bit 0x80) is set, and the channel takes the new pair. Dirty bits
//! at +0x154 note which channels changed. Two channels also carry a block
//! of model data (a 4x4 matrix or a descriptor-sized copy) bump-allocated
//! from +0x24 (cursor) below +0x28 (end).
use crate::semantic::{call_from, call_guest, copy_guest};
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

// Render state fields (offsets from `this`).
const ST_DESCRIPTORS: u32 = 0x18; // -> object whose +0x558 / +0x89c hold buffer descriptors
const ST_OWNER: u32 = 0x20; // -> owner supplying the channel keys
const ST_CURSOR: u32 = 0x24; // bump allocator: next free byte
const ST_CURSOR_END: u32 = 0x28;
const ST_FLAGS: u32 = 0x5c; // 0x80: record changes, 0x02: block in progress
const ST_RECORD_COUNT: u32 = 0x6c;
const ST_RECORDS: u32 = 0x30c7c; // 12-byte records: header, word 1, word 2
const ST_DIRTY: u32 = 0x154;
const ST_FILTER: u32 = 0x16a; // u16 mask tested against the pass filter
const ST_PARENT_KEYS: u32 = 0x1d4; // object supplying the 0x210 and 0x264 keys
const ST_FADE_BLOCK: u32 = 0x1330; // (block, aux) of the data-block channel
const ST_MATRIX_BLOCK: u32 = 0xca8; // block holding the model matrix (+ its size at 0xcac)

// Channels: (key word, tag word) pairs.
const CH_ALPHA: u32 = 0x2868;
const CH_KAPPA: u32 = 0x30a0;
const CH_BETA: u32 = 0x1c78;
const CH_ZETA: u32 = 0x1aa0;
const CH_EPSILON: u32 = 0x2eb0;
const CH_GAMMA: u32 = 0x1cf0;
const CH_IOTA: u32 = 0x30d8;
const CH_LAMBDA: u32 = 0x2248;

// Model object fields.
const MODEL_MATRIX: u32 = 0xb0; // 4x4 floats, 0x10-byte rows
const MODEL_PARENT: u32 = 0x30;
const MODEL_TEXTURE: u32 = 0x100;
const MODEL_FLAGS_A: u32 = 0x104; // bit 2: take the bounds of the parent, bit 4: mode 7
const MODEL_FLAGS_B: u32 = 0x108; // bit 31: static (no matrix), bit 30: second block, bits 25..28: index
const MODEL_FADE: u32 = 0x158;
const MODEL_OBJECT: u32 = 0x160;
const MODEL_BOUNDS: u32 = 0x1c0; // two 8-byte bounds words

// Globals.
const GLOBAL_DEVICE: u32 = 0x018d_2c80; // object with the device's vtable and keys
const GLOBAL_SERIAL: u32 = 0x018d_2ce8; // serial stamped into key objects
const GLOBAL_STAMP: u32 = 0x0140_6ce0; // stamp written into the second block

// Guest functions.
const FN_ALLOC: u32 = 0x00fe_d890;
const FN_BOUNDS_CALL: u32 = 0x00fe_66a0;
const FN_SET_FADE: u32 = 0x00ff_1310;

/// The frame's copy of the model matrix, column by column (column-major).
const MATRIX_LOCAL: u32 = 0x30;
/// The bounds words passed to the first guest call.
const BOUNDS_LOCAL: u32 = 0x20;

/// The record flag: the byte at +0x5c bit 0x80.
fn records_on(up: &Upload) -> bool {
    up.pg.u8(&*up.m, up.st + ST_FLAGS) & 0x80 != 0
}

/// Working state of one call: the render state, the model, and the frame
/// the original keeps its locals in (ESP after its prologue).
struct Upload<'a> {
    m: &'a mut dyn Mem,
    pg: Pages,
    st: u32,
    model: u32,
    frame: u32,
}

impl Upload<'_> {
    fn word(&self, addr: u32) -> u32 {
        self.pg.u32(&*self.m, addr)
    }

    fn st_word(&self, offset: u32) -> u32 {
        self.word(self.st + offset)
    }

    fn set_st(&mut self, offset: u32, value: u32) {
        self.pg.set_u32(self.m, self.st + offset, value);
    }

    fn or_st(&mut self, offset: u32, bits: u32) {
        let value = self.st_word(offset) | bits;
        self.set_st(offset, value);
    }

    fn and_st(&mut self, offset: u32, mask: u32) {
        let value = self.st_word(offset) & mask;
        self.set_st(offset, value);
    }

    /// Appends the record `(header, a, b)` and counts it.
    fn emit(&mut self, header: u32, a: u32, b: u32) {
        let count = self.st_word(ST_RECORD_COUNT);
        let record = self.st.wrapping_add(ST_RECORDS).wrapping_add(count.wrapping_mul(12));
        self.pg.set_u32(self.m, record, header);
        self.pg.set_u32(self.m, record + 4, a);
        self.pg.set_u32(self.m, record + 8, b);
        self.set_st(ST_RECORD_COUNT, count + 1);
    }

    /// Records the channel's old pair (when records are on) under `header`.
    fn record_old(&mut self, slot: u32, header: u32) {
        if records_on(self) {
            let (a, b) = (self.st_word(slot), self.st_word(slot + 4));
            self.emit(header, a, b);
        }
    }

    /// A channel whose tag is a constant: when `key` differs from the
    /// channel's key, switch to `(key, tag)` and set dirty bits 1 and 2.
    fn constant_channel(&mut self, slot: u32, header: u32, key: u32, tag: u32) {
        if self.st_word(slot) == key {
            return;
        }
        self.record_old(slot, header);
        self.set_st(slot, key);
        self.set_st(slot + 4, tag);
        self.or_st(ST_DIRTY, 3);
    }

    /// A channel whose key is a refcounted object (or null): the tag is
    /// derived from the key's word at +0xc. The key object gets the next
    /// serial at +4. Dirty bit 2 only when the tag changes; bit 1 always.
    fn object_channel(&mut self, slot: u32, header: u32, key: u32, tag_bits: u32) {
        if self.st_word(slot) == key {
            return;
        }
        self.record_old(slot, header);
        let tag = if key == 0 {
            self.set_st(slot, 0);
            0
        } else {
            let serial = self.word(GLOBAL_SERIAL).wrapping_add(1);
            self.pg.set_u32(self.m, key + 4, serial);
            self.set_st(slot, key);
            self.word(key + 0xc) | tag_bits
        };
        if self.st_word(slot + 4) != tag {
            self.set_st(slot + 4, tag);
            self.or_st(ST_DIRTY, 2);
        }
        self.or_st(ST_DIRTY, 1);
    }

    /// Size of a block described by the descriptor at `desc` (its u16 word
    /// count at +0x18, four bytes each, rounded up to 16).
    fn block_size(&self, desc: u32) -> u32 {
        let words = self.pg.u16(&*self.m, desc + 0x18) as u32;
        (words * 4 + 0xf) & !0xf
    }

    /// Bump-allocates `size` bytes from the cursor. When the block does not
    /// fit, the render state's allocator (0xFED890) makes room first, with
    /// `back` as its return address. Returns the block's start.
    fn reserve(&mut self, c: &mut Cpu, size: u32, back: u32) -> u32 {
        let cursor = self.st_word(ST_CURSOR);
        if cursor.wrapping_add(size) > self.st_word(ST_CURSOR_END) {
            let (frame, this) = (self.frame, self.st);
            call_from(self.m, c, self.pg, frame, this, &[size], back, |m, c| {
                call_guest(m, c, FN_ALLOC)
            });
        }
        let block = self.st_word(ST_CURSOR);
        self.set_st(ST_CURSOR, block.wrapping_add(size));
        block
    }

    /// The matrix part of the upload: a fresh block receives the first three
    /// rows of the model matrix, transposed through the frame's copy.
    fn upload_matrix(&mut self, c: &mut Cpu) {
        let descriptor = self.word(self.st_word(ST_DESCRIPTORS) + 0x558);
        let size = self.block_size(descriptor);
        let block = self.reserve(c, size, 0xea5d32);
        self.set_st(ST_MATRIX_BLOCK, block);
        self.or_st(ST_DIRTY, 1);
        if block == 0 {
            return;
        }
        for k in 0..16 {
            let (column, row) = (k / 4, k % 4);
            let v = self.word(self.model + MODEL_MATRIX + 0x10 * row + 4 * column);
            self.pg.set_u32(self.m, self.frame + MATRIX_LOCAL + 4 * k, v);
        }
        for k in 0..12 {
            let v = self.word(self.frame + MATRIX_LOCAL + 4 * k);
            self.pg.set_u32(self.m, block + 4 * k, v);
        }
    }

    /// Copies the 8-byte word at `from` to `to` (two dwords).
    fn copy_pair(&mut self, from: u32, to: u32) {
        let low = self.word(from);
        let high = self.word(from + 4);
        self.pg.set_u32(self.m, to, low);
        self.pg.set_u32(self.m, to + 4, high);
    }

    /// The bounds the first guest call sees (a frame copy of the model's
    /// bounds, or of its parent's when the model asks for them), then the
    /// two calls: the device's `0xFE66A0` and the vtable slot 13 method.
    fn bounds_calls(&mut self, c: &mut Cpu) {
        let (model, frame) = (self.model, self.frame);
        self.copy_pair(model + MODEL_BOUNDS, frame + BOUNDS_LOCAL);
        self.copy_pair(model + MODEL_BOUNDS + 8, frame + BOUNDS_LOCAL + 8);
        let flags = self.word(model + MODEL_FLAGS_A);
        if flags & 2 != 0 {
            let parent = self.word(model + MODEL_PARENT);
            if parent != 0 {
                self.copy_pair(parent + MODEL_BOUNDS, frame + BOUNDS_LOCAL);
                self.copy_pair(parent + MODEL_BOUNDS + 8, frame + BOUNDS_LOCAL + 8);
            }
        }
        let mode = if flags & 4 != 0 { 7 } else { 5 };
        let device = self.word(GLOBAL_DEVICE);
        let texture = self.word(model + MODEL_TEXTURE);
        let st = self.st;
        call_from(
            self.m,
            c,
            self.pg,
            frame,
            device,
            &[st, texture, mode, frame + BOUNDS_LOCAL],
            0xea60d4,
            |m, c| call_guest(m, c, FN_BOUNDS_CALL),
        );
        let device = self.word(GLOBAL_DEVICE);
        let method = self.word(self.word(device) + 0x34);
        let texture = self.word(model + MODEL_TEXTURE);
        let bits = (self.word(model + MODEL_FLAGS_B) >> 25) & 0xf;
        call_from(
            self.m,
            c,
            self.pg,
            frame,
            device,
            &[st, texture, bits, model + MODEL_OBJECT],
            0xea60fd,
            |m, c| call_guest(m, c, method),
        );
    }

    /// The second data block (the descriptor at +0x89c, bit 30 of the
    /// model's flags): the previous one is recorded, and the stamp goes into
    /// its last word.
    fn second_block(&mut self, c: &mut Cpu) {
        self.or_st(ST_FLAGS, 2);
        if records_on(self) {
            let (a, b) = (self.st_word(ST_FADE_BLOCK), self.st_word(ST_FADE_BLOCK + 4));
            self.emit(0x227, a, b);
        }
        let descriptor = self.word(self.st_word(ST_DESCRIPTORS) + 0x89c);
        let size = self.block_size(descriptor);
        let block = self.reserve(c, size, 0xea6566);
        self.set_st(ST_FADE_BLOCK, block);
        self.or_st(ST_DIRTY, 1);
        if block != 0 {
            let words = self.pg.u16(&*self.m, descriptor + 0x18) as u32;
            let source = self.word(descriptor + 0x24);
            copy_guest(self.m, self.pg, block, source, words * 4);
        }
        let stamp = self.word(GLOBAL_STAMP);
        self.pg.set_u32(self.m, block + 8, stamp);
        self.and_st(ST_FLAGS, !2);
        let owner = self.st_word(ST_OWNER);
        let epsilon_key = self.word(owner + 0x2cc8);
        self.constant_channel(CH_EPSILON, 0x597, epsilon_key, 0x950a_6599);
        let device = self.word(GLOBAL_DEVICE);
        let gamma_key = self.word(device + 0x2cc);
        self.object_channel(CH_GAMMA, 0x35f, gamma_key, 0x45fb_b000);
    }

    /// The first data block (the descriptor at +0x89c): reserved, filled
    /// from the descriptor's data, and stamped with zero.
    fn first_block(&mut self, c: &mut Cpu) {
        let descriptor = self.word(self.st_word(ST_DESCRIPTORS) + 0x89c);
        let size = self.block_size(descriptor);
        let block = self.reserve(c, size, 0xea6468);
        self.set_st(ST_FADE_BLOCK, block);
        self.or_st(ST_DIRTY, 1);
        if block != 0 {
            let words = self.pg.u16(&*self.m, descriptor + 0x18) as u32;
            let source = self.word(descriptor + 0x24);
            copy_guest(self.m, self.pg, block, source, words * 4);
        }
        self.pg.set_u32(self.m, block + 8, 0);
    }

    fn run(&mut self, c: &mut Cpu) {
        let owner = self.st_word(ST_OWNER);

        // Alpha: the owner's key, chosen by the model's static bit.
        let is_static = self.word(self.model + MODEL_FLAGS_B) & 0x8000_0000 != 0;
        if is_static {
            let key = self.word(owner + 0x2670);
            self.constant_channel(CH_ALPHA, 0x4ce, key, 0x4ce);
        } else {
            let key = self.word(owner + 0x2678);
            self.constant_channel(CH_ALPHA, 0x4ce, key, 0xd3d3_74cf);
            // A changed matrix: record the previous block, reserve the new one.
            self.or_st(ST_FLAGS, 2);
            if records_on(self) {
                let (a, b) = (self.st_word(ST_MATRIX_BLOCK), self.st_word(0xcac));
                self.emit(0x156, a, b);
            }
            self.upload_matrix(c);
            self.and_st(ST_FLAGS, !2);
        }

        let key = self.word(owner + 0x2ea8);
        self.constant_channel(CH_KAPPA, 0x5d5, key, 0x5d5);

        let parent = self.st_word(ST_PARENT_KEYS);
        let key = if parent != 0 { self.word(parent + 0x210) } else { 0 };
        self.object_channel(CH_BETA, 0x350, key, 0xc496_e000);

        if self.pg.u16(&*self.m, self.st + ST_FILTER) as u32 & 0x813 != 0 {
            self.bounds_calls(c);
        }
        let fade = self.word(self.model + MODEL_FADE);
        let (frame, st) = (self.frame, self.st);
        call_from(self.m, c, self.pg, frame, st, &[fade], 0xea6112, |m, c| {
            call_guest(m, c, FN_SET_FADE)
        });

        let parent = self.st_word(ST_PARENT_KEYS);
        let key = if parent != 0 { self.word(parent + 0x264) } else { 0 };
        self.object_channel(CH_ZETA, 0x315, key, 0x377b_6000);

        let key = self.word(owner + 0x2cc8);
        self.constant_channel(CH_EPSILON, 0x597, key, 0x950a_6599);

        let device = self.word(GLOBAL_DEVICE);
        let key = self.word(device + 0x2cc);
        self.object_channel(CH_GAMMA, 0x35f, key, 0x45fb_b000);

        // The first data block, then the second when the model asks for it.
        self.or_st(ST_FLAGS, 2);
        if records_on(self) {
            let (a, b) = (self.st_word(ST_FADE_BLOCK), self.st_word(ST_FADE_BLOCK + 4));
            self.emit(0x227, a, b);
        }
        self.first_block(c);
        self.and_st(ST_FLAGS, !2);
        if self.word(self.model + MODEL_FLAGS_B) & 0x4000_0000 != 0
            && self.pg.u16(&*self.m, self.st + ST_FILTER) as u32 & 0x208 == 0
        {
            self.second_block(c);
        }

        let key = self.word(owner + 0x2ee0);
        self.constant_channel(CH_IOTA, 0x5dc, key, 0x5dc);
        let key = self.word(owner + 0x2050);
        self.constant_channel(CH_LAMBDA, 0x40a, key, 0x40a);
    }
}

// Translated from 0x00EA5B50 (DDDA.exe 2017-12-13 Steam), uBaseModel::vf25.
/// Thiscall with the model in ECX and the render state as the one stack
/// argument (`ret 4`). Writes the channel records and blocks described above
/// and returns with ESP past its argument.
pub fn model_upload_state(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let saved = (c.ebp, c.ebx, c.esi, c.edi);
    // Prologue: the saved EBP, then the frame (16-byte aligned, 0x64 bytes of
    // locals) with EBX, ESI and EDI pushed below it.
    let frame = (entry.wrapping_sub(4) & !0xf).wrapping_sub(0x70);
    let model = c.ecx;
    let st = pg.u32(m, entry.wrapping_add(4));
    pg.set_u32(m, entry.wrapping_sub(4), saved.0);
    pg.set_u32(m, frame + 8, saved.1);
    pg.set_u32(m, frame + 4, saved.2);
    pg.set_u32(m, frame, saved.3);
    pg.set_u32(m, frame + 0x1c, model);

    let mut up = Upload { m, pg, st, model, frame };
    up.run(c);

    // Epilogue: the saved registers come back from the frame.
    c.ebp = up.word(entry.wrapping_sub(4));
    c.ebx = up.word(frame + 8);
    c.esi = up.word(frame + 4);
    c.edi = up.word(frame);
    c.esp = entry.wrapping_add(8);
}

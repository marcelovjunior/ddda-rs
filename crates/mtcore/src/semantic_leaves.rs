//! Small leaf functions (no guest calls) ported from their disassembly. Same ABI-level conventions as
//! `semantic_l190.rs`; oracles in `crates/mtcore/tests/semantic_leaves.rs` (vectors from
//! `research/native/record_leaves.py`). Registers are reproduced as the originals leave them (these
//! are leaves, so a caller could in principle read any of them); EFLAGS only where cheap.
use ddnative_base::{
    mem::{Mem, Pages},
    x86::Cpu,
};

/// A grid of 29 x 29 floats at stride 0x14 inside each 0x41D0-byte record.
const GRID: u32 = 29;
const GRID_STRIDE: u32 = 0x14;
const RECORD_SIZE: u32 = 0x41D0;

// Translated from 0x005a65e0 (DDDA.exe 2017-12-13 Steam).
/// 0x005A65E0, `clear_grid` (register arguments: EAX = record index, ECX = base; plain `ret`, no
/// result): zeroes the first float of every cell of the 29 x 29 grid of record `index`, one cell
/// every 0x14 bytes from `base + index * 0x41D0 + 0x1C`. A start address of zero (the sum wraps
/// to 0) returns at once. Leaves EAX one past the last cell step, ECX = EDX = 0 (EAX = 0 and the
/// others unchanged for the wrapped case). Flags are not reproduced (no caller reads them).
pub fn clear_grid(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let record = c.eax.wrapping_mul(RECORD_SIZE).wrapping_add(c.ecx).wrapping_add(0x10);
    c.eax = record;
    if record != 0 {
        let mut cell = record.wrapping_add(0xC);
        for _ in 0..GRID {
            for _ in 0..GRID {
                pg.set_u32(m, cell, 0);
                cell = cell.wrapping_add(GRID_STRIDE);
            }
        }
        c.eax = cell;
        c.ecx = 0;
        c.edx = 0;
    }
    // plain ret
    c.esp = entry.wrapping_add(4);
}

/// The lane masks of `lanes_in_range`, in the image.
const LANES_FORCE: u32 = 0x0162_ac60;
const LANES_WANT: u32 = 0x0162_ac30;

// Translated from 0x00767b40 (DDDA.exe 2017-12-13 Steam).
/// 0x00767B40, `this->lanes_in_range(other) -> bool` (thiscall, `ret 4`; the result in AL, EAX = 0 or
/// 1): four-lane test of two 16-byte-aligned vector triples. Lane `i` passes when
/// `this[0x10 + i] <= other[0x10 + i]` and `other[i] <= this[0x20 + i]` (a NaN fails). The two masks in
/// the image then decide which lanes count: where `LANES_FORCE` has its bits the lane's outcome is
/// replaced by the matching bits of `LANES_WANT`, and the test is that every lane's (possibly
/// replaced) outcome equals `LANES_WANT` in that lane. With the game's masks the ignored lane is
/// forced to pass. Flags are those of the final `cmp mask, 0xF`.
pub fn lanes_in_range(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let other = pg.u32(m, entry.wrapping_add(4));
    let this = c.ecx;
    let lane = |m: &dyn Mem, base: u32, i: u32| f32::from_bits(pg.u32(m, base.wrapping_add(4 * i)));
    let word = |m: &dyn Mem, base: u32, i: u32| pg.u32(m, base.wrapping_add(4 * i));
    let mut mask = 0u32;
    for i in 0..4 {
        // cmpleps this[0x10] <= other[0x10], cmpleps other[0] <= this[0x20], pand
        let pass = lane(m, this.wrapping_add(0x10), i) <= lane(m, other.wrapping_add(0x10), i)
            && lane(m, other, i) <= lane(m, this.wrapping_add(0x20), i);
        let outcome = if pass { u32::MAX } else { 0 };
        let force = word(m, LANES_FORCE, i);
        let want = word(m, LANES_WANT, i);
        // (~force & outcome) | (force & want), compared with want
        let blended = (!force & outcome) | (force & want);
        if blended == want {
            mask |= 1 << i;
        }
    }
    c.fl.alu_sub(mask, 0xF, 32);
    c.eax = (mask == 0xF) as u32;
    c.esp = entry.wrapping_add(4 + 4);
}

// Translated from 0x004ea200 (DDDA.exe 2017-12-13 Steam).
/// 0x004EA200, `table->find(key) -> u32` (thiscall, `ret 4`): searches the table's `count` (+8)
/// records of five dwords from `records` (+4) for the first equal to the five dwords at `key`, and
/// returns `base` (+0x10) plus its index, or 0 when there is none. EDX = the index reached (0 for an
/// empty table). Flags are those of the last `add`/`xor`.
pub fn find_record(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let key = pg.u32(m, entry.wrapping_add(4));
    let count = pg.u32(m, this.wrapping_add(8));
    let mut index = 0u32;
    let mut found = None;
    if count != 0 {
        let first = pg.u32(m, key);
        let records = pg.u32(m, this.wrapping_add(4));
        let mut record = records;
        while index < count {
            let equal = pg.u32(m, record) == first
                && (1..5).all(|w| pg.u32(m, record.wrapping_add(4 * w)) == pg.u32(m, key.wrapping_add(4 * w)));
            if equal {
                found = Some(index);
                break;
            }
            index += 1;
            record = record.wrapping_add(0x14);
        }
    }
    c.edx = index;
    c.eax = match found {
        Some(i) => {
            let base = pg.u32(m, this.wrapping_add(0x10));
            c.fl.alu_add(base, i, 32)
        }
        None => c.fl.alu_xor(0, 0, 32),
    };
    c.esp = entry.wrapping_add(4 + 4);
}

// Translated from 0x01328fd0 (DDDA.exe 2017-12-13 Steam).
/// 0x01328FD0, `advance_bits(reader, bits)` (cdecl, plain `ret`, no result): moves the bit reader's
/// position by `bits` (signed). With `pos = reader[+4] + bits`: the byte pointer (+0) and the byte
/// count (+0xC) move by `pos / 8` (rounded toward zero) and the bit offset (+4) becomes `pos & 7`.
/// Leaves EAX = that byte step, ECX = the reader, EDX = 7 for a negative position, else 0; flags
/// those of the pointer's `add`.
pub fn advance_bits(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let reader = pg.u32(m, entry.wrapping_add(4));
    let bits = pg.u32(m, entry.wrapping_add(8));
    let position = bits.wrapping_add(pg.u32(m, reader.wrapping_add(4)));
    // cdq; and edx, 7: the bias that makes the arithmetic shift round toward zero
    let bias = if (position as i32) < 0 { 7 } else { 0 };
    let step = (position.wrapping_add(bias) as i32 >> 3) as u32;
    let count = pg.u32(m, reader.wrapping_add(0xC));
    pg.set_u32(m, reader.wrapping_add(0xC), count.wrapping_add(step));
    let pointer = pg.u32(m, reader);
    let moved = c.fl.alu_add(pointer, step, 32);
    pg.set_u32(m, reader, moved);
    pg.set_u32(m, reader.wrapping_add(4), position & 7);
    c.eax = step;
    c.ecx = reader;
    c.edx = bias;
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x00d00690 (DDDA.exe 2017-12-13 Steam).
/// 0x00D00690, `rng->step()` (thiscall, plain `ret`): one step of a xorshift128 generator whose four
/// state words are at +0, +4, +8 and +0xC. With `t = s0 ^ (s0 << 15)` the state shifts down one word
/// (`s0 = s1`, `s1 = s2`, `s2 = s3`) and the new last word is `s3 ^ t ^ (((s3 >> 17) ^ t) >> 4)`.
/// Leaves EAX = the new word, EDX = `t`; ECX unchanged.
pub fn xorshift_step(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let s0 = pg.u32(m, this);
    let t = s0 ^ (s0 << 15);
    let s1 = pg.u32(m, this.wrapping_add(4));
    pg.set_u32(m, this, s1);
    let s2 = pg.u32(m, this.wrapping_add(8));
    pg.set_u32(m, this.wrapping_add(4), s2);
    let s3 = pg.u32(m, this.wrapping_add(0xC));
    let mixed = (((s3 >> 17) ^ t) >> 4) ^ s3;
    pg.set_u32(m, this.wrapping_add(8), s3);
    let next = c.fl.alu_xor(mixed, t, 32);
    pg.set_u32(m, this.wrapping_add(0xC), next);
    c.eax = next;
    c.edx = t;
    c.esp = entry.wrapping_add(4);
}

/// Entries of 0xA0 bytes, indexed by a byte table: the object's entry array (+0x364) and index
/// table (+0x368); an index byte of 0xFF means "none".
const ENTRIES: u32 = 0x364;
const ENTRY_INDEX: u32 = 0x368;
const ENTRY_SIZE: u32 = 0xA0;
const ENTRY_FIRST: u32 = 0x10;

// Translated from 0x00ff9a20 (DDDA.exe 2017-12-13 Steam).
/// 0x00FF9A20, `table->entry_or_default(index) -> entry*` (thiscall, `ret 4`): the entry the low byte
/// of `index` maps to, `this + 0xB0` (the default entry) for a negative index or an unmapped (0xFF)
/// byte. Entries are `ENTRIES[b] * 0xA0 + 0x10` bytes into the array. Leaves EDX = the index table
/// (non-negative index), ECX = the array (mapped entry); flags are not reproduced.
pub fn table_entry_or_default(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let index = pg.u32(m, entry.wrapping_add(4));
    let default = this.wrapping_add(0xB0);
    c.eax = default;
    if (index as i32) >= 0 {
        let table = pg.u32(m, this.wrapping_add(ENTRY_INDEX));
        c.edx = table;
        let slot = pg.u8(m, table.wrapping_add(index & 0xFF)) as u32;
        if slot != 0xFF {
            let array = pg.u32(m, this.wrapping_add(ENTRIES));
            c.ecx = array;
            c.eax = array.wrapping_add(slot * ENTRY_SIZE).wrapping_add(ENTRY_FIRST);
        }
    }
    c.esp = entry.wrapping_add(4 + 4);
}

// Translated from 0x00421ed0 (DDDA.exe 2017-12-13 Steam).
/// 0x00421ED0, `table->entry_or_null(index) -> entry*` (thiscall, `ret 4`): as `table_entry_or_default`
/// for the low byte of `index`, but an unmapped byte gives 0 and the entry is `ENTRIES[b] * 0xA0`
/// past the array (no `+ 0x10`). EDX = the index table; flags are those of the final `xor`/`add`.
pub fn table_entry_or_null(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let index = pg.u32(m, entry.wrapping_add(4));
    let table = pg.u32(m, this.wrapping_add(ENTRY_INDEX));
    c.edx = table;
    let slot = pg.u8(m, table.wrapping_add(index & 0xFF)) as u32;
    c.eax = if slot == 0xFF {
        c.fl.alu_xor(slot, slot, 32)
    } else {
        let offset = slot * ENTRY_SIZE;
        c.fl.alu_add(offset, pg.u32(m, this.wrapping_add(ENTRIES)), 32)
    };
    c.esp = entry.wrapping_add(4 + 4);
}

// Translated from 0x005d8eb0 (DDDA.exe 2017-12-13 Steam).
/// 0x005D8EB0, `object->promote_state()` (thiscall, plain `ret`): of the flag word at +4, unless bit 13
/// is set, a state (bits 0..2) of 1 or 2 becomes 3 and bit 2 is cleared first (`(w & !4) | 3`).
/// Leaves EAX = the word, EDX = `w >> 13` when bit 13 is set, else the state `w & 7`.
pub fn promote_state(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let word = pg.u32(m, this.wrapping_add(4));
    c.eax = word;
    c.edx = word >> 13;
    c.fl.alu_and(c.edx & 0xFF, 1, 8);
    if (word >> 13) & 1 == 0 {
        let state = word & 7;
        c.edx = state;
        c.fl.alu_sub(state, 2, 32);
        let promote = if state == 2 {
            true
        } else {
            c.fl.alu_sub(state, 1, 32);
            state == 1
        };
        if promote {
            let cleared = c.fl.alu_and(word, 0xFFFF_FFFB, 32);
            c.eax = c.fl.alu_or(cleared, 3, 32);
            pg.set_u32(m, this.wrapping_add(4), c.eax);
        }
    }
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x00e0a4e0 (DDDA.exe 2017-12-13 Steam).
/// 0x00E0A4E0, `object->find_entry(key) -> entry*` (thiscall, `ret 4`): follows `this+0xFC` to a holder
/// (0 gives none), its +0x60 to a table whose entry count is at +0x48 and entry array (16-byte
/// entries) at +0xB8, and returns the first entry whose first dword is `key`, or 0. ECX = the
/// index reached (0 if the table was not reached), EDX = the count. Flags those of the final
/// compare or `xor`.
pub fn find_entry_by_key(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let holder = pg.u32(m, this.wrapping_add(0xFC));
    c.eax = 0;
    let mut found = false;
    if holder != 0 {
        let table = pg.u32(m, holder.wrapping_add(0x60));
        let array = pg.u32(m, table.wrapping_add(0xB8));
        let count = pg.u32(m, table.wrapping_add(0x48));
        c.edx = count;
        c.ecx = 0;
        if count != 0 {
            let key = pg.u32(m, entry.wrapping_add(4));
            let mut at = array;
            while c.ecx < count {
                let first = pg.u32(m, at);
                if first == key {
                    c.fl.alu_sub(first, key, 32);
                    c.eax = at;
                    found = true;
                    break;
                }
                c.ecx += 1;
                at = at.wrapping_add(0x10);
            }
        }
    }
    if !found {
        c.eax = c.fl.alu_xor(0, 0, 32);
    }
    c.esp = entry.wrapping_add(4 + 4);
}

// Translated from 0x00de5bd0 (DDDA.exe 2017-12-13 Steam).
/// 0x00DE5BD0, `record->init()` (thiscall, plain `ret`): resets a record to its initial state: vtable
/// 0x01558134, -1 at +4 and +0x1C, zero at +0x18, +0x32 (byte), +0x34 and +0x40, the word 0x100 at +0x48,
/// and the shared empty object 0x016295C0 at +0x4C, +0x50, +0x54 and +0x58. Leaves EAX = 0x016295C0.
pub fn init_record_a(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let minus_one = c.fl.alu_or(c.eax, 0xFFFF_FFFF, 32);
    pg.set_u32(m, this.wrapping_add(4), minus_one);
    pg.set_u32(m, this.wrapping_add(0x1C), minus_one);
    let zero = c.fl.alu_xor(minus_one, minus_one, 32);
    pg.set_u32(m, this.wrapping_add(0x18), zero);
    pg.set_u8(m, this.wrapping_add(0x32), 0);
    pg.set_u32(m, this.wrapping_add(0x34), zero);
    pg.set_u32(m, this.wrapping_add(0x40), zero);
    c.eax = 0x0162_95C0;
    pg.set_u16(m, this.wrapping_add(0x48), 0x100);
    for at in [0x4C, 0x50, 0x54, 0x58] {
        pg.set_u32(m, this.wrapping_add(at), c.eax);
    }
    pg.set_u32(m, this, 0x0155_8134);
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x00de0aa0 (DDDA.exe 2017-12-13 Steam).
/// 0x00DE0AA0, `record->init()` (thiscall, plain `ret`): vtable 0x0142B73C, 0x5253 at +4, 1 at +0x48,
/// zero at +8 (byte), +0x4C, +0x54, +0x58 and +0x5C, and 0x200 at +0x50. Leaves EAX = the record,
/// ECX = 0x200.
pub fn init_record_b(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    c.eax = this;
    c.ecx = 0;
    pg.set_u8(m, this.wrapping_add(0x50), 0);
    for at in [0x4C, 0x54, 0x58, 0x5C] {
        pg.set_u32(m, this.wrapping_add(at), 0);
    }
    pg.set_u8(m, this.wrapping_add(8), 0);
    c.ecx = c.fl.alu_or(0, 0x200, 32);
    pg.set_u32(m, this, 0x0142_B73C);
    pg.set_u32(m, this.wrapping_add(4), 0x5253);
    pg.set_u32(m, this.wrapping_add(0x48), 1);
    pg.set_u32(m, this.wrapping_add(0x50), c.ecx);
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x01105ef0 (DDDA.exe 2017-12-13 Steam).
/// 0x01105EF0, `value->set(new, force)` (thiscall, `ret 8`): when the float at +0xC differs from `new`
/// (or is unordered with it), or `force` (a byte) is 1, sets bit 16 of the flags at +8, moves the
/// old value to +0x10 and stores `new` at +0xC. The original compares with UCOMISS and reads the
/// result through LAHF, so AH of EAX holds the compare's flags afterwards.
pub fn set_float_if_changed(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let old = pg.u32(m, this.wrapping_add(0xC));
    let new = pg.u32(m, entry.wrapping_add(4));
    c.fl = ddnative_base::x86::comiss_hw::<false>(old, new);
    let ah = c.fl.lahf() as u32;
    c.eax = (c.eax & !0xFF00) | (ah << 8);
    // test ah, 0x44; jp: parity of the two compare bits, even when both or neither are set
    c.fl.alu_and(ah, 0x44, 8);
    let differs = (ah & 0x44).count_ones() % 2 == 0;
    let force = pg.u8(m, entry.wrapping_add(8)) as u32;
    let store = differs || {
        c.fl.alu_sub(force, 1, 8);
        force == 1
    };
    if store {
        let flags = pg.u32(m, this.wrapping_add(8));
        let flags = c.fl.alu_or(flags, 0x10000, 32);
        pg.set_u32(m, this.wrapping_add(8), flags);
        pg.set_u32(m, this.wrapping_add(0x10), old);
        pg.set_u32(m, this.wrapping_add(0xC), new);
    }
    c.esp = entry.wrapping_add(4 + 8);
}

/// The table of `(1 << n) - 1` masks, indexed by bit count, in the image.
const BIT_MASKS: u32 = 0x0147_0658;

// Translated from 0x01328f30 (DDDA.exe 2017-12-13 Steam).
/// 0x01328F30, `look_bits(reader, bits) -> u32` (cdecl, plain `ret`): the next `bits` (0..=32) bits of a
/// little-endian bit reader without consuming them (the Ogg `look`). The reader has the current byte
/// index (+0), bit offset (+4), a byte pointer (+0xC) and the storage size (+0x10). When fewer than
/// four bytes would remain the request is first checked against the storage (`-1` if it would run
/// past the end); then up to five bytes are combined, each shifted into place, and the result is
/// masked with `BIT_MASKS[bits]`. ECX and EDX are left as the original leaves them (the last shift
/// count, and either `bits + offset` or the last byte shifted); flags are those of the final `and`/`or`.
pub fn look_bits(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let reader = pg.u32(m, entry.wrapping_add(4));
    let bits = pg.u32(m, entry.wrapping_add(8));
    let byte_index = pg.u32(m, reader);
    let mask = pg.u32(m, BIT_MASKS.wrapping_add(bits.wrapping_mul(4)));
    let storage = pg.u32(m, reader.wrapping_add(0x10));
    let offset = pg.u32(m, reader.wrapping_add(4));
    let span = bits.wrapping_add(offset);
    c.ecx = reader;
    c.edx = span;
    c.esp = entry.wrapping_add(4);
    if (byte_index.wrapping_add(4) as i32) >= storage as i32 {
        // near the end: the request must fit in the storage's bits
        let storage_bits = storage.wrapping_mul(8);
        let end = span.wrapping_add(byte_index.wrapping_mul(8));
        if (end as i32) > storage_bits as i32 {
            c.eax = c.fl.alu_or(end, 0xFFFF_FFFF, 32);
            return;
        }
    }
    let data = pg.u32(m, reader.wrapping_add(0xC));
    let mut value = pg.u8(m, data) as u32;
    c.ecx = (reader & 0xFFFF_FF00) | (offset & 0xFF);
    value = value.wrapping_shr(offset);
    // byte k is shifted left by 8k - offset, while more bits than 8k were asked for
    let mut limit = 8u32;
    for k in 1..=4u32 {
        if (span as i32) <= limit as i32 || (k == 4 && offset == 0) {
            break;
        }
        let byte = pg.u8(m, data.wrapping_add(k)) as u32;
        c.ecx = limit.wrapping_sub(offset);
        let part = byte.wrapping_shl(c.ecx);
        if k == 4 {
            c.edx = part;
        }
        value |= part;
        limit += 8;
    }
    c.eax = c.fl.alu_and(value, mask, 32);
}

// Translated from 0x011ae1f0 (DDDA.exe 2017-12-13 Steam).
/// 0x011AE1F0, `dominant_row(out, matrix, v)` (cdecl, plain `ret`): the three dot products
/// `t_i = v . row_i` of the vector (first three lanes) with the rows at `matrix + 0x10`, `+ 0x20` and
/// `+ 0x30` are compared and the whole row (four lanes, copied bit for bit) with the largest product
/// is stored at `out`: row 3 if `max(t0, t1) < t2`, else row 2 if `t0 < t1`, else row 1 (a NaN
/// compares false). Each product is summed `(v1*r1 + v0*r0) + v2*r2`. EAX = `out`; the other
/// registers and the flags are untouched (the original works in XMM registers only).
pub fn dominant_row(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let out = pg.u32(m, entry.wrapping_add(4));
    let matrix = pg.u32(m, entry.wrapping_add(8));
    let vector = pg.u32(m, entry.wrapping_add(12));
    let quad = |m: &dyn Mem, at: u32| -> [u32; 4] { std::array::from_fn(|k| pg.u32(m, at.wrapping_add(4 * k as u32))) };
    let rows = [quad(m, matrix.wrapping_add(0x10)), quad(m, matrix.wrapping_add(0x20)), quad(m, matrix.wrapping_add(0x30))];
    let v = quad(m, vector).map(f32::from_bits);
    let product = |row: &[u32; 4]| {
        let r = row.map(f32::from_bits);
        (v[1] * r[1] + v[0] * r[0]) + v[2] * r[2]
    };
    let [t0, t1, t2] = rows.each_ref().map(product);
    let widest = if t0 < t1 { t1 } else { t0 };
    let chosen = if widest < t2 { &rows[2] } else if t0 < t1 { &rows[1] } else { &rows[0] };
    for (k, word) in chosen.iter().enumerate() {
        pg.set_u32(m, out.wrapping_add(4 * k as u32), *word);
    }
    c.eax = out;
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x004241f0 (DDDA.exe 2017-12-13 Steam).
/// 0x004241F0, `init_node(node)` (register argument EAX = the node, EDX ignored; plain `ret`): resets a
/// node. Two copies of the float triple at 0x018CFE80 go to +0 and +0x10, the triple at 0x01629510 to
/// +0x30; +0x20, +0x40 and +0x54 become -1; +0x48 0x17; +0x60 0xFFFF; +0x64 0xFF; +0xC, +0x1C, +0x3C,
/// +0x44, +0x4C, +0x50, +0x58 and +0x5C zero; and the flag word at +0x24 keeps only the bits of
/// `0x80000FFF` after `(w & 0xFFFFF346) | 0x346`. Leaves ECX = 0, EDX = -1; flags those of the final `xor`.
pub fn init_node(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let node = c.eax;
    let at = |offset: u32| node.wrapping_add(offset);
    let axis: [u32; 3] = std::array::from_fn(|k| pg.u32(m, 0x018c_fe80 + 4 * k as u32));
    let scale: [u32; 3] = std::array::from_fn(|k| pg.u32(m, 0x0162_9510 + 4 * k as u32));
    for k in 0..3u32 {
        pg.set_u32(m, at(4 * k), axis[k as usize]);
        pg.set_u32(m, at(0x10 + 4 * k), axis[k as usize]);
        pg.set_u32(m, at(0x30 + 4 * k), scale[k as usize]);
    }
    for offset in [0xC, 0x1C, 0x3C, 0x44, 0x4C, 0x50, 0x58, 0x5C] {
        pg.set_u32(m, at(offset), 0);
    }
    for offset in [0x20, 0x40, 0x54] {
        pg.set_u32(m, at(offset), 0xFFFF_FFFF);
    }
    let flags = (pg.u32(m, at(0x24)) & 0xFFFF_F346) | 0x346;
    pg.set_u32(m, at(0x24), flags & 0x8000_0FFF);
    pg.set_u32(m, at(0x48), 0x17);
    pg.set_u32(m, at(0x60), 0xFFFF);
    pg.set_u32(m, at(0x64), 0xFF);
    c.ecx = c.fl.alu_xor(flags, flags, 32);
    c.edx = 0xFFFF_FFFF;
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x005f1f90 (DDDA.exe 2017-12-13 Steam).
/// 0x005F1F90, `flag_to_index(slot) -> i32` (register argument ECX = the slot, result in EAX; plain
/// `ret`): slot 0x38 gives 8; otherwise the flag value stored for the slot in the engine object (the
/// pointer at 0x018D8BB0, word `0x198C + 4 * slot`) is mapped to a small index:
/// `1 -> 7, 8 -> 6, 0x100 -> 4, 0x200 -> 5, 0x400 -> 4, 0x800 -> 5, 0x1000 -> 1, 0x2000 -> 2,
/// 0x4000 -> 3, 0x8000 -> 0, 0x400000 -> 8`, anything else -> -1. ECX and EDX are left as the original
/// leaves them (the flag value, or `value - 1` / `value - 8` after the small-value tests; EDX the
/// engine object). Flags are not reproduced.
pub fn flag_to_index(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    c.esp = entry.wrapping_add(4);
    if c.ecx == 0x38 {
        c.eax = 8;
        return;
    }
    let engine = pg.u32(m, 0x018d_8bb0);
    c.edx = engine;
    let value = pg.u32(m, engine.wrapping_add(c.ecx.wrapping_mul(4)).wrapping_add(0x198C));
    c.ecx = value;
    c.eax = 0xFFFF_FFFF;
    c.eax = match value {
        0x800 | 0x200 => 5,
        0x100 | 0x400 => 4,
        0x4000 => 3,
        0x2000 => 2,
        0x1000 => 1,
        0x8000 => 0,
        0x40_0000 => 8,
        v if v < 0x100 => {
            // dec ecx; je; sub ecx, 7; jne
            c.ecx = v.wrapping_sub(1);
            if c.ecx == 0 {
                7
            } else {
                c.ecx = c.ecx.wrapping_sub(7);
                if c.ecx == 0 { 6 } else { 0xFFFF_FFFF }
            }
        }
        _ => 0xFFFF_FFFF,
    };
}

// Translated from 0x00de5cc0 (DDDA.exe 2017-12-13 Steam).
/// 0x00DE5CC0, `record->init()` (thiscall, plain `ret`): as `init_record_a` with the vtable 0x0142BB30
/// and the two floats at +0x7C and +0x8C cleared. Leaves EAX = the record, ECX = 0x016295C0.
pub fn init_record_c(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    c.eax = this;
    pg.set_u32(m, this.wrapping_add(4), 0xFFFF_FFFF);
    pg.set_u32(m, this.wrapping_add(0x1C), 0xFFFF_FFFF);
    c.fl.alu_xor(0xFFFF_FFFF, 0xFFFF_FFFF, 32);
    for offset in [0x18, 0x34, 0x40] {
        pg.set_u32(m, this.wrapping_add(offset), 0);
    }
    pg.set_u8(m, this.wrapping_add(0x32), 0);
    c.ecx = 0x0162_95C0;
    pg.set_u16(m, this.wrapping_add(0x48), 0x100);
    for offset in [0x4C, 0x50, 0x54, 0x58] {
        pg.set_u32(m, this.wrapping_add(offset), c.ecx);
    }
    pg.set_u32(m, this, 0x0142_BB30);
    pg.set_u32(m, this.wrapping_add(0x7C), 0);
    pg.set_u32(m, this.wrapping_add(0x8C), 0);
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x00b76580 (DDDA.exe 2017-12-13 Steam).
/// 0x00B76580, `object->update_flag()` (thiscall, plain `ret`; returns 0 in AL): sets bit 4 of the flag
/// word at +0x1DDC, and clears it again when bit 0 of the word at +0x26BC is set. Leaves EAX = 0,
/// EDX = that bit; flags those of the epilogue's `add esp, 8`.
pub fn update_flag_bit4(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let word = pg.u32(m, this.wrapping_add(0x1DDC));
    let with_bit = c.fl.alu_or(word, 0x10, 32);
    pg.set_u32(m, this.wrapping_add(0x1DDC), with_bit);
    let bit = pg.u32(m, this.wrapping_add(0x26BC)) & 1;
    if bit != 0 {
        let cleared = c.fl.alu_and(with_bit, 0xFFFF_FFEF, 32);
        pg.set_u32(m, this.wrapping_add(0x1DDC), cleared);
    }
    c.edx = bit;
    c.fl.alu_xor(bit, bit, 8);
    // the epilogue's `add esp, 8` has the last word on the flags (the original reserved 8 bytes of frame)
    c.fl.alu_add(entry.wrapping_sub(8), 8, 32);
    c.eax = 0;
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x00850410 (DDDA.exe 2017-12-13 Steam).
/// 0x00850410, `object->clear_entry_flags()` (thiscall, plain `ret`): clears bit 16 of the word at +0x94
/// of each of the `count` (+0x360) entries of 0xA0 bytes at the array pointer +0x364. Leaves EAX = the
/// array pointer and EDX = `count * 0xA0` after at least one entry (EAX, EDX unchanged for an empty
/// object); flags are not reproduced.
pub fn clear_entry_flags(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let count = pg.u32(m, this.wrapping_add(0x360));
    if count != 0 {
        let mut offset = 0u32;
        for _ in 0..count {
            let array = pg.u32(m, this.wrapping_add(0x364));
            c.eax = array;
            let at = array.wrapping_add(offset).wrapping_add(0x94);
            let word = pg.u32(m, at);
            pg.set_u32(m, at, word & 0xFFFE_FFFF);
            offset = offset.wrapping_add(0xA0);
        }
        c.edx = offset;
    }
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x00b85f70 (DDDA.exe 2017-12-13 Steam).
/// 0x00B85F70, `object->select_constant() -> f32 bits` (thiscall, plain `ret`, result in EAX): with the
/// flag byte at +0x2F84 set, bits 1, 2 and 3 of the mode byte at +0x206D (tested in that order)
/// select 0x3F54E9C7, 0x3F5469C7 or 0x3F5429C7, and none selects 0x3F74E9C7; with it clear, bit 1 selects
/// 0x3F74E9C7 again, bits 2 and 3 select 0x3F7469C7 and 0x3F7429C7, and none 0x3F74E9C7. CL is
/// replaced by the mode byte; flags those of the last test.
pub fn select_constant(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let flag = pg.u8(m, this.wrapping_add(0x2F84)) as u32;
    c.fl.alu_sub(flag, 0, 8);
    let mode = pg.u8(m, this.wrapping_add(0x206D)) as u32;
    c.ecx = (this & 0xFFFF_FF00) | mode;
    let mut test = |bit: u32| c.fl.alu_and(mode, bit, 8) != 0;
    c.eax = if flag != 0 {
        if test(2) {
            0x3F54_E9C7
        } else if test(4) {
            0x3F54_69C7
        } else if test(8) {
            0x3F54_29C7
        } else {
            0x3F74_E9C7
        }
    } else if test(2) {
        0x3F74_E9C7
    } else if test(4) {
        0x3F74_69C7
    } else if test(8) {
        0x3F74_29C7
    } else {
        0x3F74_E9C7
    };
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x004ca510 (DDDA.exe 2017-12-13 Steam).
/// 0x004CA510, `object->init_vtables()` (thiscall, plain `ret`): the object's own vtable slot (+0) and
/// one at +0x70 are set to 0x01558134 (after passing through 0x01568A20 and 0x01568A0C), and so are
/// the 24 slots at 0x90-byte spacing from +0xE48 down to +0x80 (+0xE48, then +0xCE0 ... +0x80).
/// Leaves EAX = `this + 0x80`, EDX = -1; flags are not reproduced.
pub fn init_vtables(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    const VTABLE: u32 = 0x0155_8134;
    pg.set_u32(m, this, 0x0156_8A20);
    pg.set_u32(m, this.wrapping_add(0x70), 0x0156_8A0C);
    pg.set_u32(m, this.wrapping_add(0xE48), VTABLE);
    let mut slot = this.wrapping_add(0xD70);
    let mut left = 0x16i32;
    loop {
        slot = slot.wrapping_sub(0x90);
        left -= 1;
        pg.set_u32(m, slot, VTABLE);
        if left < 0 {
            break;
        }
    }
    pg.set_u32(m, this.wrapping_add(0x70), VTABLE);
    pg.set_u32(m, this, VTABLE);
    c.eax = slot;
    c.edx = left as u32;
    c.esp = entry.wrapping_add(4);
}

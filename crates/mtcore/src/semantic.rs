//! Semantic ports (L171): native rewrites of hot game functions that keep
//! the ABI-visible result of the original (return value, callee-saved
//! registers, ESP, and every guest memory write above the returned stack)
//! instead of replaying its instructions. Caller-saved registers (ECX,
//! EDX), the arithmetic flags and the popped stack scratch are not
//! reproduced: MSVC-compiled callers never read them after a call.
//!
//! Each port is opt-in (`DDRUNTIME_SEMANTIC=1`, crates/ddports) and is
//! checked against the original's recorded vectors at that ABI level
//! (crates/mtcore/tests/semantic.rs).
use ddnative_base::mem::Pages;
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

/// 0x00E0EC50, `this->expand(packed, values, out) -> out_end` (thiscall,
/// `ret 0xc`): expand a packed dependency into `out`.
///
/// `this + 0x28` points at the descriptor bank: 8-byte descriptors whose
/// first word holds the recursive-child count in bits 20..25 and the
/// terminal count in bits 26..31, and whose second word points at the u16
/// indices of the children followed by the terminals. `values` is a bank
/// of 8-byte entries whose second word is a packed value: bits 0..11 a
/// descriptor index, bits 12..31 an optional output value. A packed value
/// emits its high part when non-zero, then its recursive children
/// (expanded in turn) and its non-zero terminal values. The original is
/// this recursion unrolled five levels deep, calling itself below that.
pub fn expand_dependencies(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let packed = pg.u32(m, c.esp.wrapping_add(4));
    let values = pg.u32(m, c.esp.wrapping_add(8));
    let out = pg.u32(m, c.esp.wrapping_add(0xc));
    let bank = pg.u32(m, c.ecx.wrapping_add(0x28));
    c.eax = expand(m, pg, bank, values, packed, out);
    c.esp = c.esp.wrapping_add(4 + 0xc);
}

fn expand(m: &mut dyn Mem, pg: Pages, bank: u32, values: u32, packed: u32, mut out: u32) -> u32 {
    let high = packed & 0xffff_f000;
    if high != 0 {
        pg.set_u32(m, out, high);
        out = out.wrapping_add(4);
    }
    let descriptor = bank.wrapping_add((packed & 0xfff).wrapping_mul(8));
    let mut index = pg.u32(m, descriptor.wrapping_add(4));
    // The original reloads the descriptor word for every loop test; keep
    // that, so a descriptor rewritten through `out` behaves the same.
    let mut i = 0u32;
    while i < (pg.u32(m, descriptor) >> 20) & 0x3f {
        let child = pg.u16(m, index) as u32;
        index = index.wrapping_add(2);
        let value = pg.u32(m, values.wrapping_add(child.wrapping_mul(8)).wrapping_add(4));
        out = expand(m, pg, bank, values, value, out);
        i += 1;
    }
    let mut i = 0u32;
    while i < pg.u32(m, descriptor) >> 26 {
        let child = pg.u16(m, index) as u32;
        index = index.wrapping_add(2);
        let value = pg.u32(m, values.wrapping_add(child.wrapping_mul(8)).wrapping_add(4));
        if value != 0 {
            pg.set_u32(m, out, value);
            out = out.wrapping_add(4);
        }
        i += 1;
    }
    out
}

/// 0x00E10FC0, `this->intern_keys(owner, values, keys, extra, crc_out) ->
/// interned` (thiscall, `ret 0x14`): build the key sequence of a
/// primitive's shader/technique lookup, optionally store its CRC, and
/// return the sequence's hash (the "interned" id).
///
/// The sequence: the four `keys` words tagged with their slot in the low
/// bits (zero words skipped), `extra`, then the expansions of the six
/// `owner` words in the order 0, 3, 4, 2, 1, 5 (zero words skipped). The
/// CRC (when `crc_out` is not null) is the table CRC (table at 0x014075C0)
/// of the words' bytes, high byte first. The hash
/// is what 0x00E0E5E0 computes (FNV prime 0x01000193 over the bytes of
/// `word ^ 0x14DA14DA`, multiply-then-xor, from basis 0x811C9DC5).
///
/// The original builds the sequence in a 1 KiB buffer on its stack and
/// calls 0x00E0E5E0 and (per owner word) 0x00E0EC50; both are pure, so the
/// sequence is built here in a local buffer and hashed natively: no guest
/// call and no guest memory traffic except the reads of the inputs. (The
/// buffer lives below the returned ESP in the original, so nothing
/// observable is lost.) [`intern_dependency_keys_with`] keeps the
/// original's shape for tests that replay the guest calls.
pub fn intern_dependency_keys(m: &mut dyn Mem, c: &mut Cpu) {
    intern_dependency_keys_table(m, c, true)
}

/// [`intern_dependency_keys`] with the CRC table either cached after its
/// first read (production: the table is read-only data) or read from guest
/// memory on every lookup, like the original (tests whose memories hold a
/// different table in every case).
pub fn intern_dependency_keys_table(m: &mut dyn Mem, c: &mut Cpu, cached: bool) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let (owner, values, keys, extra, crc_out) = (arg(m, 0), arg(m, 1), arg(m, 2), arg(m, 3), arg(m, 4));
    c.eax = intern_keys_with(m, pg, c.ecx, owner, values, keys, extra, crc_out, cached);
    c.esp = entry.wrapping_add(4 + 0x14);
}

/// The body of [`intern_dependency_keys`] without the guest calling
/// convention (also used by [`shader_lookup`]).
#[allow(clippy::too_many_arguments)]
pub fn intern_keys(m: &mut dyn Mem, pg: Pages, this: u32, owner: u32, values: u32, keys: u32, extra: u32, crc_out: u32) -> u32 {
    intern_keys_with(m, pg, this, owner, values, keys, extra, crc_out, true)
}

#[allow(clippy::too_many_arguments)]
fn intern_keys_with(m: &mut dyn Mem, pg: Pages, this: u32, owner: u32, values: u32, keys: u32, extra: u32, crc_out: u32, cached: bool) -> u32 {
    let mut words = [0u32; KEY_CAPACITY];
    let mut count = 0usize;
    for slot in 0..4u32 {
        let k = pg.u32(m, keys.wrapping_add(4 * slot));
        if k != 0 {
            words[count] = (k & 0xffff_f000) | slot;
            count += 1;
        }
    }
    words[count] = extra;
    count += 1;
    let mut bank = None;
    for field in [0u32, 3, 4, 2, 1, 5] {
        let packed = pg.u32(m, owner.wrapping_add(4 * field));
        if packed != 0 {
            let bank = *bank.get_or_insert_with(|| pg.u32(m, this.wrapping_add(0x28)));
            count = expand_into(m, pg, bank, values, packed, &mut words, count);
        }
    }
    let words = &words[..count];
    if crc_out != 0 {
        let crc = if cached { key_crc(crc_table(m, pg), words) } else { key_crc_guest(m, pg, words) };
        pg.set_u32(m, crc_out, crc);
    }
    key_hash(words)
}

/// Room for the key words (the original's buffer is 0x400 bytes).
const KEY_CAPACITY: usize = 0x100;

/// The guest's CRC table at 0x014075C0 (256 words, read-only data), read
/// from guest memory the first time a CRC is needed.
static CRC_TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();

fn crc_table(m: &dyn Mem, pg: Pages) -> &'static [u32; 256] {
    CRC_TABLE.get_or_init(|| std::array::from_fn(|i| pg.u32(m, 0x0140_75c0 + 4 * i as u32)))
}

/// CRC over the words' bytes, high byte of each word first, as the
/// original's table loop does.
pub fn key_crc(table: &[u32; 256], words: &[u32]) -> u32 {
    let mut crc = u32::MAX;
    for &word in words {
        for shift in [24u32, 16, 8, 0] {
            crc = (crc >> 8) ^ table[(((word >> shift) ^ crc) & 0xff) as usize];
        }
    }
    crc
}

/// [`key_crc`] reading the table entries from guest memory as they are used.
fn key_crc_guest(m: &dyn Mem, pg: Pages, words: &[u32]) -> u32 {
    let mut crc = u32::MAX;
    for &word in words {
        for shift in [24u32, 16, 8, 0] {
            let index = ((word >> shift) ^ crc) & 0xff;
            crc = (crc >> 8) ^ pg.u32(m, 0x0140_75c0u32.wrapping_add(index * 4));
        }
    }
    crc
}

/// The hash of the key words (what 0x00E0E5E0 computes with the basis
/// 0x811C9DC5).
pub fn key_hash(words: &[u32]) -> u32 {
    words.iter().fold(0x811c_9dc5u32, |h, &word| hash_word(h, word))
}

#[inline(always)]
fn hash_word(h: u32, word: u32) -> u32 {
    const PRIME: u32 = 0x0100_0193;
    let w = word ^ 0x14da_14da;
    let h = (h.wrapping_mul(PRIME) ^ (w & 0xff)).wrapping_mul(PRIME);
    let h = (h ^ ((w >> 8) & 0xff)).wrapping_mul(PRIME);
    let h = (h ^ ((w >> 16) & 0xff)).wrapping_mul(PRIME);
    h ^ (w >> 24)
}

/// 0x00E0E5E0, `hash_key_words(words, byte_length, basis) -> hash`
/// (stdcall, `ret 0xc`): FNV-style hash (prime 0x01000193, multiply then
/// xor, per byte of `word ^ 0x14DA14DA`) of `byte_length / 4` words.
pub fn hash_key_words(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let (words, length, basis) = (arg(m, 0), arg(m, 1), arg(m, 2));
    let mut h = basis;
    for i in 0..length >> 2 {
        h = hash_word(h, pg.u32(m, words.wrapping_add(4 * i)));
    }
    c.eax = h;
    c.esp = entry.wrapping_add(4 + 0xc);
}

/// [`expand`] into a local buffer: the expansion of `packed` appended at
/// `words[count..]`; returns the new count.
fn expand_into(m: &dyn Mem, pg: Pages, bank: u32, values: u32, packed: u32, words: &mut [u32; KEY_CAPACITY], mut count: usize) -> usize {
    let high = packed & 0xffff_f000;
    if high != 0 {
        words[count] = high;
        count += 1;
    }
    let descriptor = bank.wrapping_add((packed & 0xfff).wrapping_mul(8));
    let mut index = pg.u32(m, descriptor.wrapping_add(4));
    let head = pg.u32(m, descriptor);
    for _ in 0..(head >> 20) & 0x3f {
        let child = pg.u16(m, index) as u32;
        index = index.wrapping_add(2);
        let value = pg.u32(m, values.wrapping_add(child.wrapping_mul(8)).wrapping_add(4));
        count = expand_into(m, pg, bank, values, value, words, count);
    }
    for _ in 0..head >> 26 {
        let child = pg.u16(m, index) as u32;
        index = index.wrapping_add(2);
        let value = pg.u32(m, values.wrapping_add(child.wrapping_mul(8)).wrapping_add(4));
        if value != 0 {
            words[count] = value;
            count += 1;
        }
    }
    count
}

/// The test seam: the original's shape, with the interning
/// ([`intern_dependency_keys_with`]) and the expansions ([`Expander`])
/// replayed as guest calls. The previous production body.
pub fn intern_dependency_keys_reference(m: &mut dyn Mem, c: &mut Cpu) {
    intern_dependency_keys_with(m, c, native_expand)
}

/// One expansion of `packed` into `out` for [`intern_dependency_keys`]:
/// the native recursion (production) or a guest call to 0x00E0EC50.
pub type Expander = fn(&mut dyn Mem, &mut Cpu, Pages, u32, u32, u32, u32) -> u32;

fn native_expand(m: &mut dyn Mem, _: &mut Cpu, pg: Pages, this: u32, values: u32, packed: u32, out: u32) -> u32 {
    let bank = pg.u32(m, this.wrapping_add(0x28));
    expand(m, pg, bank, values, packed, out)
}

/// The original's own call to 0x00E0EC50 (for tests against vectors that
/// record those calls as callees): `push out; push values; push packed;
/// mov ecx, this; call` with ESP at the original's frame.
pub fn guest_expand(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, values: u32, packed: u32, out: u32) -> u32 {
    for v in [out, values, packed, RETURN_FROM_EXPAND.with(|r| r.get())] {
        c.esp = c.esp.wrapping_sub(4);
        pg.set_u32(m, c.esp, v);
    }
    c.ecx = this;
    crate::semantic::call_guest(m, c, 0x00e0_ec50);
    c.eax
}

thread_local! {
    /// The return address of the expansion call being made (each of the
    /// six call sites has its own), for [`guest_expand`].
    static RETURN_FROM_EXPAND: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// [`intern_dependency_keys`] with its expansion step chosen by the caller.
pub fn intern_dependency_keys_with(m: &mut dyn Mem, c: &mut Cpu, expander: Expander) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let owner = arg(m, 0);
    let values = arg(m, 1);
    let keys = arg(m, 2);
    let extra = arg(m, 3);
    let crc_out = arg(m, 4);
    let this = c.ecx;
    let buffer = entry.wrapping_sub(0x400);
    let mut out = buffer;
    for slot in 0..4u32 {
        let k = pg.u32(m, keys.wrapping_add(4 * slot));
        if k != 0 {
            pg.set_u32(m, out, (k & 0xffff_f000) | slot);
            out = out.wrapping_add(4);
        }
    }
    pg.set_u32(m, out, extra);
    out = out.wrapping_add(4);
    // The original's frame during the expansions: ESP = entry - 0x414.
    c.esp = entry.wrapping_sub(0x414);
    for (field, resume) in [(0u32, 0x00e1_1058u32), (3, 0x00e1_1069), (4, 0x00e1_107a), (2, 0x00e1_108b), (1, 0x00e1_109c), (5, 0x00e1_10ad)] {
        let packed = pg.u32(m, owner.wrapping_add(4 * field));
        if packed != 0 {
            RETURN_FROM_EXPAND.with(|r| r.set(resume));
            out = expander(m, c, pg, this, values, packed, out);
        }
    }
    let count = out.wrapping_sub(buffer) >> 2;
    if crc_out != 0 {
        let mut crc = u32::MAX;
        for i in 0..(count & 0x3fff_ffff) {
            let word = pg.u32(m, buffer.wrapping_add(4 * i));
            for shift in [24u32, 16, 8, 0] {
                let index = ((word >> shift) ^ crc) & 0xff;
                crc = (crc >> 8) ^ pg.u32(m, 0x0140_75c0u32.wrapping_add(index * 4));
            }
        }
        pg.set_u32(m, crc_out, crc);
    }
    // The original's frame at the call: ESP = entry - 0x414 (0x404 locals,
    // four pushes), then three arguments and the return address.
    c.esp = entry.wrapping_sub(0x414);
    for v in [0x811c_9dc5u32, count.wrapping_mul(4), buffer, 0x00e1_118f] {
        c.esp = c.esp.wrapping_sub(4);
        pg.set_u32(m, c.esp, v);
    }
    c.ecx = this;
    crate::semantic::call_guest(m, c, 0x00e0_e5e0);
    // EAX = the interned sequence; `ret 0x14` from the entry frame
    c.esp = entry.wrapping_add(4 + 0x14);
}

/// The original body of 0x00E12A20, set by the runtime when it installs
/// [`shader_lookup`] (the rare branch runs it unchanged).
pub static E12A20_ORIGINAL: std::sync::OnceLock<ddnative_base::x86::LiftFn> = std::sync::OnceLock::new();

/// A call from a native port to the guest function at `target`, with the
/// stack already set up as for `call_ext`: straight into the native port
/// registered as an override for `target` (no dispatch), otherwise through
/// `call_ext` (replayed seams in tests, the lifted program in the game).
#[inline(always)]
pub(crate) fn call_guest(m: &mut dyn Mem, c: &mut Cpu, target: u32) {
    // one bit test per 16 bytes of code: no override, no further cost
    if ddnative_base::x86::may_be_overridden(target) {
        return call_overridden(m, c, target);
    }
    ddnative_base::x86::call_ext(m, c, target)
}

/// The rare half of [`call_guest`], kept out of line so that recursive ports
/// (a deep recursion in a debug build) keep small frames.
#[inline(never)]
fn call_overridden(m: &mut dyn Mem, c: &mut Cpu, target: u32) {
    match ddnative_base::x86::override_of(target) {
        Some(f) => f(m, c),
        None => ddnative_base::x86::call_ext(m, c, target),
    }
}

/// The CRT `memcpy` 0x012EFEA0 done natively: it behaves like memmove (the
/// whole source is read before the destination is written). Whole pages are
/// moved with host copies; anything the page table does not serve directly
/// goes byte by byte through the memory.
pub(crate) fn copy_guest(m: &mut dyn Mem, pg: Pages, destination: u32, source: u32, bytes: u32) {
    use ddnative_base::mem::{PAGE, READ, WRITE};
    if bytes == 0 {
        return;
    }
    if let (Some(from), Some(to)) = (pg.bytes(source, bytes as usize, READ), pg.bytes(destination, bytes as usize, WRITE)) {
        // SAFETY: both ranges lie inside one backed page each; `copy` allows overlap
        unsafe { std::ptr::copy(from, to, bytes as usize) };
        return;
    }
    let mut data = vec![0u8; bytes as usize];
    let mut done = 0u32;
    while done < bytes {
        let at = source.wrapping_add(done);
        let n = (bytes - done).min(PAGE - at % PAGE);
        match pg.bytes(at, n as usize, READ) {
            // SAFETY: `n` bytes inside one backed page
            Some(p) => unsafe { std::ptr::copy_nonoverlapping(p, data[done as usize..].as_mut_ptr(), n as usize) },
            None => (0..n).for_each(|k| data[(done + k) as usize] = pg.u8(m, at.wrapping_add(k))),
        }
        done += n;
    }
    done = 0;
    while done < bytes {
        let at = destination.wrapping_add(done);
        let n = (bytes - done).min(PAGE - at % PAGE);
        match pg.bytes(at, n as usize, WRITE) {
            // SAFETY: `n` bytes inside one backed page
            Some(p) => unsafe { std::ptr::copy_nonoverlapping(data[done as usize..].as_ptr(), p, n as usize) },
            None => (0..n).for_each(|k| pg.set_u8(m, at.wrapping_add(k), data[(done + k) as usize])),
        }
        done += n;
    }
}

/// A guest call from the frame `frame` (the caller's ESP at its call
/// sites): push `args` (last first) and `back`, set ECX and run `body`.
/// Returns EAX; ESP is `frame` again after the callee's own `ret n`.
pub(crate) fn call_from(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, this: u32, args: &[u32], back: u32, body: impl FnOnce(&mut dyn Mem, &mut Cpu)) -> u32 {
    c.esp = frame;
    for &v in args.iter().rev() {
        c.esp = c.esp.wrapping_sub(4);
        pg.set_u32(m, c.esp, v);
    }
    c.esp = c.esp.wrapping_sub(4);
    pg.set_u32(m, c.esp, back);
    c.ecx = this;
    body(m, c);
    c.eax
}

/// 0x00E12A20, `this->lookup(a, bank, slot, keys_owner_values, keys,
/// extra, other_bank) -> entry` (thiscall, `ret 0x1c`): the shader/
/// technique lookup of every model primitive draw. The common branch
/// (byte `this + 0x39` clear) interns the dependency keys of
/// `bucket(bank) + slot * 0x28 + 0x1c` and looks them up (0x00E0F3C0),
/// then again with their CRC (0x00E0F4B0), then for `other_bank`; the rare
/// branch (shader creation) runs the original body. Only reads happen
/// before the branch, so the fallback is exact.
pub fn shader_lookup(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let this = c.ecx;
    if pg.u8(m, this.wrapping_add(0x39)) != 0 {
        let original = E12A20_ORIGINAL.get().expect("0x00E12A20 original body");
        return original(m, c);
    }
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let (bank, slot, owner_values, keys, extra, other) = (arg(m, 1), arg(m, 2), arg(m, 3), arg(m, 4), arg(m, 5), arg(m, 6));
    let table = pg.u32(m, this.wrapping_add(0x24));
    let bucket = pg.u32(m, table.wrapping_add((bank & 0xfff).wrapping_mul(4)));
    let owner = bucket.wrapping_add(slot.wrapping_mul(0x28)).wrapping_add(0x1c);
    // The original's frame after its prologue (0x8c locals, four pushes);
    // its CRC local is at frame + 0x10.
    let frame = entry.wrapping_sub(0x9c);
    let crc = frame.wrapping_add(0x10);
    let ext = |target: u32| move |m: &mut dyn Mem, c: &mut Cpu| crate::semantic::call_guest(m, c, target);
    let finish = |c: &mut Cpu, r: u32| {
        c.eax = r;
        c.esp = entry.wrapping_add(4 + 0x1c);
    };
    let key = intern_keys(m, pg, this, owner, owner_values, keys, extra, 0);
    let r = call_from(m, c, pg, frame, this, &[key], 0x00e1_3210, ext(0x00e0_f3c0));
    if r != 0 {
        return finish(c, r);
    }
    let key = intern_keys(m, pg, this, owner, owner_values, keys, extra, crc);
    let sum = pg.u32(m, crc);
    let r = call_from(m, c, pg, frame, this, &[key, sum], 0x00e1_3244, ext(0x00e0_f4b0));
    if r != 0 || pg.u8(m, this.wrapping_add(0xab229)) != 0 || other == 0 {
        return finish(c, r);
    }
    let table = pg.u32(m, this.wrapping_add(0x24));
    let owner = pg.u32(m, table.wrapping_add((other & 0xfff).wrapping_mul(4))).wrapping_add(0x1c);
    let key = intern_keys(m, pg, this, owner, owner_values, keys, extra, crc);
    let sum = pg.u32(m, crc);
    let r = call_from(m, c, pg, frame, this, &[key, sum], 0x00e1_3293, ext(0x00e0_f4b0));
    finish(c, r)
}

/// 0x01054700, `model->update_joints(indices, count, flags)` (thiscall,
/// `ret 0xc`): run the model's joint-update routine over a list of joint
/// indices. The routine is chosen once from the model's flags at +0x378
/// (none, 0x0104EF10, 0x01051B60, 0x010533A0 or 0x01052330; some choices
/// also set bit 6 of +0x108); joints are 0xA0-byte records from +0x364.
/// A joint flagged 0x20 at +0x90 is skipped, and with `flags & 1` only
/// joints with bit 0 of +0x96 are updated. Unless `flags & 2`, joints
/// with an attachment (+4) also get its virtual +0x14 call when model
/// byte +0x37B bit 0 is set. A pending reset (byte +0xE19) runs
/// 0x01054690 first.
pub fn update_joints(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let model = c.ecx;
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    // The index list is read only inside the loop, as the original does.
    let count = arg(m, 1);
    let flags = pg.u8(m, entry.wrapping_add(0xc)); // the original reads the byte only
    let notify = if flags & 2 != 0 { false } else { pg.u8(m, model + 0x37b) & 1 != 0 };
    let visible_only = flags & 1 != 0;
    let guest = |target: u32| move |m: &mut dyn Mem, c: &mut Cpu| crate::semantic::call_guest(m, c, target);
    // The original's frames: four pushes, then ESI pushed for the loop.
    let frame = entry.wrapping_sub(16);
    let loop_frame = entry.wrapping_sub(20);
    if pg.u8(m, model + 0xe19) != 0 {
        call_from(m, c, pg, frame, model, &[], 0x0105_4738, guest(0x0105_4690));
        pg.set_u8(m, model + 0xe19, 0);
    }
    let mode = pg.u32(m, model + 0x378);
    let routine = if mode & 0x0800_0000 != 0 {
        0
    } else if mode & 0x0400_0000 != 0 {
        0x0104_ef10
    } else if mode & 0x40 == 0 {
        0x0105_1b60
    } else {
        let routine = if mode & 0x10_0000 != 0 {
            0x0105_1b60
        } else if mode & 0x4000 != 0 {
            0x0105_33a0
        } else {
            0x0105_2330
        };
        let marks = pg.u32(m, model + 0x108);
        pg.set_u32(m, model + 0x108, marks | 0x40);
        routine
    };
    let joints = model + 0x364;
    for i in 0..count {
        let indices = arg(m, 0);
        let index = pg.u8(m, indices.wrapping_add(i)) as u32;
        let joint = pg.u32(m, joints).wrapping_add(index * 0xa0);
        if pg.u8(m, joint + 0x90) & 0x20 != 0 {
            continue;
        }
        if visible_only && pg.u8(m, joint + 0x96) & 1 == 0 {
            continue;
        }
        if routine != 0 {
            call_from(m, c, pg, loop_frame, model, &[joint], 0x0105_47d6, guest(routine));
        }
        if notify {
            let attachment = pg.u32(m, joint + 4);
            if attachment != 0 {
                let method = pg.u32(m, pg.u32(m, attachment) + 0x14);
                call_from(m, c, pg, loop_frame, attachment, &[joint, model], 0x0105_47ef, guest(method));
            }
        }
    }
    c.esp = entry.wrapping_add(4 + 0xc);
}

/// 0x00FEDD60, `sort_pairs(records, count, scratch) -> sorted` (stdcall,
/// `ret 0xc`): a stable bottom-up merge sort of `count` 8-byte (key,
/// value) records by their unsigned key, ping-ponging between `records`
/// and `scratch`; returns the buffer holding the result.
///
/// Both buffers stay as the original leaves them: a pair pass writes all
/// records into `scratch`, then merges of runs of 2, 4, 8, ... alternate
/// direction (`scratch -> records -> scratch ...`) while the merged width
/// does not exceed the next power of two of `count`. On equal keys the
/// left run's record goes first.
pub fn sort_pairs(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let records = arg(m, 0);
    let count = arg(m, 1) as i32;
    c.esp = entry.wrapping_add(4 + 0xc);
    // the next power of two of `count` (at most 32 doublings)
    let mut span: i32 = 1;
    for _ in 0..32 {
        if count <= span {
            break;
        }
        span = span.wrapping_mul(2);
    }
    if span < 2 {
        c.eax = records;
        return;
    }
    let scratch = arg(m, 2);
    let n = count as usize;
    let read = |m: &dyn Mem, base: u32| -> Vec<[u32; 2]> {
        (0..n as u32).map(|i| [pg.u32(m, base + 8 * i), pg.u32(m, base + 8 * i + 4)]).collect()
    };
    let mut a = read(m, records);
    // pair pass: records -> scratch
    let mut b = Vec::with_capacity(n);
    for pair in a.chunks(2) {
        match pair {
            [x, y] if y[0] < x[0] => b.extend([*y, *x]),
            _ => b.extend_from_slice(pair),
        }
    }
    let mut a_written = false;
    // merge passes; `in_scratch`: the current runs live in `b`
    let mut in_scratch = true;
    let mut width: i32 = 4;
    if span > 3 {
        while width <= span {
            let (src, dst) = if in_scratch { (&b, &mut a) } else { (&a, &mut b) };
            merge_runs(src, dst, (width / 2) as usize);
            if in_scratch {
                a_written = true;
            }
            in_scratch = !in_scratch;
            width = width.wrapping_mul(2);
        }
    }
    let write = |m: &mut dyn Mem, base: u32, v: &[[u32; 2]]| {
        for (i, r) in v.iter().enumerate() {
            pg.set_u32(m, base + 8 * i as u32, r[0]);
            pg.set_u32(m, base + 8 * i as u32 + 4, r[1]);
        }
    };
    write(m, scratch, &b);
    if a_written {
        write(m, records, &a);
    }
    c.eax = if in_scratch { scratch } else { records };
}

/// One merge pass: each pair of adjacent `run`-long runs of `src` merged
/// into the same range of `dst` (stable; the right record first only when
/// its key is strictly smaller).
fn merge_runs(src: &[[u32; 2]], dst: &mut [[u32; 2]], run: usize) {
    let n = src.len();
    let mut start = 0;
    while start < n {
        let mid = (start + run).min(n);
        let end = (mid + run).min(n);
        let (mut l, mut r, mut out) = (start, mid, start);
        while l < mid && r < end {
            if src[r][0] < src[l][0] {
                dst[out] = src[r];
                r += 1;
            } else {
                dst[out] = src[l];
                l += 1;
            }
            out += 1;
        }
        dst[out..out + (mid - l)].copy_from_slice(&src[l..mid]);
        out += mid - l;
        dst[out..out + (end - r)].copy_from_slice(&src[r..end]);
        start += 2 * run;
    }
}

/// 0x00FF1090, `context->set_pass(pass, force)` (thiscall, `ret 8`):
/// select a render pass. When `force` or the pass differs from the low 5
/// bits of +0x164, store it there and, if the context has a pass table
/// (+0x1D4), bind that pass's colour targets (up to four, from the table's
/// per-pass slots at +0x18 + pass * 0x10, ending at the first empty one)
/// and depth target (+0x198 + pass * 4): one colour target through
/// 0x00FEFB90, several through 0x00FF00A0, with the pass's binding flags.
pub fn set_pass(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let ctx = c.ecx;
    let entry = c.esp;
    let force = pg.u8(m, entry.wrapping_add(8)) != 0;
    let pass = pg.u32(m, entry.wrapping_add(4));
    let finish = |c: &mut Cpu| c.esp = entry.wrapping_add(4 + 8);
    let mode = pg.u32(m, ctx + 0x164);
    if !force && mode & 0x1f == pass {
        return finish(c);
    }
    let table = pg.u32(m, ctx + 0x1d4);
    pg.set_u32(m, ctx + 0x164, mode ^ ((mode ^ pass) & 0x1f));
    if table == 0 {
        return finish(c);
    }
    // The original's frame: 0x10 locals (the colour array), then four pushes.
    let colours_at = entry.wrapping_sub(0x10);
    let frame = entry.wrapping_sub(0x20);
    let mut colours = 0u32;
    for i in 0..4u32 {
        let slot = pg.u32(m, table.wrapping_add(pass.wrapping_mul(0x10) + 0x18 + 4 * i));
        let target = if slot == 0 { 0 } else { pg.u32(m, pg.u32(m, slot + 0x44)) };
        pg.set_u32(m, colours_at + 4 * i, target);
        if target == 0 {
            break;
        }
        colours += 1;
    }
    let depth_slot = pg.u32(m, table.wrapping_add(0x198).wrapping_add(pass.wrapping_mul(4)));
    let depth = if depth_slot == 0 { 0 } else { pg.u32(m, pg.u32(m, depth_slot + 0x48)) };
    if colours == 0 && depth == 0 {
        return finish(c);
    }
    let caps = pg.u32(m, table + 0x10);
    let mut flags = if caps & 0x10000 != 0 && (pass == 0xb || pass == 0xc) { 0x40 } else { 0 } | 3;
    if caps & 0x4000 != 0 && caps & 0x2000 != 0 && (pass == 8 || pass == 9) && pg.u32(m, table + 0x2dc) != 2 {
        flags |= 0x20;
    }
    if pg.u8(m, table + 0x2e0) != 0 && pass == 0x12 {
        flags |= 0x20;
    }
    let guest = |target: u32| move |m: &mut dyn Mem, c: &mut Cpu| crate::semantic::call_guest(m, c, target);
    if colours < 2 {
        let first = pg.u32(m, colours_at);
        call_from(m, c, pg, frame, ctx, &[first, depth, flags, 0], 0x00ff_1187, guest(0x00fe_fb90));
    } else {
        call_from(m, c, pg, frame, ctx, &[colours, colours_at, depth, flags, 0], 0x00ff_119c, guest(0x00ff_00a0));
    }
    finish(c)
}

/// The owner methods [`find_identifier`] devirtualizes: `count()` (vtable
/// +0x40) returning `this[0x64]`, and `entry(i)` (vtable +0x3c, `ret 4`)
/// returning `this[0x60] + i * 0x70`.
const IDENTIFIER_COUNT: u32 = 0x0040_e330;
const IDENTIFIER_ENTRY: u32 = 0x00cd_7a30;

// Translated from 0x00cd8760 (DDDA.exe 2017-12-13 Steam).
/// 0x00CD8760, `find(owner = ESI, packed = EAX) -> entry` (`ret`): the
/// owner's entry whose u16 type at +0x10 and u16 id at +0x12 equal the
/// packed identifier's high and low halves, or 0. Only types 0x62..=0x7a
/// are searched; a negative identifier finds nothing.
///
/// The original walks `entry(i)` for `i < count()` through two virtual
/// calls per entry (~7% of the main thread, 54 searches a frame from
/// 0x00485850). When the owner's methods are the array accessors above,
/// the walk reads the array directly; any other owner runs the
/// instruction-level port, so the result is the same for every owner.
pub fn find_identifier(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let packed = c.eax;
    let owner = c.esi;
    let (kind, id) = (packed >> 16, packed & 0xffff);
    if (packed as i32) < 0 || !(0x62..=0x7a).contains(&kind) {
        c.eax = 0;
        c.esp = c.esp.wrapping_add(4);
        return;
    }
    let vtable = pg.u32(m, owner);
    if pg.u32(m, vtable.wrapping_add(0x40)) != IDENTIFIER_COUNT || pg.u32(m, vtable.wrapping_add(0x3c)) != IDENTIFIER_ENTRY {
        return crate::hot::resolve_identifier(m, c);
    }
    let count = pg.u32(m, owner.wrapping_add(0x64));
    let entries = pg.u32(m, owner.wrapping_add(0x60));
    c.eax = (0..count)
        .map(|i| entries.wrapping_add(i.wrapping_mul(0x70)))
        .find(|&entry| {
            entry != 0
                && u32::from(pg.u16(m, entry.wrapping_add(0x10))) == kind
                && u32::from(pg.u16(m, entry.wrapping_add(0x12))) == id
        })
        .unwrap_or(0);
    c.esp = c.esp.wrapping_add(4);
}

// Translated from 0x00dd42c0 (DDDA.exe 2017-12-13 Steam).
/// 0x00DD42C0, `this->dispatch(event)` (thiscall, `ret 4`): hand `event`
/// to every live listener of the `this + 0x4cd48` listeners (0x6e0 bytes
/// each from `this + 0x30`) that matches it, then walk up to the event's
/// parent id and repeat, at most `this[0x4cd50]` levels.
///
/// A listener is live when it has a handle (+4) and is not in state 2
/// (+8), or its state is 1 or 5, or its weight (+0xc8) is above the
/// constant at 0x01561738. It matches when its owner (+0xc), id (+0x14)
/// and channel (+0x18) equal the event's (+0x30, +4 then the parents,
/// +8), an event value of 0 (owner) or -1 (id, channel) matching any.
/// A match calls 0x00DCC3E0 `this->deliver(listener, event)`, which may
/// change the listeners, so the count is read again after every one. The
/// parent of an id comes from 0x00F73B90 on the event's owner.
pub fn dispatch_event(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let (this, entry) = (c.ecx, c.esp);
    let event = pg.u32(m, entry.wrapping_add(4));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // the original's frame after its five pushes: its calls are made here
    let frame = entry.wrapping_sub(0x14);
    let weight_floor = f32::from_bits(pg.u32(m, 0x0156_1738));
    let mut id = pg.u32(m, event.wrapping_add(4));
    let mut level = 0u32;
    loop {
        let mut i = 0u32;
        while i < pg.u32(m, this.wrapping_add(0x4cd48)) {
            let listener = this.wrapping_add(0x30).wrapping_add(i.wrapping_mul(0x6e0));
            let state = pg.u32(m, listener.wrapping_add(8));
            let live = (pg.u32(m, listener.wrapping_add(4)) != 0 && state != 2)
                || state == 1
                || state == 5
                || f32::from_bits(pg.u32(m, listener.wrapping_add(0xc8))) > weight_floor;
            if live {
                let owner = pg.u32(m, event.wrapping_add(0x30));
                let channel = pg.u32(m, event.wrapping_add(8));
                let matches = (owner == pg.u32(m, listener.wrapping_add(0xc)) || owner == 0)
                    && (id == pg.u32(m, listener.wrapping_add(0x14)) || id == u32::MAX)
                    && (channel == pg.u32(m, listener.wrapping_add(0x18)) || channel == u32::MAX);
                if matches {
                    (c.ebx, c.ebp, c.esi, c.edi) = (i, id, listener.wrapping_add(8), event);
                    call_from(m, c, pg, frame, this, &[listener, event], 0x00dd_4377, |m, c| {
                        crate::semantic::call_guest(m, c, 0x00dc_c3e0)
                    });
                }
            }
            i += 1;
        }
        let owner = pg.u32(m, event.wrapping_add(0x30));
        let mut parent = u32::MAX;
        if owner != 0 {
            (c.ebx, c.ebp, c.esi, c.edi) = (i, id, this.wrapping_add(0x38).wrapping_add(i.wrapping_mul(0x6e0)), event);
            let found = call_from(m, c, pg, frame, owner, &[id], 0x00dd_439b, |m, c| {
                crate::semantic::call_guest(m, c, 0x00f7_3b90)
            });
            if found != 0 {
                let up = pg.u16(m, found.wrapping_add(0x1c)) as i16 as i32 as u32;
                if up != id {
                    parent = up;
                }
            }
        }
        if pg.u8(m, event.wrapping_add(0x2c)) == 0 || parent == u32::MAX {
            break;
        }
        id = parent;
        level += 1;
        if level > pg.u32(m, this.wrapping_add(0x4cd50)) {
            break;
        }
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(8);
}

/// The table methods [`find_sorted`] devirtualizes (the
/// rAIStageActionParameter family): `count()` (vtable +0x3c) the u16 at
/// `[this + 0x60] + 6`, `entry(i)` (vtable +0x40, `ret 4`)
/// `[this + 0x60] + 8 + 0xec i`.
const SORTED_COUNT: u32 = 0x00ca_8d60;
const SORTED_ENTRY: u32 = 0x00ca_a160;

// Translated from 0x005b31a0 (DDDA.exe 2017-12-13 Steam).
/// 0x005B31A0, `find(key, table) -> index` (cdecl): binary search of
/// `table`'s entries, sorted by their first word (signed), for `key`; -1
/// when absent. The original asks `table` for the count and each probed
/// entry through two virtual calls; for the accessor pair above the search
/// reads the table directly, for any other class it makes the same calls.
pub fn find_sorted(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let table = pg.u32(m, entry.wrapping_add(8));
    let vtable = pg.u32(m, table);
    let count_method = pg.u32(m, vtable.wrapping_add(0x3c));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // the original's frame after its four pushes: its calls are made here
    let frame = entry.wrapping_sub(0x10);
    let count = if count_method == SORTED_COUNT {
        u32::from(pg.u16(m, pg.u32(m, table.wrapping_add(0x60)).wrapping_add(6)))
    } else {
        (c.ebx, c.ebp) = (table, 0);
        call_from(m, c, pg, frame, table, &[], 0x005b_31b3, |m, c| crate::semantic::call_guest(m, c, count_method))
    };
    let (mut low, mut high) = (0i32, count.wrapping_sub(1) as i32);
    let mut found = u32::MAX;
    while low <= high {
        let middle = high.wrapping_add(low) / 2;
        let entry_method = pg.u32(m, vtable.wrapping_add(0x40));
        let row = if entry_method == SORTED_ENTRY {
            pg.u32(m, table.wrapping_add(0x60)).wrapping_add(8).wrapping_add((middle as u32).wrapping_mul(0xec))
        } else {
            (c.ebx, c.ebp, c.esi, c.edi) = (table, low as u32, middle as u32, high as u32);
            call_from(m, c, pg, frame, table, &[middle as u32], 0x005b_31cc, |m, c| crate::semantic::call_guest(m, c, entry_method))
        };
        let value = pg.u32(m, row) as i32;
        let key = pg.u32(m, entry.wrapping_add(4)) as i32;
        if value == key {
            found = middle as u32;
            break;
        }
        if value < key {
            low = middle + 1;
        } else {
            high = middle - 1;
        }
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.eax = found;
    c.esp = entry.wrapping_add(4);
}

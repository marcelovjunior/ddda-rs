//! L185 s7: semantic ports of the resource loader's buffered input stream
//! (0x00CFEEC0 skip, 0x00CFEC00 read, 0x00CFE9D0 refill).
//!
//! The stream object (the loader's `BufferedStream`) wraps an underlying
//! stream (typically the inflate decompress stream) with a byte buffer:
//!
//! | offset | field |
//! |---|---|
//! | `+4` | the underlying stream (virtual `read(ptr, count)` at vtable +0x30) |
//! | `+8` | buffer |
//! | `+0xc` | read position in the buffer |
//! | `+0x10` | number of valid bytes in the buffer (0 = not filled yet) |
//! | `+0x14` | buffer capacity |
//!
//! Every resource parser (textures, models, motions ...) reads its headers
//! and payloads through these three functions; texture loading skips the
//! mip levels it does not need through [`stream_skip`]. The original skips
//! one byte per loop iteration, which was a large part of the loader
//! threads' busy time; the port skips whole runs inside the buffer.
//!
//! The refill keeps its guest calls to the underlying stream (virtual
//! methods at vtable +0x10, +0x24, +0x30, +0x44) in the original's order
//! and frames, so a stream implemented by another port or the lifted code
//! sees the same calls.
//!
//! Translated from 0x00CFEEC0, 0x00CFEC00 and 0x00CFE9D0 (DDDA.exe
//! 2017-12-13 Steam).
use crate::semantic::call_from;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{call_ext, Cpu};

mod field {
    pub const SOURCE: u32 = 4;
    pub const BUFFER: u32 = 8;
    pub const POSITION: u32 = 0xc;
    pub const VALID: u32 = 0x10;
    pub const CAPACITY: u32 = 0x14;
}
mod source_slot {
    pub const STATE_OBJECT: u32 = 0x10;
    pub const TELL: u32 = 0x44;
    pub const SIZE: u32 = 0x24;
    pub const READ: u32 = 0x30;
}
/// The error code the state object reports when a short read is retried (0x018D0230).
const RETRY_ERROR_ADDRESS: u32 = 0x018d_0230;

/// A virtual call on the underlying stream `source` from the original's
/// frame `frame`: thiscall, `args` pushed, vtable slot `slot`.
#[allow(clippy::too_many_arguments)]
fn source_call(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, source: u32, slot: u32, args: &[u32], back: u32) -> u32 {
    let vtable = pg.u32(m, source);
    let target = pg.u32(m, vtable.wrapping_add(slot));
    call_from(m, c, pg, frame, source, args, back, |m, c| call_ext(m, c, target))
}

/// The CRT memcpy (which behaves as memmove) of `len` bytes, through the
/// page table when one page holds each side.
fn copy_guest(m: &mut dyn Mem, pg: Pages, destination: u32, source: u32, len: u32) {
    const READ: u8 = 1;
    const WRITE: u8 = 2;
    let len = len as usize;
    if len == 0 {
        return;
    }
    if let (Some(from), Some(to)) = (pg.bytes(source, len, READ), pg.bytes(destination, len, WRITE)) {
        // SAFETY: bytes() returned `len` readable and writable bytes, each inside one backed page
        unsafe { std::ptr::copy(from, to, len) };
        return;
    }
    let data: Vec<u8> = (0..len).map(|k| pg.u8(m, source.wrapping_add(k as u32))).collect();
    for (k, b) in data.iter().enumerate() {
        pg.set_u8(m, destination.wrapping_add(k as u32), *b);
    }
}

/// 0x00CFE9D0, `bool stream->refill()` (fastcall, plain `ret`): moves the
/// unread tail of the buffer to its front and reads more from the
/// underlying stream to fill it; returns whether the buffer now holds any
/// bytes. `entry` is ESP at the function's entry. Returns the original's
/// EAX (the bool is in AL, the rest is what the code left there).
fn refill(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, entry: u32, this: u32) -> u32 {
    let frame = entry.wrapping_sub(0x1c); // after `sub esp, 0x10` and four pushes
    let position = pg.u32(m, this.wrapping_add(field::POSITION));
    let valid = pg.u32(m, this.wrapping_add(field::VALID));
    let unread = valid.wrapping_sub(position);
    if valid == 0 {
        let capacity = pg.u32(m, this.wrapping_add(field::CAPACITY));
        pg.set_u32(m, this.wrapping_add(field::VALID), capacity);
    }
    let buffer = pg.u32(m, this.wrapping_add(field::BUFFER));
    if unread != 0 {
        // the unread tail moves to the front (source above destination: a plain move)
        let position = pg.u32(m, this.wrapping_add(field::POSITION));
        copy_guest(m, pg, buffer, buffer.wrapping_add(position), unread);
    }
    let source = pg.u32(m, this.wrapping_add(field::SOURCE));
    let valid_now = pg.u32(m, this.wrapping_add(field::VALID));
    let want = valid_now.wrapping_sub(unread);
    let mut eax = source_call(m, c, pg, frame, source, source_slot::READ, &[buffer.wrapping_add(unread), want], 0x00cf_ea21);
    let mut got = eax;
    if got != want {
        let state = source_call(m, c, pg, frame, source, source_slot::STATE_OBJECT, &[], 0x00cf_ea37);
        eax = state;
        if pg.u32(m, state.wrapping_add(0x1c)) == pg.u32(m, RETRY_ERROR_ADDRESS) {
            // the original pushed EBX: its calls here are from one word lower
            let frame = frame.wrapping_sub(4);
            let tell = source_call(m, c, pg, frame, source, source_slot::TELL, &[], 0x00cf_ea57);
            let size = source_call(m, c, pg, frame, source, source_slot::SIZE, &[], 0x00cf_ea62);
            eax = size;
            if size != tell {
                let mut remaining = want.wrapping_sub(got);
                let mut retries = 10u32;
                if remaining != 0 {
                    loop {
                        let tell = source_call(m, c, pg, frame, source, source_slot::TELL, &[], 0x00cf_ea79);
                        let size = source_call(m, c, pg, frame, source, source_slot::SIZE, &[], 0x00cf_ea84);
                        eax = size;
                        if size == tell || retries == 0 {
                            break;
                        }
                        let destination = got.wrapping_add(unread).wrapping_add(pg.u32(m, this.wrapping_add(field::BUFFER)));
                        let n = source_call(m, c, pg, frame, source, source_slot::READ, &[destination, remaining], 0x00cf_eaa7);
                        eax = n;
                        got = got.wrapping_add(n);
                        retries -= 1;
                        remaining = remaining.wrapping_sub(n);
                        if remaining == 0 {
                            break;
                        }
                    }
                }
            }
        }
    }
    let new_valid = unread.wrapping_add(got);
    pg.set_u32(m, this.wrapping_add(field::VALID), new_valid);
    pg.set_u32(m, this.wrapping_add(field::POSITION), 0);
    (eax & !0xff) | (new_valid != 0) as u32
}

/// 0x00CFE9D0 (fastcall: ECX = stream; plain `ret`).
pub fn stream_refill(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    c.eax = refill(m, c, pg, entry, c.ecx);
    c.esp = entry.wrapping_add(4);
}

/// 0x00CFEEC0, `stream->skip(count)` (thiscall, `ret 4`): advance the
/// position by `count` bytes, refilling the buffer when it runs out (a
/// refill that yields nothing still consumes one count).
pub fn stream_skip(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let mut count = pg.u32(m, entry.wrapping_add(4));
    while count != 0 {
        let position = pg.u32(m, this.wrapping_add(field::POSITION));
        let valid = pg.u32(m, this.wrapping_add(field::VALID));
        if position < valid {
            // the original steps one byte at a time through the buffer
            let run = count.min(valid - position);
            pg.set_u32(m, this.wrapping_add(field::POSITION), position + run);
            count -= run;
        } else {
            // the refill is called from the skip's frame (after two pushes and the call's return address)
            let refill_entry = entry.wrapping_sub(8).wrapping_sub(4);
            let eax = refill(m, c, pg, refill_entry, this);
            if eax & 0xff != 0 {
                let position = pg.u32(m, this.wrapping_add(field::POSITION));
                pg.set_u32(m, this.wrapping_add(field::POSITION), position.wrapping_add(1));
            }
            count -= 1;
        }
    }
    c.esp = entry.wrapping_add(8);
}

/// 0x00CFEC00, `size_t stream->read(dst, count)` (thiscall, `ret 8`):
/// copies `count` bytes out of the buffer; when the buffer cannot supply
/// them all, copies what it has, reads the rest straight from the
/// underlying stream and refills the buffer with one capacity-sized read.
pub fn stream_read(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let destination = pg.u32(m, entry.wrapping_add(4));
    let count = pg.u32(m, entry.wrapping_add(8));
    let position = pg.u32(m, this.wrapping_add(field::POSITION));
    let valid = pg.u32(m, this.wrapping_add(field::VALID));
    let buffer = pg.u32(m, this.wrapping_add(field::BUFFER));
    if position.wrapping_add(count) < valid {
        copy_guest(m, pg, destination, buffer.wrapping_add(position), count);
        pg.set_u32(m, this.wrapping_add(field::POSITION), position.wrapping_add(count));
        c.eax = count;
    } else {
        let available = valid.wrapping_sub(position);
        if valid == 0 {
            let capacity = pg.u32(m, this.wrapping_add(field::CAPACITY));
            pg.set_u32(m, this.wrapping_add(field::VALID), capacity);
        }
        copy_guest(m, pg, destination, buffer.wrapping_add(position), available);
        let source = pg.u32(m, this.wrapping_add(field::SOURCE));
        let frame = entry.wrapping_sub(0x10);
        let rest = source_call(m, c, pg, frame, source, source_slot::READ, &[available.wrapping_add(destination), count.wrapping_sub(available)], 0x00cf_ec4a);
        let total = available.wrapping_add(rest);
        let source = pg.u32(m, this.wrapping_add(field::SOURCE));
        let valid_now = pg.u32(m, this.wrapping_add(field::VALID));
        let buffer = pg.u32(m, this.wrapping_add(field::BUFFER));
        let filled = source_call(m, c, pg, frame, source, source_slot::READ, &[buffer, valid_now], 0x00cf_ec5f);
        pg.set_u32(m, this.wrapping_add(field::VALID), filled);
        pg.set_u32(m, this.wrapping_add(field::POSITION), 0);
        c.eax = total;
    }
    c.esp = entry.wrapping_add(12);
}

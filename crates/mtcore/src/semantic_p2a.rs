//! L172 P2a: semantic ports of the parallel shadow branch's recording-lane
//! machinery. A "lane" is one recording context a worker fills while the
//! shadow job's units run in parallel; the root context owns the merged
//! command stream. Both ports keep the original's ABI-visible effects (EAX
//! unused, callee-saved registers, ESP after `ret 8`, every guest write above
//! the stack) and make no guest calls: the CRT `memset` / `memcpy` the
//! originals call are done natively here (`memcpy` has `memmove` semantics in
//! this CRT, so overlapping copies stay exact).
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

/// Bytes between two lanes in the lane array (the size of one recording context).
const LANE_STRIDE: u32 = 0x33c84;

/// `memcpy` of the CRT at 0x012EFEA0, overlap-safe: read the source, then
/// store it page by page.
fn copy_guest(m: &mut dyn Mem, pg: Pages, destination: u32, source: u32, length: u32) {
    if length == 0 {
        return;
    }
    let mut bytes = vec![0u8; length as usize];
    m.read_bytes(source, &mut bytes);
    store_guest(m, pg, destination, &bytes);
}

/// Store `bytes` at `address`: in-page runs through the page table (aligned
/// words), the rest byte by byte.
fn store_guest(m: &mut dyn Mem, pg: Pages, address: u32, bytes: &[u8]) {
    let mut done = 0usize;
    while done < bytes.len() {
        let at = address.wrapping_add(done as u32);
        let room = (4096 - (at & 4095)) as usize;
        let run = room.min(bytes.len() - done);
        if let Some(target) = pg.bytes(at, run, ddnative_base::mem::WRITE) {
            let mut i = 0usize;
            while i + 4 <= run && (target as usize + i) & 3 == 0 {
                let word = u32::from_le_bytes([bytes[done + i], bytes[done + i + 1], bytes[done + i + 2], bytes[done + i + 3]]);
                // SAFETY: `bytes()` returned `run` writable bytes inside one backed page
                unsafe { ddnative_base::mem::store_at(target.add(i), 4, word) };
                i += 4;
            }
            while i < run {
                // SAFETY: as above
                unsafe { ddnative_base::mem::store_at(target.add(i), 1, bytes[done + i] as u32) };
                i += 1;
            }
        } else {
            for i in 0..run {
                m.set_u8(at.wrapping_add(i as u32), bytes[done + i]);
            }
        }
        done += run;
    }
}

// Translated from 0x00fed200 (DDDA.exe 2017-12-13 Steam).
/// 0x00FED200, `context->init_lanes(lanes, count)` (thiscall, `ret 8`):
/// initialise `count` recording lanes at `lanes` (stride 0x33C84).
///
/// The context's `+0x4c` units are split evenly (`units / count`, a divide
/// error for `count == 0` as in the original); lane `i` gets the share
/// `[start, start + 8 * share)` with `start = context[+0x54] + 8 * share * i`
/// (lane `+0x2c`; `+0x34` and `+0x38` hold the share, `+0x40` the lane
/// number). Each lane starts empty (counters and cursors zeroed), takes the
/// current frame's cursors from the globals at 0x018D12CC (the draw
/// manager: `+0x24`, `+0x28`, `+0x34`, and `+0x3c` as an address) and the
/// render singleton 0x018D08F4 (`+0x433e04`, `+0x433e08`, and the entries of
/// its three-deep ring picked by `[+0x33dbc] % 3` at `+0x433dec` / `+0x433df8`),
/// and gets a copy of the context's first 0x144 + 8 * `[+0x14]` bytes at
/// `+0xb4` as its command header (at lane `+0x9c`).
pub fn init_recording_lanes(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let (context, entry) = (c.ecx, c.esp);
    let lanes = pg.u32(m, entry.wrapping_add(4));
    let count = pg.u32(m, entry.wrapping_add(8));
    let units = pg.u32(m, context.wrapping_add(0x4c));
    let share = units.checked_div(count).expect("x86 divide error");
    let mut start_offset = 0u32;
    for lane in 0..count {
        // `esi` of the original: lane + 0x18
        let at = lanes.wrapping_add(lane.wrapping_mul(LANE_STRIDE)).wrapping_add(0x18);
        let word = |offset: u32| at.wrapping_add(offset);
        let draw_manager = pg.u32(m, 0x018d_12cc);
        pg.set_u32(m, word(8), draw_manager.wrapping_add(0x3c));
        let value = pg.u32(m, draw_manager.wrapping_add(0x24));
        pg.set_u32(m, at, value);
        let value = pg.u32(m, draw_manager.wrapping_add(0x34));
        pg.set_u32(m, at.wrapping_sub(4), value);
        let value = pg.u32(m, draw_manager.wrapping_add(0x28));
        pg.set_u32(m, word(4), value);
        let render = pg.u32(m, 0x018d_08f4);
        let value = pg.u32(m, render.wrapping_add(0x433e04));
        pg.set_u32(m, word(0x18), value);
        let value = pg.u32(m, render.wrapping_add(0x433e08));
        pg.set_u32(m, word(0x24), value);
        for offset in [0x1c, 0x14, 0x28, 0x20, 0x10, 0xc, 0x34, 0x2c, 0x30, 0x3c, 0x3085c, 0x30c60, 0x38, 0x40, 0x44] {
            pg.set_u32(m, word(offset), 0);
        }
        // memset(lane + 0x64, 0, 0x30), then the ring entry in its first word
        store_guest(m, pg, word(0x64), &[0u8; 0x30]);
        let ring = pg.u32(m, render.wrapping_add(0x33dbc)) % 3;
        let value = pg.u32(m, render.wrapping_add(ring * 4).wrapping_add(0x433dec));
        pg.set_u32(m, word(0x64), value);
        for offset in [0x14u32, 0x10, 0xc, 8] {
            pg.set_u32(m, at.wrapping_sub(offset), 0);
        }
        let value = pg.u32(m, render.wrapping_add(ring * 4).wrapping_add(0x433df8));
        pg.set_u32(m, word(0x8c), value);
        for offset in [0x94, 0x98, 0x33c64, 0x33c68] {
            pg.set_u32(m, word(offset), 0);
        }
        let first = pg.u32(m, context.wrapping_add(0x54)).wrapping_add(start_offset);
        pg.set_u32(m, word(0x30), 0);
        pg.set_u32(m, word(0x2c), first);
        pg.set_u32(m, word(0x38), share);
        pg.set_u32(m, word(0x34), share);
        pg.set_u32(m, word(0x40), lane);
        let header = pg.u32(m, context.wrapping_add(0x14)).wrapping_mul(8).wrapping_add(0x144);
        copy_guest(m, pg, word(0x9c), context.wrapping_add(0xb4), header);
        start_offset = start_offset.wrapping_add(share.wrapping_mul(8));
    }
    c.esp = entry.wrapping_add(4 + 8);
}

// Translated from 0x00fed3b0 (DDDA.exe 2017-12-13 Steam).
/// 0x00FED3B0, `context->merge_lanes(lanes, count)` (thiscall, `ret 8`):
/// append the `count` lanes' recordings to the root context in lane order.
///
/// Each lane's command words (`[lane+0x44]`, `[lane+0x48]` eight-byte
/// commands) are appended to the context's command buffer (`+0x44`, length
/// `+0x48`). Each of the lane's nested command arrays (`[lane+0x30c78]`
/// records at `lane+0x30878`; record `+8` the data, `+0xc` its length in
/// eight-byte units) is copied into the context's buffer from its far end
/// (cursor `+0x4c` counts down), the record is re-pointed at the copy and
/// appended to the context's own record list (`+0x30878`, count `+0x30c78`).
pub fn merge_recording_lanes(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let (context, entry) = (c.ecx, c.esp);
    let lanes = pg.u32(m, entry.wrapping_add(4));
    let count = pg.u32(m, entry.wrapping_add(8));
    for lane in 0..count {
        // `ebp` of the original: lane + 0x44
        let at = lanes.wrapping_add(0x44).wrapping_add(lane.wrapping_mul(LANE_STRIDE));
        let commands = pg.u32(m, at.wrapping_add(4));
        let source = pg.u32(m, at);
        let used = pg.u32(m, context.wrapping_add(0x48));
        let buffer = pg.u32(m, context.wrapping_add(0x44));
        copy_guest(m, pg, buffer.wrapping_add(used.wrapping_mul(8)), source, commands.wrapping_mul(8));
        let used = pg.u32(m, context.wrapping_add(0x48)).wrapping_add(commands);
        pg.set_u32(m, context.wrapping_add(0x48), used);
        let nested = pg.u32(m, at.wrapping_add(0x30c34));
        for k in 0..nested {
            let record = pg.u32(m, at.wrapping_add(0x30834).wrapping_add(4 * k));
            let length = pg.u32(m, record.wrapping_add(0xc));
            let cursor = pg.u32(m, context.wrapping_add(0x4c)).wrapping_sub(length);
            let destination = pg.u32(m, context.wrapping_add(0x44)).wrapping_add(cursor.wrapping_mul(8));
            pg.set_u32(m, context.wrapping_add(0x4c), cursor);
            let data = pg.u32(m, record.wrapping_add(8));
            copy_guest(m, pg, destination, data, length.wrapping_mul(8));
            pg.set_u32(m, record.wrapping_add(8), destination);
            let listed = pg.u32(m, context.wrapping_add(0x30c78));
            pg.set_u32(m, context.wrapping_add(listed.wrapping_mul(4)).wrapping_add(0x30878), record);
            pg.set_u32(m, context.wrapping_add(0x30c78), listed.wrapping_add(1));
        }
    }
    c.esp = entry.wrapping_add(4 + 8);
}

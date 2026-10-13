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

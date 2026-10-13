"""Regenerates crates/mtcore/src/shadow_primitive.rs (L172 P2b, s9): the
sPrimitive singleton's per-lane begin/end methods, translated mechanically
by research/native/semport_s9.py, plus the hand-written dispatch thunks.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe from the repository root.
"""
import subprocess
import sys
from pathlib import Path

PY = sys.executable
ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / 'research/native/semport_s9.py'


def translate(entry, last, name, doc):
    out = ROOT / 'target' / f'{name}.rs.tmp'
    out.parent.mkdir(exist_ok=True)
    docfile = out.with_suffix('.doc')
    docfile.write_text(doc)
    subprocess.run([PY, str(TOOL), hex(entry), hex(last), name, str(out), str(docfile)], check=True, cwd=ROOT / 'research/native')
    text = out.read_text()
    return text.replace('pg.u32(m, 0x018d0b28u32)', 'scheduler(m, pg)')


VF11_DOC = '''/// 0x00E61010, `sPrimitive::vf11(this, context, mode, flag, id)` (thiscall, `ret 0x10`):
/// open one nested primitive list. With `mode == 0` the primitive lane of
/// `context + 0x58` (`this + 0x30 + lane * 4`) begins a list (0x0111D610 with
/// the last argument); otherwise every lane (`this + 0xD0` of them) begins
/// one, and the result is true when any of them did not.
// Translated from 0x00E61010 (DDDA.exe 2017-12-13 Steam), mechanically by
// research/native/semport_s9.py (one commented statement per instruction).
'''
VF12_DOC = '''/// 0x00E68390, `sPrimitive::vf12(this, context, lanes, parallel)` (thiscall,
/// `ret 0xc`): close the primitive lists of a unit pass.
///
/// Serial (`parallel == 0`): sort the lane's pool (the allocator vtable at
/// 0x0187663C allocates and frees a scratch copy, 0x0111E180 sorts it,
/// the unit's virtual +0x3C consumes it) and end the list (0x0111D650).
/// Parallel: queue one sorting job (0x0111E180, `DBE560` kind 2) per
/// non-empty pool of the eight lanes' four pools in the scheduler at
/// `[0x018D0B28]` (`+0x8C` job count, jobs from `+0xC0`), run them, merge
/// the sorted runs by descending key into one array, then either hand each
/// lane its slice (serial finish) or queue one draw job per lane
/// (0x010B7440, kind 3) after publishing the lanes' recording contexts at
/// `[0x018D0B28] + 0x1BDD0 + lane * 0x20`; finally end every list.
/// The scheduler is read through [`scheduler`] only.
// Translated from 0x00E68390 (DDDA.exe 2017-12-13 Steam), mechanically by
// research/native/semport_s9.py (one commented statement per instruction).
'''
HEADER = '''//! L172 P2b (s9): native ports of the sPrimitive singleton methods the
//! shadow job's units reach through `0x0107FF00` / `0x0107FF30`
//! (`*0x018D1848`, vtable `0x01430E08`, slots +0x2C / +0x30): `vf11`
//! 0x00E61010 and `vf12` 0x00E68390, and the two dispatch thunks. The
//! bodies are mechanical translations (registers and flags in locals, the
//! stack in guest memory so every callee sees the original's frame); their
//! callees stay guest calls. Verified by `tests/shadow_primitive.rs`
//! against Unicorn runs of the original (`research/native/
//! record_shadow_primitive.py`). Regenerate with
//! `research/native/gen_shadow_primitive.py`.
use crate::semantic_rt as rt;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{call_ext, Cpu};

/// The scheduler object the unit callbacks queue into (`[0x018D0B28]`).
/// Every scheduler access of the translated methods goes through here.
#[inline(always)]
fn scheduler(m: &dyn Mem, pg: Pages) -> u32 {
    pg.u32(m, 0x018d_0b28)
}

/// The CRT memcpy (memmove semantics, overlap-safe).
fn copy_guest(m: &mut dyn Mem, pg: Pages, dst: u32, src: u32, len: u32) {
    let backward = dst.wrapping_sub(src) < len;
    let mut done = 0u32;
    while done < len {
        let left = len - done;
        let (offset, chunk) = if backward {
            let since_boundary = |a: u32| match a.wrapping_add(left) & 0xfff {
                0 => 0x1000,
                n => n,
            };
            let chunk = left.min(since_boundary(src)).min(since_boundary(dst));
            (left - chunk, chunk)
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
'''
SORT = '''
/// 0x0111E180, `pool->sort_pairs(scratch)` (thiscall, `ret 4`): the kind-2
/// sort job of the sPrimitive pools. Sort the pool's current list (a count
/// and an array of 8-byte records in its descriptor `[pool + 4]`, at
/// `desc + depth * 16 - 4 / - 8`) by the unsigned second dword, largest
/// first, stable, with a bottom-up merge sort of doubling run widths
/// through `scratch` (merge pass 0x0111D490). EAX returns the count.
///
/// The merge pass copies each pair of runs into `scratch` (same indices) and
/// merges back; the last pass spans the whole list, so `scratch` ends up
/// holding the list as it was before that pass.
// Translated from 0x0111E180 and 0x0111D490 (DDDA.exe 2017-12-13 Steam).
pub fn sprimitive_sort(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let pool = c.ecx;
    let scratch = pg.u32(m, entry.wrapping_add(4));
    c.esp = entry.wrapping_add(8);
    let desc = pg.u32(m, pool.wrapping_add(4));
    let row = desc.wrapping_add(pg.u32(m, desc.wrapping_add(0x64)).wrapping_mul(16));
    let count = pg.u32(m, row.wrapping_sub(4));
    let array = if count != 0 { pg.u32(m, row.wrapping_sub(8)) } else { 0 };
    c.eax = count;
    let n = count as i32;
    if n <= 1 {
        return;
    }
    let n = n as usize;
    let mut list: Vec<[u32; 2]> = (0..n as u32)
        .map(|i| {
            let at = array.wrapping_add(8 * i);
            [pg.u32(m, at), pg.u32(m, at.wrapping_add(4))]
        })
        .collect();
    let mut before_last = Vec::new();
    let mut run = vec![[0u32; 2]; n];
    let mut width = 1usize;
    while width < n {
        before_last.clone_from(&list);
        let mut lo = 0usize;
        while lo + width < n {
            let hi = (lo + 2 * width).min(n);
            let mid = lo + width;
            run[lo..hi].copy_from_slice(&list[lo..hi]);
            let (mut left, mut right, mut out) = (lo, mid, lo);
            while left < mid && right < hi {
                // the right record goes first only when its key is larger
                if run[right][1] <= run[left][1] {
                    list[out] = run[left];
                    left += 1;
                } else {
                    list[out] = run[right];
                    right += 1;
                }
                out += 1;
            }
            let rest = if left < mid { &run[left..mid] } else { &run[right..hi] };
            list[out..out + rest.len()].copy_from_slice(rest);
            lo += 2 * width;
        }
        width *= 2;
    }
    for (i, record) in list.iter().enumerate() {
        let at = array.wrapping_add(8 * i as u32);
        pg.set_u32(m, at, record[0]);
        pg.set_u32(m, at.wrapping_add(4), record[1]);
    }
    for (i, record) in before_last.iter().enumerate() {
        let at = scratch.wrapping_add(8 * i as u32);
        pg.set_u32(m, at, record[0]);
        pg.set_u32(m, at.wrapping_add(4), record[1]);
    }
}
'''
THUNKS = '''
/// 0x0107FF00, `sPrimitive::begin(a0, a1, a2, a3)` (cdecl, caller pops):
/// forward the four stack arguments to virtual +0x2C of the singleton at
/// `0x018D1848`; the known vf11 is called directly.
// Translated from 0x0107FF00 (DDDA.exe 2017-12-13 Steam).
pub fn dispatch_begin(m: &mut dyn Mem, c: &mut Cpu) {
    dispatch(m, c, 4, 0x2c, 0x00e6_1010, 0x0107_ff21, sprimitive_vf11)
}

/// 0x0107FF30, `sPrimitive::end(a0, a1, a2)` (cdecl, caller pops): forward
/// the three stack arguments to virtual +0x30 of the singleton; the known
/// vf12 is called directly.
// Translated from 0x0107FF30 (DDDA.exe 2017-12-13 Steam).
pub fn dispatch_end(m: &mut dyn Mem, c: &mut Cpu) {
    dispatch(m, c, 3, 0x30, 0x00e6_8390, 0x0107_ff4c, sprimitive_vf12)
}

fn dispatch(m: &mut dyn Mem, c: &mut Cpu, count: u32, slot: u32, known: u32, back: u32, native: fn(&mut dyn Mem, &mut Cpu)) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let singleton = pg.u32(m, 0x018d_1848);
    let target = pg.u32(m, pg.u32(m, singleton).wrapping_add(slot));
    // push the arguments again, last first (they keep their order)
    let mut esp = entry;
    for k in (0..count).rev() {
        let v = pg.u32(m, entry.wrapping_add(4 + 4 * k));
        esp = esp.wrapping_sub(4);
        pg.set_u32(m, esp, v);
    }
    esp = esp.wrapping_sub(4);
    pg.set_u32(m, esp, back);
    c.esp = esp;
    c.ecx = singleton;
    if target == known {
        native(m, c);
    } else {
        call_ext(m, c, target);
    }
    // the callee popped the arguments; RET
    c.esp = entry.wrapping_add(4);
}
'''
vf11 = translate(0xe61010, 0xe61077, 'sprimitive_vf11', VF11_DOC)
vf12 = translate(0xe68390, 0xe68a24, 'sprimitive_vf12', VF12_DOC)
(ROOT / 'crates/mtcore/src/shadow_primitive.rs').write_text(HEADER + '\n' + vf11 + '\n' + vf12 + THUNKS, newline='\n') if False else None
open(ROOT / 'crates/mtcore/src/shadow_primitive.rs', 'w', newline='\n').write(HEADER + '\n' + vf11 + '\n' + vf12 + SORT + THUNKS)
print('ok')

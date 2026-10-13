//! Semantic port (L176) of the draw recording's record sort 0x00FEDD60,
//! the hottest unowned nearest-lifted function on the job threads in
//! profile_c34 (120 samples over the four runs). It keeps the original's
//! ABI-visible result (EAX, the callee-saved registers, ESP, every memory
//! write outside the popped stack scratch); ECX, EDX, the flags and the
//! scratch are not reproduced. Checked against the original's recorded
//! vectors in tests/material.rs; opt-in through crates/ddports
//! (`DDRUNTIME_RECORD_SORT=1`).
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{self, Cpu};

/// The CRT memcpy (cdecl), a platform model.
const MEMCPY: u32 = 0x012E_FEA0;

// Translated from 0x00FEDD60 (DDDA.exe 2017-12-13 Steam).
/// 0x00FEDD60, `sort(records, count, scratch) -> sorted` (stdcall,
/// `ret 0xc`): a stable bottom-up merge sort of `count` 8-byte records by
/// their first word (unsigned), between `records` and `scratch`. The first
/// pass orders pairs into `scratch`; each merge pass then doubles the run
/// width, alternating the buffers, up to the smallest power of two above
/// `count` (at most 32 doublings). Returns the buffer that holds the
/// result (`records` when there are fewer than two).
///
/// Every pass writes all records to its buffer, so the port runs the same
/// passes in guest memory with the original's run boundaries and its order
/// of reads and writes (exact even if the buffers overlap); passes with
/// runs of 32 or more copy a run's tail with memcpy, as the original does.
pub fn sort_records(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let records = pg.u32(m, entry.wrapping_add(4));
    // (the count is reread while the width grows, as the original)
    let mut top: i32 = 1;
    let mut doublings = 0;
    loop {
        if pg.u32(m, entry.wrapping_add(8)) as i32 <= top {
            break;
        }
        doublings += 1;
        top = top.wrapping_add(top);
        if doublings >= 32 {
            break;
        }
    }
    let n = pg.u32(m, entry.wrapping_add(8)) as i32;
    let done = |c: &mut Cpu, eax: u32| {
        c.eax = eax;
        c.esp = entry.wrapping_add(0x10);
    };
    if top < 2 {
        return done(c, records);
    }
    let scratch = pg.u32(m, entry.wrapping_add(0xc));
    if let Some(sorted) = sort_apart(m, pg, records, scratch, n, top) {
        return done(c, sorted);
    }
    // the original's frame, for its memcpy calls: 0x30 bytes of locals
    // and four saved registers
    let frame = entry.wrapping_sub(0x40);

    // pairs, from the records into the scratch
    let mut out = scratch;
    let mut i = 0i32;
    let mut p = records;
    while i < n {
        let a = pg.u32(m, p);
        if i.wrapping_add(1) >= n {
            pg.set_u32(m, out, a);
            let v = pg.u32(m, p.wrapping_add(4));
            pg.set_u32(m, out.wrapping_add(4), v);
            out = out.wrapping_add(8);
        } else {
            if a > pg.u32(m, p.wrapping_add(8)) {
                for k in [8u32, 0xc, 0, 4] {
                    let v = pg.u32(m, p.wrapping_add(k));
                    pg.set_u32(m, out.wrapping_add(k ^ 8), v);
                }
            } else {
                pg.set_u32(m, out, a);
                for k in [4u32, 8, 0xc] {
                    let v = pg.u32(m, p.wrapping_add(k));
                    pg.set_u32(m, out.wrapping_add(k), v);
                }
            }
            out = out.wrapping_add(16);
        }
        i = i.wrapping_add(2);
        p = p.wrapping_add(16);
    }

    let mut width: i32 = 4;
    let mut in_scratch = true;
    while width <= top {
        let (to, from) = if in_scratch { (records, scratch) } else { (scratch, records) };
        if n > 0 {
            merge_pass(m, c, pg, from, to, n, width, frame);
        }
        width = width.wrapping_add(width);
        in_scratch = !in_scratch;
    }
    done(c, if in_scratch { scratch } else { records })
}

/// The sort when the two buffers do not overlap: every pass writes all
/// `n` records to its buffer, so each buffer ends with the last pass that
/// wrote it. After the pass of width `w` each aligned block of `w`
/// records is that block of the input, stably sorted (the merge sort's
/// invariant), so the result is the input read once, the whole stably
/// sorted into the last pass's buffer and its half-width blocks into the
/// other's (pass 1, the pairs, goes to the scratch; passes alternate).
/// None when the buffers overlap or wrap: the passes then run in guest
/// memory as the original's.
fn sort_apart(m: &mut dyn Mem, pg: Pages, records: u32, scratch: u32, n: i32, top: i32) -> Option<u32> {
    let len = (n as u64) * 8;
    let (a, b) = (records as u64, scratch as u64);
    if n < 2 || a + len > 1 << 32 || b + len > 1 << 32 || (a < b + len && b < a + len) {
        return None;
    }
    let input: Vec<(u32, u32)> = (0..n as u32)
        .map(|k| {
            let at = records.wrapping_add(8 * k);
            (pg.u32(m, at), pg.u32(m, at.wrapping_add(4)))
        })
        .collect();
    let blocks = |w: usize| -> Vec<(u32, u32)> {
        let mut v = input.clone();
        for chunk in v.chunks_mut(w) {
            chunk.sort_by_key(|r| r.0); // stable
        }
        v
    };
    let passes = top.trailing_zeros(); // top = 2^passes
    let into = |pass: u32| if pass % 2 == 1 { scratch } else { records };
    let mut write = |to: u32, v: &[(u32, u32)]| {
        for (k, &(key, value)) in v.iter().enumerate() {
            let at = to.wrapping_add(8 * k as u32);
            pg.set_u32(m, at, key);
            pg.set_u32(m, at.wrapping_add(4), value);
        }
    };
    if passes >= 2 {
        write(into(passes - 1), &blocks(1 << (passes - 1)));
    }
    write(into(passes), &blocks(1 << passes));
    Some(into(passes))
}

/// One merge pass: adjacent runs of `width / 2` records of `from` merged,
/// stably, into runs of `width` in `to`.
#[allow(clippy::too_many_arguments)]
fn merge_pass(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, from: u32, to: u32, n: i32, width: i32, frame: u32) {
    let at = |i: i32| from.wrapping_add((i as u32).wrapping_mul(8));
    let half = width / 2;
    let mut out = to;
    let mut start = 0i32;
    let mut mid = half;
    loop {
        let left_end = if n < mid { n } else { mid };
        let right_end = {
            let r = half.wrapping_add(left_end);
            if n < r { n } else { r }
        };
        let mut l = start;
        let mut r = left_end;
        let count = right_end.wrapping_sub(start);
        let mut k = 0i32;
        while k < count {
            if l >= left_end {
                // the right run's rest
                if r < right_end {
                    out = tail(m, c, pg, out, at(r), right_end.wrapping_sub(r), width, frame);
                }
                break;
            }
            if r >= right_end {
                // the left run's rest
                out = tail(m, c, pg, out, at(l), left_end.wrapping_sub(l), width, frame);
                break;
            }
            let lk = pg.u32(m, at(l));
            if lk > pg.u32(m, at(r)) {
                for o in [0u32, 4] {
                    let v = pg.u32(m, at(r).wrapping_add(o));
                    pg.set_u32(m, out.wrapping_add(o), v);
                }
                r = r.wrapping_add(1);
            } else {
                pg.set_u32(m, out, lk);
                let v = pg.u32(m, at(l).wrapping_add(4));
                pg.set_u32(m, out.wrapping_add(4), v);
                l = l.wrapping_add(1);
            }
            out = out.wrapping_add(8);
            k = k.wrapping_add(1);
        }
        start = start.wrapping_add(width);
        mid = mid.wrapping_add(width);
        if start >= n {
            return;
        }
    }
}

/// `count` records from `src` to `out`: one by one for runs under 32
/// records, by memcpy (from the original's frame) for longer ones.
#[allow(clippy::too_many_arguments)]
fn tail(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, out: u32, src: u32, count: i32, width: i32, frame: u32) -> u32 {
    let bytes = (count as u32).wrapping_mul(8);
    if width < 32 {
        let mut o = out;
        let mut s = src;
        for _ in 0..count {
            for k in [0u32, 4] {
                let v = pg.u32(m, s.wrapping_add(k));
                pg.set_u32(m, o.wrapping_add(k), v);
            }
            o = o.wrapping_add(8);
            s = s.wrapping_add(8);
        }
        return o;
    }
    c.esp = frame;
    for v in [bytes, src, out, 0x00FE_E082] {
        c.esp = c.esp.wrapping_sub(4);
        pg.set_u32(m, c.esp, v);
    }
    x86::call_local(m, c, MEMCPY, ddnative_base::platform::memcpy);
    c.esp = frame;
    out.wrapping_add(bytes)
}

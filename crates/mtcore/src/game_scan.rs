//! Semantic ports (L183) of three game-side searches the main thread runs
//! each frame: the nearest object of a type (0x0049E920), the per-player
//! table scan of vf06 0x00417120 (0x004172C0) and the segment-box test
//! (0x00D1B840). Each keeps the original's ABI-visible result: EAX, the
//! callee-saved registers, ESP and every memory write outside the popped
//! stack scratch, and makes the original's guest calls with the registers
//! and stack the original has at them. ECX, EDX, the flags and the scratch
//! are not reproduced. Checked against the original's recorded vectors in
//! tests/game_scan.rs; opt-in through crates/ddports (`DDRUNTIME_SCAN=1`).
//!
//! Floats are computed with the host's SSE (`f32`), under the guest's live
//! MXCSR, in the original's operation order. `COMISS a, b` then `JA` is
//! `a > b` (false when unordered); `JBE` is its negation.
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{self, Cpu};

fn f(pg: Pages, m: &dyn Mem, a: u32) -> f32 {
    f32::from_bits(pg.u32(m, a))
}

fn push(m: &mut dyn Mem, pg: Pages, c: &mut Cpu, v: u32) {
    c.esp = c.esp.wrapping_sub(4);
    pg.set_u32(m, c.esp, v);
}

// --- 0x0049E920: the nearest object of a type ---------------------------------------

/// The three type names 0x0049E920 tests (globals holding name pointers).
const TYPE_A: u32 = 0x019A_5E30;
const TYPE_B: u32 = 0x019A_60B0;
const TYPE_C: u32 = 0x019A_60D0;
/// The entries' initial distance (-1.0), the radius scale (2.0) and the
/// absolute-value mask, in the image.
const MINUS_ONE: u32 = 0x0161_C20C;
const RADIUS_SCALE: u32 = 0x014F_9E04;
const ABS_MASK: u32 = 0x0161_C330;

/// `object->vf04()` (the type descriptor): a getter that is `mov eax,
/// imm32; ret` is read, not called (the same result, no other effect);
/// anything else is called as the original calls it.
#[allow(clippy::too_many_arguments)]
fn descriptor(m: &mut dyn Mem, c: &mut Cpu, s: u32, object: u32, ebx: u32, edi: u32, ebp: u32, resume: u32) -> u32 {
    let pg = Pages::of(m);
    let vtable = pg.u32(m, object);
    let target = pg.u32(m, vtable.wrapping_add(0x10));
    if pg.u8(m, target) == 0xB8 && pg.u8(m, target.wrapping_add(5)) == 0xC3 {
        return pg.u32(m, target.wrapping_add(1));
    }
    c.esp = s;
    push(m, pg, c, resume);
    (c.eax, c.ecx, c.edx, c.ebx, c.ebp, c.esi, c.edi) = (target, object, vtable, ebx, ebp, object, edi);
    x86::call_ext(m, c, target);
    c.eax
}

/// Whether descriptor `d` or one of its parents (+0x10) has the name the
/// global `name` holds (read after the call, as the original does).
fn is_a(m: &dyn Mem, pg: Pages, d: u32, name: u32) -> bool {
    let want = pg.u32(m, name);
    if pg.u32(m, d.wrapping_add(4)) == want {
        return true;
    }
    let mut p = pg.u32(m, d.wrapping_add(0x10));
    while p != 0 {
        if pg.u32(m, p.wrapping_add(4)) == want {
            return true;
        }
        p = pg.u32(m, p.wrapping_add(0x10));
    }
    false
}

/// 0x0049E920 `nearest(owner, skip, id, position, radius, height, id2)`
/// (stdcall, `ret 0x1c`) -> object. Translated from 0x0049E920 (DDDA.exe
/// 2017-12-13 Steam). Of the `owner+0x24` objects listed at `owner+0x30`
/// (all but `skip`) whose flags (+4) have kind 2 and bit 0x400, each is
/// typed through its descriptor: an object of none of the three types ends
/// the search with 0; one of type A whose +0x2574 is `id` (and +0x2534 is
/// `id2`, unless 0), with radius > 0 (or NaN), within `2 * radius` in xz
/// of `position` (+0x40, +0x48) and within `height` in y (+0x44), is a
/// candidate. The first 256 candidates (object, xz distance) are bubble
/// sorted by distance (stable; an unordered pair is left as it is) and the
/// first is returned (0 without any).
pub fn nearest_object(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let e = c.esp;
    // the arguments are read where the original reads them
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, e.wrapping_add(4 + 4 * k));
    let owner = arg(m, 0);
    let (ebx0, ebp0, esi0, edi0) = (c.ebx, c.ebp, c.esi, c.edi);
    let ebp = e.wrapping_sub(4);
    // the frame after the prologue: aligned to 8, 0x80c of locals, 3 pushes
    let s = (ebp & !7).wrapping_sub(0x80c + 12);
    let unset = pg.u32(m, MINUS_ONE);
    let count = pg.u32(m, owner.wrapping_add(0x24));
    let mut found: Vec<(u32, u32)> = Vec::new();
    let mut n = 0u32;
    let (mut ebx, mut edi) = (ebx0, edi0);
    let finish = |c: &mut Cpu, eax: u32| {
        (c.eax, c.ebx, c.ebp, c.esi, c.edi) = (eax, ebx0, ebp0, esi0, edi0);
        c.esp = e.wrapping_add(4 + 0x1c);
    };
    for i in 0..count {
        let object = pg.u32(m, pg.u32(m, owner.wrapping_add(0x30)).wrapping_add(i.wrapping_mul(4)));
        if object == arg(m, 1) {
            continue;
        }
        let flags = pg.u32(m, object.wrapping_add(4));
        if flags & 7 != 2 || flags & 0x400 == 0 {
            continue;
        }
        let d = descriptor(m, c, s, object, ebx, edi, ebp, 0x0049_E9A1);
        ebx = if is_a(m, pg, d, TYPE_A) { object } else { 0 };
        let d = descriptor(m, c, s, object, ebx, edi, ebp, 0x0049_E9D2);
        edi = if is_a(m, pg, d, TYPE_B) { object } else { 0 };
        let d = descriptor(m, c, s, object, ebx, edi, ebp, 0x0049_EA0B);
        let third = is_a(m, pg, d, TYPE_C);
        if ebx == 0 {
            if edi == 0 && !third {
                return finish(c, 0);
            }
            continue;
        }
        if pg.u32(m, ebx.wrapping_add(0x2574)) != arg(m, 2) {
            continue;
        }
        let id2 = arg(m, 6);
        if id2 != 0 && pg.u32(m, ebx.wrapping_add(0x2534)) != id2 {
            continue;
        }
        let position = arg(m, 3);
        let dz = f(pg, m, position.wrapping_add(8)) - f(pg, m, object.wrapping_add(0x48));
        let dx = f(pg, m, position) - f(pg, m, object.wrapping_add(0x40));
        let y = f(pg, m, object.wrapping_add(0x44));
        let distance = (dz * dz + dx * dx).sqrt();
        let r = f32::from_bits(arg(m, 4));
        // COMISS 0, r; JNC: on when 0 >= r (ordered)
        if 0.0 >= r {
            continue;
        }
        if distance > r * f(pg, m, RADIUS_SCALE) {
            continue;
        }
        let dy = f32::from_bits((f(pg, m, position.wrapping_add(4)) - y).to_bits() & pg.u32(m, ABS_MASK));
        if dy > f32::from_bits(arg(m, 5)) {
            continue;
        }
        if n < 0x100 {
            found.push((object, distance.to_bits()));
        }
        n = n.wrapping_add(1);
    }
    if n > 0x100 {
        // the original's sort runs past its 256 entries into the frame
        // above them: done on the guest stack, as it does it
        (c.ebx, c.ebp, c.esi, c.edi) = (ebx0, ebp0, esi0, edi0);
        return overflowing_sort(m, c, s, ebp, &found, n, unset);
    }
    // bubble sort: pass k compares pairs (j, j + 1) for j < n - 1 - k
    let last = n as usize;
    for end in (1..last).rev() {
        for j in 0..end {
            if f32::from_bits(found[j].1) > f32::from_bits(found[j + 1].1) {
                found.swap(j, j + 1);
            }
        }
    }
    finish(c, found.first().map_or(0, |e| e.0));
}

/// The sort of more than 256 candidates on the guest stack: the frame as
/// the original has it (its pushes, the 256 entries at `s + 0x18`), the
/// bubble sort over `n` entries running past the array, then the
/// original's epilogue (the registers popped from the frame).
#[allow(clippy::too_many_arguments)]
fn overflowing_sort(m: &mut dyn Mem, c: &mut Cpu, s: u32, ebp: u32, found: &[(u32, u32)], n: u32, unset: u32) {
    let pg = Pages::of(m);
    let e = ebp.wrapping_add(4);
    pg.set_u32(m, ebp, c.ebp);
    pg.set_u32(m, s.wrapping_add(8), c.ebx);
    pg.set_u32(m, s.wrapping_add(4), c.esi);
    pg.set_u32(m, s, c.edi);
    pg.set_u32(m, s.wrapping_add(0xc), n);
    for k in 0..0x100u32 {
        let (p, d) = found.get(k as usize).copied().unwrap_or((0, unset));
        pg.set_u32(m, s.wrapping_add(0x18 + 8 * k), p);
        pg.set_u32(m, s.wrapping_add(0x1c + 8 * k), d);
    }
    let back = pg.u32(m, e);
    let at = |k: u32| s.wrapping_add(0x18).wrapping_add(k.wrapping_mul(8));
    for end in (1..n).rev() {
        for j in 0..end {
            let (x, y) = (at(j), at(j + 1));
            if f(pg, m, x.wrapping_add(4)) > f(pg, m, y.wrapping_add(4)) {
                let (p, d) = (pg.u32(m, x), pg.u32(m, x.wrapping_add(4)));
                let (q, r) = (pg.u32(m, y), pg.u32(m, y.wrapping_add(4)));
                pg.set_u32(m, x, q);
                pg.set_u32(m, x.wrapping_add(4), r);
                pg.set_u32(m, y, p);
                pg.set_u32(m, y.wrapping_add(4), d);
            }
        }
    }
    assert_eq!(pg.u32(m, e), back, "0x0049E920: the sort moved its own return address");
    c.eax = pg.u32(m, s.wrapping_add(0x18));
    c.edi = pg.u32(m, s);
    c.esi = pg.u32(m, s.wrapping_add(4));
    c.ebx = pg.u32(m, s.wrapping_add(8));
    c.ebp = pg.u32(m, ebp);
    c.esp = e.wrapping_add(4 + 0x1c);
}

// --- 0x004172C0: vf06's table scan ---------------------------------------------------

/// The scan's rows (key, row index; 0x5C of them, 8 bytes each).
const ROWS: u32 = 0x0153_B6E8;
/// The keyed table searched for each row's key (97 entries of 0x14 bytes:
/// key, a list of 5 ids).
const KEYED: u32 = 0x0152_31E0;
const KEYED_BYTES: u32 = 0x794;
/// Per (row, slot) thresholds: a float scaled by 30.0 and a count.
const LEVELS: u32 = 0x0153_B9C8;
const COUNTS: u32 = 0x0153_BF68;
const SCALE: u32 = 0x014E_A160;
/// The player manager (its three player slots at +0x9A0).
const PLAYERS: u32 = 0x018F_A4EC;
/// The unlock taken for an id: `unlock(state = EAX, id, 1)` (`ret 8`).
const UNLOCK: u32 = 0x005B_4800;

/// 0x004172C0 `scan(this)` (stdcall, `ret 4`; vf06 0x00417120's).
/// Translated from 0x004172C0 (DDDA.exe 2017-12-13 Steam). For each row
/// (key, r) and each of the three players whose state (`[+0x2E64]+0x6C`)
/// is not locked (flag 4 at +0x206D clear, or no state, or its byte +0x10
/// zero) and whose data (`this + 0x20 + 4 * player`) is set: the key's
/// entry of the keyed table gives up to five ids (an id 0, or a key not in
/// the table, ends the player's turn); an id not yet set in the state's
/// bitset (+8; ids from 0x2840 count as unset) is unlocked when the
/// player's value r (a float) is at least `LEVELS[r][k] * 30` and its
/// count r (+0x120) at least `COUNTS[r][k]`, else the turn ends.
pub fn scan_unlocks(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let e = c.esp;
    let (ebx0, ebp0, esi0, edi0) = (c.ebx, c.ebp, c.esi, c.edi);
    let s = e.wrapping_sub(0x2c);
    let mut eax = c.eax;
    // the keyed table's keys read so far (a prefix: the original reads the
    // entries up to the first match each time), kept until a call
    let mut prefix: Vec<u32> = Vec::with_capacity((KEYED_BYTES / 0x14) as usize);
    for off in (0..0x2e0u32).step_by(8) {
        let key = pg.u32(m, ROWS + off);
        let row = pg.u32(m, ROWS + off + 4);
        eax = key;
        for player in 0..3u32 {
            let slot = 0x9a0 + 4 * player;
            let state_owner = pg.u32(m, pg.u32(m, PLAYERS).wrapping_add(slot));
            if state_owner == 0 {
                continue;
            }
            // the lock byte (AL), then SETZ
            let locked = if pg.u8(m, state_owner.wrapping_add(0x206d)) & 4 == 0 {
                eax &= !0xff;
                0
            } else {
                let a = pg.u32(m, state_owner.wrapping_add(0x2e64));
                if a == 0 {
                    eax = 0;
                    0
                } else {
                    let b = pg.u32(m, a.wrapping_add(0x6c));
                    if b == 0 {
                        eax = 0;
                        0
                    } else {
                        let v = pg.u8(m, b.wrapping_add(0x10));
                        eax = (b & !0xff) | v as u32;
                        v
                    }
                }
            };
            eax = (eax & !0xff) | (locked == 0) as u32;
            if locked != 0 {
                continue;
            }
            let state = pg.u32(m, pg.u32(m, state_owner.wrapping_add(0x2e64)).wrapping_add(0x6c));
            eax = state;
            if state == 0 {
                continue;
            }
            // `this` (the argument) is read only here
            let this = pg.u32(m, e.wrapping_add(4));
            let data = pg.u32(m, this.wrapping_add(slot).wrapping_sub(0x980));
            if data == 0 {
                continue;
            }
            for k in 0..5u32 {
                let entry = match prefix.iter().position(|&k0| k0 == key) {
                    Some(j) => Some(KEYED + 0x14 * j as u32),
                    None => loop {
                        let j = prefix.len() as u32;
                        if j >= KEYED_BYTES / 0x14 {
                            break None;
                        }
                        let k0 = pg.u32(m, KEYED + 0x14 * j);
                        prefix.push(k0);
                        if k0 == key {
                            break Some(KEYED + 0x14 * j);
                        }
                    },
                };
                let Some(entry) = entry else {
                    eax = KEYED + KEYED_BYTES;
                    break;
                };
                eax = entry;
                let id = pg.u32(m, pg.u32(m, entry.wrapping_add(4)).wrapping_add(4 * k));
                if id == 0 {
                    break;
                }
                if id < 0x2840 {
                    let bits = pg.u32(m, state.wrapping_add(8));
                    let set = pg.u32(m, bits.wrapping_add((id >> 5) * 4)) & (1 << (id & 31)) != 0;
                    eax = ((id >> 5) & !0xff) | set as u32;
                    if set {
                        continue;
                    }
                }
                let at = k.wrapping_add(row.wrapping_mul(5)).wrapping_mul(4);
                eax = at;
                let level = f(pg, m, LEVELS.wrapping_add(at)) * f(pg, m, SCALE);
                if level > f(pg, m, data.wrapping_add(row.wrapping_mul(4))) {
                    break;
                }
                let have = pg.u32(m, data.wrapping_add(row.wrapping_mul(4)).wrapping_add(0x120));
                if have < pg.u32(m, COUNTS.wrapping_add(at)) {
                    break;
                }
                // the call, with the original's registers and frame
                c.esp = s;
                push(m, pg, c, 1);
                push(m, pg, c, id);
                push(m, pg, c, 0x0041_7423);
                (c.eax, c.ecx, c.edx, c.ebx, c.ebp, c.esi, c.edi) = (state, have, id, data, row, 4 * k + 4, k);
                x86::call_ext(m, c, UNLOCK);
                eax = c.eax;
                prefix.clear();
            }
        }
    }
    (c.eax, c.ebx, c.ebp, c.esi, c.edi) = (eax, ebx0, ebp0, esi0, edi0);
    c.esp = e.wrapping_add(8);
}

// --- 0x00D1B840: a segment against a box --------------------------------------------

const EPSILON: u32 = 0x0140_7124;
const ONE: u32 = 0x0140_6CE0;
const FLT_MAX: u32 = 0x0161_C1F4;
const NEG_FLT_MAX: u32 = 0x0140_8E5C;
const SMALL: u32 = 0x0140_8E58;
const LANE_MASK: u32 = 0x0162_AC60;
const ALL_ONES: u32 = 0x0162_AC30;

/// `1 / v`, or 0 when `v == 0` (UCOMISS, LAHF, TEST AH 0x44, JNP: equal
/// and ordered).
fn inverse(one: f32, v: f32) -> f32 {
    if v == 0.0 {
        0.0
    } else {
        one / v
    }
}

/// One slab of the test with a significant direction: the entry and exit
/// parameters `t1`, `t2` against the segment's `t_end`, narrowing `(lo,
/// hi)`. False when the segment misses the slab.
fn slab(t1: f32, t2: f32, t_end: f32, lo: &mut f32, hi: &mut f32, lo_bound: f32, hi_bound: f32) -> bool {
    if 0.0 > t1 && 0.0 > t2 {
        return false;
    }
    if t1 > t_end && t2 > t_end {
        return false;
    }
    let (a, b) = if t1 > t2 { (t2, t1) } else { (t1, t2) };
    if a > lo_bound {
        *lo = a;
    }
    if hi_bound > b {
        *hi = b;
    }
    !(*lo > *hi)
}

/// 0x00D1B840 `hits(box, segment, t_out)` (cdecl) -> AL. Translated from
/// 0x00D1B840 (DDDA.exe 2017-12-13 Steam). A segment (p0 at +0, p1 at
/// +0x10) shorter than sqrt(epsilon) is a point: inside the box (+0 min,
/// +0x10 max, per lane under the image's lane mask), `*t_out = 0`.
/// Otherwise the slab test along the unit direction, in the order x, z, y
/// (a direction component under 0.001 in size tests the start point
/// against that slab instead); on a hit `*t_out` is the entry parameter
/// (distance along the direction).
pub fn segment_hits_box(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let e = c.esp;
    let (bx, sg) = (pg.u32(m, e.wrapping_add(4)), pg.u32(m, e.wrapping_add(8)));
    c.esp = e.wrapping_add(4);
    // `t_out` is read only where it is written
    let out = |m: &dyn Mem| pg.u32(m, e.wrapping_add(12));
    match segment_test(m, pg, bx, sg) {
        Test::Point(inside) => {
            let out = out(m);
            if out != 0 {
                pg.set_u32(m, out, 0);
            }
            c.eax = inside as u32;
        }
        Test::Hit(t) => {
            let out = out(m);
            if out != 0 {
                pg.set_u32(m, out, t.to_bits());
            }
            c.eax = (out & !0xff) | 1;
        }
        // AH holds the last LAHF's flags (UCOMISS ux, 0): ZF PF CF and bit 1
        Test::Miss(ux) => {
            let ah = if ux.is_nan() {
                0x47
            } else if ux == 0.0 {
                0x42
            } else if ux < 0.0 {
                0x03
            } else {
                0x02
            };
            c.eax = (sg & 0xffff_0000) | ah << 8;
        }
    }
}

enum Test {
    /// a segment shorter than sqrt(epsilon): whether its start is in the box
    Point(bool),
    /// the entry parameter
    Hit(f32),
    /// a miss, after the direction's x component (ux) was compared with 0
    Miss(f32),
}

fn segment_test(m: &dyn Mem, pg: Pages, bx: u32, sg: u32) -> Test {
    let v = |a: u32| f(pg, m, a);
    let (p0x, p0y, p0z) = (v(sg), v(sg.wrapping_add(4)), v(sg.wrapping_add(8)));
    let (p1x, p1y, p1z) = (v(sg.wrapping_add(0x10)), v(sg.wrapping_add(0x14)), v(sg.wrapping_add(0x18)));
    let dy = p1y - p0y;
    let dx = p1x - p0x;
    let dz = p1z - p0z;
    let length2 = dy * dy + dx * dx + dz * dz;
    if v(EPSILON) > length2 {
        // CMPLEPS min <= p, p <= max; lanes under the mask count as inside
        let mut mask = 0;
        for k in 0..4u32 {
            let p = v(sg.wrapping_add(4 * k));
            let inside = v(bx.wrapping_add(4 * k)) <= p && p <= v(bx.wrapping_add(0x10 + 4 * k));
            let lane = pg.u32(m, LANE_MASK + 4 * k);
            let all = pg.u32(m, ALL_ONES + 4 * k);
            if (!lane & if inside { u32::MAX } else { 0 }) | (lane & all) == all {
                mask |= 1 << k;
            }
        }
        return Test::Point(mask == 0xf);
    }
    let one = v(ONE);
    let (minx, miny, minz) = (v(bx), v(bx.wrapping_add(4)), v(bx.wrapping_add(8)));
    let (maxx, maxy, maxz) = (v(bx.wrapping_add(0x10)), v(bx.wrapping_add(0x14)), v(bx.wrapping_add(0x18)));
    let inv = one / length2.sqrt();
    let (ux, uy, uz) = (dx * inv, dy * inv, dz * inv);
    let ivz = inverse(one, uz);
    let ivy = inverse(one, uy);
    let ivx = inverse(one, ux);
    let abs_mask = pg.u32(m, ABS_MASK);
    let abs = |x: f32| f32::from_bits(x.to_bits() & abs_mask);
    let small = v(SMALL);
    let (mut lo, mut hi) = (v(NEG_FLT_MAX), v(FLT_MAX));
    // x (its bounds: the initial ones, as constants)
    if small > abs(ux) {
        if minx > p0x || p0x > maxx {
            return Test::Miss(ux);
        }
    } else {
        let (blo, bhi) = (lo, hi);
        if !slab(ivx * (minx - p0x), ivx * (maxx - p0x), ivx * dx, &mut lo, &mut hi, blo, bhi) {
            return Test::Miss(ux);
        }
    }
    // z
    if small > abs(uz) {
        if minz > p0z || p0z > maxz {
            return Test::Miss(ux);
        }
    } else {
        let (blo, bhi) = (lo, hi);
        if !slab(ivz * (minz - p0z), ivz * (maxz - p0z), dz * ivz, &mut lo, &mut hi, blo, bhi) {
            return Test::Miss(ux);
        }
    }
    // y
    if small > abs(uy) {
        if miny > p0y || p0y > maxy {
            return Test::Miss(ux);
        }
    } else {
        let (blo, bhi) = (lo, hi);
        if !slab(ivy * (miny - p0y), ivy * (maxy - p0y), dy * ivy, &mut lo, &mut hi, blo, bhi) {
            return Test::Miss(ux);
        }
    }
    Test::Hit(lo)
}

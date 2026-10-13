//! Semantic ports (L183) of the two box queries of the collision AABB tree
//! (called from 0x010A8570): 0x0109D480 (an axis-aligned box per lane)
//! and 0x0109DAC0 (a box against the children's boxes by separating axes).
//! Both walk the tree depth first through a record stack of their own and
//! call a guest callback for every leaf whose box passes; they keep the
//! original's ABI-visible result (EAX, the callee-saved registers, ESP, and
//! every memory write outside the popped stack scratch: the record stack is
//! global memory) and make the callback calls with the original's ECX,
//! arguments and stack. Checked against the original's recorded vectors in
//! tests/tree_query.rs; opt-in through crates/ddports (`DDRUNTIME_SCAN=1`).
//!
//! The tree (`this + 0x58`, 0x70-byte nodes): a node's word +0 holds per
//! child k a "node" bit in byte k of `& 0x08040201` and a "leaf" bit in
//! byte k of `& 0x80402010`; +4 four u16 child indices; +0x10.. its four
//! children's boxes as lanes (min x, y, z at +0x10, +0x20, +0x30; max x, y,
//! z at +0x40, +0x50, +0x60). A record (32 bytes): the four lanes' pass
//! masks, the node word's two bit sets (+0x10 node, +0x14 leaf) and the
//! node (+0x18). The records live in one of 0x19 * depth buffers chosen by
//! the caller's slot (`[0x018D5E20 + 4 * (depth * 0x19 + slot)]`, the
//! slot's depth counter at `0x018D5EE8 + 4 * slot` counting nesting).
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{self, Cpu};

const DEPTHS: u32 = 0x018D_5EE8;
const BUFFERS: u32 = 0x018D_5E20;
/// The query box's centre scale and the separating-axis slack, per lane.
const CENTRE: u32 = 0x018D_0630;
const SLACK: u32 = 0x018D_0660;

fn f(pg: Pages, m: &dyn Mem, a: u32) -> f32 {
    f32::from_bits(pg.u32(m, a))
}

fn push(m: &mut dyn Mem, pg: Pages, c: &mut Cpu, v: u32) {
    c.esp = c.esp.wrapping_sub(4);
    pg.set_u32(m, c.esp, v);
}

/// One function's frame and call sites.
struct Shape {
    /// locals below the 16-aligned frame pointer (before the three pushes)
    locals: u32,
    /// the slot >= 0x19 fallback (stdcall, five arguments) and its return
    fallback: u32,
    fallback_back: u32,
    /// the leaf callback's return addresses, per child
    back: [u32; 4],
}

/// The query: which lanes of `node` pass.
type Test = fn(&dyn Mem, Pages, u32, u32) -> [bool; 4];

/// The walk both functions share (thiscall `query(box, ctx, callback, arg,
/// slot)`, `ret 0x14`): `callback(ctx in ECX; arg, index, 0)` (`ret 0xc`)
/// for every passing leaf, ORing its results; a non-zero result ends the
/// walk. Returns the result.
fn walk(m: &mut dyn Mem, c: &mut Cpu, shape: &Shape, test: Test) {
    let pg = Pages::of(m);
    let e = c.esp;
    let this = c.ecx;
    let (ebx0, ebp0, esi0, edi0) = (c.ebx, c.ebp, c.esi, c.edi);
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, e.wrapping_add(4 + 4 * k));
    let ebp = e.wrapping_sub(4);
    let s = (ebp & !15).wrapping_sub(shape.locals + 12);
    let finish = |c: &mut Cpu, eax: u32| {
        (c.eax, c.ebx, c.ebp, c.esi, c.edi) = (eax, ebx0, ebp0, esi0, edi0);
        c.esp = e.wrapping_add(4 + 0x14);
    };
    let slot = arg(m, 4);
    if slot >= 0x19 {
        let tree = pg.u32(m, this.wrapping_add(0x58));
        c.esp = s;
        for k in [3, 2, 1, 0] {
            let v = arg(m, k);
            push(m, pg, c, v);
        }
        push(m, pg, c, tree);
        push(m, pg, c, shape.fallback_back);
        (c.ecx, c.ebp) = (this, ebp);
        x86::call_ext(m, c, shape.fallback);
        let r = c.eax;
        return finish(c, r);
    }
    let depth = pg.u32(m, DEPTHS.wrapping_add(slot.wrapping_mul(4)));
    let stack = pg.u32(m, BUFFERS.wrapping_add(depth.wrapping_mul(0x19).wrapping_add(slot).wrapping_mul(4)));
    pg.set_u32(m, DEPTHS.wrapping_add(slot.wrapping_mul(4)), depth.wrapping_add(1));
    let query = arg(m, 0);
    // a node's record at `at`
    let record = |m: &mut dyn Mem, at: u32, node: u32| {
        let lanes = test(m, pg, query, node);
        for (k, &pass) in lanes.iter().enumerate() {
            pg.set_u32(m, at.wrapping_add(4 * k as u32), if pass { u32::MAX } else { 0 });
        }
        pg.set_u32(m, at.wrapping_add(0x18), node);
        let word = pg.u32(m, node);
        pg.set_u32(m, at.wrapping_add(0x14), word & 0x8040_2010);
        let word = pg.u32(m, node);
        pg.set_u32(m, at.wrapping_add(0x10), word & 0x0804_0201);
    };
    let root = pg.u32(m, this.wrapping_add(0x58));
    record(m, stack, root, );
    let mut count = 1u32;
    let mut result = 0u32;
    while count != 0 {
        count -= 1;
        let at = stack.wrapping_add(count.wrapping_mul(0x20));
        let masks: [u32; 4] = std::array::from_fn(|k| pg.u32(m, at.wrapping_add(4 * k as u32)));
        let nodes = pg.u32(m, at.wrapping_add(0x10));
        let leaves = pg.u32(m, at.wrapping_add(0x14));
        let node = pg.u32(m, at.wrapping_add(0x18));
        for k in 0..4u32 {
            if masks[k as usize] == 0 {
                continue;
            }
            let index = pg.u16(m, node.wrapping_add(4 + 2 * k)) as u32;
            if (nodes >> (8 * k)) & 0xff != 0 {
                let child = pg.u32(m, this.wrapping_add(0x58)).wrapping_add(index.wrapping_mul(0x70));
                record(m, stack.wrapping_add(count.wrapping_mul(0x20)), child);
                count += 1;
            } else if (leaves >> (8 * k)) & 0xff != 0 {
                let (ctx, callback, data) = (arg(m, 1), arg(m, 2), arg(m, 3));
                c.esp = s;
                push(m, pg, c, 0);
                push(m, pg, c, index);
                push(m, pg, c, data);
                push(m, pg, c, shape.back[k as usize]);
                (c.eax, c.ecx, c.ebp, c.esi) = (index, ctx, ebp, query);
                x86::call_ext(m, c, callback);
                result |= c.eax;
                if result != 0 {
                    count = 0;
                    break;
                }
            }
        }
    }
    let slot = arg(m, 4);
    let at = DEPTHS.wrapping_add(slot.wrapping_mul(4));
    let d = pg.u32(m, at);
    pg.set_u32(m, at, d.wrapping_sub(1));
    finish(c, result);
}

/// 0x0109D480's lanes: the query (lanes of min x, y, z at +0x10, +0x20,
/// +0x30 and max at +0x40, +0x50, +0x60) overlaps the child's box
/// (CMPLEPS: ordered).
fn overlap(m: &dyn Mem, pg: Pages, q: u32, node: u32) -> [bool; 4] {
    std::array::from_fn(|k| {
        let k = 4 * k as u32;
        (0..3u32).all(|axis| {
            let o = 0x10 * axis + k;
            f(pg, m, q.wrapping_add(0x10 + o)) <= f(pg, m, node.wrapping_add(0x40 + o))
                && f(pg, m, node.wrapping_add(0x10 + o)) <= f(pg, m, q.wrapping_add(0x40 + o))
        })
    })
}

/// 0x0109D480 `query_boxes`. Translated from 0x0109D480 (DDDA.exe
/// 2017-12-13 Steam): the walk with `overlap`.
pub fn query_boxes(m: &mut dyn Mem, c: &mut Cpu) {
    let shape = Shape {
        locals: 0x354,
        fallback: 0x0109_B820,
        fallback_back: 0x0109_D4B4,
        back: [0x0109_D719, 0x0109_D842, 0x0109_D96B, 0x0109_DA94],
    };
    walk(m, c, &shape, overlap)
}

/// 0x0109DAC0's lanes: the query box (min at +0x10, max at +0x20, x y z
/// w) against each child's box by the separating axes of two axis-aligned
/// boxes along the box edges' cross products. Centres are (min + max) *
/// CENTRE (the query's per component, the children's per lane), half
/// extents max - centre; lane k uses SLACK[k] on the query's extents for
/// the cross axes. Every comparison is ordered (`<=`, false with a NaN).
fn separating_axes(m: &dyn Mem, pg: Pages, q: u32, node: u32) -> [bool; 4] {
    let scale: [f32; 4] = std::array::from_fn(|k| f(pg, m, CENTRE + 4 * k as u32));
    let slack: [f32; 4] = std::array::from_fn(|k| f(pg, m, SLACK + 4 * k as u32));
    let qc: [f32; 3] = std::array::from_fn(|i| {
        let i = 4 * i as u32;
        (f(pg, m, q.wrapping_add(0x10 + i)) + f(pg, m, q.wrapping_add(0x20 + i))) * scale[i as usize / 4]
    });
    let qh: [f32; 3] = std::array::from_fn(|i| f(pg, m, q.wrapping_add(0x20 + 4 * i as u32)) - qc[i]);
    let qa = qh.map(f32::abs);
    std::array::from_fn(|k| {
        let lane = 4 * k as u32;
        let nc: [f32; 3] = std::array::from_fn(|i| {
            let o = 0x10 * i as u32 + lane;
            (f(pg, m, node.wrapping_add(0x10 + o)) + f(pg, m, node.wrapping_add(0x40 + o))) * scale[k]
        });
        let ne: [f32; 3] = std::array::from_fn(|i| f(pg, m, node.wrapping_add(0x40 + 0x10 * i as u32 + lane)) - nc[i]);
        let d: [f32; 3] = std::array::from_fn(|i| qc[i] - nc[i]);
        let e = slack[k];
        let [hx, hy, hz] = qh;
        let [ax, ay, az] = qa;
        let [dx, dy, dz] = d;
        let [ex, ey, ez] = ne;
        (dz * hx - dx * hz).abs() <= (az + e) * ex + (ax + e) * ez
            && dz.abs() <= az + ez
            && (dx * hy - dy * hx).abs() <= (ax + e) * ey + (ay + e) * ex
            && dx.abs() <= ax + ex
            && (dy * hz - dz * hy).abs() <= (az + e) * ey + (ay + e) * ez
            && dy.abs() <= ay + ey
    })
}

/// 0x0109DAC0 `query_box`. Translated from 0x0109DAC0 (DDDA.exe 2017-12-13
/// Steam): the walk with `separating_axes`. (The original takes absolute
/// values both with SSE masks and through 0x00422F80, an x87 load and
/// store that quiets a signalling NaN; either way the value only meets
/// ordered comparisons, which a NaN fails.)
pub fn query_box(m: &mut dyn Mem, c: &mut Cpu) {
    let shape = Shape {
        locals: 0x1474,
        fallback: 0x0109_B930,
        fallback_back: 0x0109_DAFC,
        back: [0x0109_EB58, 0x0109_F3BF, 0x0109_FB4F, 0x010A_02D3],
    };
    walk(m, c, &shape, separating_axes)
}

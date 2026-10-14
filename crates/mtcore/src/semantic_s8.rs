//! L184 s8: the IK chain set-up of `uCnsIK` (0x00E516F0), as readable Rust.
//!
//! Offsets and arithmetic come from the 2017-12-13 Steam executable. All
//! float math is IEEE single precision in the original's exact operation
//! order (SSE scalar code in the original), so results are bit-identical;
//! the structures stay in guest memory.
use ddnative_base::{
    mem::{Mem, Pages},
    x86::Cpu,
};

/// 1.0f, 2.0f and the length epsilon (2^-23) the original loads from
/// 0x01406CE0, 0x01406CEC and 0x01407124 (read-only data).
const ONE: f32 = 1.0;
const TWO: f32 = 2.0;
const EPSILON: f32 = 1.1920929e-7;

/// Joint record stride in the skeleton's joint array (+0x364).
const JOINT_STRIDE: u32 = 0xa0;
/// Chain node stride.
const NODE_STRIDE: u32 = 0x260;

// ---- guest layouts -----------------------------------------------------

/// Skeleton (`param_2`): joint array pointer, id -> joint-index table, and
/// the vtable slot returning the root matrix.
mod skeleton {
    pub const JOINTS: u32 = 0x364;
    pub const INDEX_BY_ID: u32 = 0x368;
    /// vtable +0x58, thiscall `root_matrix(-1) -> *const [f32; 16]`, `ret 4`
    pub const ROOT_MATRIX_SLOT: u32 = 0x58;
}

/// Joint record (0xA0 bytes).
mod joint {
    /// World matrix, 16 floats (three axis rows, then the position row).
    pub const WORLD: u32 = 0x10;
    /// Local rotation quaternion (x, y, z, w).
    pub const ROTATION: u32 = 0x60;
    /// Local scale (x, y, z).
    pub const SCALE: u32 = 0x70;
    /// Local translation (x, y, z).
    pub const TRANSLATION: u32 = 0x80;
    /// Parent joint index (0xFF: none, the skeleton's root matrix).
    pub const PARENT: u32 = 0x91;
}

/// Chain node (0x260 bytes).
mod node {
    /// Length of the chain direction vector (1.0 for unscaled chains).
    pub const AXIS_LENGTH: u32 = 0x4;
    /// Segment length bias and factor (inputs).
    pub const SEGMENT_BIAS: u32 = 0x8;
    pub const SEGMENT_FACTOR: u32 = 0xc;
    /// The node frame (look-at matrix: chain axis, its perpendicular and
    /// their cross product, then the position).
    pub const FRAME: u32 = 0x10;
    /// The joint's world matrix expressed in the node frame, 16 floats.
    pub const JOINT_IN_FRAME: u32 = 0x150;
    /// Axis scale lengths of the joint's world matrix (rows 0..3).
    pub const ROW_LENGTHS: u32 = 0x190;
    /// Joint id in the low byte.
    pub const JOINT_ID: u32 = 0x1a0;
    /// Set on the last node of the chain.
    pub const IS_LAST: u32 = 0x218;
    /// The resolved joint record (0 when the id has no joint).
    pub const JOINT: u32 = 0x220;
    /// Distance to the next node, scaled (output; 0 for the last node).
    pub const SEGMENT_LENGTH: u32 = 0x244;
}

/// The chain object (`this`): per-chain mode byte.
mod chain {
    /// Non-zero: joints carry scale (world matrices rebuilt with the
    /// parent's axis lengths); zero: rigid joints.
    pub const SCALED: u32 = 0xcfa;
    /// The `is_last` flag is written through `this` at this offset + node
    /// (the nodes start at `this + 0xd0`, so it is `node + 0x218`).
    pub const IS_LAST_THIS: u32 = 0x2e8;
}

/// Unscaled chains' per-node axis lengths: three floats read from the
/// globals 0x01629510..0x01629518 (1.0 each).
const UNIT_LENGTHS: u32 = 0x0162_9510;

// ---- small memory helpers ------------------------------------------------

#[inline(always)]
fn rf(pg: Pages, m: &dyn Mem, a: u32) -> f32 {
    f32::from_bits(pg.u32(m, a))
}
#[inline(always)]
fn wf(pg: Pages, m: &mut dyn Mem, a: u32, v: f32) {
    pg.set_u32(m, a, v.to_bits());
}
fn read_floats<const N: usize>(pg: Pages, m: &dyn Mem, a: u32) -> [f32; N] {
    let mut out = [0f32; N];
    for (i, v) in out.iter_mut().enumerate() {
        *v = rf(pg, m, a.wrapping_add(4 * i as u32));
    }
    out
}
fn write_floats(pg: Pages, m: &mut dyn Mem, a: u32, values: &[f32]) {
    for (i, &v) in values.iter().enumerate() {
        wf(pg, m, a.wrapping_add(4 * i as u32), v);
    }
}

/// `sqrt(((a*a + b*b) + c*c) + d*d)` in the original's association.
#[inline(always)]
fn length4(a: f32, b: f32, c: f32, d: f32) -> f32 {
    (((a * a) + (b * b)) + (c * c) + (d * d)).sqrt()
}

// ---- the entry point -------------------------------------------------------

/// The arguments of one chain set-up.
struct Chain {
    pg: Pages,
    /// `uCnsIK` (ECX): its mode byte and the nodes' `is_last` flags.
    this: u32,
    skeleton: u32,
    nodes: u32,
    count: i32,
    /// `mIKDir` / `mIKUp`: which joint matrix row the chain runs along / which one is "up"
    /// (0..2 = X, Y, Z row, 3..5 = the same negated).
    axis_selector: u32,
    up_selector: u32,
    /// ESP of the original's frame at its calls (after its prologue).
    call_frame: u32,
}

/// 0x00E516F0, `ik->init_chain(skeleton, nodes, count, axis_selector,
/// up_selector) -> ok` (thiscall, `ret 0x14`): prepare the node chain of
/// a `uCnsIK` constraint.
///
/// For every node, in order: look up its joint (id in the low byte of the
/// word at +0x1A0 through the skeleton's id table); fail when the id has
/// no joint. Mark the last node. Except for the first node, rebuild the
/// joint's world matrix from its parent's world matrix (a skeleton root
/// matrix through the virtual call at vtable +0x58 when the parent is
/// 0xFF) and its local rotation, translation and (scaled mode) scale.
/// Choose the chain axis and up vector from the joint's matrix rows
/// (`axis_selector`, `up_selector`), build the node's look-at frame from
/// them (0x01130A00, ported inline as [`look_at_frame`]'s core) at the
/// point where the joint meets the previous node's axis, and store the
/// joint's world matrix relative to that frame. Last, measure the
/// distance between neighbouring node frames (plus a bias, times a
/// factor) into every node's segment length; the set-up fails when one is
/// not above epsilon.
///
/// Returns AL = 1 on success, 0 on failure; the upper bytes of EAX are the
/// original's leftovers (reproduced: the node pointer or loop count it
/// held when it returned).
///
/// Translated from 0x00e516f0 (DDDA.exe 2017-12-13 Steam).
pub fn init_chain(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let eax_in = c.eax;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let chain = Chain {
        pg,
        this: c.ecx,
        skeleton: arg(m, 0),
        nodes: arg(m, 1),
        count: arg(m, 2) as i32,
        axis_selector: arg(m, 3),
        up_selector: arg(m, 4),
        // push ebp, `and esp, -16`, `sub esp, 0x334`, three pushes
        call_frame: (entry.wrapping_sub(4) & !0xf).wrapping_sub(0x334 + 12),
    };
    c.eax = chain.run(m, c, eax_in);
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4 + 0x14);
}


impl Chain {
    /// The whole set-up; returns EAX as the original leaves it.
    fn run(&self, m: &mut dyn Mem, c: &mut Cpu, eax_in: u32) -> u32 {
        let pg = self.pg;
        let (nodes, count) = (self.nodes, self.count);
        // The vectors are locals of the original that persist from node to
        // node (a selector above 5 keeps the previous node's choice).
        let mut axis = None;
        let mut up = None;
        // what EAX holds when the first loop ends (its counter, or the
        // caller's EAX when there are no nodes)
        let mut eax = eax_in;
        for index in 0..count.max(0) {
            let node = nodes.wrapping_add((index as u32).wrapping_mul(NODE_STRIDE));
            let joint = self.resolve_joint(m, node);
            pg.set_u32(m, node.wrapping_add(node::JOINT), joint);
            if joint == 0 {
                return 0;
            }
            // the flag is also reachable through `this` (nodes start at this + 0xd0)
            let flag_at = self.this.wrapping_add((index as u32).wrapping_mul(NODE_STRIDE)).wrapping_add(chain::IS_LAST_THIS);
            pg.set_u8(m, flag_at, (index == count - 1) as u8);
            if index > 0 {
                self.rebuild_world_matrix(m, c, joint);
            }
            // no valid selector: the vector stays as the original's stack
            // local was (the previous node's, or stack garbage at the first)
            axis = choose_axis(m, pg, joint, self.axis_selector).or(axis);
            up = choose_axis(m, pg, joint, self.up_selector).or(up);
            let stack_vector = |m: &dyn Mem, offset: u32| read_floats::<3>(pg, m, self.call_frame.wrapping_add(offset));
            let axis_now = axis.unwrap_or_else(|| stack_vector(m, 0x50));
            let up_now = up.unwrap_or_else(|| stack_vector(m, 0x60));
            let joint_rows = if self.scaled(m) {
                store_scaled_row_lengths(m, pg, node, joint, axis_now)
            } else {
                store_unit_row_lengths(m, pg, node, joint)
            };
            let origin = if index == 0 {
                read_floats::<3>(pg, m, joint.wrapping_add(joint::WORLD + 0x30))
            } else {
                point_on_previous_axis(m, pg, node, joint)
            };
            let frame = node.wrapping_add(node::FRAME);
            build_look_at_frame(m, pg, frame, axis_now, up_now, [origin[0], origin[1], origin[2], ONE]);
            store_joint_in_frame(m, pg, node, &joint_rows);
            eax = index as u32 + 1;
        }
        // segment lengths between neighbouring node frames
        let mut ptr = nodes.wrapping_add(0x44);
        for _ in 0..count.wrapping_sub(1).max(0) {
            let next = ptr.wrapping_add(NODE_STRIDE);
            let dy = rf(pg, m, next) - rf(pg, m, ptr);
            let dx = rf(pg, m, next.wrapping_sub(4)) - rf(pg, m, ptr.wrapping_sub(4));
            let dz = rf(pg, m, next.wrapping_add(4)) - rf(pg, m, ptr.wrapping_add(4));
            let dw = rf(pg, m, next.wrapping_add(8)) - rf(pg, m, ptr.wrapping_add(8));
            let distance = (((dy * dy) + (dx * dx)) + (dz * dz) + (dw * dw)).sqrt();
            let bias = rf(pg, m, ptr.wrapping_sub(0x3c));
            let factor = rf(pg, m, ptr.wrapping_sub(0x38));
            let segment = (distance + bias) * factor;
            wf(pg, m, ptr.wrapping_add(0x200), segment);
            if EPSILON >= segment {
                // fails (AL = 0); the rest of EAX is the node pointer
                return ptr & !0xff;
            }
            ptr = next;
            eax = ptr;
        }
        // the last node ends the chain
        let last = nodes.wrapping_add((count as u32).wrapping_mul(NODE_STRIDE)).wrapping_sub(0x1c);
        pg.set_u32(m, last, 0);
        (eax & !0xff) | 1
    }

    /// The chain's mode byte: non-zero for scaled joints.
    fn scaled(&self, m: &dyn Mem) -> bool {
        self.pg.u8(m, self.this.wrapping_add(chain::SCALED)) != 0
    }

    /// The node's joint record: through the id table; 0 when the id has no
    /// joint (index 0xFF).
    fn resolve_joint(&self, m: &dyn Mem, node: u32) -> u32 {
        let pg = self.pg;
        let id = pg.u32(m, node.wrapping_add(node::JOINT_ID)) & 0xff;
        let table = pg.u32(m, self.skeleton.wrapping_add(skeleton::INDEX_BY_ID));
        let slot = pg.u8(m, table.wrapping_add(id)) as u32;
        if slot == 0xff {
            0
        } else {
            slot.wrapping_mul(JOINT_STRIDE).wrapping_add(pg.u32(m, self.skeleton.wrapping_add(skeleton::JOINTS)))
        }
    }

    /// Recompute `joint`'s world matrix (16 floats at +0x10) from its
    /// parent's world matrix and its own local rotation/translation (and,
    /// in scaled mode, scale).
    fn rebuild_world_matrix(&self, m: &mut dyn Mem, c: &mut Cpu, joint: u32) {
        let pg = self.pg;
        let scaled = self.scaled(m);
        let parent = pg.u8(m, joint.wrapping_add(joint::PARENT)) as u32;
        let (parent_matrix, root) = if parent == 0xff {
            // the skeleton's root matrix: a virtual call (-1 selects it)
            let vtable = pg.u32(m, self.skeleton);
            let method = pg.u32(m, vtable.wrapping_add(skeleton::ROOT_MATRIX_SLOT));
            // return sites of the original's two call sites
            let back = if scaled { 0x00e5_18d0 } else { 0x00e5_260a };
            let matrix = crate::semantic::call_from(m, c, pg, self.call_frame, self.skeleton, &[u32::MAX], back, |m, c| {
                crate::semantic::call_guest(m, c, method)
            });
            (matrix, true)
        } else {
            let joints = pg.u32(m, self.skeleton.wrapping_add(skeleton::JOINTS));
            (parent.wrapping_mul(JOINT_STRIDE).wrapping_add(joints).wrapping_add(joint::WORLD), false)
        };
        let parent_world = read_floats::<16>(pg, m, parent_matrix);
        if scaled {
            // each axis of the new matrix is scaled by the joint's scale
            // (the root has none)
            let scale = if root { [ONE; 3] } else { read_floats::<3>(pg, m, joint.wrapping_add(joint::SCALE)) };
            rebuild_scaled(m, pg, joint, &parent_world, scale);
        } else {
            rebuild_rigid(m, pg, joint, &parent_world);
        }
    }
}

/// Rigid joints: the joint's local rotation (quaternion) and translation
/// as a matrix, times the parent's world matrix (row-vector convention:
/// `local * parent`).
fn rebuild_rigid(m: &mut dyn Mem, pg: Pages, joint: u32, p: &[f32; 16]) {
    let [qx, qy, qz, qw] = read_floats::<4>(pg, m, joint.wrapping_add(joint::ROTATION));
    let x2 = qx * TWO;
    let y2 = qy * TWO;
    let z2 = qz * TWO;
    let (x2w, y2w, z2w) = (x2 * qw, y2 * qw, z2 * qw);
    let (x2x, y2x, z2x) = (x2 * qx, y2 * qx, z2 * qx);
    let y2y = y2 * qy;
    let z2z = z2 * qz;
    let z2y = z2 * qy;
    // the rotation matrix rows
    let (r0, r1, r2) = (ONE - (z2z + y2y), y2x + z2w, z2x - y2w);
    let (r4, r5, r6) = (y2x - z2w, ONE - (z2z + x2x), z2y + x2w);
    let (r8, r9, r10) = (z2x + y2w, z2y - x2w, ONE - (y2y + x2x));
    let [tx, ty, tz] = read_floats::<3>(pg, m, joint.wrapping_add(joint::TRANSLATION));
    let zero = 0.0f32;
    let o = [
        // row 0
        p[0] * r0 + p[4] * r1 + p[8] * r2 + p[12] * zero,
        p[5] * r1 + p[1] * r0 + p[9] * r2 + p[13] * zero,
        p[6] * r1 + p[2] * r0 + p[10] * r2 + p[14] * zero,
        p[7] * r1 + p[3] * r0 + p[11] * r2 + p[15] * zero,
        // row 1
        r4 * p[0] + p[4] * r5 + p[8] * r6 + p[12] * zero,
        p[5] * r5 + p[1] * r4 + p[9] * r6 + p[13] * zero,
        p[6] * r5 + p[2] * r4 + p[10] * r6 + p[14] * zero,
        p[7] * r5 + p[3] * r4 + p[11] * r6 + p[15] * zero,
        // row 2
        r8 * p[0] + p[4] * r9 + p[8] * r10 + p[12] * zero,
        p[5] * r9 + p[1] * r8 + p[9] * r10 + p[13] * zero,
        p[6] * r9 + p[2] * r8 + p[10] * r10 + p[14] * zero,
        p[7] * r9 + p[3] * r8 + p[11] * r10 + p[15] * zero,
        // translation row (w = 1)
        p[4] * ty + tx * p[0] + p[8] * tz + p[12] * ONE,
        p[5] * ty + p[1] * tx + p[9] * tz + p[13] * ONE,
        p[6] * ty + p[2] * tx + p[10] * tz + p[14] * ONE,
        p[7] * ty + p[3] * tx + p[11] * tz + p[15] * ONE,
    ];
    write_floats(pg, m, joint.wrapping_add(joint::WORLD), &o);
}

/// Scaled joints: the parent's axes are normalised (4-component length,
/// left alone below epsilon), the joint's rotation is scaled by the parent
/// axis lengths times the joint's scale, and the translation by the parent
/// axis lengths; the result is combined with the normalised parent axes.
fn rebuild_scaled(m: &mut dyn Mem, pg: Pages, joint: u32, p: &[f32; 16], scale: [f32; 3]) {
    let [qx, qy, qz, qw] = read_floats::<4>(pg, m, joint.wrapping_add(joint::ROTATION));
    let [tx, ty, tz] = read_floats::<3>(pg, m, joint.wrapping_add(joint::TRANSLATION));
    let y2 = qy * TWO;
    let z2 = qz * TWO;
    let x2 = qx * TWO;
    let x2w = qw * x2;
    let z2w = qw * z2;
    let x2x = qx * x2;
    let y2z2 = qy * z2;
    let z2z = qz * z2;
    // parent axis lengths (4 components each), scaled by the joint's scale
    let len0 = length4(p[0], p[1], p[2], p[3]);
    let len1 = length4(p[5], p[4], p[6], p[7]);
    let len2 = length4(p[9], p[8], p[10], p[11]);
    let s0 = len0 * scale[0];
    let s1 = len1 * scale[1];
    let s2 = len2 * scale[2];
    // normalised parent axes n0, n1, n2 (x, y, z, w)
    let n0 = normalized_row(p[0], p[1], p[2], p[3], len0);
    let n1 = normalized_row(p[4], p[5], p[6], p[7], len1);
    let n2 = normalized_row(p[8], p[9], p[10], p[11], len2);
    let (p12, p13, p14, p15) = (p[12], p[13], p[14], p[15]);
    let tx_scaled = tx * len0;
    let tz_scaled = tz * len2;
    // the rotation, scaled row by row
    let l00 = (ONE - (z2z + qy * y2)) * s0;
    let l01 = (qx * y2 + z2w) * s0;
    let l02 = (qx * z2 - qw * y2) * s0;
    let l10 = (qx * y2 - z2w) * s1;
    let l11 = (ONE - (z2z + x2x)) * s1;
    let l12 = (y2z2 + x2w) * s1;
    let l20 = (qx * z2 + qw * y2) * s2;
    let l21 = (y2z2 - x2w) * s2;
    let l22 = (ONE - (qy * y2 + x2x)) * s2;
    let zero = 0.0f32;
    let (t0, t1, t2) = (s0 * zero, s1 * zero, s2 * zero);
    let ty_scaled = ty * len1;
    let o = [
        l01 * n1[0] + n0[0] * l00 + l02 * n2[0] + p12 * t0,
        n0[1] * l00 + n1[1] * l01 + n2[1] * l02 + p13 * t0,
        n0[2] * l00 + n1[2] * l01 + n2[2] * l02 + p14 * t0,
        n0[3] * l00 + n1[3] * l01 + n2[3] * l02 + p15 * t0,
        l11 * n1[0] + l10 * n0[0] + l12 * n2[0] + t1 * p12,
        l11 * n1[1] + n0[1] * l10 + l12 * n2[1] + p13 * t1,
        l11 * n1[2] + n0[2] * l10 + l12 * n2[2] + p14 * t1,
        l11 * n1[3] + n0[3] * l10 + l12 * n2[3] + p15 * t1,
        l21 * n1[0] + l20 * n0[0] + l22 * n2[0] + t2 * p12,
        l21 * n1[1] + n0[1] * l20 + l22 * n2[1] + p13 * t2,
        l21 * n1[2] + n0[2] * l20 + l22 * n2[2] + p14 * t2,
        l21 * n1[3] + n0[3] * l20 + l22 * n2[3] + p15 * t2,
        ty_scaled * n1[0] + tx_scaled * n0[0] + tz_scaled * n2[0] + p12,
        ty_scaled * n1[1] + n0[1] * tx_scaled + tz_scaled * n2[1] + p13,
        ty_scaled * n1[2] + n0[2] * tx_scaled + tz_scaled * n2[2] + p14,
        ty_scaled * n1[3] + n0[3] * tx_scaled + tz_scaled * n2[3] + p15,
    ];
    write_floats(pg, m, joint.wrapping_add(joint::WORLD), &o);
}

/// `row / length` for a row whose length is not below epsilon (the w
/// component scaled as well); a shorter row is returned as is.
#[inline(always)]
fn normalized_row(x: f32, y: f32, z: f32, w: f32, length: f32) -> [f32; 4] {
    if EPSILON > length {
        [x, y, z, w]
    } else {
        let inverse = ONE / length;
        [x * inverse, y * inverse, z * inverse, w * inverse]
    }
}

/// The chain direction (or up vector) a selector picks from a joint's
/// world matrix: 0..2 the X/Y/Z row, 3..5 the same negated; anything else
/// gives None.
fn choose_axis(m: &dyn Mem, pg: Pages, joint: u32, selector: u32) -> Option<[f32; 3]> {
    let row = |k: u32| read_floats::<3>(pg, m, joint.wrapping_add(joint::WORLD + 0x10 * k));
    match selector {
        0..=2 => Some(row(selector)),
        3..=5 => {
            let r = row(selector - 3);
            Some([-r[0], -r[1], -r[2]])
        }
        _ => None,
    }
}

/// Unscaled chains: the node's axis lengths are the constants (1.0) at
/// 0x01629510..; returns the joint's world matrix as is.
fn store_unit_row_lengths(m: &mut dyn Mem, pg: Pages, node: u32, joint: u32) -> [f32; 16] {
    wf(pg, m, node.wrapping_add(node::AXIS_LENGTH), ONE);
    for k in 0..3 {
        let v = pg.u32(m, UNIT_LENGTHS + 4 * k);
        pg.set_u32(m, node.wrapping_add(node::ROW_LENGTHS + 4 * k), v);
    }
    pg.set_u32(m, node.wrapping_add(node::ROW_LENGTHS + 0xc), 0);
    read_floats::<16>(pg, m, joint.wrapping_add(joint::WORLD))
}

/// Scaled chains: store the chain direction's length and the lengths of
/// the joint's three axis rows; returns the world matrix with the three
/// axis rows normalised (4 components) and the position row as is.
fn store_scaled_row_lengths(m: &mut dyn Mem, pg: Pages, node: u32, joint: u32, axis: [f32; 3]) -> [f32; 16] {
    wf(pg, m, node.wrapping_add(node::AXIS_LENGTH), ((axis[0] * axis[0]) + (axis[1] * axis[1]) + (axis[2] * axis[2])).sqrt());
    let mut j = read_floats::<16>(pg, m, joint.wrapping_add(joint::WORLD));
    for k in 0..3usize {
        let r = &j[4 * k..4 * k + 4];
        wf(pg, m, node.wrapping_add(node::ROW_LENGTHS + 4 * k as u32), length4(r[0], r[1], r[2], r[3]));
    }
    for k in 0..3usize {
        let inverse = ONE / length4(j[4 * k], j[4 * k + 1], j[4 * k + 2], j[4 * k + 3]);
        for e in &mut j[4 * k..4 * k + 4] {
            *e *= inverse;
        }
    }
    j
}

/// Where the joint's position projects onto the previous node's chain
/// axis (the frame's X row through its position).
fn point_on_previous_axis(m: &dyn Mem, pg: Pages, node: u32, joint: u32) -> [f32; 3] {
    let previous = node.wrapping_sub(NODE_STRIDE).wrapping_add(node::FRAME);
    let ax = rf(pg, m, previous);
    let ay = rf(pg, m, previous.wrapping_add(4));
    let az = rf(pg, m, previous.wrapping_add(8));
    let [px, py, pz] = read_floats::<3>(pg, m, previous.wrapping_add(0x30));
    let [jx, jy, jz] = read_floats::<3>(pg, m, joint.wrapping_add(joint::WORLD + 0x30));
    let along = ay * (jy - py) + ax * (jx - px) + az * (jz - pz);
    [px + ax * along, ay * along + py, az * along + pz]
}

/// 0x01130A00, `look_at_frame(out, axis, up, position) -> out` (cdecl,
/// plain `ret`): see [`build_look_at_frame`]. The position is read after
/// the rotation rows are written, as the original does.
///
/// Translated from 0x01130a00 (DDDA.exe 2017-12-13 Steam).
pub fn look_at_frame(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let (out, axis, up, position) = (arg(m, 0), arg(m, 1), arg(m, 2), arg(m, 3));
    let axis = read_floats::<3>(pg, m, axis);
    let up = read_floats::<3>(pg, m, up);
    build_look_at_rotation(m, pg, out, axis, up);
    let position = read_floats::<4>(pg, m, position);
    write_floats(pg, m, out.wrapping_add(0x30), &position);
    c.eax = out;
    c.esp = entry.wrapping_add(4);
}

/// A rotation frame in `out` (16 floats) and the position row: row 0 the
/// normalised `axis`, row 2 the normalised `axis x up`, row 1
/// `row2 x row0`, row 3 `position`. When `axis x up` is shorter than
/// epsilon only the position row is written (the original copies it
/// through the x87 stack: a bit copy except that a signalling NaN comes
/// out quiet).
fn build_look_at_frame(m: &mut dyn Mem, pg: Pages, out: u32, axis: [f32; 3], up: [f32; 3], position: [f32; 4]) {
    build_look_at_rotation(m, pg, out, axis, up);
    write_floats(pg, m, out.wrapping_add(0x30), &position);
}

/// Rows 0..2 of the look-at frame (nothing when `axis x up` is below
/// epsilon).
fn build_look_at_rotation(m: &mut dyn Mem, pg: Pages, out: u32, axis: [f32; 3], up: [f32; 3]) {
    let (a0, a1, a2) = (axis[0], axis[1], axis[2]);
    let (b0, b1, b2) = (up[0], up[1], up[2]);
    let mut cx = b2 * a1 - b1 * a2;
    let mut cy = b0 * a2 - a0 * b2;
    let mut cz = a0 * b1 - b0 * a1;
    let squared = ((cy * cy) + (cx * cx)) + (cz * cz);
    if EPSILON > squared {
        return; // axis and up are parallel
    }
    let length = squared.sqrt();
    if !(EPSILON > length) {
        let inverse = ONE / length;
        cx *= inverse;
        cy *= inverse;
        cz *= inverse;
    }
    let axis_length = ((a1 * a1) + (a0 * a0) + (a2 * a2)).sqrt();
    let (n0, n1, n2) = if EPSILON > axis_length {
        (a0, a1, a2)
    } else {
        let inverse = ONE / axis_length;
        (a0 * inverse, a1 * inverse, a2 * inverse)
    };
    let rows = [
        n0, n1, n2, 0.0,
        n2 * cy - n1 * cz, cz * n0 - n2 * cx, n1 * cx - cy * n0, 0.0,
        cx, cy, cz, 0.0,
    ];
    write_floats(pg, m, out, &rows);
}

/// Store the joint's world matrix `j` relative to the node frame (the
/// inverse of the rigid frame applied to each row), 16 floats at +0x150.
fn store_joint_in_frame(m: &mut dyn Mem, pg: Pages, node: u32, j: &[f32; 16]) {
    // (the frame's w column is not read)
    let frame = node.wrapping_add(node::FRAME);
    let mut f = [0f32; 16];
    for k in [0, 1, 2, 4, 5, 6, 8, 9, 10, 12, 13, 14] {
        f[k] = rf(pg, m, frame.wrapping_add(4 * k as u32));
    }
    // the frame's inverse translation: minus the position on each axis
    let t = [f[12], f[13], f[14]];
    let i0 = -(f[1] * t[1] + f[0] * t[0] + f[2] * t[2]);
    let i1 = -(f[5] * t[1] + f[4] * t[0] + f[6] * t[2]);
    let i2 = -(f[9] * t[1] + f[8] * t[0] + f[10] * t[2]);
    let zero = 0.0f32;
    let o = [
        // row 0
        f[0] * j[0] + f[1] * j[1] + f[2] * j[2] + i0 * j[3],
        f[5] * j[1] + f[4] * j[0] + f[6] * j[2] + i1 * j[3],
        f[9] * j[1] + f[8] * j[0] + f[10] * j[2] + i2 * j[3],
        (j[1] + j[0] + j[2]) * zero + j[3],
        // row 1
        j[4] * f[0] + j[5] * f[1] + j[6] * f[2] + j[7] * i0,
        j[5] * f[5] + j[4] * f[4] + j[6] * f[6] + j[7] * i1,
        j[5] * f[9] + j[4] * f[8] + j[6] * f[10] + j[7] * i2,
        (j[5] + j[4] + j[6]) * zero + j[7],
        // row 2
        j[8] * f[0] + j[9] * f[1] + j[10] * f[2] + j[11] * i0,
        j[9] * f[5] + j[8] * f[4] + j[10] * f[6] + j[11] * i1,
        j[9] * f[9] + j[8] * f[8] + j[10] * f[10] + j[11] * i2,
        (j[9] + j[8] + j[10]) * zero + j[11],
        // position row
        j[12] * f[0] + j[13] * f[1] + j[14] * f[2] + j[15] * i0,
        j[13] * f[5] + j[12] * f[4] + j[14] * f[6] + j[15] * i1,
        j[13] * f[9] + j[12] * f[8] + j[14] * f[10] + j[15] * i2,
        (j[13] + j[12] + j[14]) * zero + j[15],
    ];
    write_floats(pg, m, node.wrapping_add(node::JOINT_IN_FRAME), &o);
}

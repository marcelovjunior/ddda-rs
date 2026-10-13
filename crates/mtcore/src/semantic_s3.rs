//! L184 semantic ports, set s3: the unit-draw job's finishing virtual
//! method and the joint/model update helper (see each function).
use crate::semantic::call_from;
use ddnative_base::mem::{Mem, Pages, READ};
use ddnative_base::x86::{call_ext, Cpu};

/// An x87 `fld dword` / `fstp dword` round trip: exact for every float but a
/// signalling NaN, which the load quiets.
fn through_x87(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

/// SSE single-precision arithmetic with the hardware's NaN rule: a NaN
/// first operand (the destination register) wins, else a NaN second operand,
/// and either is quieted. Rust would be free to swap the operands of `+`
/// and `*`, which changes which NaN (sign and payload) is kept.
fn quiet(x: f32) -> f32 {
    f32::from_bits(x.to_bits() | 0x0040_0000)
}
fn addss(dst: f32, src: f32) -> f32 {
    if dst.is_nan() { quiet(dst) } else if src.is_nan() { quiet(src) } else { dst + src }
}
fn subss(dst: f32, src: f32) -> f32 {
    if dst.is_nan() { quiet(dst) } else if src.is_nan() { quiet(src) } else { dst - src }
}
fn mulss(dst: f32, src: f32) -> f32 {
    if dst.is_nan() { quiet(dst) } else if src.is_nan() { quiet(src) } else { dst * src }
}
fn sqrtss(src: f32) -> f32 {
    if src.is_nan() { quiet(src) } else { src.sqrt() }
}

// Translated from 0x00c52a30 (DDDA.exe 2017-12-13 Steam).
/// 0x00C52A30, `unit->finish_draw()` (a vf93 virtual method, thiscall, plain
/// `ret`): the end-of-frame step of a drawable unit.
///
/// Only runs for a unit whose flag byte `+0x2627` has bits 1, 2 and 3 set.
/// Then: when bytes `+0x25f2` and `+0x25f3` are set, every one of the
/// `+0x2684` entries at `+0x2688` is handed to 0x0076F740 (`EDI` = the
/// record that entry's index selects in the table at `+0x1d1c`, or 0);
/// 0x00884270 runs on `+0x2378`, the words `+0x2390/+0x2394` are cleared and,
/// if bit 6 of `+0x25f0` is set, the unit's virtual `+0x50` and `+0x54` run,
/// bit 6 may be dropped again, and (while bit 7 is clear) the unit's draw
/// weight float is stored at `+0x2928` from the globals block (`0x018fa4bc`)
/// according to flag bit 11 and the float at `+0x16c`, and bit 7 is set.
/// Then the model update (0x0076B470), 0x007A5FA0 on `+0x1f1c`, 0x00794550 on
/// `+0x1ed8` and the virtual `+0x20` of the two objects it holds, 0x005D1AF0
/// for each non-zero entry of the 16 slots at `+0xfb0`, the virtual `+0x14`
/// of the object at `+0x2070`, and 0x00C52C50; when 0x00C540D0 returns
/// non-zero, `+0x29a7` gets bit 4 for the state 0x29f9 at `+0x2574` and
/// 0x0049E300 runs with the global `0x018fa500`.
pub fn finish_draw(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let unit = c.ecx;
    let entry = c.esp;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // the original's frame at its calls: `push ebp` / 8-byte alignment, 8
    // bytes of locals, four pushes
    let frame = (entry.wrapping_sub(4) & !7).wrapping_sub(8 + 16);
    let finish = |c: &mut Cpu| {
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(4);
    };
    let guest = |target: u32| move |m: &mut dyn Mem, c: &mut Cpu| call_ext(m, c, target);
    let flags = pg.u8(m, unit.wrapping_add(0x2627));
    if flags & 2 == 0 || flags & 8 == 0 || flags & 4 == 0 {
        return finish(c);
    }
    let model = unit.wrapping_add(0x10e0);
    if pg.u8(m, unit.wrapping_add(0x25f2)) != 0 && pg.u8(m, unit.wrapping_add(0x25f3)) != 0 && pg.u32(m, unit.wrapping_add(0x2684)) != 0 {
        let mut i = 0u32;
        loop {
            let slot = unit.wrapping_add(0x2688).wrapping_add(4 * i);
            let table = pg.u32(m, model.wrapping_add(0xc3c));
            let second = pg.u32(m, slot.wrapping_add(0xc));
            let index = pg.u32(m, slot);
            let record = if table == 0 || index >= pg.u32(m, table.wrapping_add(0x78)) {
                0
            } else {
                pg.u32(m, pg.u32(m, table.wrapping_add(0x84)).wrapping_add(index.wrapping_mul(4)))
            };
            c.edi = record;
            call_from(m, c, pg, frame, index, &[model, second, 0, 0, 0], 0x00c5_2ac0, guest(0x0076_f740));
            i += 1;
            if i >= pg.u32(m, unit.wrapping_add(0x2684)) {
                break;
            }
        }
    }
    call_from(m, c, pg, frame, unit, &[unit.wrapping_add(0x2378)], 0x00c5_2ae0, guest(0x0088_4270));
    pg.set_u32(m, unit.wrapping_add(0x2390), 0);
    pg.set_u32(m, unit.wrapping_add(0x2394), 0);
    if pg.u8(m, unit.wrapping_add(0x25f0)) & 0x40 != 0 {
        for (slot, back) in [(0x50u32, 0x00c5_2b05u32), (0x54, 0x00c5_2b0f)] {
            let method = pg.u32(m, pg.u32(m, unit).wrapping_add(slot));
            call_from(m, c, pg, frame, unit, &[], back, guest(method));
        }
        let state = pg.u32(m, unit.wrapping_add(0x25ac));
        if state & 1 != 0 && state & 0x10_0000 == 0 {
            let byte = pg.u8(m, unit.wrapping_add(0x25f0));
            pg.set_u8(m, unit.wrapping_add(0x25f0), byte & 0xbf);
        }
        let byte = pg.u8(m, unit.wrapping_add(0x25f0));
        if byte & 0x80 == 0 {
            let globals = |m: &dyn Mem| pg.u32(m, 0x018f_a4bc);
            let weight = if state & 0x800 != 0 {
                pg.u32(m, globals(m).wrapping_add(0xb_e454))
            } else {
                let speed = f32::from_bits(pg.u32(m, unit.wrapping_add(0x16c)));
                let low = f32::from_bits(pg.u32(m, 0x0153_1ae8));
                // comiss + jc: taken when below or unordered
                if !(low < speed) && !low.is_nan() && !speed.is_nan() {
                    pg.u32(m, globals(m).wrapping_add(0xb_e458))
                } else {
                    let g = globals(m);
                    let high = f32::from_bits(pg.u32(m, g.wrapping_add(0xb_e460)));
                    if !(high < speed) && !high.is_nan() && !speed.is_nan() {
                        pg.u32(m, g.wrapping_add(0xb_e458))
                    } else {
                        pg.u32(m, g.wrapping_add(0xb_e45c))
                    }
                }
            };
            pg.set_u32(m, unit.wrapping_add(0x2928), through_x87(weight));
            pg.set_u8(m, unit.wrapping_add(0x25f0), byte | 0x80);
        }
    }
    call_from(m, c, pg, frame, unit, &[model], 0x00c5_2ba7, guest(0x0076_b470));
    let slot_1f1c = unit.wrapping_add(0x1f1c);
    c.esi = slot_1f1c;
    call_from(m, c, pg, frame, unit, &[], 0x00c5_2bb2, guest(0x007a_5fa0));
    let holder = unit.wrapping_add(0x1ed8);
    c.esi = holder;
    call_from(m, c, pg, frame, unit, &[holder], 0x00c5_2bbe, guest(0x0079_4550));
    for (offset, back) in [(0x20u32, 0x00c5_2bcdu32), (0x24, 0x00c5_2bdc)] {
        if pg.u32(m, holder.wrapping_add(offset)) != 0 {
            let object = pg.u32(m, holder.wrapping_add(offset));
            let method = pg.u32(m, pg.u32(m, object).wrapping_add(0x20));
            call_from(m, c, pg, frame, object, &[], back, guest(method));
        }
    }
    for k in 0..16u32 {
        let slot = unit.wrapping_add(0xfb0).wrapping_add(4 * k);
        let value = pg.u32(m, slot);
        if value != 0 {
            call_from(m, c, pg, frame, 0, &[value, unit], 0x00c5_2bf4, guest(0x005d_1af0));
        }
    }
    let object = unit.wrapping_add(0x2070);
    let method = pg.u32(m, pg.u32(m, object).wrapping_add(0x14));
    call_from(m, c, pg, frame, object, &[], 0x00c5_2c0b, guest(method));
    c.esi = unit;
    call_from(m, c, pg, frame, 0, &[], 0x00c5_2c12, guest(0x00c5_2c50));
    c.eax = unit;
    call_from(m, c, pg, frame, 0, &[], 0x00c5_2c19, guest(0x00c5_40d0));
    if c.eax & 0xff == 0 {
        return finish(c);
    }
    if pg.u32(m, unit.wrapping_add(0x2574)) == 0x29f9 {
        let byte = pg.u8(m, unit.wrapping_add(0x29a7));
        pg.set_u8(m, unit.wrapping_add(0x29a7), byte | 0x10);
    }
    let global = pg.u32(m, 0x018f_a500);
    c.eax = unit;
    call_from(m, c, pg, frame, 0, &[global], 0x00c5_2c3d, guest(0x0049_e300));
    finish(c)
}

/// The running bounds of [`update_model_bounds`]: the original keeps them in
/// xmm registers and four stack slots.
#[derive(Clone, Copy)]
struct Bounds {
    min: [f32; 3],
    max: [f32; 3],
}

impl Bounds {
    /// Grow by the box `centre - radius ..= centre + radius`. A bound is
    /// replaced exactly as the original's `comiss` / `ja` pairs do, so a NaN
    /// operand ends up as the new bound.
    fn include(&mut self, centre: [f32; 3], radius: f32) {
        for axis in 0..3 {
            let low = subss(centre[axis], radius);
            let high = addss(centre[axis], radius);
            self.min[axis] = if low > self.min[axis] { self.min[axis] } else { low };
            self.max[axis] = if self.max[axis] > high { self.max[axis] } else { high };
        }
    }
}

/// How many joints' radius and scale [`update_model_bounds`] keeps between
/// its two passes (the rest are read again).
const KEPT_JOINTS: usize = 64;

/// A joint record's bytes 0x10..0x97 (basis rows, position, parent index
/// and flags), read through one page lookup when the record lies in one
/// backed page, else field by field through `Pages`.
#[derive(Clone, Copy)]
struct JointView {
    at: u32,
    bulk: Option<*const u8>,
}

impl JointView {
    #[inline(always)]
    fn new(pg: Pages, at: u32) -> JointView {
        JointView { at, bulk: pg.bytes(at.wrapping_add(0x10), 0x87, READ).map(|p| p as *const u8) }
    }
    #[inline(always)]
    fn word(&self, m: &dyn Mem, pg: Pages, offset: u32) -> u32 {
        match self.bulk {
            // SAFETY: `bulk` covers offsets 0x10..0x97 of this record
            Some(p) => unsafe { (p.add(offset as usize - 0x10) as *const u32).read_unaligned() },
            None => pg.u32(m, self.at.wrapping_add(offset)),
        }
    }
    #[inline(always)]
    fn float(&self, m: &dyn Mem, pg: Pages, offset: u32) -> f32 {
        f32::from_bits(self.word(m, pg, offset))
    }
    #[inline(always)]
    fn byte(&self, m: &dyn Mem, pg: Pages, offset: u32) -> u8 {
        match self.bulk {
            // SAFETY: as in `word`
            Some(p) => unsafe { *p.add(offset as usize - 0x10) },
            None => pg.u8(m, self.at.wrapping_add(offset)),
        }
    }
    /// The joint's position (+0x40, +0x44, +0x48).
    #[inline(always)]
    fn position(&self, m: &dyn Mem, pg: Pages) -> [f32; 3] {
        [0x40, 0x44, 0x48].map(|offset| self.float(m, pg, offset))
    }
}

// Translated from 0x00ffa620 (DDDA.exe 2017-12-13 Steam).
/// 0x00FFA620, `model->update_bounds(visible_only)` (thiscall, `ret 4`):
/// compute the model's bounding sphere and box from its joints.
///
/// `model + 0xf0` is the model's resource (0: nothing to do); its +0x60 is
/// the table of 0x18-byte per-joint records whose float at +4 is the joint's
/// bounding radius; `model + 0x360` is the joint count (0 calls 0x00EA3D00
/// and stops) and `model + 0x364` the joints (0xA0 bytes each). A joint's
/// position is the vector at +0x40, its parent index (0xff: none) the byte
/// at +0x91, and the three basis rows at +0x10, +0x20 and +0x30 (four
/// floats each) are measured for their lengths; with `visible_only` joints
/// whose bit 0 of +0x96 is clear are ignored.
///
/// 1. the box: every joint's position grown by `radius * scale`, with scale
///    the largest of the three row lengths (bit 6 of `model + 0x108` set,
///    "skinned" models); otherwise by the joint's radius alone, and also
///    the parent's position grown by the same radius;
/// 2. the sphere: centre = box middle, radius = the largest distance from
///    the centre to a joint plus the joint's radius (times `scale` for
///    skinned models);
/// 3. stores the centre and radius at +0x160..+0x16c, the box half extents
///    at +0x1b0.., a constant basis block at +0x170..+0x1ac and the first
///    joint's position at +0x1c0..+0x1c8.
///
/// Initial bounds, the 0.5 and the constant block come from the exe's float
/// tables (0x01629560.., 0x01406cf0, 0x016295c0.., 0x01406ce0).
pub fn update_model_bounds(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let model = c.ecx;
    let entry = c.esp;
    let saved = (c.ebx, c.esi, c.edi);
    let finish = |c: &mut Cpu| {
        (c.ebx, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(4 + 4);
    };
    let float = |m: &dyn Mem, address: u32| f32::from_bits(pg.u32(m, address));
    let resource = pg.u32(m, model.wrapping_add(0xf0));
    if resource == 0 {
        return finish(c);
    }
    let count = pg.u32(m, model.wrapping_add(0x360));
    if count == 0 {
        // the original's frame at the call: 16-byte aligned, 0xc4 locals, three pushes
        let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0xc4 + 12);
        call_from(m, c, pg, frame, model, &[], 0x00ff_b0e9, |m, c| call_ext(m, c, 0x00ea_3d00));
        return finish(c);
    }
    let skinned = pg.u8(m, model.wrapping_add(0x108)) & 0x40 != 0;
    let radii = pg.u32(m, resource.wrapping_add(0x60));
    let visible_only = pg.u8(m, entry.wrapping_add(4)) != 0;
    let joints = pg.u32(m, model.wrapping_add(0x364));
    let half = float(m, 0x0140_6cf0);
    let joint = |k: u32| JointView::new(pg, joints.wrapping_add(k.wrapping_mul(0xa0)));
    let radius = |m: &dyn Mem, k: u32| float(m, radii.wrapping_add(4).wrapping_add(k.wrapping_mul(0x18)));
    let counted = |m: &dyn Mem, j: &JointView| !visible_only || j.byte(m, pg, 0x96) & 1 != 0;
    // length of the basis row of four floats at `row`
    let row_length = |m: &dyn Mem, j: &JointView, row: u32| {
        let f = |i: u32| j.float(m, pg, row + 4 * i);
        sqrtss(addss(addss(addss(mulss(f(0), f(0)), mulss(f(1), f(1))), mulss(f(2), f(2))), mulss(f(3), f(3))))
    };
    // the scale of a joint: the largest row length (ties and NaN keep the later row, as the original)
    let scale = |m: &dyn Mem, j: &JointView| {
        let (first, second, third) = (row_length(m, j, 0x30), row_length(m, j, 0x20), row_length(m, j, 0x10));
        let larger = if third > second { third } else { second };
        if larger > first { larger } else { first }
    };
    let mut bounds = Bounds {
        min: [float(m, 0x0162_9570), float(m, 0x0162_9574), float(m, 0x0162_9578)],
        max: [float(m, 0x0162_9560), float(m, 0x0162_9564), float(m, 0x0162_9568)],
    };
    // radius and scale of the first joints, kept for the second pass (the
    // original reads them again; nothing writes them in between)
    let mut kept = [(0.0f32, 0.0f32); KEPT_JOINTS];
    for k in 0..count {
        let j = joint(k);
        if !counted(m, &j) {
            continue;
        }
        let centre = j.position(m, pg);
        let reach = radius(m, k);
        if skinned {
            let scale = scale(m, &j);
            if (k as usize) < KEPT_JOINTS {
                kept[k as usize] = (reach, scale);
            }
            bounds.include(centre, mulss(scale, reach));
        } else {
            if (k as usize) < KEPT_JOINTS {
                kept[k as usize].0 = reach;
            }
            bounds.include(centre, reach);
            let parent = j.byte(m, pg, 0x91);
            if parent != 0xff {
                bounds.include(joint(parent as u32).position(m, pg), reach);
            }
        }
    }
    let centre = [0, 1, 2].map(|a| mulss(addss(bounds.max[a], bounds.min[a]), half));
    let mut sphere = 0.0f32;
    for k in 0..count {
        let j = joint(k);
        if !counted(m, &j) {
            continue;
        }
        let p = j.position(m, pg);
        let [dx, dy, dz] = [0, 1, 2].map(|a| subss(p[a], centre[a]));
        let distance = sqrtss(addss(addss(mulss(dx, dx), mulss(dy, dy)), mulss(dz, dz)));
        let (reach, scale) = match kept.get(k as usize) {
            Some(&kept) => kept,
            None => (radius(m, k), if skinned { scale(m, &j) } else { 0.0 }),
        };
        let reach = if skinned { addss(mulss(reach, scale), distance) } else { addss(distance, reach) };
        sphere = if sphere > reach { sphere } else { reach };
    }
    for (a, v) in centre.into_iter().enumerate() {
        pg.set_u32(m, model.wrapping_add(0x160 + 4 * a as u32), v.to_bits());
    }
    pg.set_u32(m, model.wrapping_add(0x16c), sphere.to_bits());
    // the constant block (12 floats from 0x016295c0) and the centre again
    for i in 0..12u32 {
        let v = pg.u32(m, 0x0162_95c0 + 4 * i);
        pg.set_u32(m, model.wrapping_add(0x170 + 4 * i), v);
    }
    for (a, v) in centre.into_iter().enumerate() {
        pg.set_u32(m, model.wrapping_add(0x1a0 + 4 * a as u32), v.to_bits());
    }
    let one = pg.u32(m, 0x0140_6ce0);
    pg.set_u32(m, model.wrapping_add(0x1ac), one);
    let extent = [0, 1, 2].map(|a| mulss(subss(bounds.max[a], bounds.min[a]), half));
    for (a, v) in extent.into_iter().enumerate() {
        pg.set_u32(m, model.wrapping_add(0x1b0 + 4 * a as u32), v.to_bits());
    }
    pg.set_u32(m, model.wrapping_add(0x1bc), 0);
    let root = pg.u32(m, model.wrapping_add(0x364));
    for i in 0..3u32 {
        let v = pg.u32(m, root.wrapping_add(0x40 + 4 * i));
        pg.set_u32(m, model.wrapping_add(0x1c0 + 4 * i), through_x87(v));
    }
    finish(c)
}

// Translated from 0x0076b470 (DDDA.exe 2017-12-13 Steam).
/// 0x0076B470, `scene->update_objects()` (stdcall with the scene as its one
/// stack argument, `ret 4`): the per-frame update of a scene's object lists.
///
/// 1. 0x0076F2D0 on the scene (with `+0xd44/+0xd48` set to 0x18 / 1 around
///    it, then 0x7fffffff / 0).
/// 2. each object of the first list (`+0xab8` count, `+0xac4` array): its
///    value `+0x68` is set to the u16 at scene `+0xd04` (0x00F77BF0), the
///    object is updated by 0x00773830 (ESI = object, EDI = the scene's slot
///    `+0xcd4 + 4 * object[+0x150]`), the scene's `+0xcc4` (when non-zero)
///    is bound to it (0x01106E20, 0x01107D70), and an object whose state
///    (`+0x18c`) is 0 or 4 (data at `+0xa0`) or 1 (data at `+0xe0`) gets
///    its virtual `+0x3c` called with that data. If the object's flag
///    `+0x1c0` bit 4 is set, and the scene byte `+0xd98` is clear, it is
///    registered with the global (0x018d0dd0) through 0x00ED5370 and then
///    0x00ED5360.
/// 3. each object of the second list (`+0xae0`, `+0xaec`): 0x00773830 only.
/// 4. each entry of the third list (`+0xda8`, `+0xdb4`): 0x00779BF0.
/// 5. clears `+0x8c0`, `+0x8c4`, `+0x8cc`, `+0x8c8`, byte `+0x8d0`, sets
///    `+0x890..+0x898` from the three floats at 0x018cfe80 and `+0x89c` to 0.
pub fn update_scene_objects(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let scene = pg.u32(m, entry.wrapping_add(4));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // the original's frame at its calls: four pushes
    let frame = entry.wrapping_sub(16);
    let guest = |target: u32| move |m: &mut dyn Mem, c: &mut Cpu| call_ext(m, c, target);
    let at = |offset: u32| scene.wrapping_add(offset);
    pg.set_u32(m, at(0xd48), 1);
    pg.set_u32(m, at(0xd44), 0x18);
    call_from(m, c, pg, frame, 0, &[scene], 0x0076_b492, guest(0x0076_f2d0));
    let first_count = pg.u32(m, at(0xab8));
    pg.set_u32(m, at(0xd48), 0);
    pg.set_u32(m, at(0xd44), 0x7fff_ffff);
    let slot_of = |m: &dyn Mem, object: u32| pg.u32(m, at(0xcd4).wrapping_add(pg.u32(m, object.wrapping_add(0x150)).wrapping_mul(4)));
    let update_object = |m: &mut dyn Mem, c: &mut Cpu, object: u32, back: u32| {
        c.esi = object;
        c.edi = slot_of(m, object);
        call_from(m, c, pg, frame, 0, &[], back, guest(0x0077_3830));
    };
    for i in 0..first_count {
        let object = pg.u32(m, pg.u32(m, at(0xac4)).wrapping_add(i.wrapping_mul(4)));
        if object == 0 {
            continue;
        }
        let value = u32::from(pg.u16(m, at(0xd04)));
        call_from(m, c, pg, frame, object, &[value], 0x0076_b4d6, guest(0x00f7_7bf0));
        update_object(m, c, object, 0x0076_b4e8);
        if pg.u32(m, at(0xcc4)) != 0 {
            call_from(m, c, pg, frame, object, &[scene, 0], 0x0076_b4fb, guest(0x0110_6e20));
            let binding = pg.u32(m, at(0xcc4));
            call_from(m, c, pg, frame, object, &[binding], 0x0076_b509, guest(0x0110_7d70));
        }
        let data = match pg.u32(m, object.wrapping_add(0x18c)) {
            0 | 4 => object.wrapping_add(0xa0),
            1 => object.wrapping_add(0xe0),
            _ => continue,
        };
        let method = pg.u32(m, pg.u32(m, object).wrapping_add(0x3c));
        call_from(m, c, pg, frame, object, &[data, 0, 0], 0x0076_b534, guest(method));
        let registers = |m: &dyn Mem| pg.u8(m, object.wrapping_add(0x1c0)) & 0x10 != 0;
        if !registers(m) {
            continue;
        }
        if pg.u8(m, at(0xd98)) == 0 {
            let global = pg.u32(m, 0x018d_0dd0);
            call_from(m, c, pg, frame, global, &[object, 2, scene, 0x0076_d910, 0, 2], 0x0076_b55e, guest(0x00ed_5370));
        }
        if registers(m) && pg.u8(m, at(0xd98)) == 0 {
            let global = pg.u32(m, 0x018d_0dd0);
            call_from(m, c, pg, frame, global, &[object, 2], 0x0076_b57e, guest(0x00ed_5360));
        }
    }
    let second_count = pg.u32(m, at(0xae0));
    for i in 0..second_count {
        let object = pg.u32(m, pg.u32(m, at(0xaec)).wrapping_add(i.wrapping_mul(4)));
        if object != 0 {
            update_object(m, c, object, 0x0076_b5b8);
        }
    }
    let mut i = 0u32;
    while i < pg.u32(m, at(0xda8)) {
        let entry_object = pg.u32(m, pg.u32(m, at(0xdb4)).wrapping_add(i.wrapping_mul(4)));
        call_from(m, c, pg, frame, 0, &[entry_object], 0x0076_b5df, guest(0x0077_9bf0));
        i += 1;
    }
    pg.set_u32(m, at(0x8c0), 0);
    pg.set_u32(m, at(0x8c4), 0);
    pg.set_u8(m, at(0x8d0), 0);
    for k in 0..3u32 {
        let v = pg.u32(m, 0x018c_fe80 + 4 * k);
        pg.set_u32(m, at(0x890 + 4 * k), v);
    }
    pg.set_u32(m, at(0x89c), 0);
    pg.set_u32(m, at(0x8cc), 0);
    pg.set_u32(m, at(0x8c8), 0);
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4 + 4);
}

// Translated from 0x007a5fa0 (DDDA.exe 2017-12-13 Steam).
/// 0x007A5FA0, `attachment->sync_runtime_object()` (ESI = the attachment
/// record, no stack arguments, plain `ret`): create, update or drop the
/// runtime object (`+0x28`) that mirrors an attachment record.
///
/// - Bytes `+0x62` / `+0x63` make a record that has not yet run but is
///   flagged wait for the import at `[0x0139d0d0]` (called with the global
///   `0x018fa50c + 0x20`): a result above 2 returns at once.
/// - With no runtime object but a target (`+0x24`) whose flag bit 0 is set,
///   and unless the globals block (`0x018fa4bc`) has bit 16 of `+0xb883c`
///   set, one is allocated (0xbf0 bytes, 16-aligned, 0x00F0E170) and built
///   (0x00F12400); it is then registered (0x00DB58B0 on `0x018d09e0`), bound
///   to the target and the description (`+0x1c`) and its state set to 4,
///   with `+0x5b` marking it as created. Without bit 0 the target is
///   released (0x00DE0B90) if its bit 4 is set.
/// - An existing runtime object whose state (`+4`, low 3 bits) is 4 is
///   dropped (target released when `+0x5b`), clearing `+0x24`, `+0x28`,
///   `+0x5a`, `+0x5d`, `+0x5f`; any other copies the record's settings
///   (`+0x38..+0x59`) into the object, then 0x007A65C0 runs on the record.
pub fn sync_runtime_object(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let record = c.esi;
    let entry = c.esp;
    // the original's frame at its calls: `push ebp` / 8-byte alignment,
    // then two pushes
    let frame = (entry.wrapping_sub(4) & !7).wrapping_sub(8);
    let at = |offset: u32| record.wrapping_add(offset);
    let guest = |target: u32| move |m: &mut dyn Mem, c: &mut Cpu| call_ext(m, c, target);
    let finish = |c: &mut Cpu| c.esp = entry.wrapping_add(4);
    // the runtime object, read again after every call as the original does
    let object = |m: &dyn Mem| pg.u32(m, at(0x28));
    let call = |m: &mut dyn Mem, c: &mut Cpu, this: u32, args: &[u32], back: u32, target: u32| {
        call_from(m, c, pg, frame, this, args, back, guest(target))
    };
    // `call 0x007a65c0` with the record, then return
    let run_tail = |m: &mut dyn Mem, c: &mut Cpu, back: u32| {
        call(m, c, 0, &[record], back, 0x007a_65c0);
        finish(c)
    };
    let set_object = |m: &mut dyn Mem, offset: u32, value: u32| {
        let object = object(m);
        pg.set_u32(m, object.wrapping_add(offset), value)
    };
    let call_object = |m: &mut dyn Mem, c: &mut Cpu, args: &[u32], back: u32, target: u32| {
        let this = object(m);
        call(m, c, this, args, back, target)
    };
    let release_target = |m: &mut dyn Mem, c: &mut Cpu, target: u32, back: u32| {
        call(m, c, target, &[], back, 0x00de_0b90);
        pg.set_u32(m, at(0x24), 0);
    };
    if pg.u8(m, at(0x62)) == 0 && pg.u8(m, at(0x63)) != 0 {
        let import = pg.u32(m, 0x0139_d0d0);
        let argument = pg.u32(m, 0x018f_a50c).wrapping_add(0x20);
        if call(m, c, 0, &[argument], 0x007a_5fc3, import) as i32 > 2 {
            return finish(c);
        }
    }
    pg.set_u8(m, at(0x62), 1);
    if object(m) == 0 {
        let target = pg.u32(m, at(0x24));
        if target != 0 {
            let flags = pg.u8(m, target.wrapping_add(0x50));
            if flags & 1 != 0 {
                if pg.u32(m, pg.u32(m, 0x018f_a4bc).wrapping_add(0xb_883c)) & 0x1_0000 == 0 {
                    // cdecl: the original pops its two arguments itself (the next call sets the frame again)
                    let memory = call(m, c, 0, &[0xbf0, 0x10], 0x007a_600c, 0x00f0_e170);
                    let built = if memory != 0 { call(m, c, memory, &[], 0x007a_601a, 0x00f1_2400) } else { 0 };
                    pg.set_u32(m, at(0x28), built);
                }
                let created = object(m);
                if created != 0 {
                    let info = pg.u32(m, at(0x1c));
                    let description = pg.u32(m, info.wrapping_add(0x1c)) & 0xfdff_ffff;
                    let identity = pg.u32(m, info.wrapping_add(0x18));
                    let kind = ((pg.u32(m, info.wrapping_add(4)) >> 3) & 0x7f) + 1;
                    let registry = pg.u32(m, 0x018d_09e0);
                    call(m, c, registry, &[kind, created, identity, description], 0x007a_6054, 0x00db_58b0);
                    let info = pg.u32(m, at(0x1c));
                    let created = object(m);
                    if pg.u32(m, info.wrapping_add(0xf0)) != 0 {
                        pg.set_u32(m, created.wrapping_add(0x1e0), info);
                        call(m, c, created, &[], 0x007a_606d, 0x00f2_3830);
                    }
                    let target = pg.u32(m, at(0x24));
                    call_object(m, c, &[target], 0x007a_6079, 0x00f2_3f80);
                    set_object(m, 0x274, 4);
                    call_object(m, c, &[], 0x007a_608e, 0x00f2_3710);
                    pg.set_u8(m, at(0x5b), 1);
                    set_object(m, 0x26c, 1);
                    set_object(m, 0x270, 0x3f74_69c7);
                    let owner = pg.u32(m, at(0x18));
                    call_object(m, c, &[owner], 0x007a_60b8, 0x00f0_ea30);
                    call_object(m, c, &[u32::MAX], 0x007a_60c2, 0x00f0_eb60);
                } else {
                    let target = pg.u32(m, at(0x24));
                    if target != 0 {
                        release_target(m, c, target, 0x007a_60da);
                    }
                }
            } else if flags & 0x10 != 0 {
                release_target(m, c, target, 0x007a_60da);
            }
        }
    }
    let runtime = object(m);
    if runtime == 0 {
        return run_tail(m, c, 0x007a_62b5);
    }
    if pg.u8(m, runtime.wrapping_add(4)) & 7 == 4 {
        let was_created = pg.u8(m, at(0x5b)) != 0;
        if was_created {
            let target = pg.u32(m, at(0x24));
            if target != 0 {
                release_target(m, c, target, 0x007a_6104);
            }
            pg.set_u8(m, at(0x5b), 0);
        } else {
            pg.set_u32(m, at(0x24), 0);
        }
        pg.set_u32(m, at(0x28), 0);
        pg.set_u8(m, at(0x5a), 0);
        pg.set_u8(m, at(0x5f), 0);
        pg.set_u8(m, at(0x5d), 0);
        return run_tail(m, c, if was_created { 0x007a_611c } else { 0x007a_6137 });
    }
    let value = pg.u32(m, at(0x38));
    pg.set_u32(m, runtime.wrapping_add(0xdc), value);
    // the target's value, unless an environment record (+0x20) with its flag set overrides it
    let environment = pg.u32(m, at(0x20));
    let source = if environment != 0 && pg.u8(m, environment.wrapping_add(0x21b0)) != 0 { 0 } else { pg.u32(m, at(0x24)) };
    let value = if source != 0 { through_x87(pg.u32(m, source.wrapping_add(0x94))) } else { 0 };
    set_object(m, 0x314, value);
    let state = pg.u32(m, at(0x3c));
    set_object(m, 0x274, state);
    let (method, back) = if pg.u8(m, at(0x55)) != 0 { (0x00f2_3710, 0x007a_6194) } else { (0x00f0_ebc0, 0x007a_619b) };
    call_object(m, c, &[], back, method);
    let runtime = object(m);
    for (flag, field) in [(0x51u32, 0x1eau32), (0x52, 0x10f), (0x54, 0x928), (0x56, 0x41c)] {
        let value = (pg.u8(m, at(flag)) != 0) as u8;
        pg.set_u8(m, runtime.wrapping_add(field), value);
    }
    for (source, field) in [(0x40u32, 0x92cu32), (0x44, 0xb30), (0x48, 0xb34)] {
        let value = through_x87(pg.u32(m, at(source)));
        pg.set_u32(m, runtime.wrapping_add(field), value);
    }
    let value = pg.u32(m, at(0x4c));
    pg.set_u32(m, runtime.wrapping_add(0xb38), value);
    if pg.u8(m, at(0x57)) != 0 {
        call(m, c, runtime, &[], 0x007a_6240, 0x00f0_eba0);
    } else {
        pg.set_u8(m, runtime.wrapping_add(0x1e9), 0);
        pg.set_u32(m, runtime.wrapping_add(0x418), 0);
        pg.set_u8(m, runtime.wrapping_add(0xa0), 0);
    }
    let runtime = object(m);
    for (flag, field) in [(0x58u32, 0x110u32), (0x53, 0x1ec)] {
        let value = (pg.u8(m, at(flag)) != 0) as u8;
        pg.set_u8(m, runtime.wrapping_add(field), value);
    }
    if pg.u8(m, at(0x59)) != 0 {
        pg.set_u8(m, runtime.wrapping_add(0xb26), 1);
        return run_tail(m, c, 0x007a_62a0);
    }
    pg.set_u8(m, runtime.wrapping_add(0xb26), 0);
    run_tail(m, c, 0x007a_62b5)
}

// Translated from 0x00794550 (DDDA.exe 2017-12-13 Steam).
/// 0x00794550, `toggle->apply(job)` (stdcall, one stack argument, `ret 4`):
/// switch the two linked objects of a job record on or off.
///
/// `job + 0xc` is the requested state. For the object at `job + 0x24`: every
/// sub-handle of `+0x4c` (their count is the top 24 bits of `[+0x38] + 0x68`)
/// is switched through 0x01083990 (on, once: bit 2 of `+0x64`) or 0x01084210
/// (off, clearing that bit); bits 3, 5 and 4 of `+0x64` follow the job bytes
/// `+0xd`, `+0xe`, `+0xf`; a pending request in `+0x10` is cleared and
/// handled by 0x010819E0; and the job's value `+0x14` is stored into the
/// `+0x6c` of each 0x120-byte element at `+0x174`. For the object at
/// `job + 0x20`: bit 0 of `+0x170` and its sub-handles follow the state
/// (0x0088D660 with ESI = the object and DL = 0 or 1, and 0x010806C0 on
/// switching off), and the job's value goes to its `+0x5c`. Finally a
/// switched-on job with `+0x34` set and `+0x35` clear counts its timer
/// (`+0x38`) down by the frame time (`[0x018d0b28] + 0x70`) and, when it
/// reaches 0, runs 0x00794A90 (EDI = the job) and sets `+0x35`; a
/// switched-off job clears `+0x34`.
pub fn apply_effect_toggle(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let job = pg.u32(m, entry.wrapping_add(4));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // the original's frame at its calls: four pushes
    let frame = entry.wrapping_sub(16);
    let at = |offset: u32| job.wrapping_add(offset);
    let guest = |target: u32| move |m: &mut dyn Mem, c: &mut Cpu| call_ext(m, c, target);
    let call = |m: &mut dyn Mem, c: &mut Cpu, this: u32, args: &[u32], back: u32, target: u32| {
        call_from(m, c, pg, frame, this, args, back, guest(target))
    };
    let enabled = pg.u8(m, at(0xc)) != 0;
    // 0x010805D0 `object->handle_count()`, run natively: the top 24 bits of the table's +0x68
    let handle_count = |m: &dyn Mem, object: u32| {
        let table = pg.u32(m, object.wrapping_add(0x38));
        if table == 0 { 0 } else { pg.u32(m, table.wrapping_add(0x68)) >> 8 }
    };
    // `for each sub-handle of object: call method(handle)`, the count read again after every call
    let each_handle = |m: &mut dyn Mem, c: &mut Cpu, object: u32, method: u32, back: u32| {
        let table = pg.u32(m, object.wrapping_add(0x38));
        if pg.u32(m, table.wrapping_add(0x68)) & 0xffff_ff00 == 0 {
            return;
        }
        let mut i = 0u32;
        loop {
            let handle = pg.u32(m, pg.u32(m, object.wrapping_add(0x4c)).wrapping_add(i.wrapping_mul(4)));
            call(m, c, handle, &[], back, method);
            i += 1;
            let table = pg.u32(m, object.wrapping_add(0x38));
            if i >= pg.u32(m, table.wrapping_add(0x68)) >> 8 {
                break;
            }
        }
    };
    let flags_of = |m: &mut dyn Mem, object: u32, mask: u32, on: bool| {
        let flags = pg.u32(m, object.wrapping_add(0x64));
        pg.set_u32(m, object.wrapping_add(0x64), if on { flags | mask } else { flags & !mask });
    };
    let primary = pg.u32(m, at(0x24));
    if primary != 0 {
        if enabled {
            let flags = pg.u32(m, primary.wrapping_add(0x64));
            if (flags >> 2) & 1 == 0 {
                pg.set_u32(m, primary.wrapping_add(0x64), flags | 4);
                each_handle(m, c, primary, 0x0108_3990, 0x0079_459b);
            }
        } else {
            let flags = pg.u32(m, primary.wrapping_add(0x64));
            pg.set_u32(m, primary.wrapping_add(0x64), flags & !4);
            each_handle(m, c, primary, 0x0108_4210, 0x0079_45cb);
        }
        for (source, mask) in [(0xdu32, 8u32), (0xe, 0x20), (0xf, 0x10)] {
            let on = pg.u8(m, at(source)) != 0;
            flags_of(m, primary, mask, on);
        }
        if pg.u8(m, at(0x10)) != 0 {
            pg.set_u8(m, at(0x10), 0);
            call(m, c, primary, &[], 0x0079_461a, 0x0108_19e0);
        }
        let value = pg.u32(m, at(0x14));
        let mut i = 0u32;
        if handle_count(m, primary) != 0 {
            let mut offset = 0u32;
            loop {
                // 0x011181F0 `element->set_value(value)`, run natively
                let element = pg.u32(m, primary.wrapping_add(0x174)).wrapping_add(offset);
                pg.set_u32(m, element.wrapping_add(0x6c), value);
                i += 1;
                offset = offset.wrapping_add(0x120);
                if i >= handle_count(m, primary) {
                    break;
                }
            }
        }
    }
    let secondary = pg.u32(m, at(0x20));
    if secondary != 0 {
        if enabled {
            let flags = pg.u32(m, secondary.wrapping_add(0x170));
            if flags & 1 == 0 {
                pg.set_u32(m, secondary.wrapping_add(0x170), flags | 1);
                c.esi = secondary;
                c.edx &= !0xff;
                call(m, c, 0, &[], 0x0079_469a, 0x0088_d660);
                each_handle(m, c, secondary, 0x0108_3990, 0x0079_46bb);
            }
        } else {
            let flags = pg.u32(m, secondary.wrapping_add(0x170));
            pg.set_u32(m, secondary.wrapping_add(0x170), flags & !1);
            call(m, c, secondary, &[0x018c_fe80], 0x0079_46de, 0x0108_06c0);
            c.esi = secondary;
            c.edx = (c.edx & !0xff) | 1;
            call(m, c, 0, &[], 0x0079_46e5, 0x0088_d660);
            each_handle(m, c, secondary, 0x0108_4210, 0x0079_46fe);
        }
        // 0x00F65CB0 `object->set_value(value)`, run natively
        let value = pg.u32(m, at(0x14));
        pg.set_u32(m, secondary.wrapping_add(0x5c), value);
    }
    if enabled {
        if pg.u8(m, at(0x34)) != 0 && pg.u8(m, at(0x35)) == 0 {
            let clock = pg.u32(m, 0x018d_0b28);
            let timer = subss(f32::from_bits(pg.u32(m, at(0x38))), f32::from_bits(pg.u32(m, clock.wrapping_add(0x70))));
            pg.set_u32(m, at(0x38), timer.to_bits());
            // comiss 0, timer; jc: not taken only when 0 >= timer (ordered)
            if timer <= 0.0 {
                c.edi = job;
                call(m, c, 0, &[0], 0x0079_4755, 0x0079_4a90);
                pg.set_u8(m, at(0x35), 1);
            }
        }
    } else {
        pg.set_u8(m, at(0x34), 0);
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4 + 4);
}

// ---------------------------------------------------------------------------
// Box / sphere distance (0x00D53730, 0x00D53F30, 0x00D13AB0)

/// The extended-precision (x87, 64-bit significand) values the box tests
/// compute with, in software: the original squares and adds `f32`
/// differences on the x87 stack and compares the result with the squared
/// radius, which `f64` arithmetic would not reproduce bit for bit.
///
/// Only what the box tests need: conversion from `f32`, subtraction,
/// multiplication, addition and ordering of non-negative values, each
/// rounded to nearest-even at 64 bits as the hardware does.
#[derive(Clone, Copy, Debug)]
enum Extended {
    Nan,
    Infinity(bool),
    Zero(bool),
    /// `mantissa * 2^exponent`, `mantissa` normalised (top bit set)
    Finite { negative: bool, exponent: i32, mantissa: u64 },
}

impl Extended {
    fn from_f32(bits: u32) -> Extended {
        let negative = bits >> 31 != 0;
        let field = (bits >> 23) & 0xff;
        let fraction = bits & 0x7f_ffff;
        match (field, fraction) {
            (0xff, 0) => Extended::Infinity(negative),
            (0xff, _) => Extended::Nan,
            (0, 0) => Extended::Zero(negative),
            (0, _) => Extended::round(negative, fraction as u128, -149),
            _ => Extended::round(negative, (fraction | 0x80_0000) as u128, field as i32 - 150),
        }
    }

    /// `value * 2^exponent` rounded to a 64-bit significand (nearest even).
    /// The low bit of `value` may be a sticky bit for bits shifted out.
    fn round(negative: bool, value: u128, exponent: i32) -> Extended {
        if value == 0 {
            return Extended::Zero(false);
        }
        let shift = value.leading_zeros();
        let normalised = value << shift;
        let mut mantissa = (normalised >> 64) as u64;
        let rest = normalised as u64;
        let mut exponent = exponent - shift as i32 + 64;
        if rest > 1 << 63 || (rest == 1 << 63 && mantissa & 1 == 1) {
            let (next, carried) = mantissa.overflowing_add(1);
            mantissa = if carried { 1 << 63 } else { next };
            exponent += carried as i32;
        }
        Extended::Finite { negative, exponent, mantissa }
    }

    fn negated(self) -> Extended {
        match self {
            Extended::Nan => Extended::Nan,
            Extended::Infinity(n) => Extended::Infinity(!n),
            Extended::Zero(n) => Extended::Zero(!n),
            Extended::Finite { negative, exponent, mantissa } => Extended::Finite { negative: !negative, exponent, mantissa },
        }
    }

    fn add(self, other: Extended) -> Extended {
        use Extended::*;
        match (self, other) {
            (Nan, _) | (_, Nan) => Nan,
            (Infinity(a), Infinity(b)) => if a == b { Infinity(a) } else { Nan },
            (Infinity(a), _) | (_, Infinity(a)) => Infinity(a),
            (Zero(a), Zero(b)) => Zero(a && b),
            (Zero(_), value) | (value, Zero(_)) => value,
            (Finite { negative: na, exponent: ea, mantissa: ma }, Finite { negative: nb, exponent: eb, mantissa: mb }) => {
                // `high` has the larger magnitude
                let ((nh, eh, mh), (nl, el, ml)) = if (ea, ma) >= (eb, mb) { ((na, ea, ma), (nb, eb, mb)) } else { ((nb, eb, mb), (na, ea, ma)) };
                let high = (mh as u128) << 62;
                let wide = (ml as u128) << 62;
                let distance = (eh - el) as u32;
                // shift the smaller right, keeping a sticky bit
                let low = if distance >= 128 {
                    1
                } else {
                    let lost = wide & ((1u128 << distance) - 1);
                    (wide >> distance) | (lost != 0) as u128
                };
                if nh == nl {
                    Extended::round(nh, high + low, eh - 62)
                } else if high == low {
                    Zero(false)
                } else {
                    Extended::round(nh, high - low, eh - 62)
                }
            }
        }
    }

    fn mul(self, other: Extended) -> Extended {
        use Extended::*;
        match (self, other) {
            (Nan, _) | (_, Nan) => Nan,
            (Infinity(_), Zero(_)) | (Zero(_), Infinity(_)) => Nan,
            (Infinity(a), Infinity(b)) => Infinity(a != b),
            (Infinity(a), Finite { negative, .. }) | (Finite { negative, .. }, Infinity(a)) => Infinity(a != negative),
            (Zero(a), Zero(b)) => Zero(a != b),
            (Zero(a), Finite { negative, .. }) | (Finite { negative, .. }, Zero(a)) => Zero(a != negative),
            (Finite { negative: na, exponent: ea, mantissa: ma }, Finite { negative: nb, exponent: eb, mantissa: mb }) => {
                Extended::round(na != nb, ma as u128 * mb as u128, ea + eb)
            }
        }
    }

    /// `self < other` for non-negative values, false when unordered.
    fn less_than(self, other: Extended) -> bool {
        use Extended::*;
        let key = |v: Extended| match v {
            Zero(_) => Some((0u8, 0i32, 0u64)),
            Finite { exponent, mantissa, .. } => Some((1, exponent, mantissa)),
            Infinity(_) => Some((2, 0, 0)),
            Nan => None,
        };
        match (key(self), key(other)) {
            (Some(a), Some(b)) => a < b,
            _ => false,
        }
    }

    fn is_nan(self) -> bool {
        matches!(self, Extended::Nan)
    }

    /// The x87 register image (significand, sign and biased exponent) of a
    /// zero or finite value.
    fn to_register(self) -> Option<(u64, u16)> {
        match self {
            Extended::Zero(negative) => Some((0, (negative as u16) << 15)),
            Extended::Finite { negative, exponent, mantissa } => Some((mantissa, ((negative as u16) << 15) | (exponent + 63 + 16383) as u16)),
            _ => None,
        }
    }
}

/// `(dx*dx + dy*dy) + dz*dz` for `d = point - clamped` in software
/// extended precision (what [`push_squared_distance`] leaves in ST0).
fn squared_distance_software(point: [u32; 3], clamped: [u32; 3]) -> Extended {
    let square = |axis: usize| {
        let difference = Extended::from_f32(point[axis]).add(Extended::from_f32(clamped[axis]).negated());
        difference.mul(difference)
    };
    square(0).add(square(1)).add(square(2))
}

/// For the tests: the software squared distance as an x87 register image
/// (`None` for NaN or infinity).
#[doc(hidden)]
pub fn squared_distance_software_register(point: [u32; 3], clamped: [u32; 3]) -> Option<(u64, u16)> {
    squared_distance_software(point, clamped).to_register()
}

/// For the tests: the hardware x87 sequence of the original, leaving the
/// squared distance in ST0.
#[doc(hidden)]
pub fn squared_distance_hardware(c: &mut Cpu, point: [u32; 3], clamped: [u32; 3]) {
    push_squared_distance(c, point, clamped)
}

/// Clamp `point` into the box whose lower corner is at `box_address` and
/// upper corner at `+0x10` (three floats each): per axis, the lower bound
/// replaces the coordinate when it is above it, then the upper bound when
/// the (possibly replaced) coordinate is above it. Comparisons with NaN
/// are false, so a NaN bound never clamps. Returns the clamped coordinates
/// (as float bits).
fn clamp_to_box(m: &dyn Mem, pg: Pages, box_address: u32, point: [u32; 3]) -> [u32; 3] {
    let float = |address: u32| f32::from_bits(pg.u32(m, address));
    let mut clamped = point;
    for axis in 0..3u32 {
        let coordinate = f32::from_bits(point[axis as usize]);
        let low = float(box_address.wrapping_add(4 * axis));
        let high = float(box_address.wrapping_add(0x10 + 4 * axis));
        if low > coordinate {
            clamped[axis as usize] = low.to_bits();
        }
        if coordinate > high {
            clamped[axis as usize] = high.to_bits();
        }
    }
    clamped
}

/// The squared distance between two points, as the original's x87 sequence
/// computes it, left in ST0: `(dx*dx + dy*dy) + dz*dz` with
/// `d = point - clamped` and every step in extended precision.
fn push_squared_distance(c: &mut Cpu, point: [u32; 3], clamped: [u32; 3]) {
    // FLD m32 point, FSUB m32 clamped (D8 /4)
    let difference = |c: &mut Cpu, axis: usize| {
        c.st.push(point[axis]);
        let mut operand = [0u8; 10];
        operand[..4].copy_from_slice(&clamped[axis].to_le_bytes());
        c.st.exec::<0x20d8>(&mut operand);
    };
    difference(c, 0);
    difference(c, 1);
    difference(c, 2);
    c.st.register::<0xc2d9>(); // fld st(2)
    c.st.register::<0xcbde>(); // fmulp st(3), st
    c.st.register::<0xc1d9>(); // fld st(1)
    c.st.register::<0xcade>(); // fmulp st(2), st
    c.st.register::<0xcad9>(); // fxch st(2)
    c.st.register::<0xc1de>(); // faddp st(1), st
    c.st.register::<0xc1d9>(); // fld st(1)
    c.st.register::<0xcade>(); // fmulp st(2), st
    c.st.register::<0xc1de>(); // faddp st(1), st
}

fn read_point(m: &dyn Mem, pg: Pages, address: u32) -> [u32; 3] {
    [0, 4, 8].map(|offset| pg.u32(m, address.wrapping_add(offset)))
}

// Translated from 0x00d53730 (DDDA.exe 2017-12-13 Steam).
/// 0x00D53730, `closest_point_on_box(box, point, out) -> distance^2` (cdecl,
/// plain `ret`): clamp `point` into the axis-aligned box (lower corner at
/// `box`, upper corner at `box + 0x10`), store the clamped point at
/// `out[0..3]` and 0.0 at `out[3]`, and return the squared distance between
/// the point and its clamped copy in ST0 (extended precision).
pub fn closest_point_on_box(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let box_address = pg.u32(m, entry.wrapping_add(4));
    let point_address = pg.u32(m, entry.wrapping_add(8));
    let out = pg.u32(m, entry.wrapping_add(12));
    let point = read_point(m, pg, point_address);
    let clamped = clamp_to_box(m, pg, box_address, point);
    for (axis, bits) in clamped.into_iter().enumerate() {
        pg.set_u32(m, out.wrapping_add(4 * axis as u32), bits);
    }
    pg.set_u32(m, out.wrapping_add(12), 0);
    push_squared_distance(c, point, clamped);
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x00d53f30 (DDDA.exe 2017-12-13 Steam).
/// 0x00D53F30, `distance_sq_to_box(box, point) -> distance^2` (cdecl, plain
/// `ret`): 0x00D53730 with a scratch output, the squared distance in ST0.
pub fn distance_sq_to_box(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let box_address = pg.u32(m, entry.wrapping_add(4));
    let point = read_point(m, pg, pg.u32(m, entry.wrapping_add(8)));
    let clamped = clamp_to_box(m, pg, box_address, point);
    push_squared_distance(c, point, clamped);
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x00d13ab0 (DDDA.exe 2017-12-13 Steam).
/// 0x00D13AB0, `sphere_touches_box(box, sphere) -> bool` (cdecl, plain
/// `ret`; `sphere` is x, y, z, radius): 1 when the radius squared is at
/// least the squared distance from the centre to the box (0x00D53F30), 0
/// otherwise and when either is NaN. The original leaves the radius in the
/// caller's second argument slot (it reuses it to pass the float through
/// the x87), so the port stores it there too.
///
/// The x87 work is done in software ([`Extended`]) with the same roundings,
/// so this makes no x87 or guest calls at all.
pub fn sphere_touches_box(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let box_address = pg.u32(m, entry.wrapping_add(4));
    let sphere = pg.u32(m, entry.wrapping_add(8));
    let radius = pg.u32(m, sphere.wrapping_add(0xc));
    // FLD m32 / FSTP m32 of the radius into the argument slot
    pg.set_u32(m, entry.wrapping_add(8), through_x87(radius));
    let centre = read_point(m, pg, sphere);
    let clamped = clamp_to_box(m, pg, box_address, centre);
    // Almost every call is decided by a wide margin: estimate both sides in
    // f64 (relative error below 1e-15, f32 inputs and squares are in range)
    // and use the exact extended arithmetic only when they are within 1e-12.
    let estimate = {
        let difference = |axis: usize| f32::from_bits(centre[axis]) as f64 - f32::from_bits(clamped[axis]) as f64;
        let (dx, dy, dz) = (difference(0), difference(1), difference(2));
        let radius = f32::from_bits(radius) as f64;
        (radius * radius, (dx * dx + dy * dy) + dz * dz)
    };
    const MARGIN: f64 = 1e-12;
    if estimate.0.is_nan() || estimate.1.is_nan() {
        c.eax = 0;
        c.esp = entry.wrapping_add(4);
        return;
    }
    if estimate.0 > estimate.1 * (1.0 + MARGIN) || estimate.0 * (1.0 + MARGIN) < estimate.1 {
        c.eax = (estimate.0 > estimate.1) as u32;
        c.esp = entry.wrapping_add(4);
        return;
    }
    let squared_distance = squared_distance_software(centre, clamped);
    let radius = Extended::from_f32(radius);
    let squared_radius = radius.mul(radius);
    // fcomip st, st(1): CF = radius^2 < distance^2 or unordered
    let below = squared_radius.less_than(squared_distance) || squared_radius.is_nan() || squared_distance.is_nan();
    c.eax = (!below) as u32;
    c.esp = entry.wrapping_add(4);
}

// ---------------------------------------------------------------------------
// Small leaves of the shadow job

// Translated from 0x00ea1eb0 (DDDA.exe 2017-12-13 Steam).
/// 0x00EA1EB0, `object->copy_position(out) -> bool` (thiscall, `ret 4`): when
/// the object has its resource (`+0xf0`), copy its 16-byte position block
/// (`+0x160`) to `out` and return true. The original sets only AL, so the
/// rest of EAX is `out` (true) or the caller's EAX (false).
pub fn copy_position(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let (object, entry) = (c.ecx, c.esp);
    if pg.u32(m, object.wrapping_add(0xf0)) == 0 {
        c.eax &= !0xff;
    } else {
        let out = pg.u32(m, entry.wrapping_add(4));
        for offset in [0u32, 4, 8, 12] {
            let word = pg.u32(m, object.wrapping_add(0x160 + offset));
            pg.set_u32(m, out.wrapping_add(offset), word);
        }
        c.eax = (out & !0xff) | 1;
    }
    c.esp = entry.wrapping_add(8);
}

// Translated from 0x00ea1f40 (DDDA.exe 2017-12-13 Steam).
/// 0x00EA1F40, `object->world_position(out) -> out` (thiscall, `ret 4`):
/// write the object's local position (`+0x160..+0x168`, passed through the
/// x87 like the original) and 0.0 to `out[0..4]`; then, if the object has a
/// parent (`+0x30`), walk to the root of the parent chain and call its
/// virtual `+0x30` with a scratch vector; when that returns true the
/// vector replaces `out[0..3]`. Returns `out`.
pub fn world_position(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let (object, entry) = (c.ecx, c.esp);
    let out = pg.u32(m, entry.wrapping_add(4));
    for axis in 0..3u32 {
        let bits = pg.u32(m, object.wrapping_add(0x160 + 4 * axis));
        pg.set_u32(m, out.wrapping_add(4 * axis), through_x87(bits));
    }
    pg.set_u32(m, out.wrapping_add(12), 0);
    c.eax = out;
    let mut root = pg.u32(m, object.wrapping_add(0x30));
    if root != 0 {
        loop {
            let parent = pg.u32(m, root.wrapping_add(0x30));
            if parent == 0 {
                break;
            }
            root = parent;
        }
        // the original's frame: `push ebp` / 16-byte alignment, 0x1c locals, push esi;
        // the scratch vector is at +0x10 of the call frame
        let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x1c + 4);
        let local = frame.wrapping_add(0x10);
        let method = pg.u32(m, pg.u32(m, root).wrapping_add(0x30));
        let found = call_from(m, c, pg, frame, root, &[local], 0x00ea_1f8d, |m, c| call_ext(m, c, method)) & 0xff != 0;
        c.eax = out;
        if found {
            for axis in 0..3u32 {
                let bits = pg.u32(m, local.wrapping_add(4 * axis));
                pg.set_u32(m, out.wrapping_add(4 * axis), bits);
            }
            pg.set_u32(m, out.wrapping_add(12), 0);
        }
    }
    c.esp = entry.wrapping_add(8);
}

/// The frustum-test ports `classify_overlap` may call directly instead of
/// through the guest call dispatch: `(0x00FF0780 cull_sphere, 0x00FF0AD0
/// cull_oriented)`, set once by the runtime (mtdraw::cull, which this crate
/// cannot depend on).
pub static CULL_PORTS: std::sync::OnceLock<(ddnative_base::x86::LiftFn, ddnative_base::x86::LiftFn)> = std::sync::OnceLock::new();

// Translated from 0x00ea1ee0 (DDDA.exe 2017-12-13 Steam).
/// 0x00EA1EE0, `object->classify(other) -> mask` (thiscall, `ret 4`): with
/// none of the low six bits of `+0x108` set the answer is 0x3f. Otherwise
/// `other->cull_sphere(object + 0x160)` (0x00FF0780) is the answer if it is
/// not positive; if it is not 0x3f, `other->cull_oriented(object + 0x170,
/// answer)` (0x00FF0AD0) refines it; a still-positive answer becomes 0x3f
/// when those bits of `+0x108` equal 1.
///
/// The two tests are called directly when [`CULL_PORTS`] is set.
pub fn classify_overlap(m: &mut dyn Mem, c: &mut Cpu) {
    classify_overlap_with(m, c, CULL_PORTS.get().copied())
}

/// [`classify_overlap`] with the frustum tests chosen by the caller (`None`:
/// guest calls to the originals).
pub fn classify_overlap_with(m: &mut dyn Mem, c: &mut Cpu, ports: Option<(ddnative_base::x86::LiftFn, ddnative_base::x86::LiftFn)>) {
    let pg = Pages::of(m);
    let (object, entry) = (c.ecx, c.esp);
    let (saved_esi, saved_edi) = (c.esi, c.edi);
    let finish = |c: &mut Cpu| {
        (c.esi, c.edi) = (saved_esi, saved_edi);
        c.esp = entry.wrapping_add(8);
    };
    if pg.u8(m, object.wrapping_add(0x108)) & 0x3f == 0 {
        c.eax = 0x3f;
        return finish(c);
    }
    let other = pg.u32(m, entry.wrapping_add(4));
    // the original's frame at its calls: esi and edi pushed
    let frame = entry.wrapping_sub(8);
    let test = |m: &mut dyn Mem, c: &mut Cpu, port: Option<ddnative_base::x86::LiftFn>, target: u32, args: &[u32], back: u32| {
        call_from(m, c, pg, frame, other, args, back, |m, c| match port {
            Some(port) => port(m, c),
            None => call_ext(m, c, target),
        })
    };
    let mut answer = test(m, c, ports.map(|p| p.0), 0x00ff_0780, &[object.wrapping_add(0x160)], 0x00ea_1f08);
    if answer as i32 > 0 {
        if answer != 0x3f {
            answer = test(m, c, ports.map(|p| p.1), 0x00ff_0ad0, &[object.wrapping_add(0x170), answer], 0x00ea_1f20);
        }
        if answer as i32 > 0 && pg.u8(m, object.wrapping_add(0x108)) & 0x3f == 1 {
            answer = 0x3f;
        }
    }
    c.eax = answer;
    finish(c)
}

// Translated from 0x0111d610 (DDDA.exe 2017-12-13 Steam), with 0x01192260 inlined.
/// 0x0111D610, `state_set->push(value) -> error` (thiscall, `ret 4`): push a
/// state record onto a set's stack of six (`+0x54` is the depth, records are
/// 12 bytes from `+0xc`: the value byte, then two zero words) and, first, a
/// snapshot onto its owner's stack (`+4`: depth at `+0x64`, 16-byte records
/// from `+4` holding the words `+0x74` and `+0x78`, then 0). Returns 9 when
/// the set is full, else 0 (the owner being full is not reported).
pub fn push_state(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let value = pg.u8(m, entry.wrapping_add(4));
    c.eax = push_state_record(m, pg, c.ecx, value);
    c.esp = entry.wrapping_add(8);
}

fn push_state_record(m: &mut dyn Mem, pg: Pages, set: u32, value: u8) -> u32 {
    let depth = pg.u32(m, set.wrapping_add(0x54));
    if depth >= 6 {
        return 9;
    }
    let owner = pg.u32(m, set.wrapping_add(4));
    let slot = set.wrapping_add((depth * 3 + 3) * 4);
    // 0x01192260 `owner->push_snapshot()`
    let owner_depth = pg.u32(m, owner.wrapping_add(0x64));
    if owner_depth < 6 {
        let record = owner.wrapping_add(owner_depth * 16 + 4);
        let first = pg.u32(m, owner.wrapping_add(0x74));
        let second = pg.u32(m, owner.wrapping_add(0x78));
        pg.set_u32(m, record, first);
        pg.set_u32(m, record.wrapping_add(4), second);
        pg.set_u32(m, record.wrapping_add(8), 0);
        pg.set_u8(m, record.wrapping_add(0xc), 0);
        let again = pg.u32(m, owner.wrapping_add(0x64)).wrapping_add(1);
        pg.set_u32(m, owner.wrapping_add(0x64), again);
    }
    pg.set_u32(m, slot.wrapping_add(4), 0);
    pg.set_u32(m, slot.wrapping_add(8), 0);
    pg.set_u8(m, slot, value);
    let again = pg.u32(m, set.wrapping_add(0x54)).wrapping_add(1);
    pg.set_u32(m, set.wrapping_add(0x54), again);
    0
}

// Translated from 0x00e61010 (DDDA.exe 2017-12-13 Steam).
/// 0x00E61010, `set->test(a, b, single, value) -> bool` (thiscall, `ret
/// 0x10`): a fan-out over the object's `+0xd0` members (`+0x30` array),
/// each pushed a state record with `value` ([`push_state`], 0x0111D610, run
/// natively). With `single` clear only the member
/// at index `a[+0x58]` is asked and its answer returned as is; otherwise
/// the result is true when any member answered non-zero.
pub fn any_member_test(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let (object, entry) = (c.ecx, c.esp);
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    if arg(m, 2) == 0 {
        let index = pg.u32(m, arg(m, 0).wrapping_add(0x58));
        let member = pg.u32(m, object.wrapping_add(index.wrapping_mul(4)).wrapping_add(0x30));
        let value = arg(m, 3) as u8;
        c.eax = push_state_record(m, pg, member, value);
    } else {
        let mut none_answered = true;
        let mut i = 0u32;
        if pg.u32(m, object.wrapping_add(0xd0)) != 0 {
            loop {
                let member = pg.u32(m, object.wrapping_add(0x30 + 4 * i));
                let value = arg(m, 3) as u8;
                let answer = push_state_record(m, pg, member, value);
                none_answered = none_answered && answer == 0;
                i += 1;
                if i >= pg.u32(m, object.wrapping_add(0xd0)) {
                    break;
                }
            }
        }
        c.eax = !none_answered as u32;
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4 + 0x10);
}

// ---------------------------------------------------------------------------
// Shader lookup (0x00E12A20) without guest calls

/// The most dependency words the original's 1 KiB stack buffer holds.
const KEY_WORDS: usize = 256;

/// The key sequence 0x00E10FC0 builds in its stack buffer.
struct KeyWords {
    words: [u32; KEY_WORDS],
    len: usize,
    /// More words than the buffer holds, or a dependency chain nested too
    /// deep: the caller falls back to the instruction-level port.
    overflow: bool,
}

impl KeyWords {
    fn push(&mut self, word: u32) {
        if self.len < KEY_WORDS {
            self.words[self.len] = word;
            self.len += 1;
        } else {
            self.overflow = true;
        }
    }

    /// 0x00E0E5E0: salted word FNV-1 of the sequence.
    fn hash(&self) -> u32 {
        let mut hash = 0x811c_9dc5u32;
        for &word in &self.words[..self.len] {
            let word = word ^ 0x14da_14da;
            for shift in [0, 8, 16, 24] {
                hash = hash.wrapping_mul(0x0100_0193) ^ ((word >> shift) & 255);
            }
        }
        hash
    }

    /// The table CRC (0x014075C0) of the words' bytes, high byte first. The
    /// table is constant image data: each entry is read from guest memory
    /// the first time it is used and kept.
    fn crc(&self, m: &dyn Mem, pg: Pages) -> u32 {
        static TABLE: [std::sync::atomic::AtomicU64; 256] = [const { std::sync::atomic::AtomicU64::new(0) }; 256];
        let mut crc = u32::MAX;
        for &word in &self.words[..self.len] {
            for shift in [24u32, 16, 8, 0] {
                let index = ((word >> shift) ^ crc) & 0xff;
                let slot = &TABLE[index as usize];
                let mut entry = slot.load(std::sync::atomic::Ordering::Relaxed);
                if entry == 0 {
                    entry = pg.u32(m, 0x0140_75c0u32.wrapping_add(index * 4)) as u64 | 1 << 32;
                    slot.store(entry, std::sync::atomic::Ordering::Relaxed);
                }
                crc = (crc >> 8) ^ entry as u32;
            }
        }
        crc
    }
}

/// 0x00E0EC50 `expand` into [`KeyWords`] instead of guest memory: a packed
/// dependency emits its high part (bits 12..32) when non-zero, then the
/// expansions of its recursive children and its non-zero terminal values
/// (see `semantic::expand_dependencies`).
fn expand_dependency(m: &dyn Mem, pg: Pages, bank: u32, values: u32, packed: u32, depth: u32, out: &mut KeyWords) {
    if depth > 32 {
        out.overflow = true;
        return;
    }
    let high = packed & 0xffff_f000;
    if high != 0 {
        out.push(high);
    }
    let descriptor = bank.wrapping_add((packed & 0xfff).wrapping_mul(8));
    let mut index = pg.u32(m, descriptor.wrapping_add(4));
    let counts = pg.u32(m, descriptor);
    let value_of = |m: &dyn Mem, index: u32| {
        let child = pg.u16(m, index) as u32;
        pg.u32(m, values.wrapping_add(child.wrapping_mul(8)).wrapping_add(4))
    };
    for _ in 0..(counts >> 20) & 0x3f {
        let value = value_of(m, index);
        index = index.wrapping_add(2);
        expand_dependency(m, pg, bank, values, value, depth + 1, out);
    }
    for _ in 0..counts >> 26 {
        let value = value_of(m, index);
        index = index.wrapping_add(2);
        if value != 0 {
            out.push(value);
        }
    }
}

/// 0x00E10FC0 `intern_keys` without the guest buffer: the key sequence of
/// `owner` (six packed dependencies), the four `keys` words (tagged with
/// their slot) and `extra`. `None` when the sequence does not fit the
/// original stack buffer.
fn dependency_key_words(m: &dyn Mem, pg: Pages, this: u32, owner: u32, values: u32, keys: u32, extra: u32) -> Option<KeyWords> {
    let mut out = KeyWords { words: [0; KEY_WORDS], len: 0, overflow: false };
    for slot in 0..4u32 {
        let key = pg.u32(m, keys.wrapping_add(4 * slot));
        if key != 0 {
            out.push((key & 0xffff_f000) | slot);
        }
    }
    out.push(extra);
    for field in [0u32, 3, 4, 2, 1, 5] {
        let packed = pg.u32(m, owner.wrapping_add(4 * field));
        if packed != 0 {
            let bank = pg.u32(m, this.wrapping_add(0x28));
            expand_dependency(m, pg, bank, values, packed, 0, &mut out);
        }
    }
    (!out.overflow).then_some(out)
}

/// 0x01008260 `package->find_pair(key, crc)`: the record of the node with
/// both words, or 0.
fn find_pair(m: &dyn Mem, pg: Pages, package: u32, key: u32, crc: u32) -> u32 {
    if key | crc == 0 {
        return 0;
    }
    let table = pg.u32(m, package.wrapping_add(0x84));
    let mut node = pg.u32(m, table.wrapping_add((key & 0xfff) * 4).wrapping_add(0x1c));
    while node != 0 {
        if pg.u32(m, node) == key && pg.u32(m, node.wrapping_add(4)) == crc {
            let index = pg.u32(m, node.wrapping_add(12)) & 0x7fff_ffff;
            return table.wrapping_add(index.wrapping_mul(36)).wrapping_add(0x401c);
        }
        let next = pg.u32(m, node.wrapping_add(8));
        if next == 0 {
            return 0;
        }
        node = next.wrapping_mul(16).wrapping_add(pg.u32(m, table.wrapping_add(0x18)));
    }
    0
}

/// 0x00E0F4B0 `shaders->find_pair_key(key, crc)`: [`find_pair`] in the six
/// packages (`+0xab104`), first match.
fn find_package_pair(m: &dyn Mem, pg: Pages, owner: u32, key: u32, crc: u32) -> u32 {
    for i in 0..6u32 {
        let package = pg.u32(m, owner.wrapping_add(0xab104 + 4 * i));
        if package != 0 {
            let found = find_pair(m, pg, package, key, crc);
            if found != 0 {
                return found;
            }
        }
    }
    0
}

// Translated from 0x00e12a20 (DDDA.exe 2017-12-13 Steam).
/// 0x00E12A20, `shaders->lookup(a, bank, slot, owner_values, keys, extra,
/// other_bank) -> entry` (thiscall, `ret 0x1c`): the shader/technique
/// lookup of every model primitive draw, with no guest calls.
///
/// The common branch (byte `+0x39` clear) builds the dependency key
/// sequence of `bucket(bank) + slot * 0x28 + 0x1c` (see
/// `semantic::intern_dependency_keys`), hashes it and looks the hash up
/// among the packages' keys (0x00E0F3C0), then the (hash, CRC) pair
/// (0x00E0F4B0), then the pair of `other_bank`'s sequence. The rare branch
/// (shader creation) runs the original body, as does a sequence too long
/// for the original stack buffer (through the instruction-level port).
pub fn shader_lookup(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let this = c.ecx;
    if pg.u8(m, this.wrapping_add(0x39)) != 0 {
        return crate::semantic::shader_lookup(m, c);
    }
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let (bank, slot, values, keys, extra, other) = (arg(m, 1), arg(m, 2), arg(m, 3), arg(m, 4), arg(m, 5), arg(m, 6));
    let bucket_owner = |m: &dyn Mem, bank: u32, slot: u32| {
        let table = pg.u32(m, this.wrapping_add(0x24));
        let bucket = pg.u32(m, table.wrapping_add((bank & 0xfff).wrapping_mul(4)));
        bucket.wrapping_add(slot.wrapping_mul(0x28)).wrapping_add(0x1c)
    };
    let finish = |c: &mut Cpu, result: u32| {
        c.eax = result;
        c.esp = entry.wrapping_add(4 + 0x1c);
    };
    let Some(words) = dependency_key_words(m, pg, this, bucket_owner(m, bank, slot), values, keys, extra) else {
        return crate::semantic::shader_lookup(m, c);
    };
    let hash = words.hash();
    let found = crate::semantic_draw_c::find_package_key(m, pg, this, hash);
    if found != 0 {
        return finish(c, found);
    }
    let found = find_package_pair(m, pg, this, hash, words.crc(m, pg));
    if found != 0 || pg.u8(m, this.wrapping_add(0xab229)) != 0 || other == 0 {
        return finish(c, found);
    }
    let owner = pg.u32(m, pg.u32(m, this.wrapping_add(0x24)).wrapping_add((other & 0xfff).wrapping_mul(4))).wrapping_add(0x1c);
    let Some(words) = dependency_key_words(m, pg, this, owner, values, keys, extra) else {
        return crate::semantic::shader_lookup(m, c);
    };
    let found = find_package_pair(m, pg, this, words.hash(), words.crc(m, pg));
    finish(c, found)
}

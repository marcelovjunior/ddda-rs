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

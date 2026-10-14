//! Semantic port of the per-mesh model draw (L171): 0x00EA5320,
//! `uBaseModel::vf26`. See crate::semantic for the ABI-level contract.
use crate::semantic::call_from;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

/// x86 `cvttss2si`: truncation toward zero, 0x80000000 when out of range
/// or NaN (Rust's `as i32` saturates instead).
fn cvttss2si(v: f32) -> i32 {
    if v.is_nan() || v >= 2147483648.0 || v < -2147483648.0 {
        i32::MIN
    } else {
        v as i32
    }
}

fn float(m: &dyn Mem, pg: Pages, a: u32) -> f32 {
    f32::from_bits(pg.u32(m, a))
}

/// A point transformed by the model matrix rows at +0xB0/+0xC0/+0xD0/+0xE0,
/// per lane `((row_c * y + row_b * x) + row_d * z) + row_e`: the order of
/// the original's packed SSE.
fn transform(m: &dyn Mem, pg: Pages, model: u32, x: f32, y: f32, z: f32) -> [f32; 4] {
    let lane = |row: u32, i: u32| float(m, pg, model.wrapping_add(row + 4 * i));
    let mut out = [0f32; 4];
    for i in 0..4u32 {
        out[i as usize] = ((lane(0xc0, i) * y + lane(0xb0, i) * x) + lane(0xd0, i) * z) + lane(0xe0, i);
    }
    out
}

/// The view-space depth of a point for a view matrix (0x00FED940 returns
/// it): `-(((m8 * x + m18 * y) + m28 * z) + m38)`.
fn view_depth(m: &dyn Mem, pg: Pages, view: u32, x: f32, y: f32, z: f32) -> f32 {
    let e = |o: u32| float(m, pg, view.wrapping_add(o));
    -(((e(8) * x + e(0x18) * y) + e(0x28) * z) + e(0x38))
}

/// The largest squared row norm of the model's 3x4 basis, with COMISS
/// semantics (an unordered compare is "not above").
fn largest_row_norm(m: &dyn Mem, pg: Pages, model: u32) -> f32 {
    let norm = |row: u32| {
        let e = |i: u32| float(m, pg, model.wrapping_add(row + 4 * i));
        ((e(0) * e(0) + e(1) * e(1)) + e(2) * e(2)) + e(3) * e(3)
    };
    let (n0, n1, n2) = (norm(0xb0), norm(0xc0), norm(0xd0));
    if n0 > n1 {
        if n0 > n2 { n0 } else { n2 }
    } else if n1 > n2 {
        n1
    } else {
        n2
    }
}

fn guest(target: u32) -> impl FnOnce(&mut dyn Mem, &mut Cpu) {
    move |m: &mut dyn Mem, c: &mut Cpu| crate::semantic::call_guest(m, c, target)
}

/// 0x00EA5320, `model->draw_mesh(context, mesh, materials, position,
/// pass)` (thiscall, `ret 0x18`): record the draws of one mesh's
/// primitives into a draw context.
///
/// Per mesh: bind the mesh's vertex set, pick the LOD from the distance to
/// the camera, and compute the near and far sort keys from the view depth.
/// Per primitive (0x30-byte records from `mesh + 0x70`, `mesh + 0x74` of
/// them): test visibility (the model's bitset, the enable bit, the LOD
/// mask, the context's pass mask, the optional pass filter 0x00EA42E0),
/// switch material (0x00FF14D0 / 0x00FED190) when it changes, set the
/// primitive state (0x00EA23F0), the stream binding and the sort key,
/// report the bounding sphere when asked, flush state (0x00FEEF20), append
/// a 32-byte draw command and its sort slot, and submit (0x00FF1920).
pub fn draw_mesh(m: &mut dyn Mem, c: &mut Cpu) {
    draw_mesh_with(m, c, false)
}

/// [`draw_mesh`] with its small callees folded in: the view matrix address
/// (0x00FED940), the primitive state flush (0x00FEEF20) and, inside it,
/// the state commit 0x00FEE320 ([`crate::semantic_draw_c::commit_primitive_state`])
/// run as direct native code instead of guest calls.
pub fn draw_mesh_folded(m: &mut dyn Mem, c: &mut Cpu) {
    draw_mesh_with(m, c, true)
}

fn draw_mesh_with(m: &mut dyn Mem, c: &mut Cpu, fold: bool) {
    let pg = Pages::of(m);
    let model = c.ecx;
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let (ctx, mesh, materials, position, pass) = (arg(m, 0), arg(m, 1), arg(m, 2), arg(m, 3), arg(m, 4));
    let finish = |c: &mut Cpu| c.esp = entry.wrapping_add(4 + 0x18);
    if pg.u8(m, ctx + 0x16a) & 0x20 != 0 {
        return finish(c);
    }
    let prims = pg.u32(m, mesh + 0x70);
    let stream = pg.u32(m, mesh + 0xa0);
    let mut count = pg.u32(m, mesh + 0x74);
    if count == 0 {
        return finish(c);
    }
    // The original's frame: push ebp; and esp, -16; 0x64 locals; 3 pushes.
    let frame = (entry.wrapping_sub(4) & !0xf).wrapping_sub(0x64 + 12);
    let tick = |m: &dyn Mem| pg.u32(m, 0x018d_2ce8).wrapping_add(1);
    let vertices = pg.u32(m, mesh + 0x9c);
    if pg.u32(m, ctx + 0x14c) != vertices {
        let t = tick(m);
        pg.set_u32(m, vertices + 4, t);
        pg.set_u32(m, ctx + 0x14c, vertices);
        pg.set_u32(m, ctx + 0x150, 1);
    }
    let bias = pg.u32(m, model + 0x154) as i32 as f32;
    let view = view_matrix(m, c, pg, frame, ctx, 0x00ea_53aa, fold);
    let (px, py, pz) = (float(m, pg, position), float(m, pg, position + 4), float(m, pg, position + 8));
    let depth = view_depth(m, pg, view, px, py, pz);
    let lods = pg.u32(m, model + 0xf0);
    let dx = px - float(m, pg, ctx + 0x184);
    let dy = py - float(m, pg, ctx + 0x188);
    let dz = pz - float(m, pg, ctx + 0x18c);
    let distance = cvttss2si(((dx * dx + dy * dy) + dz * dz).sqrt());
    let mut lod = ((pg.u32(m, model + 0x108) << 19) as i32) >> 28;
    if lod < 0 {
        let quality = pg.u32(m, pg.u32(m, 0x018d_1d20) + 0x38);
        lod = if quality == 3 {
            1
        } else {
            let scale = if quality == 2 { 2 } else { 1 };
            if distance > (pg.u32(m, lods + 0xe4) as i32).wrapping_mul(scale) {
                4
            } else {
                (distance > (pg.u32(m, lods + 0xe0) as i32).wrapping_mul(scale)) as i32 + 1
            }
        };
    }
    let lod_mask = lod as u8;
    let offset = float(m, pg, model + 0x16c);
    let near = cvttss2si((depth + bias) + offset).min(0x7fff).max(0);
    let near_key = ((0x7fff - near) as u32) << 12;
    let far = cvttss2si(offset + depth);
    let far = if far > 0xff_ffff { 0xff_ffff } else if far < 0 { 0 } else { far };
    let far_key = (pg.u32(m, model + 0x10c) << 24) | far as u32;
    let mut bounds_callback = pg.u8(m, model + 0x104) & 1 != 0 && pg.u16(m, ctx + 0x16a) & 0x813 != 0;
    if pg.u32(m, model + 0x15c) != 0 {
        bounds_callback = false;
    }
    let mut material: i32 = -1;
    let mut prim = prims.wrapping_add(4);
    while count != 0 {
        let index = pg.u32(m, prim) & 0xfff;
        let visible = pg.u32(m, model + 0x110 + 4 * (index >> 5)) & (1 << (index & 31)) != 0
            && pg.u8(m, prim + 4) & 1 != 0
            && lod_mask & (pg.u32(m, prim) >> 24) as u8 != 0
            && pg.u32(m, prim - 4) & pg.u16(m, ctx + 0x16a) as u32 != 0
            && (pass == 0x3f
                || (call_from(m, c, pg, frame, model, &[ctx, prim - 4, pass], 0x00ea_55c1, guest(0x00ea_42e0)) as i32) >= 0);
        if visible {
            draw_primitive(m, c, pg, frame, model, ctx, prim, materials, stream, &mut material, Keys { bias, near_key, far_key, bounds_callback, fold });
        }
        prim = prim.wrapping_add(0x30);
        count = count.wrapping_sub(1);
    }
    if material >= 0 {
        call_from(m, c, pg, frame, ctx, &[], 0x00ea_5a4d, guest(0x00ff_14d0));
    }
    finish(c)
}

struct Keys {
    fold: bool,
    bias: f32,
    near_key: u32,
    far_key: u32,
    bounds_callback: bool,
}

/// One visible primitive of [`draw_mesh`] (0x00EA55C9..0x00EA5A29).
#[allow(clippy::too_many_arguments)]
fn draw_primitive(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, model: u32, ctx: u32, prim: u32, materials: u32, stream: u32, material: &mut i32, keys: Keys) {
    let wanted = ((pg.u32(m, prim) >> 12) & 0xfff) as i32;
    if *material != wanted {
        if *material >= 0 {
            call_from(m, c, pg, frame, ctx, &[], 0x00ea_55e7, guest(0x00ff_14d0));
        }
        *material = ((pg.u32(m, prim) >> 12) & 0xfff) as i32;
        let state = pg.u32(m, materials.wrapping_add(4 * *material as u32));
        call_from(m, c, pg, frame, ctx, &[state, 0x18], 0x00ea_5605, guest(0x00fe_d190));
    }
    call_from(m, c, pg, frame, model, &[ctx, prim - 4], 0x00ea_5611, guest(0x00ea_23f0));
    let format = pg.u8(m, prim + 6);
    let layout = pg.u32(m, prim + 0xc);
    if pg.u32(m, ctx + 0x138) != layout || pg.u32(m, ctx + 0x124) != stream || pg.u8(m, ctx + 0x134) != format {
        if stream != 0 {
            let t = pg.u32(m, 0x018d_2ce8).wrapping_add(1);
            pg.set_u32(m, stream + 4, t);
        }
        pg.set_u32(m, ctx + 0x124, stream);
        pg.set_u8(m, ctx + 0x134, format);
        pg.set_u32(m, ctx + 0x138, layout);
        pg.set_u32(m, ctx + 0x150, 1);
    }
    let mode = pg.u32(m, ctx + 0x164) & 0x1f;
    if mode != 0x10 && mode != 0xe {
        let key = (pg.u8(m, prim + 5) as u32).wrapping_add(keys.far_key);
        pg.set_u32(m, ctx + 0x164, (key << 5) | mode);
    } else if pg.u8(m, prim + 4) & 4 == 0 {
        let key = (pg.u8(m, prim + 5) as u32).wrapping_add(keys.near_key);
        pg.set_u32(m, ctx + 0x164, (key << 5) | mode);
    } else {
        let bounds = pg.u32(m, prim + 0x28);
        let center = transform(m, pg, model, float(m, pg, bounds + 0x10), float(m, pg, bounds + 0x14), float(m, pg, bounds + 0x18));
        let view = view_matrix(m, c, pg, frame, ctx, 0x00ea_56fa, keys.fold);
        let depth = view_depth(m, pg, view, center[0], center[1], center[2]) + keys.bias;
        let v = cvttss2si(depth).min(0x7fff).max(0);
        let key = (((0x7fff - v) as u32) << 12).wrapping_add(pg.u8(m, prim + 5) as u32);
        let mode = pg.u32(m, ctx + 0x164) & 0x1f;
        pg.set_u32(m, ctx + 0x164, (key << 5) | mode);
    }
    if keys.bounds_callback {
        let bounds = pg.u32(m, prim + 0x28);
        let radius = largest_row_norm(m, pg, model).sqrt() * float(m, pg, bounds + 0x1c);
        let center = transform(m, pg, model, float(m, pg, bounds + 0x10), float(m, pg, bounds + 0x14), float(m, pg, bounds + 0x18));
        // The sphere lives in the original's frame at [esp + 0x40], which
        // the callee reads through its pointer argument.
        let sphere = frame.wrapping_add(0x40);
        for (i, v) in [center[0], center[1], center[2], radius].into_iter().enumerate() {
            pg.set_u32(m, sphere + 4 * i as u32, v.to_bits());
        }
        let listener = pg.u32(m, 0x018d_2c80);
        let method = pg.u32(m, pg.u32(m, listener) + 0x34);
        let group = (pg.u32(m, model + 0x108) >> 25) & 0xf;
        let id = pg.u32(m, model + 0x100);
        call_from(m, c, pg, frame, listener, &[ctx, id, group, sphere], 0x00ea_5912, guest(method));
    }
    if keys.fold {
        flush_primitive_state(m, c, pg, frame, ctx);
    } else {
        call_from(m, c, pg, frame, ctx, &[], 0x00ea_5919, guest(0x00fe_ef20));
    }
    let (word7, word6, word5) = (pg.u32(m, prim + 0x1c), pg.u32(m, prim + 0x14), pg.u32(m, prim + 0x18));
    let flags = pg.u32(m, ctx + 0x5c);
    if flags & 0x100 != 0 || flags & 0x50 == 0 {
        if pg.u32(m, ctx + 0x24).wrapping_add(0x20) > pg.u32(m, ctx + 0x28) {
            call_from(m, c, pg, frame, ctx, &[0x20], 0x00ea_5954, guest(0x00fe_d890));
        }
        append_draw(m, pg, ctx, word5, word6, word7);
    }
    call_from(m, c, pg, frame, ctx, &[], 0x00ea_5a2e, guest(0x00ff_1920));
}

/// Append one 32-byte draw command at the context's cursor and its sort
/// slot (0x00EA5940..0x00EA5A21).
fn append_draw(m: &mut dyn Mem, pg: Pages, ctx: u32, word5: u32, word6: u32, word7: u32) {
    let record = pg.u32(m, ctx + 0x24);
    pg.set_u32(m, ctx + 0x24, record.wrapping_add(0x20));
    let mut head = (pg.u32(m, record) & 0xfff1_ffff) | 0x10000;
    pg.set_u32(m, record, head);
    let serial = pg.u32(m, ctx + 0x74);
    head = ((serial ^ head) & 0x7fff) ^ head;
    pg.set_u32(m, record, head);
    pg.set_u32(m, ctx + 0x74, serial.wrapping_add(1));
    let transparent = (pg.u8(m, ctx + 0x5c) & 0x50 != 0) as u32;
    let head = pg.u32(m, record);
    pg.set_u32(m, record, head ^ (((transparent << 15) ^ head) & 0x8000));
    let head = pg.u32(m, record);
    let pass = pg.u32(m, ctx + 0x6c);
    pg.set_u32(m, record, (pass << 20) | (head & 0xfffff));
    for (offset, field) in [(8u32, 0x194u32), (0xc, 0x198), (0x10, 0x19c)] {
        let v = pg.u32(m, ctx + field);
        pg.set_u32(m, record + offset, v);
    }
    let state = pg.u32(m, ctx + 0x1a0);
    pg.set_u32(m, record + 0x14, word5);
    pg.set_u32(m, record + 4, state);
    pg.set_u32(m, record + 0x1c, word7);
    pg.set_u32(m, record + 0x18, word6);
    let index = pg.u32(m, ctx + 0x48);
    let slot = pg.u32(m, ctx + 0x44).wrapping_add(index.wrapping_mul(8));
    pg.set_u32(m, ctx + 0x48, index.wrapping_add(1));
    let key = pg.u32(m, ctx + 0x164);
    let first = (key << 27) | (pg.u32(m, slot) & 0x7ff_ffff);
    pg.set_u32(m, slot, first);
    let order = (pg.u32(m, ctx + 0x164) >> 5).wrapping_add(pg.u32(m, ctx + 0x178));
    pg.set_u32(m, slot + 4, record);
    pg.set_u32(m, slot, ((order ^ first) & 0x7ff_ffff) ^ first);
}

/// 0x00FED940, `ctx->view_matrix()`: the address of the current view
/// matrix, `(ctx[0xC88] & ~15) + 0x70` (a guest call unless `fold`).
fn view_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, ctx: u32, back: u32, fold: bool) -> u32 {
    if fold {
        (pg.u32(m, ctx.wrapping_add(0xc88)) & !0xf).wrapping_add(0x70)
    } else {
        call_from(m, c, pg, frame, ctx, &[], back, guest(0x00fe_d940))
    }
}

/// 0x00FEEF20, `ctx->flush_primitive_state()` (thiscall, void): unless the
/// context's mode flags (+0x5C) have bit 6 set, commit the state of the
/// current primitive (0x00FEE320, with the resource `[ctx[0x20] +
/// (ctx[0x1CC] & 0xFFF) * 8]` and the index byte ctx[0x169], 0xFF meaning 0)
/// and record the outcome: flag 0x08 when it committed, 0x10 when not, with
/// the packet cursor `ctx[0x48]` copied to `ctx[0x60]`.
///
/// The commit is called directly with the original's frame: `frame` is the
/// caller's ESP at its call of 0x00FEEF20, which pushes a return address,
/// its saved ESI and two arguments before calling 0x00FEE320.
// Translated from 0x00FEEF20 (DDDA.exe 2017-12-13 Steam)
fn flush_primitive_state(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, ctx: u32) {
    if pg.u8(m, ctx.wrapping_add(0x5c)) & 0x40 != 0 {
        return;
    }
    let slot = pg.u32(m, ctx.wrapping_add(0x1cc)) & 0xfff;
    let resource = pg.u32(m, pg.u32(m, ctx.wrapping_add(0x20)).wrapping_add(slot * 8));
    let index = match pg.u8(m, ctx.wrapping_add(0x169)) {
        0xff => 0,
        v => v as u32,
    };
    let entry = frame.wrapping_sub(20);
    pg.set_u32(m, entry, 0x00fe_ef54);
    pg.set_u32(m, entry.wrapping_add(4), resource);
    pg.set_u32(m, entry.wrapping_add(8), index);
    c.esp = entry;
    c.ecx = ctx;
    crate::semantic_draw_c::commit_primitive_state(m, c);
    let committed = c.eax & 0xff != 0;
    let cursor = pg.u32(m, ctx.wrapping_add(0x48));
    let flags = pg.u32(m, ctx.wrapping_add(0x5c)) | if committed { 0x08 } else { 0x10 };
    pg.set_u32(m, ctx.wrapping_add(0x5c), flags);
    pg.set_u32(m, ctx.wrapping_add(0x60), cursor);
}

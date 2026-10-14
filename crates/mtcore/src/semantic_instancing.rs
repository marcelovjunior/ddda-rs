//! Instanced model draw dispatch with the original material scopes.
use crate::semantic::call_from;
use ddnative_base::{
    mem::{Mem, Pages},
    x86::Cpu,
};

fn call(
    m: &mut dyn Mem,
    c: &mut Cpu,
    p: Pages,
    frame: u32,
    receiver: u32,
    args: &[u32],
    back: u32,
    target: u32,
) -> u32 {
    call_from(m, c, p, frame, receiver, args, back, |m, c| {
        crate::semantic::call_guest(m, c, target)
    })
}

// Translated from 0x00C24640 (DDDA.exe 2017-12-13 Steam).
/// `uOmSwingBaseInstancing::vf26`: select per-primitive instance data,
/// refresh material bindings when its revisions change, and submit draws.
/// The instance selection object occupies the original frame's +0x5c..+0x87.
pub fn draw_instanced_mesh(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    let entry = c.esp;
    let model = c.ecx;
    let saved = [c.ebx, c.ebp, c.esi, c.edi];
    let ctx = p.u32(m, entry.wrapping_add(4));
    let mesh = p.u32(m, entry.wrapping_add(8));
    let materials = p.u32(m, entry.wrapping_add(12));
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x90);
    let instances = model.wrapping_add(0x340);
    let active = call(m, c, p, frame, instances, &[], 0x00c2465c, 0x01004ab0);
    if active == 0 || p.u32(m, active.wrapping_add(0x60)) == 0 {
        // This internal helper uses EAX/ESI/EDI, not the ordinary thiscall ABI.
        c.eax = mesh;
        c.esi = model;
        c.edi = ctx;
        call(m, c, p, frame, c.ecx, &[materials], 0x00c24bb9, 0x00c242c0);
    } else if p.u8(m, ctx.wrapping_add(0x16a)) & 0x20 == 0 {
        let count = p.u32(m, mesh.wrapping_add(0x74));
        let primitives = p.u32(m, mesh.wrapping_add(0x70));
        let stream = p.u32(m, mesh.wrapping_add(0xa0));
        if count != 0 {
            let vertices = p.u32(m, mesh.wrapping_add(0x9c));
            if p.u32(m, ctx.wrapping_add(0x14c)) != vertices {
                let tick = p.u32(m, 0x018d2ce8).wrapping_add(1);
                p.set_u32(m, vertices.wrapping_add(4), tick);
                p.set_u32(m, ctx.wrapping_add(0x14c), vertices);
                p.set_u32(m, ctx.wrapping_add(0x150), 1);
            }
            call(m, c, p, frame, ctx, &[], 0x00c246d8, 0x00fed940);
            let report_bounds = p.u8(m, model.wrapping_add(0x104)) & 1 != 0
                && p.u16(m, ctx.wrapping_add(0x16a)) & 0x813 != 0
                && p.u32(m, model.wrapping_add(0x15c)) == 0;
            let selection = frame.wrapping_add(0x5c);
            call(m, c, p, frame, instances, &[selection], 0x00c24717, 0x0113c160);
            let mut material = u32::MAX;
            for index in 0..count {
                let prim = primitives.wrapping_add(index.wrapping_mul(0x30));
                call(
                    m,
                    c,
                    p,
                    frame,
                    instances,
                    &[selection, index],
                    0x00c24743,
                    0x0113aea0,
                );
                let visible = p.u32(m, prim.wrapping_add(4)) & 0xfff;
                if p.u32(m, model.wrapping_add(0x110 + (visible >> 5) * 4)) & (1 << (visible & 31)) == 0
                    || p.u8(m, prim.wrapping_add(8)) & 1 == 0
                    || p.u32(m, prim) & p.u16(m, ctx.wrapping_add(0x16a)) as u32 == 0
                {
                    continue;
                }
                let wanted = (p.u32(m, prim.wrapping_add(4)) >> 12) & 0xfff;
                if material != wanted || selection_stale(m, p, selection) {
                    if material != wanted {
                        if (material as i32) >= 0 {
                            call(m, c, p, frame, ctx, &[], 0x00c247e7, 0x00ff14d0);
                        }
                        material = (p.u32(m, prim.wrapping_add(4)) >> 12) & 0xfff;
                        let state = p.u32(m, materials.wrapping_add(material * 4));
                        call(m, c, p, frame, ctx, &[state, 0x18], 0x00c24805, 0x00fed190);
                        bind_instance_state(m, p, ctx, 0x2eb0, 0xafad15d6);
                    }
                    call(
                        m,
                        c,
                        p,
                        frame,
                        instances,
                        &[ctx, selection],
                        0x00c248ae,
                        0x0113c1b0,
                    );
                }
                call(m, c, p, frame, model, &[ctx, prim], 0x00c248ba, 0x00ea23f0);
                let format = p.u8(m, prim.wrapping_add(10));
                let layout = p.u32(m, prim.wrapping_add(16));
                if p.u32(m, ctx.wrapping_add(0x138)) != layout
                    || p.u32(m, ctx.wrapping_add(0x124)) != stream
                    || p.u8(m, ctx.wrapping_add(0x134)) != format
                {
                    if stream != 0 {
                        let tick = p.u32(m, 0x018d2ce8).wrapping_add(1);
                        p.set_u32(m, stream.wrapping_add(4), tick);
                    }
                    p.set_u32(m, ctx.wrapping_add(0x124), stream);
                    p.set_u8(m, ctx.wrapping_add(0x134), format);
                    p.set_u32(m, ctx.wrapping_add(0x138), layout);
                    p.set_u32(m, ctx.wrapping_add(0x150), 1);
                }
                if report_bounds {
                    report_sphere(m, c, p, frame, model, ctx, prim);
                }
                let bounds = p.u32(m, prim.wrapping_add(0x2c));
                call(
                    m,
                    c,
                    p,
                    frame,
                    instances,
                    &[ctx, bounds.wrapping_add(0x20), selection],
                    0x00c24aa9,
                    0x0113aee0,
                );
                call(m, c, p, frame, ctx, &[], 0x00c24ab0, 0x00feef20);
                let material = (p.u32(m, prim.wrapping_add(4)) >> 12) & 0xfff;
                c.edx = p.u32(m, materials.wrapping_add(material * 4));
                call(m, c, p, frame, model, &[ctx, prim], 0x00c24acd, 0x00c26310);
                call(m, c, p, frame, ctx, &[], 0x00c24ad4, 0x00ff1920);
            }
            if (material as i32) >= 0 {
                call(m, c, p, frame, ctx, &[], 0x00c24af8, 0x00ff14d0);
            }
            bind_instance_state(m, p, ctx, 0x2ea8, 0x5d5);
        }
    }
    [c.ebx, c.ebp, c.esi, c.edi] = saved;
    c.esp = entry.wrapping_add(28);
}

fn selection_stale(m: &dyn Mem, p: Pages, selection: u32) -> bool {
    let first = p.u32(m, selection.wrapping_add(8));
    if p.u32(m, selection.wrapping_add(12)) != p.u32(m, first.wrapping_add(16))
        || p.u32(m, selection.wrapping_add(16)) != p.u8(m, first.wrapping_add(0x32)) as u32
    {
        return true;
    }
    let second = p.u32(m, selection.wrapping_add(32));
    second != 0
        && (p.u32(m, selection.wrapping_add(36)) != p.u32(m, second.wrapping_add(16))
            || p.u32(m, selection.wrapping_add(40)) != p.u8(m, second.wrapping_add(0x32)) as u32)
}

/// +0x30a0/+0x30a4 are the selected instance state/key; +0x6c counts
/// saved scopes in the context's twelve-byte records at +0x30c7c.
fn bind_instance_state(m: &mut dyn Mem, p: Pages, ctx: u32, slot: u32, key: u32) {
    let bank = p.u32(m, ctx.wrapping_add(0x20));
    if p.u32(m, ctx.wrapping_add(0x30a0)) == p.u32(m, bank.wrapping_add(slot)) {
        return;
    }
    if p.u8(m, ctx.wrapping_add(0x5c)) & 0x80 != 0 {
        let record = ctx
            .wrapping_add(0x30c7c)
            .wrapping_add(p.u32(m, ctx.wrapping_add(0x6c)).wrapping_mul(12));
        let tag = (p.u32(m, record) & 0x800005d5) | 0x5d5;
        p.set_u32(m, record, tag);
        let record = ctx
            .wrapping_add(0x30c7c)
            .wrapping_add(p.u32(m, ctx.wrapping_add(0x6c)).wrapping_mul(12));
        let tag = p.u32(m, record) & 0x7fffffff;
        p.set_u32(m, record, tag);
        let state = p.u32(m, ctx.wrapping_add(0x30a0));
        let record = ctx
            .wrapping_add(0x30c7c)
            .wrapping_add(p.u32(m, ctx.wrapping_add(0x6c)).wrapping_mul(12));
        p.set_u32(m, record.wrapping_add(4), state);
        let key = p.u32(m, ctx.wrapping_add(0x30a4));
        p.set_u32(m, record.wrapping_add(8), key);
        let count = p.u32(m, ctx.wrapping_add(0x6c)).wrapping_add(1);
        p.set_u32(m, ctx.wrapping_add(0x6c), count);
    }
    let state = p.u32(m, p.u32(m, ctx.wrapping_add(0x20)).wrapping_add(slot));
    p.set_u32(m, ctx.wrapping_add(0x30a0), state);
    p.set_u32(m, ctx.wrapping_add(0x30a4), key);
    let dirty = p.u32(m, ctx.wrapping_add(0x154)) | 3;
    p.set_u32(m, ctx.wrapping_add(0x154), dirty);
}

fn report_sphere(m: &mut dyn Mem, c: &mut Cpu, p: Pages, frame: u32, model: u32, ctx: u32, prim: u32) {
    let f = |a| f32::from_bits(p.u32(m, a));
    let row = |offset| -> [f32; 4] { std::array::from_fn(|i| f(model.wrapping_add(offset + i as u32 * 4))) };
    let [b, d, e, t] = [row(0xb0), row(0xc0), row(0xd0), row(0xe0)];
    let norm = |v: [f32; 4]| ((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]) + v[3] * v[3];
    let [n0, n1, n2] = [norm(b), norm(d), norm(e)];
    let largest = if n0 > n1 {
        if n0 > n2 {
            n0
        } else {
            n2
        }
    } else if n1 > n2 {
        n1
    } else {
        n2
    };
    let bounds = p.u32(m, prim.wrapping_add(0x2c));
    let [x, y, z] = [
        f(bounds.wrapping_add(16)),
        f(bounds.wrapping_add(20)),
        f(bounds.wrapping_add(24)),
    ];
    let radius = largest.sqrt() * f(bounds.wrapping_add(28));
    let center: [f32; 4] = std::array::from_fn(|i| ((d[i] * y + b[i] * x) + e[i] * z) + t[i]);
    for (i, v) in center.into_iter().enumerate() {
        p.set_u32(m, frame.wrapping_add(0x40 + i as u32 * 4), v.to_bits());
    }
    for (i, v) in [center[0], center[1], center[2], radius].into_iter().enumerate() {
        p.set_u32(m, frame.wrapping_add(0x30 + i as u32 * 4), v.to_bits());
    }
    let listener = p.u32(m, 0x018d2c80);
    let target = p.u32(m, p.u32(m, listener).wrapping_add(0x34));
    let id = p.u32(m, model.wrapping_add(0x100));
    let group = (p.u32(m, model.wrapping_add(0x108)) >> 25) & 15;
    call(
        m,
        c,
        p,
        frame,
        listener,
        &[ctx, id, group, frame.wrapping_add(0x30)],
        0x00c24a91,
        target,
    );
}

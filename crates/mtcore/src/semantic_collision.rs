//! Swept support-map collision, retaining the original simplex layout.
//! WIP L178: 640 ABI vectors pass; final optimized gate pending head handoff.
//! Deliberately absent from ddports until that gate and final benchmark pass.
use crate::semantic::call_from;
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu},
};
type Vector = [f32; 3];
fn float(m: &dyn Mem, p: Pages, a: u32) -> f32 {
    f32::from_bits(p.u32(m, a))
}
fn vector(m: &dyn Mem, p: Pages, a: u32) -> Vector {
    std::array::from_fn(|i| float(m, p, a.wrapping_add(i as u32 * 4)))
}
fn store(m: &mut dyn Mem, p: Pages, a: u32, v: Vector) {
    for (i, x) in v.into_iter().enumerate() {
        p.set_u32(m, a.wrapping_add(i as u32 * 4), x.to_bits());
    }
}
fn dot(a: Vector, b: Vector) -> f32 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}
fn plane(a: Vector, b: Vector) -> f32 {
    (a[1] * b[1] + a[0] * b[0]) + a[2] * b[2]
}
fn sub(a: Vector, b: Vector) -> Vector {
    std::array::from_fn(|i| a[i] - b[i])
}
fn scale(a: Vector, b: f32) -> Vector {
    [b * a[0], a[1] * b, a[2] * b]
}
fn call(
    m: &mut dyn Mem,
    c: &mut Cpu,
    p: Pages,
    frame: u32,
    this: u32,
    args: &[u32],
    back: u32,
    target: u32,
) -> u32 {
    call_from(m, c, p, frame, this, args, back, |m, c| {
        x86::call_ext(m, c, target)
    })
}
fn reset(m: &mut dyn Mem, p: Pages, s: u32) {
    p.set_u32(m, s, 0);
    p.set_u16(m, s.wrapping_add(0x170), 0x100);
    for i in 0..3 {
        let v = p.u32(m, 0x018cfe80 + i * 4);
        p.set_u32(m, s.wrapping_add(0x150 + i * 4), v);
    }
    p.set_u32(m, s.wrapping_add(0x15c), 0);
    for i in 0..3 {
        let v = p.u32(m, 0x01629570 + i * 4);
        p.set_u32(m, s.wrapping_add(0x160 + i * 4), v);
    }
    p.set_u32(m, s.wrapping_add(0x16c), 0);
}
fn support_offset(m: &dyn Mem, p: Pages, shape: u32, direction: Vector, first: bool) -> Vector {
    let margin = float(m, p, 0x01408e74);
    std::array::from_fn(|i| {
        let k = i as u32 * 4;
        let delta = float(m, p, shape.wrapping_add(0x10 + k)) - float(m, p, shape.wrapping_add(0x20 + k));
        // The first support uses product+delta; the second uses delta+product.
        if first || i == 2 {
            direction[i] * margin + delta
        } else {
            delta + direction[i] * margin
        }
    })
}

// Translated from 0x00D7CF70 (DDDA.exe 2017-12-13 Steam).
/// Sweep a support-mapped shape along a displacement. The owner holds the
/// iteration limit (+0x190) and squared-distance tolerance (+0x194).
/// The simplex stores count +0, points +0x10, support witnesses +0x70/+0xd0,
/// closest point +0x150, and validity/dirty bytes +0x170/+0x171.
/// The result is an x86 boolean (AL); callee-saved state and ret 0x14 remain.
pub fn sweep_support_shape(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    let entry = c.esp;
    let owner = c.ecx;
    let saved = [c.ebx, c.ebp, c.esi, c.edi];
    let shape = p.u32(m, entry.wrapping_add(4));
    let input = p.u32(m, entry.wrapping_add(8));
    let simplex = p.u32(m, entry.wrapping_add(12));
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x140);
    let result = sweep(m, c, p, frame, owner, shape, input, simplex, entry);
    c.eax = (c.eax & 0xffffff00) | u32::from(result);
    [c.ebx, c.ebp, c.esi, c.edi] = saved;
    c.esp = entry.wrapping_add(24);
}

#[allow(clippy::too_many_arguments)]
fn sweep(
    m: &mut dyn Mem,
    c: &mut Cpu,
    p: Pages,
    frame: u32,
    owner: u32,
    shape: u32,
    input: u32,
    simplex: u32,
    entry: u32,
) -> bool {
    let mut velocity = vector(m, p, input);
    reset(m, p, simplex);
    let magnitude = dot(velocity, velocity);
    let mut direction = if !(float(m, p, owner.wrapping_add(0x194)) > magnitude) {
        scale(velocity, float(m, p, 0x01406ce0) / magnitude.sqrt())
    } else {
        velocity = vector(m, p, 0x01629530);
        velocity
    };
    store(m, p, frame.wrapping_add(0x10), direction);
    p.set_u32(m, frame.wrapping_add(0x1c), 0);
    let mut normal = vector(m, p, 0x018cfe80);
    let mut point = normal;
    let mut distance = 0f32;
    call(
        m,
        c,
        p,
        frame,
        shape,
        &[frame.wrapping_add(0x10)],
        0x00d7d12f,
        0x00d63ff0,
    );
    // The helper receives a pointer to this direction and can update it.
    direction = sub(
        normal,
        support_offset(m, p, shape, vector(m, p, frame.wrapping_add(0x10)), true),
    );
    let initial_squared = dot(direction, direction);
    let unit = scale(direction, float(m, p, 0x01406ce0) / initial_squared.sqrt());
    store(m, p, frame.wrapping_add(0xa0), unit);
    p.set_u32(m, frame.wrapping_add(0xac), 0);
    call(
        m,
        c,
        p,
        frame,
        shape,
        &[frame.wrapping_add(0xa0)],
        0x00d7d243,
        0x00d63ff0,
    );
    let mut support = support_offset(m, p, shape, unit, false);
    let mut delta = sub(normal, support);
    let mut separation = plane(delta, direction);
    let mut approach = dot(direction, velocity);
    normal = vector(m, p, 0x018cfe80);
    if initial_squared > float(m, p, owner.wrapping_add(0x194)) && p.u32(m, owner.wrapping_add(0x190)) != 0 {
        let mut iteration = 0u32;
        c.eax = owner;
        loop {
            if separation > 0.0 {
                if approach >= 0.0 {
                    return false;
                }
                reset(m, p, simplex);
                distance = distance - separation / approach;
                point = std::array::from_fn(|i| velocity[i] * distance);
                delta = sub(point, support);
                if distance > float(m, p, 0x01406ce0) {
                    return false;
                }
                normal = direction;
            }
            store(m, p, frame.wrapping_add(0x70), delta);
            p.set_u32(m, frame.wrapping_add(0x7c), 0);
            let inserted = call(
                m,
                c,
                p,
                frame,
                simplex,
                &[
                    frame.wrapping_add(0x70),
                    shape.wrapping_add(0x10),
                    shape.wrapping_add(0x20),
                ],
                0x00d7d4c9,
                0x00d1b540,
            );
            if inserted as u8 == 0 {
                break;
            }
            if p.u8(m, simplex.wrapping_add(0x171)) == 0 {
                direction = vector(m, p, simplex.wrapping_add(0x150));
                if p.u8(m, simplex.wrapping_add(0x170)) == 0 {
                    return false;
                }
            } else {
                p.set_u8(m, simplex.wrapping_add(0x171), 0);
                c.eax = p.u32(m, simplex);
                match c.eax {
                    0 => {
                        p.set_u8(m, simplex.wrapping_add(0x170), 0);
                        return false;
                    }
                    1 => copy_point(m, c, p, simplex.wrapping_add(0x150), simplex.wrapping_add(16)),
                    2 => {
                        call(
                            m,
                            c,
                            p,
                            frame,
                            simplex,
                            &[simplex.wrapping_add(0x150)],
                            0x00d7d583,
                            0x00d12e10,
                        );
                    }
                    3 => reduce_triangle(m, c, p, simplex),
                    4 => {
                        let ok = call(
                            m,
                            c,
                            p,
                            frame,
                            simplex,
                            &[
                                simplex.wrapping_add(0x150),
                                simplex.wrapping_add(16),
                                simplex.wrapping_add(32),
                                simplex.wrapping_add(48),
                                simplex.wrapping_add(64),
                            ],
                            0x00d7df2d,
                            0x00d8e400,
                        );
                        if ok as u8 == 0 {
                            for i in 0..3 {
                                let v = p.u32(m, 0x018cfe80 + i * 4);
                                p.set_u32(m, simplex.wrapping_add(0x150 + i * 4), v);
                            }
                            p.set_u32(m, simplex.wrapping_add(0x15c), 0);
                        }
                    }
                    _ => return false,
                }
                direction = vector(m, p, simplex.wrapping_add(0x150));
            }
            let squared = dot(direction, direction);
            let length = squared.sqrt();
            let unit = if !(float(m, p, 0x01407124) > length) {
                scale(direction, float(m, p, 0x01406ce0) / length)
            } else {
                direction
            };
            if float(m, p, owner.wrapping_add(0x194)) >= squared {
                break;
            }
            store(m, p, frame.wrapping_add(0xa0), unit);
            p.set_u32(m, frame.wrapping_add(0xac), 0);
            call(
                m,
                c,
                p,
                frame,
                shape,
                &[frame.wrapping_add(0xa0)],
                0x00d7dfe7,
                0x00d63ff0,
            );
            support = support_offset(m, p, shape, unit, false);
            delta = sub(point, support);
            approach = dot(direction, velocity);
            separation = plane(delta, direction);
            iteration = iteration.wrapping_add(1);
            c.eax = iteration;
            if iteration >= p.u32(m, owner.wrapping_add(0x190)) {
                break;
            }
        }
    }
    let length = plane(normal, normal).sqrt();
    if !(float(m, p, 0x01407124) > length) {
        normal = scale(normal, float(m, p, 0x01406ce0) / length);
    }
    for (argument, negative) in [(16, false), (20, true)] {
        let out = p.u32(m, entry.wrapping_add(argument));
        c.eax = out;
        if out == 0 {
            continue;
        }
        let sign = if negative { p.u32(m, 0x0161c280) } else { 0 };
        for i in 0..3 {
            let v = p.u32(m, 0x018cfe80 + i * 4);
            p.set_u32(m, out.wrapping_add(i * 4), v);
        }
        p.set_u32(m, out.wrapping_add(12), 0);
        for (i, v) in normal.into_iter().enumerate() {
            let bits = v.to_bits() ^ sign;
            p.set_u32(m, out.wrapping_add(16 + i as u32 * 4), bits);
        }
        p.set_u32(m, out.wrapping_add(28), 0);
        p.set_u32(m, out.wrapping_add(32), 0);
        p.set_u32(m, out.wrapping_add(36), distance.to_bits());
        p.set_u8(m, out.wrapping_add(40), 1);
    }
    true
}

/// Original FLD/FSTP point copy, including signaling-NaN quieting.
fn copy_point(m: &mut dyn Mem, c: &mut Cpu, p: Pages, out: u32, input: u32) {
    for i in 0..3 {
        let mut bytes = [0u8; 10];
        bytes[..4].copy_from_slice(&p.u32(m, input.wrapping_add(i * 4)).to_le_bytes());
        c.st.exec::<0x00d9>(&mut bytes);
        c.st.exec::<0x18d9>(&mut bytes);
        p.set_u32(
            m,
            out.wrapping_add(i * 4),
            u32::from_le_bytes(bytes[..4].try_into().unwrap()),
        );
    }
    p.set_u32(m, out.wrapping_add(12), 0);
}
fn move_vertex(m: &mut dyn Mem, c: &mut Cpu, p: Pages, s: u32, to: u32, from: u32) {
    for base in [0x10, 0x70, 0xd0] {
        copy_point(
            m,
            c,
            p,
            s.wrapping_add(base + to * 16),
            s.wrapping_add(base + from * 16),
        );
    }
}
fn closest(m: &mut dyn Mem, p: Pages, s: u32, v: Vector) {
    store(m, p, s.wrapping_add(0x150), v);
    p.set_u32(m, s.wrapping_add(0x15c), 0);
}
fn vertex_region(m: &mut dyn Mem, c: &mut Cpu, p: Pages, s: u32, index: u32) {
    let third_x = if index == 2 {
        Some(p.u32(m, s.wrapping_add(48)))
    } else {
        None
    };
    p.set_u32(m, s, 1);
    if index != 0 {
        move_vertex(m, c, p, s, 0, index);
    }
    // The third-vertex arm writes X with MOVSS; its other copies use x87.
    if let Some(bits) = third_x {
        p.set_u32(m, s.wrapping_add(16), bits);
    }
    copy_point(m, c, p, s.wrapping_add(0x150), s.wrapping_add(16 + index * 16));
}
fn inverse_or_zero(m: &dyn Mem, p: Pages, value: f32) -> f32 {
    if value == 0.0 {
        0.0
    } else {
        float(m, p, 0x01406ce0) / value
    }
}

/// Triangle Voronoi regions in the executable's branch and arithmetic order.
fn reduce_triangle(m: &mut dyn Mem, c: &mut Cpu, p: Pages, s: u32) {
    let a = vector(m, p, s.wrapping_add(16));
    let b = vector(m, p, s.wrapping_add(32));
    let d = vector(m, p, s.wrapping_add(48));
    let origin = vector(m, p, 0x018cfe80);
    let ab = sub(b, a);
    let ac = sub(d, a);
    let ao = sub(origin, a);
    let ab_a = plane(ao, ab);
    let ac_a = (ao[2] * ac[2] + ao[1] * ac[1]) + ao[0] * ac[0];
    if ab_a <= 0.0 && ac_a <= 0.0 {
        vertex_region(m, c, p, s, 0);
        return;
    }
    let bo = sub(origin, b);
    let ab_b = plane(bo, ab);
    let ac_b = plane(bo, ac);
    if ab_b >= 0.0 && ab_b >= ac_b {
        vertex_region(m, c, p, s, 1);
        return;
    }
    let area_ab = ac_b * ab_a - ab_b * ac_a;
    if area_ab <= 0.0 && ab_a >= 0.0 && ab_b <= 0.0 {
        let weight = inverse_or_zero(m, p, ab_a - ab_b) * ab_a;
        if weight < 0.0 {
            vertex_region(m, c, p, s, 0);
            return;
        }
        p.set_u32(m, s, 2);
        let first = vector(m, p, s.wrapping_add(16));
        closest(m, p, s, std::array::from_fn(|i| ab[i] * weight + first[i]));
        return;
    }
    let co = sub(origin, d);
    let ab_c = dot(co, ab);
    let ac_c = dot(co, ac);
    if ac_c >= 0.0 && ac_c >= ab_c {
        vertex_region(m, c, p, s, 2);
        return;
    }
    let area_ac = ab_c * ac_a - ac_c * ab_a;
    if area_ac <= 0.0 && ac_a >= 0.0 && ac_c <= 0.0 {
        let weight = inverse_or_zero(m, p, ac_a - ac_c) * ac_a;
        if weight < 0.0 {
            vertex_region(m, c, p, s, 0);
            return;
        }
        p.set_u32(m, s, 2);
        move_vertex(m, c, p, s, 1, 2);
        let first = vector(m, p, s.wrapping_add(16));
        closest(m, p, s, std::array::from_fn(|i| ac[i] * weight + first[i]));
        return;
    }
    let area_bc = ac_c * ab_b - ab_c * ac_b;
    let span = ac_b - ab_b;
    if !(area_bc <= 0.0 && span >= 0.0) {
        let inverse = float(m, p, 0x01406ce0) / ((area_bc + area_ac) + area_ab);
        let wb = inverse * area_ac;
        let wc = inverse * area_ab;
        let first = vector(m, p, s.wrapping_add(16));
        closest(
            m,
            p,
            s,
            [
                (ab[0] * wb + a[0]) + ac[0] * wc,
                (ab[1] * wb + first[1]) + ac[1] * wc,
                (ab[2] * wb + first[2]) + ac[2] * wc,
            ],
        );
    } else {
        // Preserve the original reciprocal-times-span expression, including
        // its zero/NaN behavior; do not simplify it to one.
        let weight = inverse_or_zero(m, p, span) * span;
        if weight < 0.0 {
            vertex_region(m, c, p, s, 1);
            return;
        }
        p.set_u32(m, s, 2);
        move_vertex(m, c, p, s, 0, 1);
        move_vertex(m, c, p, s, 1, 2);
        let second = vector(m, p, s.wrapping_add(32));
        let third = vector(m, p, s.wrapping_add(48));
        closest(
            m,
            p,
            s,
            std::array::from_fn(|i| (third[i] - second[i]) * weight + second[i]),
        );
    }
}

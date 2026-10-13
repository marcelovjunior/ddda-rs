//! Readable procedural geometry construction with original call boundaries.
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu},
};

/// Opt-in until original replay and head's in-game A/B are complete.
pub fn install(ports: &mut Vec<(u32, x86::LiftFn)>) {
    if std::env::var("DDRUNTIME_GEOMETRY_PORT").as_deref() == Ok("1")
        && !ports.iter().any(|&(address, _)| address == 0x00fe9580)
    {
        ports.push((0x00fe9580, build_geometry));
    }
}

fn push(m: &mut dyn Mem, cpu: &mut Cpu, value: u32) {
    cpu.esp = cpu.esp.wrapping_sub(4);
    m.set_u32(cpu.esp, value);
}

fn call(m: &mut dyn Mem, cpu: &mut Cpu, target: u32, return_address: u32) {
    push(m, cpu, return_address);
    x86::call_ext(m, cpu, target);
}

fn scalar(cpu: &mut Cpu, register: usize, value: u32) {
    cpu.xmm[register] = [value, 0, 0, 0];
}

fn vector(m: &dyn Mem, address: u32) -> [u32; 3] {
    std::array::from_fn(|axis| m.u32(address.wrapping_add(axis as u32 * 4)))
}

fn store_vector(m: &mut dyn Mem, address: u32, value: [u32; 3]) {
    for (axis, value) in value.into_iter().enumerate() {
        m.set_u32(address.wrapping_add(axis as u32 * 4), value);
    }
}

/// Translated from 0x00fe9580..0x00fe999d (DDDA.exe 2017-12-13 Steam).
/// The cube's corner selection is embedded in MOVSS instructions; unit
/// values themselves are read from the player's executable.
fn cube(m: &mut dyn Mem, cpu: &mut Cpu) {
    push(m, cpu, cpu.ebp);
    cpu.ebp = cpu.esp;
    cpu.esp = cpu.fl.alu_and(cpu.esp, 0xfffffff0, 32);
    cpu.esp = cpu.fl.alu_sub(cpu.esp, 0x254, 32);
    scalar(cpu, 1, m.u32(0x01406ce0));
    scalar(cpu, 0, m.u32(0x0161c20c));
    push(m, cpu, cpu.ebx);
    push(m, cpu, cpu.esi);
    push(m, cpu, cpu.edi);
    let local = cpu.esp;
    // Original triangle order; bit 0/1/2 selects positive x/y/z.
    const CORNERS: [u8; 36] = [
        4, 5, 6, 6, 5, 7, 2, 1, 0, 3, 1, 2, 1, 3, 5, 5, 3, 7, 4, 2, 0, 6, 2, 4, 6, 3, 2, 7, 3, 6, 0, 1, 4, 4,
        1, 5,
    ];
    for (vertex, corner) in CORNERS.into_iter().enumerate() {
        for axis in 0..3 {
            let value = cpu.xmm[usize::from(corner & (1 << axis) != 0)][0];
            m.set_u32(local + 0xa0 + vertex as u32 * 12 + axis * 4, value);
        }
    }
    cpu.ecx = m.u32(0x0187663c);
    cpu.eax = m.u32(cpu.ecx);
    cpu.edx = m.u32(cpu.eax + 0x20);
    push(m, cpu, 0x10);
    push(m, cpu, 0x1b0);
    cpu.ebx = 36;
    call(m, cpu, cpu.edx, 0x00fe9984);
    cpu.ecx = 108;
    cpu.esi = cpu.esp + 0xa0;
    cpu.edi = cpu.eax;
    x86::string(m, cpu, 4, true, true);
    cpu.ecx = m.u32(cpu.ebp + 8);
    m.set_u32(cpu.esp + 0x50, cpu.eax);
    cpu.fl.alu_and(cpu.ecx, cpu.ecx, 32);
}

/// Translated from 0x00fe99a3..0x00fe9c51 (DDDA.exe 2017-12-13 Steam).
/// Each triangle becomes four, preserving operand and triangle order.
fn subdivide(m: &mut dyn Mem, cpu: &mut Cpu) {
    if cpu.ecx == 0 {
        return;
    }
    m.set_u32(cpu.esp + 0x28, cpu.ecx);
    loop {
        cpu.ecx = m.u32(0x0187663c);
        cpu.esi = cpu.eax;
        cpu.eax = m.u32(m.u32(cpu.ecx) + 0x20);
        cpu.edx = cpu.ebx.wrapping_mul(3);
        push(m, cpu, 0x10);
        let size = cpu.edx.wrapping_shl(4);
        cpu.fl = x86::Flags::new(x86::SHL, cpu.edx, 4, size, 32);
        cpu.edx = size;
        push(m, cpu, cpu.edx);
        m.set_u32(cpu.esp + 0x4c, cpu.esi);
        cpu.edi = cpu.ebx;
        call(m, cpu, cpu.eax, 0x00fe99d2);
        cpu.ebx = cpu.fl.alu_xor(cpu.ebx, cpu.ebx, 32);
        m.set_u32(cpu.esp + 0x50, cpu.eax);
        cpu.fl.alu_and(cpu.edi, cpu.edi, 32);
        if cpu.edi != 0 {
            scalar(cpu, 0, m.u32(0x01406cf0));
            cpu.ecx = cpu.eax;
            cpu.edi = cpu.edi.wrapping_sub(1);
            let product = 0xaaaaaaabu64 * cpu.edi as u64;
            cpu.eax = product as u32;
            cpu.edx = (product >> 33) as u32;
            cpu.esi = cpu.fl.alu_add(cpu.esi, 20, 32);
            cpu.edx = cpu.edx.wrapping_add(1);
            loop {
                let first = vector(m, cpu.esi - 20);
                let second = vector(m, cpu.esi - 8);
                let third = vector(m, cpu.esi + 4);
                let half = cpu.xmm[0][0];
                let midpoint = |right: [u32; 3], left: [u32; 3]| {
                    std::array::from_fn::<_, 3, _>(|axis| {
                        x86::mulss(x86::addss(right[axis], left[axis]), half)
                    })
                };
                let first_second = midpoint(second, first);
                let first_third = midpoint(third, first);
                let second_third = midpoint(third, second);
                let local = cpu.esp;
                store_vector(m, local + 0x60, second);
                m.set_u32(local + 0x94, first[1]);
                m.set_u32(local + 0x98, first[2]);
                store_vector(m, local + 0x10, third);
                store_vector(m, local + 0x30, first_second);
                m.set_u32(local + 0x258, second_third[2]);
                for (index, vertex) in [
                    first,
                    first_second,
                    first_third,
                    first_second,
                    second,
                    second_third,
                    first_second,
                    second_third,
                    first_third,
                    second_third,
                    third,
                    first_third,
                ]
                .into_iter()
                .enumerate()
                {
                    store_vector(m, cpu.ecx + index as u32 * 12, vertex);
                }
                // Last values of the original scalar SSE temporaries. MOVSS
                // memory loads clear upper lanes before the arithmetic.
                for (register, value) in [
                    (1, first_third[0]),
                    (2, first_third[1]),
                    (3, first_third[2]),
                    (4, third[2]),
                    (5, second_third[1]),
                    (6, first_second[2]),
                    (7, second_third[2]),
                ] {
                    scalar(cpu, register, value);
                }
                cpu.eax = cpu.ecx + 0x84;
                cpu.ebx = cpu.ebx.wrapping_add(12);
                cpu.ecx = cpu.ecx.wrapping_add(0x90);
                cpu.esi = cpu.fl.alu_add(cpu.esi, 36, 32);
                let remaining = cpu.edx.wrapping_sub(1);
                cpu.fl.inc_dec(x86::DEC, cpu.edx, remaining, 32);
                cpu.edx = remaining;
                if remaining == 0 {
                    break;
                }
            }
            cpu.esi = m.u32(cpu.esp + 0x44);
        }
        cpu.ecx = m.u32(0x0187663c);
        cpu.edx = m.u32(cpu.ecx);
        cpu.eax = m.u32(cpu.edx + 0x24);
        push(m, cpu, cpu.esi);
        call(m, cpu, cpu.eax, 0x00fe9c47);
        let remaining = m.u32(cpu.esp + 0x28);
        cpu.fl.inc_dec(x86::DEC, remaining, remaining.wrapping_sub(1), 32);
        m.set_u32(cpu.esp + 0x28, remaining.wrapping_sub(1));
        if remaining == 1 {
            break;
        }
        cpu.eax = m.u32(cpu.esp + 0x50);
    }
}

/// Translated from 0x00fe9c51..0x00fe9dca (DDDA.exe 2017-12-13 Steam).
/// Project the two radial pairs independently, using the original first
/// pair's modified shared coordinate when applying the second scale.
fn project(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.ecx = m.u32(0x0187663c);
    let product = 0xaaaaaaabu64 * cpu.ebx as u64;
    cpu.esi = cpu.ebx.wrapping_mul(3);
    cpu.edi = (product >> 32) as u32;
    cpu.edx = m.u32(cpu.ecx);
    cpu.eax = m.u32(cpu.edx + 0x20);
    cpu.esi = cpu.esi.wrapping_mul(4);
    push(m, cpu, 16);
    let divided = cpu.edi >> 1;
    cpu.fl = x86::Flags::new(x86::SHR, cpu.edi, 1, divided, 32);
    cpu.edi = divided;
    push(m, cpu, cpu.esi);
    m.set_u32(cpu.esp + 0x84, cpu.edi);
    m.set_u32(cpu.esp + 0x5c, cpu.esi);
    call(m, cpu, cpu.eax, 0x00fe9c7e);
    cpu.xmm[4] = [0; 4];
    cpu.edx = cpu.eax;
    cpu.eax = 0;
    m.set_u32(cpu.esp + 0x58, cpu.edx);
    m.set_u32(cpu.esp + 0x28, 0);
    cpu.fl.alu_and(cpu.ebx, cpu.ebx, 32);
    if cpu.ebx != 0 {
        cpu.esi = m.u32(cpu.esp + 0x50);
        scalar(cpu, 3, m.u32(0x0161c874));
        m.set_u32(cpu.esp + 0x4c, cpu.edx);
        cpu.edx = cpu.edx.wrapping_sub(cpu.esi);
        cpu.ecx = cpu.esi + 8;
        m.set_u32(cpu.esp + 0x48, cpu.edx);
        loop {
            let local = cpu.esp;
            let major = cpu.eax / cpu.edi;
            let adjacent = (major + 1) % 3;
            let shared = (major + 2) % 3;
            cpu.edi = 3;
            let original = vector(m, cpu.ecx - 8);
            store_vector(m, local + 0x30, original);
            m.set_u32(local + 0x3c, cpu.xmm[4][0]);
            cpu.esi = local + major * 4 + 0x30;
            scalar(cpu, 1, m.u32(cpu.esi));
            cpu.xmm[1][0] = x86::mulss(cpu.xmm[1][0], cpu.xmm[1][0]);
            m.set_u32(local + 0x44, adjacent);
            scalar(cpu, 0, m.u32(local + shared * 4 + 0x30));
            cpu.xmm[0][0] = x86::mulss(cpu.xmm[0][0], cpu.xmm[0][0]);
            cpu.xmm[1][0] = x86::addss(cpu.xmm[1][0], cpu.xmm[0][0]);
            cpu.xmm[2][0] = x86::sqrtss(cpu.xmm[1][0]);
            cpu.fl = x86::comiss_hw::<true>(cpu.xmm[2][0], cpu.xmm[3][0]);
            cpu.eax = local + shared * 4 + 0x30;
            cpu.edx = local + adjacent * 4 + 0x30;
            scalar(cpu, 1, m.u32(cpu.edx));
            cpu.xmm[1][0] = x86::mulss(cpu.xmm[1][0], cpu.xmm[1][0]);
            cpu.xmm[1][0] = x86::addss(cpu.xmm[1][0], cpu.xmm[0][0]);
            cpu.xmm[1][0] = x86::sqrtss(cpu.xmm[1][0]);
            if !cpu.fl.cond("be") {
                cpu.xmm[0] = cpu.xmm[3];
                cpu.xmm[0][0] = x86::divss(cpu.xmm[0][0], cpu.xmm[2][0]);
                cpu.xmm[2] = cpu.xmm[0];
                cpu.xmm[2][0] = x86::mulss(cpu.xmm[2][0], m.u32(cpu.esi));
                m.set_u32(cpu.esi, cpu.xmm[2][0]);
                cpu.xmm[0][0] = x86::mulss(cpu.xmm[0][0], m.u32(cpu.eax));
                m.set_u32(cpu.eax, cpu.xmm[0][0]);
            }
            cpu.fl = x86::comiss_hw::<true>(cpu.xmm[1][0], cpu.xmm[3][0]);
            if !cpu.fl.cond("be") {
                cpu.xmm[0] = cpu.xmm[3];
                cpu.xmm[0][0] = x86::divss(cpu.xmm[0][0], cpu.xmm[1][0]);
                cpu.xmm[1] = cpu.xmm[0];
                cpu.xmm[1][0] = x86::mulss(cpu.xmm[1][0], m.u32(cpu.edx));
                m.set_u32(cpu.edx, cpu.xmm[1][0]);
                cpu.xmm[0][0] = x86::mulss(cpu.xmm[0][0], m.u32(cpu.eax));
                m.set_u32(cpu.eax, cpu.xmm[0][0]);
            }
            cpu.edx = m.u32(local + 0x4c);
            cpu.eax = m.u32(local + 0x48);
            let projected = vector(m, local + 0x30);
            m.set_u32(cpu.edx, projected[0]);
            m.set_u32(cpu.edx + 4, projected[1]);
            m.set_u32(cpu.eax + cpu.ecx, projected[2]);
            scalar(cpu, 0, projected[2]);
            cpu.eax = m.u32(local + 0x28).wrapping_add(1);
            cpu.edx = cpu.edx.wrapping_add(12);
            cpu.ecx = cpu.ecx.wrapping_add(12);
            m.set_u32(local + 0x28, cpu.eax);
            m.set_u32(local + 0x4c, cpu.edx);
            cpu.fl.alu_sub(cpu.eax, cpu.ebx, 32);
            if cpu.eax >= cpu.ebx {
                break;
            }
            cpu.edi = m.u32(local + 0x7c);
        }
        cpu.esi = m.u32(cpu.esp + 0x54);
    }
    cpu.ecx = m.u32(0x0187663c);
    cpu.edx = m.u32(cpu.ecx);
    cpu.eax = m.u32(cpu.edx + 0x20);
    push(m, cpu, 16);
    push(m, cpu, cpu.esi);
    call(m, cpu, cpu.eax, 0x00fe9dca);
}

/// UCOMISS/LAHF/TEST AH,44h (0x00fe9f36 and the eight sibling sites).
/// EAX's AH remains observable; loop-tail integer arithmetic replaces flags.
fn ordered_equal(cpu: &mut Cpu, left: usize, right: usize) -> bool {
    let compare = x86::comiss_hw::<false>(cpu.xmm[left][0], cpu.xmm[right][0]);
    let ah = match compare.r {
        0 => 0x42,
        1 => 2,
        2 => 3,
        _ => 0x47,
    };
    cpu.eax = (cpu.eax & !0xff00) | (ah << 8);
    compare.r == 0
}

fn position_key(value: [u32; 3]) -> Option<[u32; 3]> {
    let mut key = value;
    for component in &mut key {
        let magnitude = *component & 0x7fffffff;
        // Normal finite operands and signed zero cannot raise comparison
        // exceptions. Keep the literal scan for NaNs, infinities and
        // subnormals (including DAZ-sensitive equality).
        if magnitude >= 0x7f800000 || (magnitude != 0 && magnitude < 0x00800000) {
            return None;
        }
        if magnitude == 0 {
            *component = 0;
        }
    }
    Some(key)
}

fn position_index(
    m: &dyn Mem,
    base: u32,
    count: u32,
) -> Option<std::collections::BTreeMap<[u32; 3], Vec<u32>>> {
    let mut index = std::collections::BTreeMap::<_, Vec<_>>::new();
    for vertex in 0..count {
        index
            .entry(position_key(vector(m, base + vertex * 12))?)
            .or_default()
            .push(vertex);
    }
    Some(index)
}

/// Translated from 0x00fe9dca..0x00fea001 (DDDA.exe 2017-12-13 Steam).
/// Accumulate each normalized face normal into every coincident vertex.
fn normals(m: &mut dyn Mem, cpu: &mut Cpu) {
    push(m, cpu, cpu.esi);
    cpu.edi = cpu.eax;
    push(m, cpu, 0);
    push(m, cpu, cpu.edi);
    m.set_u32(cpu.esp + 0x50, cpu.edi);
    call(m, cpu, 0x012efe20, 0x00fe9dd9);
    cpu.esp = cpu.esp.wrapping_add(12);
    cpu.fl.alu_and(cpu.ebx, cpu.ebx, 32);
    if cpu.ebx != 0 {
        cpu.esi = m.u32(cpu.esp + 0x58);
        cpu.eax = cpu.esi.wrapping_sub(cpu.edi);
        m.set_u32(cpu.esp + 0x5c, cpu.eax);
        cpu.edx = ((0xaaaaaaabu64 * cpu.ebx.wrapping_sub(1) as u64) >> 32) as u32;
        cpu.ecx = cpu.edi + 8;
        cpu.edi = (cpu.edx >> 1) + 1;
        m.set_u32(cpu.esp + 0x48, cpu.ecx);
        cpu.ecx = cpu.esi + 20;
        m.set_u32(cpu.esp + 0x2c, cpu.ecx);
        let positions = position_index(m, cpu.esi, cpu.ebx);
        let mut candidates = Vec::new();
        loop {
            let local = cpu.esp;
            let first = vector(m, cpu.ecx - 20);
            let second = vector(m, cpu.ecx - 8);
            let third = vector(m, cpu.ecx + 4);
            store_vector(m, local + 0x30, third);
            store_vector(m, local + 0x60, first);
            store_vector(m, local + 0x10, second);
            let first_second_x = x86::subss(first[0], second[0]);
            let first_third_z = x86::subss(first[2], third[2]);
            let first_second_y = x86::subss(first[1], second[1]);
            let first_third_y = x86::subss(first[1], third[1]);
            let first_second_z = x86::subss(first[2], second[2]);
            let cross_x_left = x86::mulss(first_third_z, first_second_y);
            let cross_y_right = x86::mulss(first_third_z, first_second_x);
            let cross_x_right = x86::mulss(first_third_y, first_second_z);
            let first_third_x = x86::subss(first[0], third[0]);
            let cross_z_right = x86::mulss(first_second_y, first_third_x);
            let normal_x = x86::subss(cross_x_left, cross_x_right);
            let normal_y = x86::subss(x86::mulss(first_second_z, first_third_x), cross_y_right);
            let normal_z = x86::subss(x86::mulss(first_third_y, first_second_x), cross_z_right);
            let xy = x86::addss(x86::mulss(normal_x, normal_x), x86::mulss(normal_y, normal_y));
            let squared = x86::addss(xy, x86::mulss(normal_z, normal_z));
            let length = x86::sqrtss(squared);
            scalar(cpu, 4, normal_x);
            scalar(cpu, 0, normal_y);
            scalar(cpu, 1, normal_z);
            cpu.fl = x86::comiss_hw::<true>(m.u32(0x01407124), length);
            if !cpu.fl.cond("a") {
                let scale = x86::divss(m.u32(0x01406ce0), length);
                cpu.xmm[4][0] = x86::mulss(scale, normal_x);
                cpu.xmm[0][0] = x86::mulss(normal_y, scale);
                cpu.xmm[1][0] = x86::mulss(normal_z, scale);
            }
            cpu.ecx = m.u32(local + 0x48);
            scalar(cpu, 6, first[0]);
            cpu.edx = cpu.ebx;
            let position_base = cpu.esi;
            let normal_base = cpu.ecx;
            if let Some(index) = &positions {
                candidates.clear();
                for corner in [first, second, third] {
                    candidates.extend_from_slice(&index[&position_key(corner).unwrap()]);
                }
                // The original visits every candidate in order. Only
                // coincident vertices write memory; face accumulation order
                // stays unchanged. Execute the final candidate even if it
                // does not match, preserving outgoing AH/XMM/loop flags.
                candidates.push(cpu.ebx - 1);
                candidates.sort_unstable();
                candidates.dedup();
            }
            let mut candidate = 0;
            loop {
                if positions.is_some() {
                    let vertex = candidates[candidate];
                    candidate += 1;
                    cpu.esi = position_base + vertex * 12;
                    cpu.ecx = normal_base + vertex * 12;
                    cpu.edx = cpu.ebx - vertex;
                }
                cpu.eax = m.u32(local + 0x5c);
                scalar(cpu, 2, m.u32(cpu.esi));
                scalar(cpu, 3, m.u32(cpu.eax.wrapping_add(cpu.ecx).wrapping_sub(4)));
                scalar(cpu, 5, m.u32(cpu.eax.wrapping_add(cpu.ecx)));
                let mut matched = false;
                if ordered_equal(cpu, 6, 2) {
                    scalar(cpu, 7, first[1]);
                    if ordered_equal(cpu, 7, 3) {
                        scalar(cpu, 7, first[2]);
                        matched = ordered_equal(cpu, 7, 5);
                    }
                }
                if !matched {
                    scalar(cpu, 7, second[0]);
                    if ordered_equal(cpu, 7, 2) {
                        scalar(cpu, 7, second[1]);
                        if ordered_equal(cpu, 7, 3) {
                            scalar(cpu, 7, second[2]);
                            matched = ordered_equal(cpu, 7, 5);
                        }
                    }
                }
                if !matched {
                    scalar(cpu, 7, third[0]);
                    if ordered_equal(cpu, 7, 2) {
                        scalar(cpu, 2, third[1]);
                        if ordered_equal(cpu, 2, 3) {
                            scalar(cpu, 2, third[2]);
                            matched = ordered_equal(cpu, 2, 5);
                        }
                    }
                }
                if matched {
                    scalar(cpu, 2, m.u32(cpu.ecx - 8));
                    scalar(cpu, 3, m.u32(cpu.ecx - 4));
                    scalar(cpu, 5, m.u32(cpu.ecx));
                    cpu.xmm[2][0] = x86::addss(cpu.xmm[2][0], cpu.xmm[4][0]);
                    cpu.xmm[3][0] = x86::addss(cpu.xmm[3][0], cpu.xmm[0][0]);
                    cpu.xmm[5][0] = x86::addss(cpu.xmm[5][0], cpu.xmm[1][0]);
                    store_vector(m, cpu.ecx - 8, [cpu.xmm[2][0], cpu.xmm[3][0], cpu.xmm[5][0]]);
                }
                cpu.esi = cpu.esi.wrapping_add(12);
                cpu.ecx = cpu.fl.alu_add(cpu.ecx, 12, 32);
                let remaining = cpu.edx.wrapping_sub(1);
                cpu.fl.inc_dec(x86::DEC, cpu.edx, remaining, 32);
                cpu.edx = remaining;
                if remaining == 0 {
                    break;
                }
            }
            let next = cpu.fl.alu_add(m.u32(local + 0x2c), 36, 32);
            m.set_u32(local + 0x2c, next);
            let remaining = cpu.edi.wrapping_sub(1);
            cpu.fl.inc_dec(x86::DEC, cpu.edi, remaining, 32);
            cpu.edi = remaining;
            if remaining == 0 {
                break;
            }
            cpu.esi = m.u32(local + 0x58);
            cpu.ecx = m.u32(local + 0x2c);
        }
    }
    push(m, cpu, 16);
    push(m, cpu, 0x28);
    call(m, cpu, 0x01128ea0, 0x00fea001);
}

fn semantic(m: &mut dyn Mem, cpu: &mut Cpu, name: u32, back: u32, use_edx: bool) -> bool {
    let value = m.u32(cpu.edi);
    if use_edx {
        cpu.edx = value;
    } else {
        cpu.eax = value;
    }
    push(m, cpu, name);
    push(m, cpu, value);
    call(m, cpu, 0x012ef5d7, back);
    cpu.esp += 8;
    cpu.fl.alu_and(cpu.eax, cpu.eax, 32);
    cpu.eax == 0
}

// Translated from 0x00fea0dc..0x00fea13c.
fn position(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.ecx = m.u32(cpu.esp + 0x54);
    scalar(cpu, 4, m.u32(cpu.ecx));
    cpu.xmm[0] = x86::sqrtsd(cpu.xmm[0], [m.u32(0x0161ca50), m.u32(0x0161ca54), 0, 0]);
    cpu.eax = m.u32(cpu.esp + 0x2c);
    cpu.ecx = m.u32(cpu.esp + 0x5c);
    scalar(cpu, 1, m.u32(cpu.eax.wrapping_add(cpu.ecx).wrapping_sub(4)));
    scalar(cpu, 2, m.u32(cpu.eax.wrapping_add(cpu.ecx)));
    cpu.xmm[3] = x86::cvtsd2ss([0; 4], cpu.xmm[0]);
    scalar(cpu, 0, m.u32(0x01406ce0));
    cpu.xmm[0][0] = x86::divss(cpu.xmm[0][0], cpu.xmm[3][0]);
    cpu.xmm[3] = cpu.xmm[0];
    cpu.xmm[1][0] = x86::mulss(cpu.xmm[1][0], cpu.xmm[0][0]);
    cpu.xmm[2][0] = x86::mulss(cpu.xmm[2][0], cpu.xmm[0][0]);
    cpu.xmm[0] = [0; 4];
    cpu.xmm[3][0] = x86::mulss(cpu.xmm[3][0], cpu.xmm[4][0]);
    store_attribute(m, cpu, [3, 1, 2, 0]);
}

fn store_attribute(m: &mut dyn Mem, cpu: &Cpu, registers: [usize; 4]) {
    for (index, register) in registers.into_iter().enumerate() {
        m.set_u32(cpu.esp + 0x10 + index as u32 * 4, cpu.xmm[register][0]);
    }
}

// Translated from 0x00fea159..0x00fea1dc.
fn normal_attribute(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = m.u32(cpu.esp + 0x2c);
    scalar(cpu, 4, m.u32(cpu.eax - 8));
    scalar(cpu, 1, m.u32(cpu.eax - 4));
    scalar(cpu, 2, m.u32(cpu.eax));
    cpu.xmm[0] = cpu.xmm[4];
    cpu.xmm[0][0] = x86::mulss(cpu.xmm[0][0], cpu.xmm[4][0]);
    cpu.xmm[3] = cpu.xmm[1];
    cpu.xmm[3][0] = x86::mulss(cpu.xmm[3][0], cpu.xmm[1][0]);
    cpu.xmm[0][0] = x86::addss(cpu.xmm[0][0], cpu.xmm[3][0]);
    cpu.xmm[3] = cpu.xmm[2];
    cpu.xmm[3][0] = x86::mulss(cpu.xmm[3][0], cpu.xmm[2][0]);
    cpu.xmm[0][0] = x86::addss(cpu.xmm[0][0], cpu.xmm[3][0]);
    cpu.xmm[3][0] = x86::sqrtss(cpu.xmm[0][0]);
    scalar(cpu, 0, m.u32(0x01407124));
    cpu.fl = x86::comiss_hw::<true>(cpu.xmm[0][0], cpu.xmm[3][0]);
    if cpu.fl.cond("be") {
        scalar(cpu, 0, m.u32(0x01406ce0));
        cpu.xmm[0][0] = x86::divss(cpu.xmm[0][0], cpu.xmm[3][0]);
        cpu.xmm[3] = cpu.xmm[0];
        cpu.xmm[1][0] = x86::mulss(cpu.xmm[1][0], cpu.xmm[0][0]);
        cpu.xmm[2][0] = x86::mulss(cpu.xmm[2][0], cpu.xmm[0][0]);
        cpu.xmm[0][0] = x86::mulss(cpu.xmm[0][0], m.u32(0x01561738));
        cpu.xmm[3][0] = x86::mulss(cpu.xmm[3][0], cpu.xmm[4][0]);
    } else {
        cpu.xmm[0] = [0; 4];
        cpu.xmm[3] = cpu.xmm[4];
    }
    store_attribute(m, cpu, [3, 1, 2, 0]);
}

// Translated from 0x00fea1f9..0x00fea362.
fn tangent(m: &mut dyn Mem, cpu: &mut Cpu) {
    let per_face = cpu.ebx / 6;
    cpu.eax = cpu.esi / per_face;
    cpu.edx = cpu.esi % per_face;
    cpu.ecx = m.u32(cpu.esp + 0x2c);
    scalar(cpu, 4, m.u32(cpu.ecx - 8));
    scalar(cpu, 3, m.u32(cpu.ecx - 4));
    scalar(cpu, 1, m.u32(cpu.ecx));
    cpu.xmm[0] = cpu.xmm[4];
    cpu.xmm[0][0] = x86::mulss(cpu.xmm[0][0], cpu.xmm[4][0]);
    cpu.xmm[2] = cpu.xmm[3];
    cpu.xmm[2][0] = x86::mulss(cpu.xmm[2][0], cpu.xmm[3][0]);
    cpu.xmm[0][0] = x86::addss(cpu.xmm[0][0], cpu.xmm[2][0]);
    cpu.xmm[2] = cpu.xmm[1];
    cpu.xmm[2][0] = x86::mulss(cpu.xmm[2][0], cpu.xmm[1][0]);
    cpu.xmm[0][0] = x86::addss(cpu.xmm[0][0], cpu.xmm[2][0]);
    cpu.xmm[2][0] = x86::sqrtss(cpu.xmm[0][0]);
    scalar(cpu, 0, m.u32(0x01407124));
    cpu.fl = x86::comiss_hw::<true>(cpu.xmm[0][0], cpu.xmm[2][0]);
    if cpu.fl.cond("be") {
        scalar(cpu, 0, m.u32(0x01406ce0));
        cpu.xmm[0][0] = x86::divss(cpu.xmm[0][0], cpu.xmm[2][0]);
        cpu.xmm[2] = cpu.xmm[0];
        cpu.xmm[2][0] = x86::mulss(cpu.xmm[2][0], cpu.xmm[4][0]);
        cpu.xmm[3][0] = x86::mulss(cpu.xmm[3][0], cpu.xmm[0][0]);
        cpu.xmm[1][0] = x86::mulss(cpu.xmm[1][0], cpu.xmm[0][0]);
        cpu.xmm[6] = cpu.xmm[2];
    } else {
        cpu.xmm[6] = cpu.xmm[4];
    }
    // The executable's switch chooses its original axis vectors.
    let axis = match cpu.eax {
        0 | 1 => 0x01629540,
        2 | 3 => 0x01629550,
        _ => 0x01629530,
    };
    scalar(cpu, 4, m.u32(axis));
    scalar(cpu, 0, m.u32(axis + 4));
    scalar(cpu, 2, m.u32(axis + 8));
    cpu.xmm[5] = cpu.xmm[1];
    cpu.fl.alu_sub(cpu.eax, 5, 32);
    cpu.xmm[1] = cpu.xmm[0];
    cpu.xmm[0][0] = x86::mulss(cpu.xmm[0][0], cpu.xmm[6][0]);
    cpu.xmm[7] = cpu.xmm[2];
    cpu.xmm[7][0] = x86::mulss(cpu.xmm[7][0], cpu.xmm[3][0]);
    cpu.xmm[3][0] = x86::mulss(cpu.xmm[3][0], cpu.xmm[4][0]);
    cpu.xmm[3][0] = x86::subss(cpu.xmm[3][0], cpu.xmm[0][0]);
    cpu.xmm[1][0] = x86::mulss(cpu.xmm[1][0], cpu.xmm[5][0]);
    cpu.xmm[5][0] = x86::mulss(cpu.xmm[5][0], cpu.xmm[4][0]);
    cpu.xmm[1][0] = x86::subss(cpu.xmm[1][0], cpu.xmm[7][0]);
    cpu.xmm[2][0] = x86::mulss(cpu.xmm[2][0], cpu.xmm[6][0]);
    cpu.xmm[2][0] = x86::subss(cpu.xmm[2][0], cpu.xmm[5][0]);
    cpu.xmm[0] = cpu.xmm[1];
    cpu.xmm[0][0] = x86::mulss(cpu.xmm[0][0], cpu.xmm[1][0]);
    cpu.xmm[4] = cpu.xmm[2];
    cpu.xmm[4][0] = x86::mulss(cpu.xmm[4][0], cpu.xmm[2][0]);
    cpu.xmm[0][0] = x86::addss(cpu.xmm[0][0], cpu.xmm[4][0]);
    cpu.xmm[4] = cpu.xmm[3];
    cpu.xmm[4][0] = x86::mulss(cpu.xmm[4][0], cpu.xmm[3][0]);
    cpu.xmm[0][0] = x86::addss(cpu.xmm[0][0], cpu.xmm[4][0]);
    cpu.xmm[4][0] = x86::sqrtss(cpu.xmm[0][0]);
    scalar(cpu, 0, m.u32(0x01407124));
    cpu.fl = x86::comiss_hw::<true>(cpu.xmm[0][0], cpu.xmm[4][0]);
    if cpu.fl.cond("be") {
        scalar(cpu, 0, m.u32(0x01406ce0));
        cpu.xmm[0][0] = x86::divss(cpu.xmm[0][0], cpu.xmm[4][0]);
        cpu.xmm[4] = cpu.xmm[0];
        cpu.xmm[4][0] = x86::mulss(cpu.xmm[4][0], cpu.xmm[1][0]);
        cpu.xmm[2][0] = x86::mulss(cpu.xmm[2][0], cpu.xmm[0][0]);
        cpu.xmm[3][0] = x86::mulss(cpu.xmm[3][0], cpu.xmm[0][0]);
        cpu.xmm[0][0] = x86::mulss(cpu.xmm[0][0], m.u32(0x01561738));
        cpu.xmm[1] = cpu.xmm[4];
    } else {
        cpu.xmm[0] = [0; 4];
    }
    store_attribute(m, cpu, [1, 2, 3, 0]);
}

// Translated from 0x00fea3ce..0x00fea45e.
fn texture_coordinates(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = m.u32(cpu.esp + 0x4c);
    cpu.ecx = m.u32(cpu.esp + 0x8c);
    cpu.edx = cpu.eax + cpu.ecx;
    cpu.eax += m.u32(cpu.esp + 0x80);
    cpu.ecx = m.u32(cpu.esp + 0x50);
    scalar(cpu, 3, m.u32(cpu.ecx + cpu.eax * 4));
    scalar(cpu, 0, m.u32(cpu.ecx + cpu.edx * 4));
    cpu.ecx = cpu.ebx / 6;
    cpu.eax = cpu.esi / cpu.ecx;
    cpu.edx = cpu.esi % cpu.ecx;
    scalar(cpu, 2, m.u32(0x01406ce0));
    cpu.xmm[3][0] = x86::addss(cpu.xmm[3][0], cpu.xmm[2][0]);
    cpu.xmm[3][0] = x86::mulss(cpu.xmm[3][0], m.u32(0x01406cf0));
    cpu.xmm[1] = cpu.xmm[2];
    cpu.xmm[1][0] = x86::subss(cpu.xmm[1][0], cpu.xmm[3][0]);
    cpu.xmm[0][0] = x86::addss(cpu.xmm[0][0], cpu.xmm[2][0]);
    cpu.xmm[0][0] = x86::mulss(cpu.xmm[0][0], m.u32(0x01406cf0));
    m.set_u32(cpu.esp + 0x14, cpu.xmm[1][0]);
    cpu.xmm[1] = [0; 4];
    m.set_u32(cpu.esp + 0x10, cpu.xmm[0][0]);
    m.set_u32(cpu.esp + 0x18, 0);
    m.set_u32(cpu.esp + 0x1c, 0);
    for face in [1, 3, 5] {
        cpu.fl.alu_sub(cpu.eax, face, 32);
        if cpu.eax == face {
            cpu.xmm[2][0] = x86::subss(cpu.xmm[2][0], cpu.xmm[0][0]);
            m.set_u32(cpu.esp + 0x10, cpu.xmm[2][0]);
            break;
        }
    }
}

// Translated from 0x00fea001..0x00fea4eb.
fn pack_vertices(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.edi = m.u32(cpu.ebp + 12);
    cpu.esp += 8;
    cpu.fl.alu_and(cpu.eax, cpu.eax, 32);
    if cpu.eax != 0 {
        let stride = m.u32(cpu.edi + 0x20);
        cpu.edx = stride.wrapping_mul(cpu.ebx);
        cpu.fl = x86::Flags::new(x86::IMUL, stride, cpu.ebx, cpu.edx, 32);
        push(m, cpu, 0);
        push(m, cpu, 0);
        push(m, cpu, cpu.edx);
        cpu.ecx = cpu.eax;
        call(m, cpu, 0x01129130, 0x00fea01d);
        m.set_u32(cpu.esp + 0x28, cpu.eax);
    } else {
        m.set_u32(cpu.esp + 0x28, 0);
    }
    cpu.ecx = m.u32(cpu.esp + 0x28);
    push(m, cpu, 3);
    call(m, cpu, 0x01128ee0, 0x00fea036);
    cpu.esi = 0;
    m.set_u32(cpu.esp + 0x88, cpu.eax);
    cpu.fl.alu_and(cpu.ebx, cpu.ebx, 32);
    if cpu.ebx != 0 {
        cpu.eax = m.u32(cpu.esp + 0x58);
        cpu.ecx = m.u32(cpu.esp + 0x44);
        m.set_u32(cpu.esp + 0x54, cpu.eax);
        cpu.edx = cpu.ecx + 8;
        cpu.eax = cpu.eax.wrapping_sub(cpu.ecx);
        m.set_u32(cpu.esp + 0x4c, 0);
        m.set_u32(cpu.esp + 0x2c, cpu.edx);
        m.set_u32(cpu.esp + 0x5c, cpu.eax);
        loop {
            let major = cpu.esi / m.u32(cpu.esp + 0x7c);
            m.set_u32(cpu.esp + 0x8c, major);
            cpu.edx = (major + 1) % 3;
            cpu.ecx = m.u32(cpu.edi + 0x1c);
            cpu.eax = m.u32(cpu.edi + 0x20);
            m.set_u32(cpu.esp + 0x80, cpu.edx);
            cpu.edx = m.u32(cpu.edi + 0x18);
            cpu.fl.alu_and(cpu.ecx, cpu.ecx, 32);
            if cpu.ecx != 0 {
                cpu.eax = cpu.eax.wrapping_mul(cpu.esi);
                cpu.eax = cpu.fl.alu_add(cpu.eax, m.u32(cpu.esp + 0x88), 32);
                cpu.edi = cpu.edx;
                m.set_u32(cpu.esp + 0x84, cpu.eax);
                m.set_u32(cpu.esp + 0x48, cpu.ecx);
                loop {
                    if semantic(m, cpu, 0x01594ed0, 0x00fea0bd, true) {
                        default_attribute(m, cpu);
                    } else if semantic(m, cpu, 0x0157c588, 0x00fea0d5, false) {
                        position(m, cpu);
                    } else if semantic(m, cpu, 0x0159b990, 0x00fea14e, true) {
                        normal_attribute(m, cpu);
                    } else if semantic(m, cpu, 0x0159b9a0, 0x00fea1ee, false) {
                        tangent(m, cpu);
                    } else {
                        let mut texture = semantic(m, cpu, 0x0143a5d0, 0x00fea374, true);
                        if !texture {
                            cpu.eax = m.u32(cpu.edi);
                            push(m, cpu, 3);
                            push(m, cpu, 0x0143a5cc);
                            push(m, cpu, cpu.eax);
                            call(m, cpu, 0x0130875a, 0x00fea38a);
                            cpu.esp += 12;
                            cpu.fl.alu_and(cpu.eax, cpu.eax, 32);
                            texture = cpu.eax == 0;
                        }
                        if texture {
                            texture_coordinates(m, cpu);
                        } else {
                            default_attribute(m, cpu);
                        }
                    }
                    cpu.eax = m.u32(cpu.esp + 0x84);
                    cpu.edx = cpu.esp + 0x10;
                    push(m, cpu, cpu.edx);
                    push(m, cpu, cpu.edi);
                    push(m, cpu, cpu.eax);
                    call(m, cpu, 0x01129a60, 0x00fea471);
                    cpu.esp += 12;
                    cpu.edi = cpu.fl.alu_add(cpu.edi, 8, 32);
                    let remaining = m.u32(cpu.esp + 0x48);
                    cpu.fl.inc_dec(x86::DEC, remaining, remaining.wrapping_sub(1), 32);
                    m.set_u32(cpu.esp + 0x48, remaining.wrapping_sub(1));
                    if remaining == 1 {
                        break;
                    }
                }
                cpu.edi = m.u32(cpu.ebp + 12);
            }
            m.set_u32(cpu.esp + 0x4c, m.u32(cpu.esp + 0x4c).wrapping_add(3));
            cpu.eax = 12;
            m.set_u32(cpu.esp + 0x54, m.u32(cpu.esp + 0x54).wrapping_add(12));
            m.set_u32(cpu.esp + 0x2c, m.u32(cpu.esp + 0x2c).wrapping_add(12));
            cpu.esi = cpu.esi.wrapping_add(1);
            cpu.fl.alu_sub(cpu.esi, cpu.ebx, 32);
            if cpu.esi >= cpu.ebx {
                break;
            }
        }
    }
    cpu.esi = m.u32(cpu.esp + 0x28);
    cpu.ecx = cpu.esi;
    call(m, cpu, 0x01128f40, 0x00fea4aa);
    cpu.ecx = m.u32(0x0187663c);
    cpu.edx = m.u32(cpu.ecx);
    cpu.eax = m.u32(cpu.esp + 0x50);
    cpu.edx = m.u32(cpu.edx + 0x24);
    push(m, cpu, cpu.eax);
    call(m, cpu, cpu.edx, 0x00fea4bc);
    cpu.ecx = m.u32(0x0187663c);
    cpu.eax = m.u32(cpu.ecx);
    cpu.edx = m.u32(cpu.esp + 0x58);
    cpu.eax = m.u32(cpu.eax + 0x24);
    push(m, cpu, cpu.edx);
    call(m, cpu, cpu.eax, 0x00fea4ce);
    cpu.ecx = m.u32(0x0187663c);
    cpu.edx = m.u32(cpu.ecx);
    cpu.eax = m.u32(cpu.esp + 0x44);
    cpu.edx = m.u32(cpu.edx + 0x24);
    push(m, cpu, cpu.eax);
    call(m, cpu, cpu.edx, 0x00fea4e0);
    cpu.edi = m.u32(cpu.esp);
    cpu.esp += 4;
    cpu.eax = cpu.esi;
    cpu.esi = m.u32(cpu.esp);
    cpu.esp += 4;
    cpu.ebx = m.u32(cpu.esp);
    cpu.esp = cpu.ebp;
    cpu.ebp = m.u32(cpu.esp);
    cpu.esp += 16;
}

// Translated from 0x00fea391..0x00fea3ce: Color and unknown semantics.
fn default_attribute(m: &mut dyn Mem, cpu: &mut Cpu) {
    for index in 0..4 {
        scalar(cpu, 0, m.u32(0x01629470 + index * 4));
        m.set_u32(cpu.esp + 0x10 + index * 4, cpu.xmm[0][0]);
    }
}

/// Translated from 0x00fe9580 (DDDA.exe 2017-12-13 Steam).
/// Runtime registration is opt-in pending head's game comparison.
pub fn build_geometry(m: &mut dyn Mem, cpu: &mut Cpu) {
    cube(m, cpu);
    subdivide(m, cpu);
    project(m, cpu);
    normals(m, cpu);
    pack_vertices(m, cpu);
}

//! ABI-level material and package lookup ports; private original vectors in tests.
//!
//! Material context: +0x20 is the immutable state bank, +0x18 the constant
//! descriptor bank, +0x24/+0x28 the buffer cursor/end, and +0x154 dirty bits.
//! With +0x5c bit 7 set, +0x6c indexes twelve-byte saved-state records at
//! +0x30c7c: tag, previous state pointer, previous key. No bank data is embedded.
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu},
};

// Translated from 0x01145BF0 (DDDA.exe 2017-12-13 Steam).
/// MaterialStd's primitive hook first resolves Material's binding, then
/// applies the standard material only when the binding is non-null.
pub fn apply_standard_material(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    let entry = c.esp;
    let material = c.ecx;
    let context = p.u32(m, entry.wrapping_add(4));
    let primitive = p.u32(m, entry.wrapping_add(8));
    let binding = draw_call(
        m,
        c,
        p,
        entry.wrapping_sub(8),
        0x01066c60,
        0x01145c03,
        material,
        &[context, primitive],
    );
    if binding != 0 {
        draw_call(
            m,
            c,
            p,
            entry.wrapping_sub(8),
            0x01144fe0,
            0x01145c10,
            material,
            &[context, binding],
        );
    }
    c.esp = entry.wrapping_add(12);
}

// Translated from 0x01008200 (DDDA.exe 2017-12-13 Steam).
/// Single shader key lookup: +0x84 selects a package table, whose bucket
/// array starts at +0x1c and records at +0x401c (36 bytes per record).
pub fn single_key(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    let key = p.u32(m, c.esp.wrapping_add(4));
    c.eax = lookup(m, p, c.ecx, key);
    c.esp = c.esp.wrapping_add(8);
}

fn lookup(m: &dyn Mem, p: Pages, owner: u32, key: u32) -> u32 {
    if key == 0 {
        return 0;
    }
    let table = p.u32(m, owner.wrapping_add(0x84));
    let mut node = p.u32(m, table.wrapping_add(0x1c + (key & 0xfff) * 4));
    while node != 0 {
        if p.u32(m, node) == key {
            let index = p.u32(m, node.wrapping_add(12));
            return if index & 0x8000_0000 != 0 {
                0
            } else {
                table.wrapping_add(index.wrapping_mul(36)).wrapping_add(0x401c)
            };
        }
        let next = p.u32(m, node.wrapping_add(8));
        if next == 0 {
            return 0;
        }
        node = p
            .u32(m, table.wrapping_add(0x18))
            .wrapping_add(next.wrapping_mul(16));
    }
    0
}

// Translated from 0x00E0F3C0 (DDDA.exe 2017-12-13 Steam).
/// Reject excluded keys, then search the six packages in their original order.
pub fn package_key(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    let key = p.u32(m, c.esp.wrapping_add(4));
    c.eax = find_package_key(m, p, c.ecx, key);
    c.esp = c.esp.wrapping_add(8);
}

fn find_package_key(m: &dyn Mem, p: Pages, owner: u32, key: u32) -> u32 {
    let count = p.u32(m, owner.wrapping_add(0xac2a0));
    for i in 0..count {
        if p.u32(m, owner.wrapping_add(0xab2a0).wrapping_add(i.wrapping_mul(4))) == key {
            return 0;
        }
    }
    for i in 0..6 {
        let package = p.u32(m, owner.wrapping_add(0xab104 + i * 4));
        if package != 0 {
            let found = lookup(m, p, package, key);
            if found != 0 {
                return found;
            }
        }
    }
    0
}

// Translated from 0x00FED190 (DDDA.exe 2017-12-13 Steam).
/// Begin a material state scope using its virtual policy result.
pub fn begin_material(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    let entry = c.esp;
    let context = c.ecx;
    let material = p.u32(m, entry.wrapping_add(4));
    let target = p.u32(m, p.u32(m, material).wrapping_add(0x30));
    let argument = p.u32(m, entry.wrapping_add(8));
    // Preserve the original virtual call frame and stack arguments.
    c.esp = entry.wrapping_sub(8);
    for value in [argument, context, 0x00fe_d1a7] {
        c.esp = c.esp.wrapping_sub(4);
        p.set_u32(m, c.esp, value);
    }
    c.ecx = material;
    crate::semantic::call_guest(m, c, target);
    let policy = c.eax;
    let flag = match policy {
        0 => 0x40,
        1 => 0x20,
        _ => 0x80,
    };
    let flags = p.u32(m, context.wrapping_add(0x5c)) | flag;
    p.set_u32(m, context.wrapping_add(0x5c), flags);
    let cursor = p.u32(m, context.wrapping_add(0x48));
    p.set_u32(m, context.wrapping_add(0x78), material);
    p.set_u32(m, context.wrapping_add(0x64), cursor);
    p.set_u32(m, context.wrapping_add(0x68), policy);
    p.set_u32(m, context.wrapping_add(0x6c), 0);
    c.esp = entry.wrapping_add(12);
}

/// The CRT's `memcpy` (0x012EFEA0), which behaves as `memmove`: `len` bytes
/// from `source` to `destination`, overlap-safe, page by page through the
/// page table (anything it does not allow goes through the trait, which
/// faults as the guest would).
fn copy_guest(m: &mut dyn Mem, p: Pages, destination: u32, source: u32, len: u32) {
    const READ: u8 = 1;
    const WRITE: u8 = 2;
    let len = len as usize;
    // one source page and one destination page: a plain overlap-safe move
    if let (Some(from), Some(to)) = (p.bytes(source, len, READ), p.bytes(destination, len, WRITE)) {
        // SAFETY: bytes() returned `len` readable and writable bytes, each inside one backed page
        unsafe { std::ptr::copy(from, to, len) };
        return;
    }
    // general case (pages crossed): read everything first, then write, a
    // page-bounded chunk at a time
    let mut data = vec![0u8; len];
    let mut done = 0usize;
    while done < len {
        let at = source.wrapping_add(done as u32);
        let chunk = (len - done).min(4096 - (at & 4095) as usize);
        match p.bytes(at, chunk, READ) {
            // SAFETY: bytes() returned `chunk` readable bytes inside one backed page
            Some(from) => unsafe { std::ptr::copy_nonoverlapping(from, data[done..].as_mut_ptr(), chunk) },
            None => data[done..done + chunk].iter_mut().enumerate().for_each(|(k, b)| *b = p.u8(m, at.wrapping_add(k as u32))),
        }
        done += chunk;
    }
    done = 0;
    while done < len {
        let at = destination.wrapping_add(done as u32);
        let chunk = (len - done).min(4096 - (at & 4095) as usize);
        match p.bytes(at, chunk, WRITE) {
            // SAFETY: bytes() returned `chunk` writable bytes inside one backed page
            Some(to) => unsafe { std::ptr::copy_nonoverlapping(data[done..].as_ptr(), to, chunk) },
            None => data[done..done + chunk].iter().enumerate().for_each(|(k, &b)| p.set_u8(m, at.wrapping_add(k as u32), b)),
        }
        done += chunk;
    }
}

/// UCOMISS equality of two floats' bits on the host under the live MXCSR
/// (denormals-are-zero included): true for equal and ordered, false for
/// unequal or unordered; the MXCSR exception flags are the hardware's.
#[inline(always)]
fn ucomiss_equal(a: u32, b: u32) -> bool {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: SSE is part of the x86_64 baseline
    unsafe {
        use std::arch::x86_64::{_mm_set_ss, _mm_ucomieq_ss};
        _mm_ucomieq_ss(_mm_set_ss(f32::from_bits(a)), _mm_set_ss(f32::from_bits(b))) != 0
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        x86::comiss_hw::<false>(a, b).r == 0
    }
}

/// COMISS "greater" of two floats' bits on the host (the original's `ja`
/// after `comiss a, b`): false for unordered.
#[inline(always)]
fn comiss_greater(a: u32, b: u32) -> bool {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: SSE is part of the x86_64 baseline
    unsafe {
        use std::arch::x86_64::{_mm_comigt_ss, _mm_set_ss};
        _mm_comigt_ss(_mm_set_ss(f32::from_bits(a)), _mm_set_ss(f32::from_bits(b))) != 0
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        x86::comiss_hw::<true>(a, b).r == 1
    }
}

fn bits(m: &mut dyn Mem, p: Pages, address: u32, keep: u32, add: u32) {
    let value = (p.u32(m, address) & keep) | add;
    p.set_u32(m, address, value);
}

/// Call a draw-context dependency with its original outgoing stack frame.
fn draw_call(
    m: &mut dyn Mem,
    c: &mut Cpu,
    p: Pages,
    frame: u32,
    target: u32,
    resume: u32,
    owner: u32,
    args: &[u32],
) -> u32 {
    c.esp = frame;
    for &value in args.iter().rev().chain(std::iter::once(&resume)) {
        c.esp = c.esp.wrapping_sub(4);
        p.set_u32(m, c.esp, value);
    }
    c.ecx = owner;
    crate::semantic::call_guest(m, c, target);
    c.eax
}

/// Reserve an aligned packet in the context's current arena, growing through
/// the original allocator. The caller publishes the packet's purpose.
fn reserve_packet(
    m: &mut dyn Mem,
    c: &mut Cpu,
    p: Pages,
    entry: u32,
    context: u32,
    size: u32,
    resume: u32,
) -> u32 {
    if p.u32(m, context.wrapping_add(0x24)).wrapping_add(size) > p.u32(m, context.wrapping_add(0x28)) {
        let capacity = size.wrapping_add(0xfff) & !0xfff;
        let owner = p.u32(m, 0x018d08f4);
        let buffer = draw_call(
            m,
            c,
            p,
            entry.wrapping_sub(32),
            0x00da7d70,
            resume,
            owner,
            &[capacity],
        );
        p.set_u32(m, context.wrapping_add(0x24), buffer);
        p.set_u32(
            m,
            context.wrapping_add(0x28),
            if buffer == 0 {
                0
            } else {
                buffer.wrapping_add(capacity)
            },
        );
    }
    p.u32(m, context.wrapping_add(0x24))
}

fn copy_packet_pair(m: &mut dyn Mem, p: Pages, source: u32, destination: u32) {
    let low = p.u32(m, source);
    let high = p.u32(m, source.wrapping_add(4));
    p.set_u32(m, destination, low);
    p.set_u32(m, destination.wrapping_add(4), high);
}

/// Evaluate the original alpha threshold with guest x87 precision. In the
/// non-blend path the comparison uses the unrounded extended product, even
/// though the packet receives a rounded f32. Replacing this with f32 changes
/// both packet allocation and the reciprocal near alpha=255.
fn alpha_threshold(m: &dyn Mem, c: &mut Cpu, p: Pages, alpha: u32, previous: u32) -> (u32, u32, bool) {
    let mut operand = [0; 10];
    operand[..4].copy_from_slice(&alpha.to_le_bytes());
    c.st.exec::<0x00db>(&mut operand); // FILD i32
    operand[..4].copy_from_slice(&p.u32(m, 0x0161c29c).to_le_bytes());
    c.st.exec::<0x08d8>(&mut operand); // FMUL f32
    c.st.exec::<0x10d9>(&mut operand); // FST f32, retain extended product
    let threshold = u32::from_le_bytes(operand[..4].try_into().unwrap());
    c.st.register::<0xe8d9>(); // FLD1
    c.st.register::<0xe1d8>(); // FSUB ST0,ST1, retaining the extended alpha
    operand[..4].copy_from_slice(&p.u32(m, 0x014091ac).to_le_bytes());
    c.st.exec::<0x38d8>(&mut operand); // FDIVR f32
    c.st.exec::<0x18d9>(&mut operand);
    let reciprocal = u32::from_le_bytes(operand[..4].try_into().unwrap());
    operand[..4].copy_from_slice(&p.u32(m, previous).to_le_bytes());
    c.st.exec::<0x00d9>(&mut operand);
    let flags = c.st.exec::<0xe9df>(&mut operand); // FUCOMIP ST0,ST1
    c.st.register::<0xd8dd>(); // FSTP ST0
    let changed = flags & 0x44 != 0x40;
    (threshold, reciprocal, changed)
}

// Translated from 0x00FEE320 (DDDA.exe 2017-12-13 Steam).
/// Commit per-primitive state to arena packets. Context +0x194/+0x19c/+0x1a0
/// are immutable snapshots; +0x198 is the last dependency packet. Dirty bits
/// in +0x154 control shader selection and packet deduplication.
pub fn commit_primitive_state(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    let entry = c.esp;
    let context = c.ecx;
    let primitive = p
        .u32(m, entry.wrapping_add(4))
        .wrapping_add(0x1c)
        .wrapping_add(p.u32(m, entry.wrapping_add(8)).wrapping_mul(40));
    c.eax = commit_state(m, c, p, entry, context, primitive);
    c.esp = entry.wrapping_add(12);
}

fn commit_state(m: &mut dyn Mem, c: &mut Cpu, p: Pages, entry: u32, context: u32, primitive: u32) -> u32 {
    if p.u32(m, context.wrapping_add(0x150)) != 0 {
        p.set_u32(m, context.wrapping_add(0x150), 0);
        let buffer = reserve_packet(m, c, p, entry, context, 48, 0x00fee367);
        // First pair is loaded before publishing the pointers in the original.
        let first = [
            p.u32(m, context.wrapping_add(0x124)),
            p.u32(m, context.wrapping_add(0x128)),
        ];
        p.set_u32(m, context.wrapping_add(0x19c), buffer);
        p.set_u32(m, context.wrapping_add(0x24), buffer.wrapping_add(48));
        p.set_u32(m, buffer, first[0]);
        p.set_u32(m, buffer.wrapping_add(4), first[1]);
        for offset in (8..48).step_by(8) {
            copy_packet_pair(
                m,
                p,
                context.wrapping_add(0x124 + offset),
                buffer.wrapping_add(offset),
            );
        }
    }
    for slot in 0..4 {
        let key = p.u32(m, context.wrapping_add(0x1bc + slot * 4));
        let present = if key == 0 {
            false
        } else {
            let registry = p.u32(m, 0x018d12cc);
            let resource = p.u32(m, registry.wrapping_add(0x3c + (key & 0xfff) * 8));
            p.u8(m, resource.wrapping_add(0x24)) != 0
        };
        let snapshot = p.u32(m, context.wrapping_add(0x19c));
        p.set_u8(m, snapshot.wrapping_add(0x24 + slot), u8::from(present));
    }
    if p.u32(m, primitive.wrapping_add(0x18)) != 0 {
        for (shift, mask, field) in [(20, 0xfff, 0x11c), (0, 0x3ff, 0x118), (10, 0x3ff, 0x120)] {
            let index = (p.u32(m, primitive.wrapping_add(0x18)) >> shift) & mask;
            if index != 0 {
                let state = p.u32(m, context.wrapping_add(0x1f8 + index * 8));
                if p.u32(m, context.wrapping_add(field)) != state {
                    p.set_u32(m, context.wrapping_add(field), state);
                    bits(m, p, context.wrapping_add(0x114), 0xfff, 0x1000);
                }
            }
        }
    }
    if p.u32(m, context.wrapping_add(0x114)) & !0xfff != 0 {
        bits(m, p, context.wrapping_add(0x114), 0xfff, 0);
        let buffer = reserve_packet(m, c, p, entry, context, 32, 0x00fee575);
        let first = [
            p.u32(m, context.wrapping_add(0x110)),
            p.u32(m, context.wrapping_add(0x114)),
        ];
        p.set_u32(m, context.wrapping_add(0x24), buffer.wrapping_add(32));
        p.set_u32(m, context.wrapping_add(0x194), buffer);
        p.set_u32(m, buffer, first[0]);
        p.set_u32(m, buffer.wrapping_add(4), first[1]);
        copy_packet_pair(m, p, context.wrapping_add(0x118), buffer.wrapping_add(8));
        let state = p.u32(m, context.wrapping_add(0x120));
        p.set_u32(m, buffer.wrapping_add(16), state);
        for offset in [0x1ac, 0x1a4, 0x1b8, 0x1b0] {
            p.set_u32(m, context.wrapping_add(offset), 0);
        }
    }
    let state_packet = p.u32(m, context.wrapping_add(0x194));
    let blend = p.u32(m, p.u32(m, state_packet.wrapping_add(8)).wrapping_add(16)) & 1;
    if (p.u32(m, context.wrapping_add(0x154)) >> 2) & 1 != blend {
        bits(m, p, context.wrapping_add(0x154), !4, blend * 4 | 1);
    }
    let special = p.u8(m, p.u32(m, state_packet.wrapping_add(16)).wrapping_add(16)) & 0x38 != 0;
    let (bank, key) = if special || p.u32(m, context.wrapping_add(0x114)) & 0x100 != 0 {
        (0x1ec8, 0x3d9)
    } else {
        (0x1ed0, 0xf43913da)
    };
    select_state(m, p, context, 0x20c0, bank, 0x3d9, key);
    if p.u32(m, context.wrapping_add(0xc8)) & 8 != 0 {
        bits(m, p, context.wrapping_add(0xc8), !8, 0);
        let buffer = reserve_packet(m, c, p, entry, context, 96, 0x00fee7a4);
        p.set_u32(m, context.wrapping_add(0x24), buffer.wrapping_add(96));
        p.set_u32(m, context.wrapping_add(0x1a0), buffer);
        for offset in (0..92).step_by(4) {
            let value = p.u32(m, context.wrapping_add(0xb4 + offset));
            p.set_u32(m, buffer.wrapping_add(offset), value);
        }
        for offset in [0x1a8, 0x1b4] {
            p.set_u32(m, context.wrapping_add(offset), 0);
        }
    }
    let flags = p.u32(m, context.wrapping_add(0x154));
    if flags & 1 == 0 {
        return (flags & !255) | 1;
    }
    let material = p.u32(m, p.u32(m, context.wrapping_add(0x1a0)));
    let alternate = (flags >> 19) & 1 != 0 && material != 0 && p.u8(m, material.wrapping_add(32)) & 0x20 != 0;
    p.set_u8(m, entry.wrapping_add(4), u8::from(alternate));
    let mut alpha = (flags >> 11) & 255;
    let mut mode = (flags >> 3) & 255;
    if flags & (1 << 20) == 0 || (flags & 4 == 0 && alternate) {
        select_state(m, p, context, 0x2248, 0x2050, 0x40a, 0x40a);
    } else {
        let blend = flags & 4 != 0;
        // The blend path computes before selecting state; the opaque path
        // computes afterwards. Keep this order for aliased guest inputs.
        let mut values = None;
        if blend {
            // Defer comparisons until the state selection has completed.
            values = Some(alpha_values(m, c, p, alpha));
            let (bank, key) = if alternate {
                mode = 4;
                alpha = 0;
                (0x2098, 0xce03a413)
            } else {
                (0x20a0, 0xcd3ad414)
            };
            select_state(m, p, context, 0x2248, bank, 0x40a, key);
        } else {
            let key = p.u32(m, 0x01447b5c + mode * 4);
            select_state(
                m,
                p,
                context,
                0x2248,
                (key & 0xfff) * 8,
                0x40a,
                (key ^ 0x84f11fff) & !0xfff | (key & 0xfff),
            );
        }
        let previous = p.u32(m, context.wrapping_add(0xcc8)) & !15;
        let (threshold, reciprocal, changed) = if let Some((a, b)) = values {
            (
                a,
                b,
                !ucomiss_equal(p.u32(m, previous.wrapping_add(12)), b) || !ucomiss_equal(p.u32(m, previous), a),
            )
        } else {
            alpha_threshold(m, c, p, alpha, previous)
        };
        if changed {
            let descriptor = p.u32(m, p.u32(m, context.wrapping_add(24)).wrapping_add(0x568));
            let size = (p.u16(m, descriptor.wrapping_add(24)) as u32 * 4 + 15) & !15;
            let buffer = reserve_packet(
                m,
                c,
                p,
                entry,
                context,
                size,
                if blend { 0x00feea6f } else { 0x00feec15 },
            );
            p.set_u32(m, context.wrapping_add(0x24), buffer.wrapping_add(size));
            p.set_u32(m, context.wrapping_add(0xcc8), buffer);
            bits(m, p, context.wrapping_add(0x154), u32::MAX, 1);
            if buffer != 0 {
                let size = p.u16(m, descriptor.wrapping_add(24)) as u32 * 4;
                let source = p.u32(m, descriptor.wrapping_add(36));
                crate::semantic::copy_guest(m, p, buffer, source, size);
                c.eax = buffer; // memcpy returns its destination
                p.set_u32(m, buffer, threshold);
                p.set_u32(m, buffer.wrapping_add(8), 0);
                p.set_u32(m, buffer.wrapping_add(4), 0);
                p.set_u32(m, buffer.wrapping_add(12), reciprocal);
                let mut value = [0; 10];
                value[..4].copy_from_slice(&p.u32(m, context.wrapping_add(0x17c)).to_le_bytes());
                c.st.exec::<0x00d9>(&mut value);
                c.st.exec::<0x18d9>(&mut value);
                p.set_u32(
                    m,
                    buffer.wrapping_add(16),
                    u32::from_le_bytes(value[..4].try_into().unwrap()),
                );
            }
        }
    }
    let mut result = p.u32(m, context.wrapping_add(0x154));
    if result & 2 != 0 {
        bits(m, p, context.wrapping_add(0x154), !2, 0);
        let args = [
            context.wrapping_add(4),
            p.u32(m, context.wrapping_add(0x1cc)),
            p.u32(m, entry.wrapping_add(8)),
            context.wrapping_add(0x1f8),
            context.wrapping_add(0x1bc),
            p.u32(m, context.wrapping_add(0x170)),
            p.u32(m, context.wrapping_add(0x1d0)),
        ];
        let owner = p.u32(m, 0x018d12cc);
        result = draw_call(
            m,
            c,
            p,
            entry.wrapping_sub(32),
            0x00e132b0,
            0x00feed67,
            owner,
            &args,
        );
        p.set_u32(m, context.wrapping_add(0x15c), result);
    }
    let dependencies = p.u32(m, context.wrapping_add(0x15c));
    if dependencies == 0 {
        return result & !255;
    }
    bits(m, p, context.wrapping_add(0x154), !1, 0);
    let count = p.u32(m, dependencies.wrapping_add(32));
    let bytes = count.wrapping_mul(4).wrapping_add(12);
    let size = bytes.wrapping_add(15) & !15;
    p.set_u32(m, entry.wrapping_add(8), bytes);
    let buffer = reserve_packet(m, c, p, entry, context, size, 0x00feedc7);
    let first = [
        p.u32(m, context.wrapping_add(0x154)),
        p.u32(m, context.wrapping_add(0x158)),
    ];
    p.set_u32(m, context.wrapping_add(0x24), buffer.wrapping_add(size));
    p.set_u32(m, buffer, first[0]);
    p.set_u32(m, buffer.wrapping_add(4), first[1]);
    let current = p.u32(m, context.wrapping_add(0x15c));
    p.set_u32(m, buffer.wrapping_add(8), current);
    let packed = ((u32::from(p.u8(m, entry.wrapping_add(4))) & 1) << 16 | alpha << 8 | mode) * 8;
    bits(m, p, buffer, 0xfff00007, packed);
    p.set_u32(m, buffer.wrapping_add(8), dependencies);
    let keys = p.u32(m, dependencies.wrapping_add(28));
    for index in 0..count {
        let key = p.u32(m, keys.wrapping_add(index.wrapping_mul(4))) & 0xfff;
        let state = p.u32(m, context.wrapping_add(0x1f8 + key * 8));
        p.set_u32(
            m,
            buffer.wrapping_add(12).wrapping_add(index.wrapping_mul(4)),
            state,
        );
    }
    let previous = p.u32(m, context.wrapping_add(0x198));
    let mut mismatch = false;
    if previous != 0 && p.u32(m, previous.wrapping_add(8)) == dependencies {
        for offset in (0..bytes / 8 * 8).step_by(4) {
            if p.u32(m, previous.wrapping_add(offset)) != p.u32(m, buffer.wrapping_add(offset)) {
                mismatch = true;
                break;
            }
        }
        if !mismatch {
            for offset in bytes / 8 * 8..bytes {
                if p.u8(m, previous.wrapping_add(offset)) != p.u8(m, buffer.wrapping_add(offset)) {
                    mismatch = true;
                    break;
                }
            }
        }
        if !mismatch {
            return buffer & !255 | 1;
        }
    }
    p.set_u32(m, context.wrapping_add(0x198), buffer);
    for offset in [0x1a4, 0x1b0] {
        p.set_u32(m, context.wrapping_add(offset), 0);
    }
    if mismatch {
        1
    } else {
        buffer & !255 | 1
    }
}

fn alpha_values(m: &dyn Mem, c: &mut Cpu, p: Pages, alpha: u32) -> (u32, u32) {
    let mut data = [0; 10];
    data[..4].copy_from_slice(&alpha.to_le_bytes());
    c.st.exec::<0x00db>(&mut data);
    data[..4].copy_from_slice(&p.u32(m, 0x0161c29c).to_le_bytes());
    c.st.exec::<0x08d8>(&mut data);
    c.st.exec::<0x10d9>(&mut data);
    let threshold = u32::from_le_bytes(data[..4].try_into().unwrap());
    c.st.register::<0xe8d9>();
    c.st.register::<0xe1de>();
    data[..4].copy_from_slice(&p.u32(m, 0x014091ac).to_le_bytes());
    c.st.exec::<0x38d8>(&mut data);
    c.st.exec::<0x18d9>(&mut data);
    (threshold, u32::from_le_bytes(data[..4].try_into().unwrap()))
}

/// Depth of a point under the original scalar SSE operation order. Bits are
/// kept throughout so guest rounding, DAZ/FTZ and NaN payloads reach hardware.
fn mesh_depth(m: &dyn Mem, p: Pages, view: u32, point: [u32; 3], swapped: bool) -> u32 {
    let x = x86::mulss(
        if swapped {
            p.u32(m, view.wrapping_add(8))
        } else {
            point[0]
        },
        if swapped {
            point[0]
        } else {
            p.u32(m, view.wrapping_add(8))
        },
    );
    let y = x86::mulss(
        if swapped {
            p.u32(m, view.wrapping_add(24))
        } else {
            point[1]
        },
        if swapped {
            point[1]
        } else {
            p.u32(m, view.wrapping_add(24))
        },
    );
    let z = x86::mulss(
        if swapped {
            p.u32(m, view.wrapping_add(40))
        } else {
            point[2]
        },
        if swapped {
            point[2]
        } else {
            p.u32(m, view.wrapping_add(40))
        },
    );
    x86::addss(
        x86::addss(
            if swapped {
                x86::addss(y, x)
            } else {
                x86::addss(x, y)
            },
            z,
        ),
        p.u32(m, view.wrapping_add(56)),
    )
}

fn touch_mesh_resource(m: &mut dyn Mem, p: Pages, resource: u32) {
    let stamp = p.u32(m, 0x018d2ce8).wrapping_add(1);
    p.set_u32(m, resource.wrapping_add(4), stamp);
}

/// Append a 32-byte indexed draw and its eight-byte sort entry. The state
/// packets are already committed by FEEF20; this only publishes references.
fn append_mesh_draw(m: &mut dyn Mem, c: &mut Cpu, p: Pages, frame: u32, context: u32, range: [u32; 3]) {
    if p.u32(m, context.wrapping_add(0x24)).wrapping_add(32) > p.u32(m, context.wrapping_add(0x28)) {
        draw_call(m, c, p, frame, 0x00fed890, 0x00ffc8e6, context, &[32]);
    }
    let command = p.u32(m, context.wrapping_add(0x24));
    p.set_u32(m, context.wrapping_add(0x24), command.wrapping_add(32));
    bits(m, p, command, 0xfff1ffff, 0x10000);
    let sequence = p.u32(m, context.wrapping_add(0x74));
    bits(m, p, command, !0x7fff, sequence & 0x7fff);
    let next = p.u32(m, context.wrapping_add(0x74)).wrapping_add(1);
    p.set_u32(m, context.wrapping_add(0x74), next);
    let scoped = p.u8(m, context.wrapping_add(0x5c)) & 0x50 != 0;
    bits(m, p, command, !0x8000, u32::from(scoped) << 15);
    let saved = p.u32(m, context.wrapping_add(0x6c));
    bits(m, p, command, 0xfffff, saved << 20);
    for (source, dest) in [(0x194, 8), (0x198, 12), (0x19c, 16), (0x1a0, 4)] {
        let value = p.u32(m, context.wrapping_add(source));
        p.set_u32(m, command.wrapping_add(dest), value);
    }
    for (offset, value) in [(28, range[2]), (20, range[0]), (24, range[1])] {
        p.set_u32(m, command.wrapping_add(offset), value);
    }
    let index = p.u32(m, context.wrapping_add(0x48));
    let sort = p
        .u32(m, context.wrapping_add(0x44))
        .wrapping_add(index.wrapping_mul(8));
    p.set_u32(m, context.wrapping_add(0x48), index.wrapping_add(1));
    let mode = p.u32(m, context.wrapping_add(0x164));
    bits(m, p, sort, 0x07ffffff, mode << 27);
    let key =
        (p.u32(m, context.wrapping_add(0x164)) >> 5).wrapping_add(p.u32(m, context.wrapping_add(0x178)));
    p.set_u32(m, sort.wrapping_add(4), command);
    bits(m, p, sort, !0x07ffffff, key & 0x07ffffff);
}

// Translated from 0x00FFC350 (DDDA.exe 2017-12-13 Steam), uModel::vf26.
/// Draw a model mesh: select LOD and material scopes, bind its vertex streams,
/// derive sort keys, then emit visible 48-byte primitive records. Models with
/// no extended data (+0x360) retain the original uBaseModel dispatch.
pub fn draw_model_mesh(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    let entry = c.esp;
    let model = c.ecx;
    let stack = (entry.wrapping_sub(4) & !15).wrapping_sub(0x60);
    let frame = stack;
    let args: [u32; 6] = std::array::from_fn(|i| p.u32(m, entry.wrapping_add(4 + i as u32 * 4)));
    c.eax = draw_model_mesh_body(m, c, p, frame, model, args);
    c.esp = entry.wrapping_add(28);
}

fn draw_model_mesh_body(
    m: &mut dyn Mem,
    c: &mut Cpu,
    p: Pages,
    frame: u32,
    model: u32,
    args: [u32; 6],
) -> u32 {
    if p.u32(m, model.wrapping_add(0x360)) == 0 {
        return draw_call(m, c, p, frame, 0x00ea5320, 0x00ffc386, model, &args);
    }
    let [context, mesh, materials, position, _, _] = args;
    let count = p.u32(m, mesh.wrapping_add(0x74));
    let pass = p.u16(m, context.wrapping_add(0x16a)) as u32;
    let primitives = p.u32(m, mesh.wrapping_add(0x70));
    let vertices = p.u32(m, mesh.wrapping_add(0xa0));
    let view = draw_call(m, c, p, frame, 0x00fed940, 0x00ffc3c2, context, &[]);
    let point = std::array::from_fn(|i| p.u32(m, position.wrapping_add(i as u32 * 4)));
    let depth = mesh_depth(m, p, view, point, false) ^ p.u32(m, 0x0161c280);
    if pass & 0x20 != 0 && comiss_greater(depth, p.u32(m, model.wrapping_add(0xd84))) {
        return view;
    }
    let skinned = p.u32(m, mesh.wrapping_add(0x220)) != 0;
    let (skin_vertices, skin_ranges) = if skinned {
        let buffers = (
            p.u32(m, mesh.wrapping_add(0x22c)),
            p.u32(m, mesh.wrapping_add(0x220)),
        );
        if p.u32(m, context.wrapping_add(0x1c8)) != 0xc25bb069 {
            if p.u8(m, context.wrapping_add(0x5c)) & 0x80 != 0 {
                let record = |m: &dyn Mem| {
                    context
                        .wrapping_add(0x30c7c)
                        .wrapping_add(p.u32(m, context.wrapping_add(0x6c)).wrapping_mul(12))
                };
                let slot = record(m);
                bits(m, p, slot, 0x80000003, 3);
                let slot = record(m);
                bits(m, p, slot, u32::MAX, 0x80000000);
                let slot = record(m);
                let old = p.u32(m, context.wrapping_add(0x1c8));
                p.set_u32(m, slot.wrapping_add(8), old);
                let next = p.u32(m, context.wrapping_add(0x6c)).wrapping_add(1);
                p.set_u32(m, context.wrapping_add(0x6c), next);
            }
            p.set_u32(m, context.wrapping_add(0x1c8), 0xc25bb069);
            bits(m, p, context.wrapping_add(0x154), u32::MAX, 3);
        }
        buffers
    } else {
        (0, 0)
    };
    let delta: [u32; 3] = std::array::from_fn(|i| {
        x86::subss(
            p.u32(m, position.wrapping_add(i as u32 * 4)),
            p.u32(m, context.wrapping_add(0x184 + i as u32 * 4)),
        )
    });
    let distance = x86::cvttss2si(x86::sqrtss(x86::addss(
        x86::addss(x86::mulss(delta[0], delta[0]), x86::mulss(delta[1], delta[1])),
        x86::mulss(delta[2], delta[2]),
    ))) as i32;
    let mut lod = ((p.u32(m, model.wrapping_add(0x108)) << 19) as i32) >> 28;
    let limits = p.u32(m, model.wrapping_add(0xf0));
    if lod < 0 {
        let quality = p.u32(m, p.u32(m, 0x018d1d20).wrapping_add(56));
        lod = 1;
        if quality != 3 {
            let scale = if quality == 2 { 2 } else { 1 };
            lod = if distance > p.u32(m, limits.wrapping_add(0xe4)).wrapping_mul(scale) as i32 {
                4
            } else {
                1 + i32::from(distance > p.u32(m, limits.wrapping_add(0xe0)).wrapping_mul(scale) as i32)
            };
        }
    }
    let bias = p.u32(m, model.wrapping_add(0x16c));
    let near = x86::cvttss2si(x86::addss(
        x86::addss(x86::cvtsi2ss(p.u32(m, model.wrapping_add(0x154))), depth),
        bias,
    )) as i32;
    let near_key = (0x7fff - near.clamp(0, 0x7fff) as u32) << 12;
    let far = x86::cvttss2si(x86::addss(bias, depth)) as i32;
    let far_key = p.u32(m, model.wrapping_add(0x10c)) << 24 | far.clamp(0, 0xffffff) as u32;
    let index_buffer = p.u32(m, mesh.wrapping_add(0x9c));
    if p.u32(m, context.wrapping_add(0x14c)) != index_buffer {
        touch_mesh_resource(m, p, index_buffer);
        p.set_u32(m, context.wrapping_add(0x14c), index_buffer);
        p.set_u32(m, context.wrapping_add(0x150), 1);
    }
    let mut result = index_buffer;
    let mut previous_material = u32::MAX;
    for index in 0..count {
        let primitive = primitives.wrapping_add(index.wrapping_mul(48));
        let selection = p.u32(m, primitive.wrapping_add(4));
        let visibility = selection & 0xfff;
        if p.u32(m, model.wrapping_add(0x110 + (visibility >> 5) * 4)) & (1 << (visibility & 31)) != 0
            && p.u8(m, primitive.wrapping_add(8)) & 1 != 0
            && (lod as u8) & (selection >> 24) as u8 != 0
            && pass & p.u32(m, primitive) & 0xffff != 0
        {
            let material_index = (p.u32(m, primitive.wrapping_add(4)) >> 12) & 0xfff;
            let material = p.u32(m, materials.wrapping_add(material_index * 4));
            if previous_material != material_index {
                if (previous_material as i32) >= 0 {
                    draw_call(m, c, p, frame, 0x00ff14d0, 0x00ffc6aa, context, &[]);
                }
                previous_material = (p.u32(m, primitive.wrapping_add(4)) >> 12) & 0xfff;
                draw_call(m, c, p, frame, 0x00fed190, 0x00ffc6c2, context, &[material, 24]);
            }
            draw_call(
                m,
                c,
                p,
                frame,
                0x00ea23f0,
                0x00ffc6d0,
                model,
                &[context, primitive],
            );
            let range = p.u32(m, primitive.wrapping_add(16));
            let stride = p.u8(m, primitive.wrapping_add(10));
            if p.u32(m, context.wrapping_add(0x138)) != range
                || p.u32(m, context.wrapping_add(0x124)) != vertices
                || p.u8(m, context.wrapping_add(0x134)) != stride
            {
                if vertices != 0 {
                    touch_mesh_resource(m, p, vertices);
                }
                p.set_u32(m, context.wrapping_add(0x124), vertices);
                p.set_u8(m, context.wrapping_add(0x134), stride);
                p.set_u32(m, context.wrapping_add(0x138), range);
                p.set_u32(m, context.wrapping_add(0x150), 1);
            }
            if skinned {
                let range = p.u32(m, skin_ranges.wrapping_add(index.wrapping_mul(4)));
                if p.u32(m, context.wrapping_add(0x144)) != range
                    || p.u32(m, context.wrapping_add(0x130)) != skin_vertices
                    || p.u8(m, context.wrapping_add(0x137)) != 4
                {
                    if skin_vertices != 0 {
                        touch_mesh_resource(m, p, skin_vertices);
                    }
                    p.set_u32(m, context.wrapping_add(0x130), skin_vertices);
                    p.set_u8(m, context.wrapping_add(0x137), 4);
                    p.set_u32(m, context.wrapping_add(0x144), range);
                    p.set_u32(m, context.wrapping_add(0x150), 1);
                }
            }
            let mut mode = p.u32(m, context.wrapping_add(0x164)) & 31;
            let sort = if mode == 16 || mode == 14 {
                if p.u8(m, primitive.wrapping_add(8)) & 4 == 0 {
                    near_key
                } else {
                    let binding = p.u32(m, primitive.wrapping_add(44));
                    let point: [u32; 3] =
                        std::array::from_fn(|i| p.u32(m, binding.wrapping_add(16 + i as u32 * 4)));
                    let joint = p.u32(m, binding);
                    let matrix = p
                        .u32(m, model.wrapping_add(0x364))
                        .wrapping_add(joint.wrapping_mul(160))
                        .wrapping_add(16);
                    let transformed: [u32; 4] = std::array::from_fn(|i| {
                        let row = |offset| p.u32(m, matrix.wrapping_add(offset + i as u32 * 4));
                        x86::addss(
                            x86::addss(
                                x86::addss(x86::mulss(row(16), point[1]), x86::mulss(row(0), point[0])),
                                x86::mulss(row(32), point[2]),
                            ),
                            row(48),
                        )
                    });
                    let local = frame.wrapping_add(0x50);
                    for (i, value) in transformed.into_iter().enumerate() {
                        p.set_u32(m, local.wrapping_add(i as u32 * 4), value);
                    }
                    let view = draw_call(m, c, p, frame, 0x00fed940, 0x00ffc80d, context, &[]);
                    let point = std::array::from_fn(|i| p.u32(m, local.wrapping_add(i as u32 * 4)));
                    let depth = mesh_depth(m, p, view, point, true);
                    let key = x86::cvttss2si(x86::subss(
                        x86::cvtsi2ss(p.u32(m, model.wrapping_add(0x154))),
                        depth,
                    )) as i32;
                    mode = p.u32(m, context.wrapping_add(0x164)) & 31;
                    (0x7fff - key.clamp(0, 0x7fff) as u32) << 12
                }
            } else {
                far_key
            };
            let sort = sort.wrapping_add(p.u8(m, primitive.wrapping_add(9)) as u32);
            p.set_u32(m, context.wrapping_add(0x164), sort << 5 | mode);
            draw_call(m, c, p, frame, 0x00feef20, 0x00ffc8af, context, &[]);
            let range = [
                p.u32(m, primitive.wrapping_add(28)),
                p.u32(m, primitive.wrapping_add(24)),
                p.u32(m, primitive.wrapping_add(32)),
            ];
            let flags = p.u32(m, context.wrapping_add(0x5c));
            if flags & 0x100 != 0 || flags & 0x50 == 0 {
                append_mesh_draw(m, c, p, frame, context, range);
            }
            draw_call(m, c, p, frame, 0x00ff1920, 0x00ffc9b8, context, &[]);
        }
        result = index.wrapping_add(1);
    }
    if count != 0 && (previous_material as i32) >= 0 {
        result = draw_call(m, c, p, frame, 0x00ff14d0, 0x00ffc9dc, context, &[]);
    }
    result
}

// State-save layout and masks from 0x00EA23F0, repeated inline in the original.
fn save_state(m: &mut dyn Mem, p: Pages, context: u32, field: u32, tag: u32) {
    if p.u8(m, context.wrapping_add(0x5c)) & 0x80 == 0 {
        return;
    }
    let slot = |m: &dyn Mem| {
        context
            .wrapping_add(0x30c7c)
            .wrapping_add(p.u32(m, context.wrapping_add(0x6c)).wrapping_mul(12))
    };
    let record = slot(m);
    bits(m, p, record, 0x8000_0000 | tag, tag);
    let record = slot(m);
    bits(m, p, record, 0x7fff_ffff, 0);
    let first = p.u32(m, context.wrapping_add(field));
    let record = slot(m);
    p.set_u32(m, record.wrapping_add(4), first);
    let second = p.u32(m, context.wrapping_add(field + 4));
    p.set_u32(m, record.wrapping_add(8), second);
    let next = p.u32(m, context.wrapping_add(0x6c)).wrapping_add(1);
    p.set_u32(m, context.wrapping_add(0x6c), next);
}

fn select_state(
    m: &mut dyn Mem,
    p: Pages,
    context: u32,
    field: u32,
    bank_field: u32,
    tag: u32,
    key: u32,
) -> u32 {
    let bank = p.u32(m, context.wrapping_add(0x20));
    let old = p.u32(m, context.wrapping_add(field));
    if old == p.u32(m, bank.wrapping_add(bank_field)) {
        return old;
    }
    save_state(m, p, context, field, tag);
    let bank = p.u32(m, context.wrapping_add(0x20));
    let value = p.u32(m, bank.wrapping_add(bank_field));
    p.set_u32(m, context.wrapping_add(field), value);
    p.set_u32(m, context.wrapping_add(field + 4), key);
    bits(m, p, context.wrapping_add(0x154), u32::MAX, 3);
    bank // EAX at the original state-update exit (also for this void function).
}

fn allocate_constants(
    m: &mut dyn Mem,
    c: &mut Cpu,
    p: Pages,
    entry: u32,
    context: u32,
    field: u32,
    descriptor: u32,
    tag: u32,
    resume: u32,
) -> (u32, Option<u32>) {
    bits(m, p, context.wrapping_add(0x5c), u32::MAX, 2);
    save_state(m, p, context, field, tag);
    let bank = p.u32(m, context.wrapping_add(0x18));
    let descriptor = p.u32(m, bank.wrapping_add(descriptor));
    let size = ((p.u16(m, descriptor.wrapping_add(0x18)) as u32) * 4 + 15) & !15;
    let end = p.u32(m, context.wrapping_add(0x24)).wrapping_add(size);
    let mut reloaded_primitive = None;
    if end > p.u32(m, context.wrapping_add(0x28)) {
        c.esp = entry.wrapping_sub(16);
        for value in [size, resume] {
            c.esp = c.esp.wrapping_sub(4);
            p.set_u32(m, c.esp, value);
        }
        c.ecx = context;
        crate::semantic::call_guest(m, c, 0x00fe_d890);
        reloaded_primitive = Some(p.u32(m, entry.wrapping_add(8)));
    }
    let buffer = p.u32(m, context.wrapping_add(0x24));
    p.set_u32(m, context.wrapping_add(0x24), buffer.wrapping_add(size));
    p.set_u32(m, context.wrapping_add(field), buffer);
    bits(m, p, context.wrapping_add(0x154), u32::MAX, 1);
    (buffer, reloaded_primitive)
}

// Translated from 0x00EA23F0 (DDDA.exe 2017-12-13 Steam).
/// Select per-primitive material states and publish their constant buffers.
pub fn primitive_material(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    let entry = c.esp;
    let model = c.ecx;
    let context = p.u32(m, entry.wrapping_add(4));
    let primitive = p.u32(m, entry.wrapping_add(8));
    c.eax = apply_primitive_material(m, c, p, entry, model, context, primitive);
    c.esp = entry.wrapping_add(12);
}

fn apply_primitive_material(
    m: &mut dyn Mem,
    c: &mut Cpu,
    p: Pages,
    entry: u32,
    model: u32,
    context: u32,
    mut primitive: u32,
) -> u32 {
    let material = p.u32(m, context.wrapping_add(0x1d4));
    let enabled = material != 0 && p.u32(m, model.wrapping_add(8)) & p.u32(m, primitive) & 0x104 != 0;
    if !enabled {
        select_state(m, p, context, 0x2c10, 0x2a18, 0x543, 0x543);
    } else {
        let special = p.u8(m, material.wrapping_add(0x10)) & 4 != 0;
        let mode = p.u32(m, context.wrapping_add(0x164)) & 0x1f == 0x10;
        if special {
            // Original saves the mode in the low byte of its first stack argument.
            p.set_u8(m, entry.wrapping_add(4), u8::from(mode));
            select_state(m, p, context, 0x2c10, 0x2a50, 0x543, 0x9345_554a);
            let (buffer, reloaded) =
                allocate_constants(m, c, p, entry, context, 0x1290, 0x84c, 0x213, 0x00ea_27c8);
            if let Some(value) = reloaded {
                primitive = value;
            }
            if buffer != 0 {
                let flags = p.u32(m, model.wrapping_add(8));
                let one = p.u32(m, 0x0140_6ce0);
                let mode = p.u8(m, entry.wrapping_add(4)) != 0;
                for (i, yes) in [flags & 4 != 0, flags & 0x100 != 0, mode].into_iter().enumerate() {
                    p.set_u32(m, buffer.wrapping_add(i as u32 * 4), if yes { one } else { 0 });
                }
            }
            bits(m, p, context.wrapping_add(0x5c), !2, 0);
        } else {
            let flag = p.u32(m, model.wrapping_add(8)) & 4 != 0;
            let (offset, key) = match (mode, flag) {
                (true, true) => (0x33d0, 0x53e4_e67a),
                (true, false) => (0x33d8, 0x20ed_867b),
                (false, true) => (0x33c0, 0x8453_6678),
                (false, false) => (0x33c8, 0xf75a_0679),
            };
            select_state(m, p, context, 0x2c10, offset, 0x543, key);
        }
    }
    let (buffer, reloaded) = allocate_constants(m, c, p, entry, context, 0x1518, 0x990, 0x264, 0x00ea_2958);
    if let Some(value) = reloaded {
        primitive = value;
    }
    if buffer != 0 {
        let first_default = p.u32(m, 0x018c_fe70);
        let mut same = ucomiss_equal(p.u32(m, model.wrapping_add(0x1d0)), first_default);
        for i in 1..4 {
            if !same {
                break;
            }
            same = ucomiss_equal(p.u32(m, model.wrapping_add(0x1d0 + i * 4)), p.u32(m, 0x018c_fe70 + i * 4));
        }
        let mut custom = false;
        if !same {
            let material = p.u32(m, context.wrapping_add(0x1d4));
            if material != 0 {
                let descriptor = p.u32(m, material.wrapping_add(0xb8));
                if descriptor != 0 {
                    custom = p.u32(m, p.u32(m, descriptor.wrapping_add(0x44))) != 0;
                }
            }
        }
        for i in 0..4 {
            let value = if custom {
                // Preserve FLD/FSTP conversion (not a raw copy for signaling NaNs).
                let mut data = [0u8; 10];
                data[..4].copy_from_slice(&p.u32(m, model.wrapping_add(0x1d0 + i * 4)).to_le_bytes());
                c.st.exec::<0x00d9>(&mut data);
                c.st.exec::<0x18d9>(&mut data);
                u32::from_le_bytes(data[..4].try_into().unwrap())
            } else if i == 0 {
                first_default
            } else {
                p.u32(m, 0x018c_fe70 + i * 4)
            };
            p.set_u32(m, buffer.wrapping_add(i * 4), value);
        }
    }
    bits(m, p, context.wrapping_add(0x5c), !2, 0);
    let key = p.u32(m, primitive.wrapping_add(20));
    if p.u32(m, context.wrapping_add(0x1bc)) != key {
        if p.u8(m, context.wrapping_add(0x5c)) & 0x80 != 0 {
            let slot = |m: &dyn Mem| {
                context
                    .wrapping_add(0x30c7c)
                    .wrapping_add(p.u32(m, context.wrapping_add(0x6c)).wrapping_mul(12))
            };
            let record = slot(m);
            bits(m, p, record, 0x8000_0000, 0);
            let record = slot(m);
            bits(m, p, record, u32::MAX, 0x8000_0000);
            let old = p.u32(m, context.wrapping_add(0x1bc));
            let record = slot(m);
            p.set_u32(m, record.wrapping_add(8), old);
            let next = p.u32(m, context.wrapping_add(0x6c)).wrapping_add(1);
            p.set_u32(m, context.wrapping_add(0x6c), next);
        }
        p.set_u32(m, context.wrapping_add(0x1bc), key);
        bits(m, p, context.wrapping_add(0x154), u32::MAX, 3);
    }
    let flags = p.u32(m, context.wrapping_add(0x154));
    let mode = u32::from(p.u8(m, primitive.wrapping_add(11)) & 0x3f);
    if (flags >> 21) & 0xff != mode {
        p.set_u32(
            m,
            context.wrapping_add(0x154),
            (flags & !0x1fe0_0000) | (mode << 21) | 1,
        );
    }
    let key = p.u32(m, primitive.wrapping_add(20));
    let (offset, value) = if key == 0x2f55_c03d || key == 0x4325_a03e {
        (0x2ea0, 0x5cc2_d5d4)
    } else {
        (0x2e98, 0x5d3)
    };
    select_state(m, p, context, 0x3090, offset, 0x5d3, value);
    let mode = (p.u32(m, primitive.wrapping_add(8)) >> 3) & 0x1f;
    if mode == 0 {
        0
    } else {
        let (first, second, key1, key2) = match mode {
            1 => (0x2680, 0x26c0, 0xf68a_c4d0, 0xc2ef_c4d8),
            2 => (0x2688, 0x26c8, 0x7723_14d1, 0x4346_14d9),
            3 | 4 => (0x2698, 0x26d8, 0x21d7_b4d3, 0x2177_04db),
            _ => (0x26a8, 0x26e8, 0x8a48_04d5, 0x8ae8_b4dd),
        };
        select_state(m, p, context, 0x2868, first, 0x4ce, key1);
        select_state(m, p, context, 0x28a8, second, 0x4d6, key2)
    }
}

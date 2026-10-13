//! L174 ABI-level material and dependency-key ports. Guest addresses and
//! constants below come from the cited Steam executable functions.
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu},
};

// Translated from 0x00E0E5E0 (DDDA.exe 2017-12-13 Steam).
/// Salted word FNV-1; byte tails are ignored. Keep the three original
/// spills because a valid input can point into those stack slots.
pub fn dependency_hash(memory: &mut dyn Mem, cpu: &mut Cpu) {
    let pages = Pages::of(memory);
    let entry_stack = cpu.esp;
    let mut hash = pages.u32(memory, entry_stack.wrapping_add(12));
    pages.set_u32(memory, entry_stack.wrapping_sub(4), cpu.esi);
    let count = pages.u32(memory, entry_stack.wrapping_add(8)) >> 2;
    if count != 0 {
        pages.set_u32(memory, entry_stack.wrapping_sub(8), cpu.ebx);
        pages.set_u32(memory, entry_stack.wrapping_sub(12), cpu.edi);
        let mut input = pages.u32(memory, entry_stack.wrapping_add(4));
        for _ in 0..count {
            // E0E603/E0E609: code-embedded multiplier and salt.
            let word = pages.u32(memory, input) ^ 0x14da_14da;
            for shift in [0, 8, 16, 24] {
                hash = hash.wrapping_mul(0x0100_0193) ^ ((word >> shift) & 255);
            }
            input = input.wrapping_add(4);
        }
    }
    cpu.eax = hash;
    cpu.esp = entry_stack.wrapping_add(16);
}

fn dirty(memory: &mut dyn Mem, pages: Pages, draw: u32, bits: u32) {
    let value = pages.u32(memory, draw.wrapping_add(0x154));
    pages.set_u32(memory, draw.wrapping_add(0x154), value | bits);
}

/// Bind a state-bank entry, preserving the old pair in the command journal.
/// Source: 00FFCA2E..00FFCAD2 and 00FFCD1E..00FFCEDE.
fn bind_bank(m: &mut dyn Mem, p: Pages, draw: u32, slot: u32, bank_slot: u32, key: u32) {
    let bank = p.u32(m, draw + 0x20);
    if p.u32(m, draw + slot) != p.u32(m, bank + bank_slot) {
        save_pair(m, p, draw, slot, (slot - 0x1f8) / 8);
        let bank = p.u32(m, draw + 0x20);
        let value = p.u32(m, bank + bank_slot);
        p.set_u32(m, draw + slot, value);
        p.set_u32(m, draw + slot + 4, key);
        dirty(m, p, draw, 3);
    }
}

/// Stamp a resource and bind its salted key. The return preserves the
/// original incidental EAX value when the pointer comparison is equal.
/// Source: 00FFCAD9..00FFCCF2, 00FFD3D9..00FFD490.
fn bind_resource(m: &mut dyn Mem, p: Pages, draw: u32, slot: u32, resource: u32, salt: u32, eax: u32) -> u32 {
    if p.u32(m, draw + slot) == resource {
        return eax;
    }
    save_pair(m, p, draw, slot, (slot - 0x1f8) / 8);
    let key = if resource == 0 {
        p.set_u32(m, draw + slot, 0);
        0
    } else {
        let stamp = p.u32(m, 0x018d2ce8).wrapping_add(1);
        p.set_u32(m, resource + 4, stamp);
        p.set_u32(m, draw + slot, resource);
        p.u32(m, resource + 12) | salt
    };
    if p.u32(m, draw + slot + 4) != key {
        p.set_u32(m, draw + slot + 4, key);
        dirty(m, p, draw, 2);
    }
    dirty(m, p, draw, 1);
    key
}

// Translated from 0x00FFC9F0 (DDDA.exe 2017-12-13 Steam).
/// uModel::vf25: bind skinning resources, generate the transposed 3x4 joint
/// palette, and copy the model's final four parameters into draw storage.
/// The base setup, palette lock/unlock, binding, growth and memcpy remain
/// original guest calls. Matrix products retain scalar SSE operand order.
pub fn prepare_model_palette(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = Pages::of(m);
    let sp = cpu.esp;
    let frame = sp.wrapping_sub(0x4c);
    let model = cpu.ecx;
    let draw = p.u32(m, sp + 4);
    call(m, cpu, p, frame, 0xea5b50, model, &[draw], 0xffca06);
    let mut eax;
    let resource = p.u32(m, model + 0xf0);
    if p.u8(m, draw + 0x168) != 9 && resource != 0 && p.u32(m, resource + 0x220) != 0 {
        bind_bank(m, p, draw, 0x30a0, 0x2ed8, 0xd04f95db);
        // The bank pointer is also EAX after a changed binding.
        eax = p.u32(m, draw + 0x20);
        for (field, slot, salt) in [
            (0xe34, 0x1ad8, 0x298de000),
            (0xe3c, 0x1af0, 0x15232000),
            (0xe38, 0x1ae8, 0x282e9000),
        ] {
            let value = p.u32(m, model + field);
            eax = bind_resource(m, p, draw, slot, value, salt, eax);
        }
    } else {
        eax = p.u32(m, model + 0x360);
        if eax != 0 {
            let manager = p.u32(m, 0x018d08f4);
            let large = eax > p.u32(m, manager + 0xfc);
            bind_bank(
                m,
                p,
                draw,
                0x2840,
                if large { 0x2648 } else { 0x2668 },
                if large { 0x4c9 } else { 0xf139f4cd },
            );
            bind_bank(m, p, draw, 0x2848, 0x2650, 0x4ca);
            let resource = p.u32(m, model + 0xf0);
            let matrices = p.u32(m, resource + 0x6c);
            p.set_u32(m, sp + 4, matrices);
            let handle = p.u32(m, model + 0xd8c);
            let manager = p.u32(m, 0x018d2c80);
            call(m, cpu, p, frame, 0xfe46c0, manager, &[handle], 0xffcefd);
            if cpu.eax != 0 {
                let count = p.u32(m, model + 0x360);
                let transforms = p.u32(m, model + 0x364);
                palette_matrices(m, p, count, matrices, transforms, cpu.eax);
                let countdown = p.u32(m, model + 0xd80);
                if countdown != 0 {
                    p.set_u32(m, model + 0xd80, countdown - 1);
                }
                let handle = p.u32(m, model + 0xd8c);
                let manager = p.u32(m, 0x018d2c80);
                call(m, cpu, p, frame, 0xbfc7d0, manager, &[handle], 0xffd2fe);
            }
            let handle = p.u32(m, model + 0xd8c);
            let manager = p.u32(m, 0x018d2c80);
            call(m, cpu, p, frame, 0xfe4780, manager, &[draw, handle], 0xffd315);
            let alternate = if p.u32(m, model + 0x108) & 0x20000000 != 0 {
                let manager = p.u32(m, 0x018d08f4);
                let index = p.u32(m, manager + 0x33db8);
                p.u32(m, model.wrapping_add(0xe2c).wrapping_add(index.wrapping_mul(4))) != 0
            } else {
                false
            };
            bind_bank(
                m,
                p,
                draw,
                0x30d8,
                if alternate { 0x2ee8 } else { 0x2ee0 },
                if alternate { 0xf32275dd } else { 0x5dc },
            );
            if alternate {
                let manager = p.u32(m, 0x018d08f4);
                let index = p.u32(m, manager + 0x33db8);
                let texture = p.u32(m, model.wrapping_add(0xe2c).wrapping_add(index.wrapping_mul(4)));
                bind_resource(m, p, draw, 0x1d18, texture, 0x1c967000, 0);
            }
            let layout = p.u32(m, draw + 0x18);
            let descriptor = p.u32(m, layout + 0x8c4);
            p.set_u32(m, sp + 4, descriptor);
            let size = (u32::from(p.u16(m, descriptor + 0x18)) * 4 + 15) & !15;
            eax = p.u32(m, draw + 0x24).wrapping_add(size);
            if eax > p.u32(m, draw + 0x28) {
                call(m, cpu, p, frame, 0xfed890, draw, &[size], 0xffd561);
                eax = cpu.eax;
            }
            let output = p.u32(m, draw + 0x24);
            let allocation_end = output.wrapping_add(size);
            p.set_u32(m, draw + 0x24, allocation_end);
            p.set_u32(m, draw + 0x1380, output);
            dirty(m, p, draw, 1);
            if output != 0 {
                let descriptor = p.u32(m, sp + 4);
                let size = u32::from(p.u16(m, descriptor + 0x18)) * 4;
                let source = p.u32(m, descriptor + 0x24);
                // ECX is the post-allocation cursor at this cdecl seam.
                call(
                    m,
                    cpu,
                    p,
                    frame,
                    0x12efea0,
                    allocation_end,
                    &[output, source, size],
                    0xffd592,
                );
                for offset in [0, 4, 8, 12] {
                    cpu.st.real_memory::<0x00d9, 4, false>(m, model + 0xe1c + offset);
                    cpu.st.real_memory::<0x18d9, 4, true>(m, output + offset);
                }
                eax = model;
            }
        }
    }
    cpu.eax = eax;
    cpu.esp = sp.wrapping_add(8);
}

/// The palette loop of 0x00FFC9F0: for each of `count` joints, joint matrix
/// `i` (64 bytes from `matrices`) times joint record `i` (0xA0 bytes from
/// `transforms`) into the 48-byte palette entry `i` of `output`.
pub fn palette_matrices(m: &mut dyn Mem, p: Pages, count: u32, matrices: u32, transforms: u32, output: u32) {
    for i in 0..count {
        palette_matrix(m, p, matrices.wrapping_add(i.wrapping_mul(64)), transforms.wrapping_add(i.wrapping_mul(160)), output.wrapping_add(i.wrapping_mul(48)));
    }
}

/// 00FFCF30..00FFD2C0: matrix rows become palette columns. Load both inputs
/// before any output store, preserving overlap within the current matrix.
fn palette_matrix(m: &mut dyn Mem, p: Pages, joints: u32, transform: u32, output: u32) {
    use ddnative_base::mem::{store_at, READ, WRITE};
    // Fast path: the 64-byte joint matrix, the 64 bytes of transform rows
    // and the 48-byte output each inside one page: one permission check per
    // block instead of one per word, and the twelve dot products as three
    // packed columns (see [`palette_block`]).
    if let (Some(jp), Some(tp), Some(op)) = (p.bytes(joints, 64, READ), p.bytes(transform.wrapping_add(16), 64, READ), p.bytes(output, 48, WRITE)) {
        // SAFETY: each pointer covers its block inside one backed page; every
        // input is read before the first output word is stored.
        unsafe {
            let result = palette_block(jp, tp);
            for i in [2, 0, 1, 3, 5, 6, 7, 4, 8, 9, 10, 11] {
                store_at(op.add(4 * i), 4, result[i]);
            }
        }
        return;
    }
    palette_matrix_reference(m, p, joints, transform, output)
}

/// The twelve palette words of one joint from the 4x4 joint matrix at `jp`
/// (64 bytes) and the four transform rows at `tp` (3 used words of each
/// 16-byte row): output word `column * 4 + row` is the scalar-SSE dot
/// product `((a[row][0] * b[0][column] + a[row][1] * b[1][column]) +
/// a[row][2] * b[2][column]) + a[row][3] * b[3][column]` with the original's
/// operand order (the matrix element first, except in row 0's second term).
/// The four rows of a column run as the four lanes of `mulps`/`addps`,
/// which perform the same IEEE operations per lane under the live MXCSR,
/// so results (and NaN payload selection, which prefers the first operand)
/// are those of the scalar code.
///
/// # Safety
/// `jp` and `tp` must point at 64 readable bytes each.
#[cfg(target_arch = "x86_64")]
unsafe fn palette_block(jp: *const u8, tp: *const u8) -> [u32; 12] {
    use std::arch::x86_64::*;
    unsafe {
        let r0 = _mm_loadu_ps(jp as *const f32);
        let r1 = _mm_loadu_ps(jp.add(16) as *const f32);
        let r2 = _mm_loadu_ps(jp.add(32) as *const f32);
        let r3 = _mm_loadu_ps(jp.add(48) as *const f32);
        // transpose: column k of the joint matrix as (row0..row3)
        let t0 = _mm_unpacklo_ps(r0, r1);
        let t1 = _mm_unpacklo_ps(r2, r3);
        let t2 = _mm_unpackhi_ps(r0, r1);
        let t3 = _mm_unpackhi_ps(r2, r3);
        let a0 = _mm_movelh_ps(t0, t1);
        let a1 = _mm_movehl_ps(t1, t0);
        let a2 = _mm_movelh_ps(t2, t3);
        let a3 = _mm_movehl_ps(t3, t2);
        let mut out = [0u32; 12];
        for column in 0..3usize {
            let broadcast = |i: usize| _mm_castsi128_ps(_mm_set1_epi32((tp.add(16 * i + 4 * column) as *const i32).read_unaligned()));
            let (b0, b1, b2, b3) = (broadcast(0), broadcast(1), broadcast(2), broadcast(3));
            // row 0 multiplies (b1, a[0][1]) in that order; the others (a[row][1], b1)
            let left1 = _mm_move_ss(a1, b1);
            let right1 = _mm_move_ss(b1, a1);
            let mut accumulator = a0;
            let mut second = left1;
            let mut third = a2;
            let mut fourth = a3;
            std::arch::asm!(
                "mulps {acc}, {r0}", "mulps {second}, {r1}", "addps {acc}, {second}",
                "mulps {third}, {r2}", "addps {acc}, {third}",
                "mulps {fourth}, {r3}", "addps {acc}, {fourth}",
                acc = inout(xmm_reg) accumulator, second = inout(xmm_reg) second,
                third = inout(xmm_reg) third, fourth = inout(xmm_reg) fourth,
                r0 = in(xmm_reg) b0, r1 = in(xmm_reg) right1, r2 = in(xmm_reg) b2, r3 = in(xmm_reg) b3,
                options(nostack, preserves_flags, pure, nomem));
            _mm_storeu_ps(out.as_mut_ptr().add(4 * column) as *mut f32, accumulator);
        }
        out
    }
}

/// The per-word version of [`palette_matrix`] (before the block fast path),
/// kept for benchmarks and as a cross-check.
#[doc(hidden)]
pub fn palette_matrix_reference(m: &mut dyn Mem, p: Pages, joints: u32, transform: u32, output: u32) {
    let a: [u32; 16] = std::array::from_fn(|i| p.u32(m, joints + i as u32 * 4));
    let b: [[u32; 3]; 4] = std::array::from_fn(|i| std::array::from_fn(|j| p.u32(m, transform + 16 + i as u32 * 16 + j as u32 * 4)));
    let mut result = [0; 12];
    for row in 0..4 {
        for column in 0..3 {
            let (second_left, second_right) = if row == 0 { (b[1][column], a[1]) } else { (a[row * 4 + 1], b[1][column]) };
            result[column * 4 + row] = palette_dot([a[row * 4], second_left, a[row * 4 + 2], a[row * 4 + 3]], [b[0][column], second_right, b[2][column], b[3][column]]);
        }
    }
    for i in [2, 0, 1, 3, 5, 6, 7, 4, 8, 9, 10, 11] {
        p.set_u32(m, output + i as u32 * 4, result[i]);
    }
}

/// Four scalar SSE products, summed left to right without contraction or
/// compiler constant folding. The live guest MXCSR controls each operation;
/// keeping both operands explicit also preserves NaN payload selection.
#[inline]
fn palette_dot(left: [u32; 4], right: [u32; 4]) -> u32 {
    let result: f32;
    unsafe {
        std::arch::asm!(
            "mulss {a}, {x}", "mulss {b}, {y}", "addss {a}, {b}",
            "mulss {c}, {z}", "addss {a}, {c}",
            "mulss {d}, {w}", "addss {a}, {d}",
            a=inout(xmm_reg) f32::from_bits(left[0])=>result,
            b=inout(xmm_reg) f32::from_bits(left[1])=>_,
            c=inout(xmm_reg) f32::from_bits(left[2])=>_,
            d=inout(xmm_reg) f32::from_bits(left[3])=>_,
            x=in(xmm_reg) f32::from_bits(right[0]),y=in(xmm_reg) f32::from_bits(right[1]),
            z=in(xmm_reg) f32::from_bits(right[2]),w=in(xmm_reg) f32::from_bits(right[3]),
            options(nostack,preserves_flags));
    }
    result.to_bits()
}

/// 011401D4..0114023C: the scalar shader-key journal has its high bit set
/// and stores one value in the third word, unlike the paired value bank.
fn bind_shader_key(m: &mut dyn Mem, p: Pages, draw: u32, key: u32) {
    if p.u32(m, draw + 0x1c8) == key {
        return;
    }
    if p.u8(m, draw + 0x5c) & 0x80 != 0 {
        let record = |m: &dyn Mem| {
            draw.wrapping_add(0x30c7c)
                .wrapping_add(p.u32(m, draw + 0x6c).wrapping_mul(12))
        };
        let a = record(m);
        let old = p.u32(m, a);
        p.set_u32(m, a, (old & 0x80000003) | 3);
        let a = record(m);
        let old = p.u32(m, a);
        p.set_u32(m, a, old | 0x80000000);
        let a = record(m);
        let old = p.u32(m, draw + 0x1c8);
        p.set_u32(m, a + 8, old);
        let cursor = p.u32(m, draw + 0x6c);
        p.set_u32(m, draw + 0x6c, cursor.wrapping_add(1));
    }
    dirty(m, p, draw, 3);
    p.set_u32(m, draw + 0x1c8, key);
}

// Translated from 0x01140180 (DDDA.exe 2017-12-13 Steam).
/// cInstancingCulling::vf16: issue up to three visible instance ranges,
/// optionally repeat with the secondary material, then clear transient
/// state. Bounds, counts, sort keys and bindings remain runtime guest data.
pub fn draw_culled_instances(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = Pages::of(m);
    let sp = cpu.esp;
    let frame = ((sp.wrapping_sub(4)) & !15).wrapping_sub(0x60);
    let object = cpu.ecx;
    let draw = p.u32(m, sp + 4);
    let indexed = p.u32(m, draw + 0x1368) & !15;
    let table = 0x018d45c0u32.wrapping_add(p.u32(m, indexed).wrapping_mul(0x70));
    if p.u32(m, table + 0x60) == 0 {
        cpu.eax = indexed;
        cpu.esp = sp.wrapping_add(16);
        return;
    }
    let primitive = p.u32(m, sp + 8);
    let mask = p.u8(m, primitive + 7) & 7;
    call(m, cpu, p, frame, 0xff1920, draw, &[], 0x11401d4);
    bind_shader_key(m, p, draw, 0x8771a00f);
    let key = p.u32(m, table + 0x64);
    let resource = p.u32(m, table + 0x60);
    if p.u32(m, draw + 0x144) != key || p.u32(m, draw + 0x130) != resource || p.u8(m, draw + 0x137) != 0x40 {
        if resource != 0 {
            let stamp = p.u32(m, 0x18d2ce8).wrapping_add(1);
            p.set_u32(m, resource + 4, stamp);
        }
        p.set_u32(m, draw + 0x130, resource);
        p.set_u8(m, draw + 0x137, 0x40);
        p.set_u32(m, draw + 0x144, key);
        p.set_u32(m, draw + 0x150, 1);
    }
    bind_bank(m, p, draw, 0x2ff8, 0x2e00, 0x5c0);
    let first_drawn = instance_ranges(m, cpu, p, frame, object, draw, primitive, table, mask, false);
    if first_drawn {
        call(m, cpu, p, frame, 0xff1920, draw, &[], 0x11405a5);
    }
    if p.u8(m, object + 0x26) != 0 {
        bind_bank(m, p, draw, 0x2ee0, 0x2cf0, 0x2399759e);
        let bank = p.u32(m, draw + 0x20);
        let value = p.u32(m, bank + 0x1138);
        if p.u32(m, draw + 0x1330) & !15 != value || p.u32(m, draw + 0x1334) != 0 {
            save_pair(m, p, draw, 0x1330, 0x227);
            p.set_u32(m, draw + 0x1330, value);
            if p.u32(m, draw + 0x1334) != 0 {
                p.set_u32(m, draw + 0x1334, 0);
                dirty(m, p, draw, 2);
            }
            dirty(m, p, draw, 1);
        }
        bind_bank(m, p, draw, 0x2eb0, 0x2cc8, 0x950a6599);
        let manager = p.u32(m, 0x18d2c80);
        let texture = p.u32(m, manager + 0x2cc);
        bind_resource(m, p, draw, 0x1cf0, texture, 0x45fbb000, 0);
        instance_parameter(m, cpu, p, frame, draw, true);
        bind_shader_key(m, p, draw, 0x8771a00f);
        let second_drawn = instance_ranges(m, cpu, p, frame, object, draw, primitive, table, mask, true);
        if second_drawn {
            call(m, cpu, p, frame, 0xff1920, draw, &[], 0x1140c32);
        }
        bind_bank(m, p, draw, 0x2eb0, 0x2cc8, 0x950a6599);
        instance_parameter(m, cpu, p, frame, draw, false);
        bind_bank(m, p, draw, 0x2ee0, 0x2ce8, 0x59d);
    }
    bind_shader_key(m, p, draw, 0);
    if p.u32(m, draw + 0x144) != 0 || p.u32(m, draw + 0x130) != 0 || p.u8(m, draw + 0x137) != 0 {
        p.set_u32(m, draw + 0x130, 0);
        p.set_u8(m, draw + 0x137, 0);
        p.set_u32(m, draw + 0x144, 0);
        p.set_u32(m, draw + 0x150, 1);
    }
    call(m, cpu, p, frame, 0xfeef20, draw, &[], 0x1140edb);
    cpu.esp = sp.wrapping_add(16);
}

/// 0114085E..01140938 / 01140CC1..01140D9E: copy the parameter template
/// through the original cdecl memcpy, then toggle its third float.
fn instance_parameter(m: &mut dyn Mem, cpu: &mut Cpu, p: Pages, frame: u32, draw: u32, enabled: bool) {
    let flags = p.u32(m, draw + 0x5c);
    p.set_u32(m, draw + 0x5c, flags | 2);
    save_pair(m, p, draw, 0x1330, 0x227);
    let layout = p.u32(m, draw + 0x18);
    let descriptor = p.u32(m, layout + 0x89c);
    let size = (u32::from(p.u16(m, descriptor + 0x18)) * 4 + 15) & !15;
    let end = p.u32(m, draw + 0x24).wrapping_add(size);
    let mut memcpy_this = end;
    if end > p.u32(m, draw + 0x28) {
        call(
            m,
            cpu,
            p,
            frame,
            0xfed890,
            draw,
            &[size],
            if enabled { 0x11408ec } else { 0x1140d63 },
        );
        memcpy_this = cpu.ecx;
    }
    let output = p.u32(m, draw + 0x24);
    p.set_u32(m, draw + 0x24, output.wrapping_add(size));
    p.set_u32(m, draw + 0x1330, output);
    dirty(m, p, draw, 1);
    if output != 0 {
        let size = u32::from(p.u16(m, descriptor + 0x18)) * 4;
        let source = p.u32(m, descriptor + 0x24);
        call(
            m,
            cpu,
            p,
            frame,
            0x12efea0,
            if enabled { size } else { memcpy_this },
            &[output, source, size],
            if enabled { 0x1140928 } else { 0x1140d93 },
        );
    }
    let value = if enabled { p.u32(m, 0x1406ce0) } else { 0 };
    p.set_u32(m, output + 8, value);
    let flags = p.u32(m, draw + 0x5c);
    p.set_u32(m, draw + 0x5c, flags & !2);
}

/// Both original three-range loops share the same bounds and draw layout;
/// the second selects the adjacent count/offset pair in each table row.
fn instance_ranges(
    m: &mut dyn Mem,
    cpu: &mut Cpu,
    p: Pages,
    frame: u32,
    object: u32,
    draw: u32,
    primitive: u32,
    table: u32,
    mask: u8,
    secondary: bool,
) -> bool {
    let mut drawn = false;
    for i in 0..3u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let pair = table + i * 8 + if secondary { 4 } else { 0 };
        let offset = u32::from(p.u16(m, pair + 2));
        let count = u32::from(p.u16(m, pair));
        if count == 0 {
            continue;
        }
        if drawn {
            call(
                m,
                cpu,
                p,
                frame,
                0xff1920,
                draw,
                &[],
                if secondary { 0x11409ff } else { 0x1140371 },
            );
        }
        let sort = p.u32(m, table + 0x20 + i * 4);
        let old = p.u32(m, draw + 0x164);
        p.set_u32(m, draw + 0x164, (old & 31) | (sort << 5));
        let sphere = table + 0x30 + i * 16;
        let radius = p.u32(m, sphere + 12);
        let center: [u32; 3] = std::array::from_fn(|j| p.u32(m, sphere + j as u32 * 4));
        let bounds = frame + 0x40;
        for j in 0..3 {
            p.set_u32(m, bounds + j as u32 * 4, x86::subss(center[j], radius));
        }
        p.set_u32(m, bounds + 12, 0);
        for j in 0..3 {
            p.set_u32(m, bounds + 16 + j as u32 * 4, x86::addss(radius, center[j]));
        }
        p.set_u32(m, bounds + 28, 0);
        let one = p.u32(m, 0x1406ce0);
        let manager = p.u32(m, 0x18d2c80);
        let vtable = p.u32(m, manager);
        let target = p.u32(m, vtable + 0x40);
        call(
            m,
            cpu,
            p,
            frame,
            target,
            manager,
            &[draw, 8, bounds, u32::MAX, u32::MAX, one, one, one],
            if secondary { 0x1140ab6 } else { 0x1140428 },
        );
        let manager = p.u32(m, 0x18d2c80);
        let vtable = p.u32(m, manager);
        let target = p.u32(m, vtable + 0x34);
        let first = p.u32(m, object + 0x20);
        let second = p.u32(m, object + 0x1c);
        call(
            m,
            cpu,
            p,
            frame,
            target,
            manager,
            &[draw, first, second, sphere],
            if secondary { 0x1140ad1 } else { 0x1140443 },
        );
        call(
            m,
            cpu,
            p,
            frame,
            0xfeef20,
            draw,
            &[],
            if secondary { 0x1140ad8 } else { 0x114044a },
        );
        let values = [
            p.u32(m, primitive + 0x18),
            p.u32(m, primitive + 0x1c),
            p.u32(m, primitive + 0x20),
        ];
        let flags = p.u32(m, draw + 0x5c);
        if flags & 0x100 != 0 || flags & 0x50 == 0 {
            let end = p.u32(m, draw + 0x24).wrapping_add(0x30);
            if end > p.u32(m, draw + 0x28) {
                call(
                    m,
                    cpu,
                    p,
                    frame,
                    0xfed890,
                    draw,
                    &[0x30],
                    if secondary { 0x1140b12 } else { 0x1140484 },
                );
            }
            instance_command(m, p, draw, count, offset, values);
        }
        drawn = true;
    }
    drawn
}

/// 01140484..0114055B / 01140B12..01140BE9: emit a 48-byte draw and its
/// eight-byte sort entry, preserving untouched command words and bitfields.
fn instance_command(m: &mut dyn Mem, p: Pages, draw: u32, count: u32, offset: u32, values: [u32; 3]) {
    let output = p.u32(m, draw + 0x24);
    p.set_u32(m, draw + 0x24, output.wrapping_add(0x30));
    let old = p.u32(m, output);
    let header = (old & 0xfff3ffff) | 0x30000;
    p.set_u32(m, output, header);
    let sequence = p.u32(m, draw + 0x74);
    p.set_u32(m, output, (header & !0x7fff) | (sequence & 0x7fff));
    let sequence = p.u32(m, draw + 0x74);
    p.set_u32(m, draw + 0x74, sequence.wrapping_add(1));
    let flag = if p.u8(m, draw + 0x5c) & 0x50 != 0 {
        0x8000
    } else {
        0
    };
    let old = p.u32(m, output);
    p.set_u32(m, output, (old & !0x8000) | flag);
    let records = p.u32(m, draw + 0x6c);
    let old = p.u32(m, output);
    p.set_u32(m, output, (old & 0xfffff) | (records << 20));
    for (dst, src) in [(8, 0x194), (12, 0x198), (16, 0x19c), (4, 0x1a0)] {
        let value = p.u32(m, draw + src);
        p.set_u32(m, output + dst, value);
    }
    for (dst, value) in [
        (20, values[1]),
        (24, values[0]),
        (28, values[2]),
        (32, count),
        (36, offset),
    ] {
        p.set_u32(m, output + dst, value);
    }
    let cursor = p.u32(m, draw + 0x48);
    let entries = p.u32(m, draw + 0x44);
    let sort = p.u32(m, draw + 0x164);
    let entry = entries.wrapping_add(cursor.wrapping_mul(8));
    p.set_u32(m, draw + 0x48, cursor.wrapping_add(1));
    let old = p.u32(m, entry);
    let high = (old & 0x7ffffff) | (sort << 27);
    p.set_u32(m, entry, high);
    let sort = p.u32(m, draw + 0x164);
    let base = p.u32(m, draw + 0x178);
    p.set_u32(m, entry + 4, output);
    p.set_u32(
        m,
        entry,
        (high & !0x7ffffff) | ((sort >> 5).wrapping_add(base) & 0x7ffffff),
    );
}

fn color_dirty(memory: &mut dyn Mem, pages: Pages, draw: u32) {
    let value = pages.u32(memory, draw.wrapping_add(0x114));
    pages.set_u32(memory, draw.wrapping_add(0x114), (value & 0xfff) | 0x1000);
}

/// Save a state pair in the original twelve-byte command record. Reload
/// the cursor at the same stores as the original (records may alias it).
/// Layout and masks: 010657D2..01065832 and the matching state-save blocks
/// at 01066D15, 01066DA6, 01066E8D and 01066F17 in the cited Steam build.
fn save_pair(memory: &mut dyn Mem, pages: Pages, draw: u32, offset: u32, tag: u32) {
    if pages.u8(memory, draw.wrapping_add(0x5c)) & 0x80 == 0 {
        return;
    }
    let record = |memory: &dyn Mem| {
        draw.wrapping_add(0x30c7c)
            .wrapping_add(pages.u32(memory, draw.wrapping_add(0x6c)).wrapping_mul(12))
    };
    let a = record(memory);
    let old = pages.u32(memory, a);
    pages.set_u32(memory, a, (old & (0x8000_0000 | tag)) | tag);
    let a = record(memory);
    let old = pages.u32(memory, a);
    pages.set_u32(memory, a, old & 0x7fff_ffff);
    let first = pages.u32(memory, draw.wrapping_add(offset));
    let a = record(memory);
    pages.set_u32(memory, a.wrapping_add(4), first);
    let second = pages.u32(memory, draw.wrapping_add(offset + 4));
    pages.set_u32(memory, a.wrapping_add(8), second);
    let cursor = pages.u32(memory, draw.wrapping_add(0x6c));
    pages.set_u32(memory, draw.wrapping_add(0x6c), cursor.wrapping_add(1));
}

fn call(
    memory: &mut dyn Mem,
    cpu: &mut Cpu,
    pages: Pages,
    frame: u32,
    target: u32,
    this: u32,
    args: &[u32],
    resume: u32,
) {
    cpu.esp = frame;
    for value in args.iter().rev().copied().chain(std::iter::once(resume)) {
        cpu.esp = cpu.esp.wrapping_sub(4);
        pages.set_u32(memory, cpu.esp, value);
    }
    cpu.ecx = this;
    crate::semantic::call_guest(memory, cpu, target);
}

// Translated from 0x010655A0 (DDDA.exe 2017-12-13 Steam).
/// Apply material state to a draw context, then call its original finalizer.
/// Material +0x10 is the dependency key; +0x14 packs render flags, +0x40
/// contains RGBA floats, and +0x30 addresses twelve-byte override records
/// (slot in the high twelve bits, then two value words). Draw +0x1f8 is
/// the eight-byte value bank; +0x154 contains its dirty/render flags.
pub fn apply_material(memory: &mut dyn Mem, cpu: &mut Cpu) {
    let pages = Pages::of(memory);
    let entry_stack = cpu.esp;
    let material = cpu.ecx;
    let draw = pages.u32(memory, entry_stack.wrapping_add(4));
    let variant = pages.u32(memory, entry_stack.wrapping_add(8));
    apply_material_state(memory, pages, material, draw, variant);
    call(
        memory,
        cpu,
        pages,
        entry_stack.wrapping_sub(12),
        0x0106_2f20,
        material,
        &[draw],
        0x0106_589c,
    );
    cpu.esp = entry_stack.wrapping_add(12);
}

fn apply_material_state(memory: &mut dyn Mem, pages: Pages, material: u32, draw: u32, variant: u32) {
    let stamp = pages.u32(memory, 0x018d_2ce8).wrapping_add(1);
    let key = pages.u32(memory, material.wrapping_add(0x10));
    pages.set_u32(memory, material.wrapping_add(4), stamp);
    if pages.u32(memory, draw.wrapping_add(0x1cc)) != key
        || pages.u8(memory, draw.wrapping_add(0x169)) as u32 != variant
        || pages.u32(memory, draw.wrapping_add(0x1d0)) != 0x10fd_ca55
    {
        pages.set_u32(memory, draw.wrapping_add(0x1cc), key);
        dirty(memory, pages, draw, 3);
        pages.set_u8(memory, draw.wrapping_add(0x169), variant as u8);
        pages.set_u32(memory, draw.wrapping_add(0x1d0), 0x10fd_ca55);
        pages.set_u16(memory, draw.wrapping_add(0x158), (key & 0xfff) as u16);
        pages.set_u16(memory, draw.wrapping_add(0x15a), variant as u16);
    }
    let bank = pages.u32(memory, material.wrapping_add(0x28));
    let mode = pages.u8(memory, material.wrapping_add(0x14));
    if pages.u32(memory, draw.wrapping_add(0x11c)) != bank
        || pages.u8(memory, draw.wrapping_add(0x114)) != mode
    {
        pages.set_u32(memory, draw.wrapping_add(0x11c), bank);
        pages.set_u8(memory, draw.wrapping_add(0x114), mode);
        color_dirty(memory, pages, draw);
    }
    // COMISS/JBE sends unordered inputs through MULSS, too. CVTTSS2SI
    // supplies the original indefinite result for NaNs and overflow.
    let one = f32::from_bits(pages.u32(memory, 0x0140_6ce0));
    let scale = f32::from_bits(pages.u32(memory, 0x0161_c298));
    let mut rgba = 0;
    for lane in 0..4 {
        let component = f32::from_bits(pages.u32(memory, material.wrapping_add(0x40 + lane * 4)));
        let scaled = if component > one { scale } else { component * scale };
        rgba |= (x86::cvttss2si(scaled.to_bits()) & 255) << (lane * 8);
    }
    let slot = pages.u32(memory, material.wrapping_add(0x2c)) & 0xfff;
    let texture = pages.u32(memory, draw.wrapping_add(0x1f8 + slot * 8));
    if pages.u32(memory, draw.wrapping_add(0x118)) != texture
        || pages.u32(memory, draw.wrapping_add(0x110)) != rgba
    {
        pages.set_u32(memory, draw.wrapping_add(0x110), rgba);
        pages.set_u32(memory, draw.wrapping_add(0x118), texture);
        color_dirty(memory, pages, draw);
    }
    let value = pages.u32(memory, material.wrapping_add(0x24));
    if pages.u32(memory, draw.wrapping_add(0x120)) != value {
        pages.set_u32(memory, draw.wrapping_add(0x120), value);
        color_dirty(memory, pages, draw);
    }
    let flags = pages.u32(memory, material.wrapping_add(0x14));
    let old = pages.u32(memory, draw.wrapping_add(0x154));
    if (old >> 20) & 1 != (flags >> 8) & 1
        || (old >> 3) & 255 != (flags >> 9) & 15
        || (old >> 11) & 255 != (flags >> 14) & 255
        || (old >> 19) & 1 != (flags >> 13) & 1
    {
        let value = ((flags >> 8) & 1) << 20
            | ((flags >> 13) & 1) << 19
            | ((flags >> 9) & 15) << 3
            | ((flags >> 14) & 255) << 11
            | (old & 0xffe0_0007)
            | 1;
        pages.set_u32(memory, draw.wrapping_add(0x154), value);
    }
    if pages.u32(memory, draw.wrapping_add(0x1c30)) != 0 {
        save_pair(memory, pages, draw, 0x1c30, 0x347);
        pages.set_u32(memory, draw.wrapping_add(0x1c30), 0);
        if pages.u32(memory, draw.wrapping_add(0x1c34)) != 0 {
            pages.set_u32(memory, draw.wrapping_add(0x1c34), 0);
            dirty(memory, pages, draw, 2);
        }
        dirty(memory, pages, draw, 1);
    }
    let mut source = pages.u32(memory, material.wrapping_add(0x30));
    let mut i = 0;
    while i < pages.u32(memory, material.wrapping_add(0x18)) & 0x1ff {
        let slot = pages.u32(memory, source) >> 20;
        let first = pages.u32(memory, source.wrapping_add(4));
        pages.set_u32(memory, draw.wrapping_add(0x1f8 + slot * 8), first);
        let second = pages.u32(memory, source.wrapping_add(8));
        pages.set_u32(memory, draw.wrapping_add(0x1fc + slot * 8), second);
        source = source.wrapping_add(12);
        i += 1;
    }
    dirty(memory, pages, draw, 3);
}

// Translated from 0x01066C60 (DDDA.exe 2017-12-13 Steam), nDraw::Material::vf12.
/// Select a material pass, preserving the original virtual/direct callees.
/// Draw +0x16a selects the material family, +0x1d4 resolves an optional
/// resource binding, and +0x3730/+0x1d90 are saved state pairs. Material
/// vf09 fills a temporary RGBA value before the opacity rejection test.
pub fn select_material_pass(memory: &mut dyn Mem, cpu: &mut Cpu) {
    let entry_stack = cpu.esp;
    let pages = Pages::of(memory);
    let material = cpu.ecx;
    let result = select_pass(memory, cpu, pages, entry_stack, material);
    cpu.eax = result;
    cpu.esp = entry_stack.wrapping_add(12);
}

fn select_pass(memory: &mut dyn Mem, cpu: &mut Cpu, pages: Pages, entry_stack: u32, material: u32) -> u32 {
    let frame = (entry_stack.wrapping_sub(4) & !15).wrapping_sub(0x20);
    let vtable = pages.u32(memory, material);
    let get_color = pages.u32(memory, vtable.wrapping_add(0x24));
    let mut pass = (pages.u32(memory, material.wrapping_add(0x14)) >> 26) & 31;
    call(
        memory,
        cpu,
        pages,
        frame,
        get_color,
        material,
        &[frame.wrapping_add(0x10)],
        0x0106_6c89,
    );
    let threshold = f32::from_bits(pages.u32(memory, 0x0140_8e58));
    let alpha = f32::from_bits(pages.u32(memory, cpu.eax.wrapping_add(12)));
    if threshold > alpha {
        return 0;
    }
    let requested = pages.u32(memory, entry_stack.wrapping_add(8));
    let draw = pages.u32(memory, entry_stack.wrapping_add(4));
    if (requested as i32) < 24 {
        pass = requested;
    }
    let selected = if pages.u16(memory, draw.wrapping_add(0x16a)) & 0x104 != 0 {
        6
    } else {
        pass
    };
    call(
        memory,
        cpu,
        pages,
        frame,
        0x00ff_1090,
        draw,
        &[selected, 0],
        0x0106_6cc3,
    );
    let owner = pages.u32(memory, draw.wrapping_add(0x1d4));
    let mut binding = 0;
    if owner != 0 {
        let resource = pages.u32(memory, owner.wrapping_add(0xb8));
        if resource != 0 {
            let indirect = pages.u32(memory, resource.wrapping_add(0x44));
            binding = pages.u32(memory, indirect);
        }
    }
    let special = (pass == 11 || pass == 12) && binding != 0;
    let bank_offset = if special { 0x3540 } else { 0x3538 };
    let bank = pages.u32(memory, draw.wrapping_add(0x20));
    if pages.u32(memory, draw.wrapping_add(0x3730)) != pages.u32(memory, bank.wrapping_add(bank_offset)) {
        save_pair(memory, pages, draw, 0x3730, 0x6a7);
        let bank = pages.u32(memory, draw.wrapping_add(0x20));
        let value = pages.u32(memory, bank.wrapping_add(bank_offset));
        pages.set_u32(memory, draw.wrapping_add(0x3730), value);
        pages.set_u32(
            memory,
            draw.wrapping_add(0x3734),
            if special { 0x49d9_b6a8 } else { 0x6a7 },
        );
        dirty(memory, pages, draw, 3);
    }
    let texture = if special {
        pages.u32(memory, binding.wrapping_add(0x10))
    } else {
        0
    };
    if pages.u32(memory, draw.wrapping_add(0x1d90)) != texture {
        save_pair(memory, pages, draw, 0x1d90, 0x373);
        let mut key = 0;
        if texture != 0 {
            let stamp = pages.u32(memory, 0x018d_2ce8).wrapping_add(1);
            pages.set_u32(memory, texture.wrapping_add(4), stamp);
            pages.set_u32(memory, draw.wrapping_add(0x1d90), texture);
            key = pages.u32(memory, texture.wrapping_add(12)) | 0x0a82_0000;
        } else {
            pages.set_u32(memory, draw.wrapping_add(0x1d90), 0);
        }
        if pages.u32(memory, draw.wrapping_add(0x1d94)) != key {
            pages.set_u32(memory, draw.wrapping_add(0x1d94), key);
            dirty(memory, pages, draw, 2);
        }
        dirty(memory, pages, draw, 1);
    }
    let flags = pages.u16(memory, draw.wrapping_add(0x16a)) as u32;
    if flags & 0x813 != 0 {
        let high = pages.u32(memory, material.wrapping_add(0x14)) >> 31;
        let manager = pages.u32(memory, 0x018d_2c80);
        let table = pages.u32(memory, manager);
        let target = pages.u32(memory, table.wrapping_add(0x38));
        call(
            memory,
            cpu,
            pages,
            frame,
            target,
            manager,
            &[draw, high],
            0x0106_6fa7,
        );
        call(
            memory,
            cpu,
            pages,
            frame,
            0x0106_55a0,
            material,
            &[draw, 0],
            0x0106_6fb1,
        );
        if pages.u32(memory, material.wrapping_add(0x18)) >> 31 != 0 {
            let texture = pages.u32(memory, draw.wrapping_add(0xd80));
            let color = pages.u32(memory, 0x0162_1120);
            if pages.u32(memory, draw.wrapping_add(0x118)) != texture
                || pages.u32(memory, draw.wrapping_add(0x110)) != color
            {
                pages.set_u32(memory, draw.wrapping_add(0x110), color);
                pages.set_u32(memory, draw.wrapping_add(0x118), texture);
                color_dirty(memory, pages, draw);
            }
        }
        if pass == 16 {
            if nonzero_factor(memory, pages, draw) {
                call(
                    memory,
                    cpu,
                    pages,
                    frame,
                    0x00ff_1090,
                    draw,
                    &[12, 0],
                    0x0106_703c,
                );
            }
        } else if pass == 11 || pass == 12 {
            // JBE also returns immediately for unordered alpha.
            if below_one(memory, pages, draw) {
                let mut selected = 12;
                let mut resume = 0x0106_70e4;
                if !nonzero_factor(memory, pages, draw) {
                    selected = 16;
                    let texture = pages.u32(memory, draw.wrapping_add(0xd40));
                    let color = pages.u32(memory, 0x0162_113c);
                    if pages.u32(memory, draw.wrapping_add(0x118)) != texture
                        || pages.u32(memory, draw.wrapping_add(0x110)) != color
                    {
                        pages.set_u32(memory, draw.wrapping_add(0x110), color);
                        color_dirty(memory, pages, draw);
                        pages.set_u32(memory, draw.wrapping_add(0x118), texture);
                        resume = 0x0106_70c7;
                    }
                }
                call(
                    memory,
                    cpu,
                    pages,
                    frame,
                    0x00ff_1090,
                    draw,
                    &[selected, 0],
                    resume,
                );
            }
        }
        1
    } else {
        let route = if flags & 0x20 != 0 {
            Some((0x0106_6380, 0x0106_7100, [draw, pass, 0], 2))
        } else if flags & 0x1000 != 0 {
            Some((0x0106_5e40, 0x0106_7127, [draw, pass, 0], 2))
        } else if flags & 0x208 != 0 {
            Some((0x0106_58b0, 0x0106_7157, [draw, (!(flags >> 3)) & 1, pass], 3))
        } else if flags & 0x104 != 0 {
            if (pass == 11 || pass == 12) && below_one(memory, pages, draw) {
                pass = if nonzero_factor(memory, pages, draw) {
                    12
                } else {
                    16
                };
            } else if pass == 16 && nonzero_factor(memory, pages, draw) {
                pass = 12;
            }
            Some((0x0106_5b00, 0x0106_71dc, [draw, (!(flags >> 2)) & 1, pass], 3))
        } else {
            None
        };
        if let Some((target, resume, args, len)) = route {
            call(memory, cpu, pages, frame, target, material, &args[..len], resume);
            u32::from(cpu.eax as u8 != 0)
        } else {
            0
        }
    }
}

fn below_one(memory: &dyn Mem, pages: Pages, draw: u32) -> bool {
    f32::from_bits(pages.u32(memory, 0x0140_6ce0))
        > f32::from_bits(pages.u32(memory, draw.wrapping_add(0x17c)))
}

fn nonzero_factor(memory: &dyn Mem, pages: Pages, draw: u32) -> bool {
    let factor = pages.u32(memory, draw.wrapping_add(0x1330)) & !15;
    f32::from_bits(pages.u32(memory, factor.wrapping_add(8)))
        != f32::from_bits(pages.u32(memory, 0x0156_1738))
}

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
    x86::call_ext(m, c, target);
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

fn bits(m: &mut dyn Mem, p: Pages, address: u32, keep: u32, add: u32) {
    let value = (p.u32(m, address) & keep) | add;
    p.set_u32(m, address, value);
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
        x86::call_ext(m, c, 0x00fe_d890);
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
        let mut same = x86::comiss_hw::<false>(p.u32(m, model.wrapping_add(0x1d0)), first_default).r == 0;
        for i in 1..4 {
            if !same {
                break;
            }
            same = x86::comiss_hw::<false>(
                p.u32(m, model.wrapping_add(0x1d0 + i * 4)),
                p.u32(m, 0x018c_fe70 + i * 4),
            )
            .r == 0;
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

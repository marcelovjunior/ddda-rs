//! Dependency traversal and geometry dispatch on the main/job update paths.
//! The original unrolls five levels before recursing. Keep that layout:
//! its spill slots (including overwritten arguments) are observable at
//! the recursive call, and the output may alias the descriptor memory.

use crate::machine::{pop, push, ret};
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu},
};

/// Whole-machine ports verified by `tests/hot.rs` and the isolated lifted
/// replay. Separate from the original core-library vector catalogue.
pub const PORTS: &[(u32, x86::LiftFn, u8)] = &[
    (0x00e0_ec50, expand_dependencies, 0xff),
    (0x0117_87c0, dispatch_geometry, 0xff),
    (0x00cd_8760, resolve_identifier, 0xff),
    (0x0112_bb40, duration_u16, 0xff),
    (0x0112_bb60, duration_u8_at3, 0xff),
    (0x0112_bb80, duration_u8_at7, 0xff),
    (0x0112_bba0, duration_high_nibble32, 0xff),
    (0x0112_bbc0, duration_high_nibble16, 0xff),
    (0x0112_bbe0, duration_high_nibble8, 0xff),
    (0x00fe_db80, save_draw_recording_state, 0xff),
    (0x00fe_dc00, restore_draw_recording_state, 0xff),
    (0x00fe_d200, initialize_draw_recording_lanes, 0xff),
    (0x00e1_0fc0, intern_dependency_keys, 0xff),
    (0x0058_28e0, forward_three_records, 0xff),
    (0x0058_2900, forward_five_primary_records, 0xff),
    (0x0058_2920, forward_five_secondary_records, 0xff),
    (0x005a_bdc0, object_flag_16, 0xff),
    (0x0058_29a0, track_identifier_transition, 0xff),
];

// Translated from 0x005828e0, 0x00582900, 0x00582920
// (DDDA.exe 2017-12-13 Steam). Their count/table operands are code constants;
// the table contents remain in the installed image, read by 0x00582a10.
fn forward_record_table(m: &mut dyn Mem, c: &mut Cpu, count: u32, table: u32, resume: u32) {
    use ddnative_base::emit;
    let pt = m.page_table();
    c.eax = m.fast_u32(pt, c.esp.wrapping_add(8));
    c.edx = m.fast_u32(pt, c.esp.wrapping_add(4));
    emit::push(count, m, pt, c);
    emit::push(table, m, pt, c);
    emit::push(c.eax, m, pt, c);
    c.eax = m.fast_u32(pt, c.esp.wrapping_add(0x18));
    emit::push(c.edx, m, pt, c);
    emit::push(c.ecx, m, pt, c);
    emit::push(resume, m, pt, c);
    x86::call_ext(m, c, 0x582a10);
    ret(c, 12);
}

// Translated from 0x005828e0 (DDDA.exe 2017-12-13 Steam).
pub fn forward_three_records(m: &mut dyn Mem, c: &mut Cpu) {
    forward_record_table(m, c, 3, 0x1578008, 0x5828fb);
}

// Translated from 0x00582900 (DDDA.exe 2017-12-13 Steam).
pub fn forward_five_primary_records(m: &mut dyn Mem, c: &mut Cpu) {
    forward_record_table(m, c, 5, 0x1578020, 0x58291b);
}

// Translated from 0x00582920 (DDDA.exe 2017-12-13 Steam).
pub fn forward_five_secondary_records(m: &mut dyn Mem, c: &mut Cpu) {
    forward_record_table(m, c, 5, 0x1578048, 0x58293b);
}

// Translated from 0x005abdc0 (DDDA.exe 2017-12-13 Steam).
/// Test bit 16 at object+0x2364, retaining the original partial-AL result.
pub fn object_flag_16(m: &mut dyn Mem, c: &mut Cpu) {
    let pt = m.page_table();
    c.eax = m.fast_u32(pt, c.esp.wrapping_add(4));
    c.ecx = m.fast_u32(pt, c.eax.wrapping_add(0x2364)) & 0x10000;
    c.eax = c.fl.alu_or(0, c.ecx, 32);
    if c.eax == 0 {
        c.fl.alu_xor(0, 0, 8);
    } else {
        c.eax |= 1;
    }
    ret(c, 4);
}

// Translated from 0x005829a0 (DDDA.exe 2017-12-13 Steam).
/// Latch a matching identifier; report when the latched identifier changes.
pub fn track_identifier_transition(m: &mut dyn Mem, c: &mut Cpu) {
    use ddnative_base::emit;
    let pt = m.page_table();
    c.eax = m.fast_u32(pt, c.esp.wrapping_add(12));
    c.edx = m.fast_u32(pt, c.esp.wrapping_add(8));
    emit::push(c.ebx, m, pt, c);
    c.ebx = m.fast_u32(pt, c.esp.wrapping_add(8));
    c.ecx = c.edx.wrapping_add(c.eax.wrapping_mul(8)).wrapping_add(8);
    c.eax = m.fast_u32(pt, c.ecx);
    emit::push(c.esi, m, pt, c);
    c.esi = 0;
    emit::push(c.edi, m, pt, c);
    c.edi = m.fast_u32(pt, c.esp.wrapping_add(0x1c));
    let mut changed = false;
    let mut edi_restored = false;
    if c.eax == 0 {
        if c.edi != m.fast_u32(pt, c.ebx.wrapping_add(0x2dd4)) {
            c.edi = emit::pop(m, pt, c);
            edi_restored = true;
            for offset in [8, 16, 24, 32] {
                m.fast_set_u32(pt, c.edx.wrapping_add(offset), c.esi);
            }
        } else {
            m.fast_set_u32(pt, c.ecx, 1);
            // Reload: the state word may alias the owner's identifier.
            c.fl.alu_sub(c.edi, m.fast_u32(pt, c.ebx.wrapping_add(0x2dd4)), 32);
            changed = !c.fl.zf();
        }
    } else {
        c.eax = c.eax.wrapping_sub(1);
        if c.eax == 0 {
            c.fl.alu_sub(c.edi, m.fast_u32(pt, c.ebx.wrapping_add(0x2dd4)), 32);
            changed = !c.fl.zf();
        }
    }
    if !edi_restored {
        c.edi = emit::pop(m, pt, c);
    }
    c.esi = emit::pop(m, pt, c);
    if changed {
        c.eax = (c.eax & !255) | 1;
    } else {
        c.eax &= !255;
        c.fl.alu_xor(0, 0, 8);
    }
    c.ebx = emit::pop(m, pt, c);
    ret(c, 16);
}

// Translated from 0x00fedb80 (DDDA.exe 2017-12-13 Steam).
/// Save the recording state's variable-length descriptor and sort cursor,
/// then begin a nested state with the requested 16-bit identifier.
pub fn save_draw_recording_state(m: &mut dyn Mem, c: &mut Cpu) {
    use ddnative_base::emit;
    let pt = m.page_table();
    emit::push(c.esi, m, pt, c);
    c.esi = c.ecx;
    c.eax = m.fast_u32(pt, c.esi.wrapping_add(0x30874));
    c.ecx = m.fast_u32(pt, c.esp.wrapping_add(12));
    c.eax = c.esi.wrapping_add(c.eax.wrapping_mul(8)).wrapping_add(0x3084c);
    m.fast_set_u32(pt, c.eax, c.ecx);
    c.edx = m.fast_u32(pt, c.esi.wrapping_add(0x48));
    m.fast_set_u32(pt, c.eax.wrapping_add(4), c.edx);
    c.eax = m.fast_u32(pt, c.esi.wrapping_add(0x14));
    c.ecx = c.eax.wrapping_mul(8).wrapping_add(0x144);
    let depth = m.fast_u32(pt, c.esi.wrapping_add(0x30874));
    c.eax = depth.wrapping_mul(0x8144);
    c.fl = x86::Flags::new(x86::IMUL, depth, 0x8144, c.eax, 32);
    emit::push(c.ecx, m, pt, c);
    c.edx = c.esi.wrapping_add(0xb4);
    emit::push(c.edx, m, pt, c);
    c.ecx = c.eax.wrapping_add(c.esi).wrapping_add(0x81f8);
    emit::push(c.ecx, m, pt, c);
    emit::push(0xfedbc7, m, pt, c);
    x86::call_ext(m, c, 0x12efea0);
    c.edx = (c.edx & 0xffff0000) | u32::from(m.fast_u16(pt, c.esp.wrapping_add(0x14)));
    let depth = m.fast_u32(pt, c.esi.wrapping_add(0x30874));
    m.fast_set_u32(pt, c.esi.wrapping_add(0x30874), depth.wrapping_add(1));
    c.eax = 0;
    c.esp = c.fl.alu_add(c.esp, 12, 32);
    m.fast_set_u16(pt, c.esi.wrapping_add(0x16a), c.edx as u16);
    for offset in [0x1d4, 0x1d8, 0x1dc, 0x180] {
        m.fast_set_u32(pt, c.esi.wrapping_add(offset), 0);
    }
    c.esi = emit::pop(m, pt, c);
    ret(c, 8);
}

// Translated from 0x00fedc00 (DDDA.exe 2017-12-13 Steam).
/// Restore a nested recording state and append its command/sort references.
pub fn restore_draw_recording_state(m: &mut dyn Mem, c: &mut Cpu) {
    use ddnative_base::emit;
    let pt = m.page_table();
    macro_rules! r {
        ($a:expr) => {
            m.fast_u32(pt, $a)
        };
    }
    macro_rules! w {
        ($a:expr, $v:expr) => {
            m.fast_set_u32(pt, $a, $v)
        };
    }
    macro_rules! field {
        ($offset:expr) => {
            c.esi.wrapping_add($offset)
        };
    }
    macro_rules! push_fast {
        ($v:expr) => {
            emit::push($v, m, pt, c)
        };
    }
    macro_rules! call {
        ($target:expr, $resume:expr) => {{
            push_fast!($resume);
            x86::call_ext(m, c, $target);
        }};
    }
    push_fast!(c.ebx);
    push_fast!(c.ebp);
    push_fast!(c.esi);
    c.esi = c.ecx;
    w!(field!(0x30874), r!(field!(0x30874)).wrapping_sub(1));
    let depth = r!(field!(0x30874));
    c.ecx = r!(field!(0x14));
    c.eax = depth.wrapping_mul(0x8144);
    c.fl = x86::Flags::new(x86::IMUL, depth, 0x8144, c.eax, 32);
    push_fast!(c.edi);
    c.edx = c.ecx.wrapping_mul(8).wrapping_add(0x144);
    push_fast!(c.edx);
    c.eax = c.eax.wrapping_add(c.esi).wrapping_add(0x81f8);
    push_fast!(c.eax);
    c.ecx = field!(0xb4);
    push_fast!(c.ecx);
    call!(0x12efea0, 0xfedc37);
    c.edx = r!(field!(0x30874));
    c.edi = r!(field!(0x48)).wrapping_sub(r!(field!(c.edx.wrapping_mul(8).wrapping_add(0x30850))));
    c.eax = r!(field!(0x4c));
    c.ecx = r!(field!(0x44));
    c.ebx = field!(c.edx.wrapping_mul(8).wrapping_add(0x3084c));
    c.eax = c.fl.alu_sub(c.eax, c.edi, 32);
    c.ebp = c.ecx.wrapping_add(c.eax.wrapping_mul(8));
    w!(field!(0x4c), c.eax);
    c.edx = r!(c.ebx.wrapping_add(4));
    c.eax = c.edi.wrapping_mul(8);
    push_fast!(c.eax);
    c.eax = c.ecx.wrapping_add(c.edx.wrapping_mul(8));
    push_fast!(c.eax);
    push_fast!(c.ebp);
    call!(0x12efea0, 0xfedc71);
    c.edx = r!(field!(0x24)).wrapping_add(0x10);
    c.ecx = r!(c.ebx.wrapping_add(4));
    c.esp = c.esp.wrapping_add(0x18);
    w!(field!(0x48), c.ecx);
    c.fl.alu_sub(c.edx, r!(field!(0x28)), 32);
    if !c.fl.cf() && !c.fl.zf() {
        c.ecx = r!(0x18d08f4);
        push_fast!(0x1000);
        call!(0xda7d70, 0xfedc95);
        w!(field!(0x24), c.eax);
        if c.eax != 0 {
            c.eax = c.eax.wrapping_add(0x1000);
            w!(field!(0x28), c.eax);
        } else {
            w!(field!(0x28), 0);
        }
    }
    c.eax = r!(field!(0x24));
    c.ecx = c.eax.wrapping_add(0x10);
    w!(field!(0x24), c.ecx);
    c.ecx = (r!(c.eax) & 0xfff6ffff) | 0x60000;
    w!(c.eax, c.ecx);
    c.edx = r!(c.ebx);
    w!(c.eax.wrapping_add(8), c.ebp);
    w!(c.eax.wrapping_add(4), c.edx);
    w!(c.eax.wrapping_add(12), c.edi);
    c.edx = (r!(field!(0x6c)) << 20) | (c.ecx & 0xfffff);
    w!(c.eax, c.edx);
    c.ecx = ((r!(field!(0x74)) ^ c.edx) & 0x7fff) ^ c.edx;
    w!(c.eax, c.ecx);
    c.ecx = r!(field!(0x48));
    c.edx = r!(field!(0x44));
    c.ebx = r!(field!(0x164));
    c.edx = c.edx.wrapping_add(c.ecx.wrapping_mul(8));
    c.ecx = c.ecx.wrapping_add(1);
    w!(field!(0x48), c.ecx);
    c.edi = 1;
    w!(field!(0x74), r!(field!(0x74)).wrapping_add(c.edi));
    c.ecx = r!(c.edx);
    c.ebx <<= 27;
    c.ecx = (c.ecx & 0x7ffffff) | c.ebx;
    w!(c.edx, c.ecx);
    c.ebx = r!(field!(0x164));
    c.ebp = r!(field!(0x178));
    c.ebx >>= 5;
    c.ebp = ((c.ebp.wrapping_add(c.ebx) ^ c.ecx) & 0x7ffffff) ^ c.ecx;
    w!(c.edx, c.ebp);
    w!(c.edx.wrapping_add(4), c.eax);
    c.ecx = r!(field!(0x30c78));
    w!(field!(c.ecx.wrapping_mul(4).wrapping_add(0x30878)), c.eax);
    let count = c.fl.alu_add(r!(field!(0x30c78)), c.edi, 32);
    w!(field!(0x30c78), count);
    c.edi = emit::pop(m, pt, c);
    c.esi = emit::pop(m, pt, c);
    c.ebp = emit::pop(m, pt, c);
    c.ebx = emit::pop(m, pt, c);
    ret(c, 0);
}

// Translated from 0x00fed200 (DDDA.exe 2017-12-13 Steam).
/// Initialize each recording lane from the current draw/resource managers.
pub fn initialize_draw_recording_lanes(m: &mut dyn Mem, c: &mut Cpu) {
    use ddnative_base::emit;
    let pt = m.page_table();
    macro_rules! r {
        ($a:expr) => {
            m.fast_u32(pt, $a)
        };
    }
    macro_rules! w {
        ($a:expr, $v:expr) => {
            m.fast_set_u32(pt, $a, $v)
        };
    }
    macro_rules! field {
        ($offset:expr) => {
            c.esi.wrapping_add($offset)
        };
    }
    macro_rules! stack {
        ($offset:expr) => {
            c.esp.wrapping_add($offset)
        };
    }
    macro_rules! push_fast {
        ($v:expr) => {
            emit::push($v, m, pt, c)
        };
    }
    macro_rules! call {
        ($target:expr, $resume:expr) => {{
            push_fast!($resume);
            x86::call_ext(m, c, $target);
        }};
    }
    c.esp = c.esp.wrapping_sub(0x1c);
    c.eax = r!(c.ecx.wrapping_add(0x4c));
    w!(stack!(4), c.ecx);
    c.ecx = r!(stack!(0x24));
    c.edx = 0;
    (c.eax, c.edx) = x86::divide(c.eax, c.edx, c.ecx, 32, false).expect("x86 divide error");
    push_fast!(c.ebp);
    push_fast!(c.edi);
    c.edi = 0;
    c.ebp = 0;
    w!(stack!(0x10), c.eax);
    c.fl.alu_sub(c.ecx, c.edi, 32);
    if c.ecx != 0 {
        push_fast!(c.ebx);
        push_fast!(c.esi);
        c.esi = r!(stack!(0x30));
        for offset in [0x1c, 0x20, 0x24, 0x28, 0x10] {
            w!(stack!(offset), c.edi);
        }
        c.esi = c.esi.wrapping_add(0x18);
        loop {
            c.eax = r!(0x18d12cc);
            c.eax = c.fl.alu_add(c.eax, 0x3c, 32);
            w!(field!(8), c.eax);
            c.ecx = r!(0x18d12cc);
            c.edx = r!(c.ecx.wrapping_add(0x24));
            w!(c.esi, c.edx);
            c.eax = r!(0x18d12cc);
            c.ecx = r!(c.eax.wrapping_add(0x34));
            w!(c.esi.wrapping_sub(4), c.ecx);
            c.edx = r!(0x18d12cc);
            c.eax = r!(c.edx.wrapping_add(0x28));
            w!(field!(4), c.eax);
            c.ecx = r!(0x18d08f4);
            c.edx = r!(c.ecx.wrapping_add(0x433e04));
            w!(field!(0x18), c.edx);
            c.eax = r!(0x18d08f4);
            c.ecx = r!(c.eax.wrapping_add(0x433e08));
            push_fast!(0x30);
            c.ebx = field!(0x64);
            push_fast!(c.edi);
            push_fast!(c.ebx);
            w!(field!(0x24), c.ecx);
            for offset in [
                0x1c, 0x14, 0x28, 0x20, 0x10, 0xc, 0x34, 0x2c, 0x30, 0x3c, 0x3085c, 0x30c60, 0x38, 0x40, 0x44,
            ] {
                w!(field!(offset), c.edi);
            }
            call!(0x12efe20, 0xfed2ca);
            c.edx = r!(0x18d08f4);
            c.eax = r!(c.edx.wrapping_add(0x33dbc));
            c.ecx = 3;
            c.edx = c.eax % c.ecx;
            c.eax = r!(0x18d08f4);
            c.xmm[0] = emit::load64(m, pt, stack!(0x28));
            c.ecx = r!(c.eax.wrapping_add(c.edx.wrapping_mul(4)).wrapping_add(0x433dec));
            w!(c.ebx, c.ecx);
            c.ecx = r!(0x18d08f4);
            c.eax = r!(c.ecx.wrapping_add(0x33dbc));
            c.ebx = 3;
            c.edx = c.eax % c.ebx;
            c.eax = r!(stack!(0x20));
            c.ebx = r!(stack!(0x24));
            c.eax = c.eax.wrapping_add(0xb4);
            c.edx = r!(c.ecx.wrapping_add(c.edx.wrapping_mul(4)).wrapping_add(0x433df8));
            emit::store64(c.xmm[0], m, pt, c.esi.wrapping_sub(0x14));
            c.xmm[0] = emit::load64(m, pt, stack!(0x30));
            w!(field!(0x8c), c.edx);
            for offset in [0x94, 0x98, 0x33c64, 0x33c68] {
                w!(field!(offset), c.edi);
            }
            emit::store64(c.xmm[0], m, pt, c.esi.wrapping_sub(0xc));
            c.ecx = r!(c.eax.wrapping_sub(0x60));
            c.ecx = c.fl.alu_add(c.ecx, r!(stack!(0x1c)), 32);
            w!(field!(0x30), c.edi);
            w!(field!(0x2c), c.ecx);
            w!(field!(0x38), c.ebx);
            w!(field!(0x34), c.ebx);
            w!(field!(0x40), c.ebp);
            c.edx = r!(c.eax.wrapping_sub(0xa0));
            c.ecx = c.edx.wrapping_mul(8).wrapping_add(0x144);
            push_fast!(c.ecx);
            push_fast!(c.eax);
            c.edx = field!(0x9c);
            push_fast!(c.edx);
            call!(0x12efea0, 0xfed37b);
            c.esp = c.esp.wrapping_add(0x18);
            c.ebp = c.ebp.wrapping_add(1);
            c.ebx = c.ebx.wrapping_mul(8);
            w!(stack!(0x10), r!(stack!(0x10)).wrapping_add(c.ebx));
            c.esi = field!(0x33c84);
            c.fl.alu_sub(c.ebp, r!(stack!(0x34)), 32);
            if !c.fl.cf() {
                break;
            }
        }
        c.esi = emit::pop(m, pt, c);
        c.ebx = emit::pop(m, pt, c);
    }
    c.edi = emit::pop(m, pt, c);
    c.ebp = emit::pop(m, pt, c);
    c.esp = c.fl.alu_add(c.esp, 0x1c, 32);
    ret(c, 8);
}

// Translated from 0x00e10fc0 (DDDA.exe 2017-12-13 Steam).
/// Assemble dependency keys, optionally compute their original CRC, and
/// intern the key sequence. The dependency and interning callees stay intact.
pub fn intern_dependency_keys(m: &mut dyn Mem, c: &mut Cpu) {
    use ddnative_base::emit;
    let pt = m.page_table();
    macro_rules! r {
        ($a:expr) => {
            m.fast_u32(pt, $a)
        };
    }
    macro_rules! w {
        ($a:expr, $v:expr) => {
            m.fast_set_u32(pt, $a, $v)
        };
    }
    macro_rules! stack {
        ($offset:expr) => {
            c.esp.wrapping_add($offset)
        };
    }
    macro_rules! push_fast {
        ($v:expr) => {
            emit::push($v, m, pt, c)
        };
    }
    c.esp = c.esp.wrapping_sub(0x404);
    c.edx = r!(stack!(0x410));
    push_fast!(c.ebx);
    push_fast!(c.ebp);
    c.ebp = c.ecx;
    c.ecx = r!(c.edx);
    push_fast!(c.esi);
    push_fast!(c.edi);
    w!(stack!(0x10), c.ebp);
    c.eax = stack!(0x14);
    if c.ecx != 0 {
        c.ecx &= 0xfffff000;
        w!(stack!(0x14), c.ecx);
        c.eax = stack!(0x18);
    }
    for index in 1..4u32 {
        c.ecx = r!(c.edx.wrapping_add(index * 4));
        if c.ecx != 0 {
            c.ecx = (c.ecx & 0xfffff000) | index;
            w!(c.eax, c.ecx);
            c.eax = c.eax.wrapping_add(4);
        }
    }
    c.ecx = r!(stack!(0x424));
    c.edi = r!(stack!(0x418));
    c.esi = r!(stack!(0x41c));
    w!(c.eax, c.ecx);
    c.ecx = r!(c.edi);
    c.eax = c.eax.wrapping_add(4);
    for (offset, resume) in [
        (0, 0xe11058),
        (12, 0xe11069),
        (16, 0xe1107a),
        (8, 0xe1108b),
        (4, 0xe1109c),
        (20, 0xe110ad),
    ] {
        // The first value was read before advancing EAX. Later values
        // must be reloaded after callbacks, which may mutate the owner.
        let value = if offset == 0 {
            c.ecx
        } else {
            r!(c.edi.wrapping_add(offset))
        };
        if offset == 20 {
            c.edi = value;
        } else {
            c.ecx = value;
        }
        c.fl.alu_and(value, value, 32);
        if value != 0 {
            push_fast!(c.eax);
            push_fast!(c.esi);
            push_fast!(value);
            c.ecx = c.ebp;
            push_fast!(resume);
            x86::call_ext(m, c, 0xe0ec50);
        }
    }
    c.ebx = r!(stack!(0x428));
    c.edx = stack!(0x14);
    c.eax = (c.eax.wrapping_sub(c.edx) as i32 >> 2) as u32;
    c.edi = c.eax;
    c.fl.alu_and(c.ebx, c.ebx, 32);
    if c.ebx != 0 {
        c.edx = c.edi.wrapping_mul(4);
        c.eax = u32::MAX;
        // LEA scales by four: the original byte-at-a-time CRC arm
        // (E110D6) is unreachable, even when the arithmetic wraps.
        c.edx >>= 2;
        c.esi = c.edx;
        c.edx = 0;
        c.fl.alu_and(c.esi, c.esi, 32);
        while c.edx < c.esi {
            c.ecx = r!(stack!(0x14).wrapping_add(c.edx.wrapping_mul(4)));
            for shift in [24, 16, 8] {
                c.ebp = ((c.ecx >> shift) ^ c.eax) & 255;
                c.eax = (c.eax >> 8) ^ r!(0x14075c0u32.wrapping_add(c.ebp * 4));
            }
            c.edx = c.edx.wrapping_add(1);
            c.ecx = (c.ecx ^ c.eax) & 255;
            c.eax = (c.eax >> 8) ^ r!(0x14075c0u32.wrapping_add(c.ecx * 4));
            c.fl.alu_sub(c.edx, c.esi, 32);
        }
        if c.esi != 0 {
            c.ebp = r!(stack!(0x10));
        }
        w!(c.ebx, c.eax);
    }
    push_fast!(0x811c9dc5);
    c.eax = c.edi.wrapping_mul(4);
    push_fast!(c.eax);
    c.eax = stack!(0x1c);
    push_fast!(c.eax);
    c.ecx = c.ebp;
    push_fast!(0xe1118f);
    x86::call_ext(m, c, 0xe0e5e0);
    c.edi = emit::pop(m, pt, c);
    c.esi = emit::pop(m, pt, c);
    c.ebp = emit::pop(m, pt, c);
    c.ebx = emit::pop(m, pt, c);
    c.esp = c.fl.alu_add(c.esp, 0x404, 32);
    ret(c, 0x14);
}

// Translated from 0x011787c0 (DDDA.exe 2017-12-13 Steam).
/// Dispatch a geometry query, constructing the original temporary AABB
/// for type 7. Both dispatch tables and SIMD masks stay in guest memory.
pub fn dispatch_geometry(m: &mut dyn Mem, c: &mut Cpu) {
    let pt = m.page_table();
    macro_rules! push_fast {
        ($v:expr) => {
            ddnative_base::emit::push($v, m, pt, c)
        };
    }
    macro_rules! r {
        ($a:expr) => {
            m.fast_u32(pt, $a)
        };
    }
    macro_rules! w {
        ($a:expr, $v:expr) => {
            m.fast_set_u32(pt, $a, $v)
        };
    }
    macro_rules! s {
        ($o:expr) => {
            c.esp.wrapping_add($o)
        };
    }
    macro_rules! arg {
        ($o:expr) => {
            r!(c.ebp.wrapping_add($o))
        };
    }
    macro_rules! scalar {
        ($x:expr, $a:expr) => {
            c.xmm[$x] = [r!($a), 0, 0, 0]
        };
    }
    macro_rules! vector {
        ($x:expr, $a:expr) => {{
            let a: u32 = $a;
            c.xmm[$x] = std::array::from_fn(|i| r!(a.wrapping_add(i as u32 * 4)));
        }};
    }
    push_fast!(c.ebp);
    c.ebp = c.esp;
    c.esp &= !15;
    c.eax = arg!(8);
    c.edx = r!(c.eax.wrapping_add(4)).wrapping_sub(2);
    c.esp = c.esp.wrapping_sub(0x40);
    let alternate = m.fast_u8(pt, c.ebp.wrapping_add(0x18)) != 0;
    c.fl.alu_sub(c.edx, 0x13, 32);
    if c.edx <= 0x13 {
        let (indices, targets) = if alternate {
            (0x1178b4c, 0x1178b2c)
        } else {
            (0x1178b80, 0x1178b60)
        };
        c.edx = m.fast_u8(pt, indices + c.edx) as u32;
        let destination = r!(targets + c.edx * 4);
        // These are instruction destinations of the two original switches.
        let (target, return_to) = match destination {
            0x11787f3 => (0x11731b0, 0x11788b9),
            0x11788bf => (0x1173520, 0x11788d9),
            0x11788df => (0x1173800, 0x11788f9),
            0x11788ff => (0x1173ae0, 0x1178919),
            0x117891f => (0x1173dc0, 0x1178939),
            0x117893f => (0x11740a0, 0x1178959),
            0x117895f => (0x1174380, 0x1178979),
            0x1178996 => (0x1174730, 0x1178a5c),
            0x1178a62 => (0x1174aa0, 0x1178a7c),
            0x1178a82 => (0x1174d80, 0x1178a9c),
            0x1178aa2 => (0x1175060, 0x1178abc),
            0x1178ac2 => (0x1175340, 0x1178adc),
            0x1178ae2 => (0x1175620, 0x1178afc),
            0x1178b02 => (0x1175900, 0x1178b1c),
            0x1178b22 => (0, 0),
            _ => panic!("invalid geometry dispatch destination {destination:x}"),
        };
        if target != 0 {
            if matches!(destination, 0x11787f3 | 0x1178996) {
                scalar!(0, 0x162adb0);
                vector!(3, c.eax.wrapping_add(0x10));
                c.edx = arg!(0x14);
                w!(s!(0x18), c.xmm[0][0]);
                vector!(0, 0x162ac60);
                c.xmm[2] = c.xmm[0];
                c.xmm[1] = c.xmm[0];
                for i in 0..4 {
                    c.xmm[1][i] &= r!(0x18cfe80 + i as u32 * 4);
                    c.xmm[2][i] = !c.xmm[2][i] & c.xmm[3][i];
                }
                vector!(3, c.eax.wrapping_add(0x20));
                c.eax = arg!(0x1c);
                for i in 0..4 {
                    c.xmm[2][i] |= c.xmm[1][i];
                }
                push_fast!(c.eax);
                c.eax = arg!(0x10);
                for i in 0..4 {
                    w!(s!(4 + i as u32 * 4), c.xmm[2][i]);
                }
                scalar!(2, s!(4));
                push_fast!(c.edx);
                c.edx = arg!(0xc);
                w!(s!(0x28), c.xmm[2][0]);
                scalar!(2, s!(0xc));
                for i in 0..4 {
                    c.xmm[0][i] = (!c.xmm[0][i] & c.xmm[3][i]) | c.xmm[1][i];
                }
                push_fast!(c.eax);
                w!(s!(0x30), c.xmm[2][0]);
                scalar!(2, s!(0x14));
                for i in 0..4 {
                    w!(s!(0xc + i as u32 * 4), c.xmm[0][i]);
                }
                scalar!(0, s!(0xc));
                push_fast!(c.edx);
                w!(s!(0x40), c.xmm[0][0]);
                scalar!(0, s!(0x14));
                c.eax = s!(0x30);
                push_fast!(c.eax);
                c.edx = s!(0x24);
                w!(s!(0x3c), c.xmm[2][0]);
                c.xmm[2] = [0; 4];
                w!(s!(0x48), c.xmm[0][0]);
                scalar!(0, s!(0x1c));
                push_fast!(c.edx);
                w!(s!(0x2c), 7);
                w!(s!(0x28), 0x1594890); // AABB vtable immediate at 117889A/1178A3D.
                w!(s!(0x44), 0);
                w!(s!(0x50), c.xmm[0][0]);
                w!(s!(0x54), 0);
            } else {
                for offset in [0x1c, 0x14, 0x10, 0xc] {
                    c.edx = arg!(offset);
                    push_fast!(c.edx);
                }
                c.edx = c.eax.wrapping_add(0x10);
                push_fast!(c.edx);
                push_fast!(c.eax);
            }
            push_fast!(return_to);
            x86::call_ext(m, c, target);
            c.esp = c.ebp;
            c.ebp = ddnative_base::emit::pop(m, pt, c);
            ret(c, 0x18);
            return;
        }
    }
    c.eax = c.fl.alu_xor(c.eax, c.eax, 32);
    c.esp = c.ebp;
    c.ebp = ddnative_base::emit::pop(m, pt, c);
    ret(c, 0x18);
}

// Translated from 0x00cd8760 (DDDA.exe 2017-12-13 Steam).
/// Resolve a packed two-u16 identifier using the owner's virtual count and
/// indexed-entry methods. EAX supplies the identifier and ESI the owner.
pub fn resolve_identifier(m: &mut dyn Mem, c: &mut Cpu) {
    let pt = m.page_table();
    macro_rules! r {
        ($a:expr) => {
            m.fast_u32(pt, $a)
        };
    }
    macro_rules! push_fast {
        ($v:expr) => {
            ddnative_base::emit::push($v, m, pt, c)
        };
    }
    macro_rules! call {
        ($return_to:expr) => {{
            let target = c.edx;
            push_fast!($return_to);
            x86::call_ext(m, c, target);
        }};
    }
    if (c.eax as i32) < 0 {
        c.eax = c.fl.alu_xor(c.eax, c.eax, 32);
        ret(c, 0);
        return;
    }
    push_fast!(c.ebx);
    push_fast!(c.ebp);
    c.ebx = c.eax >> 16;
    c.eax &= 0xffff;
    c.ebp = c.eax;
    if (0x62..=0x7a).contains(&c.ebx) {
        c.eax = r!(c.esi);
        c.edx = r!(c.eax.wrapping_add(0x40));
        push_fast!(c.edi);
        c.ecx = c.esi;
        c.edi = c.fl.alu_xor(c.edi, c.edi, 32);
        call!(0xcd879b);
        c.fl.alu_and(c.eax, c.eax, 32);
        if c.eax != 0 {
            loop {
                c.eax = r!(c.esi);
                c.edx = r!(c.eax.wrapping_add(0x3c));
                push_fast!(c.edi);
                c.ecx = c.esi;
                call!(0xcd87aa);
                c.fl.alu_and(c.eax, c.eax, 32);
                if c.eax != 0 {
                    c.ecx = m.fast_u16(pt, c.eax.wrapping_add(0x10)) as u32;
                    c.fl.alu_sub(c.ecx, c.ebx, 32);
                    if c.ecx == c.ebx {
                        c.edx = m.fast_u16(pt, c.eax.wrapping_add(0x12)) as u32;
                        c.fl.alu_sub(c.edx, c.ebp, 32);
                        if c.edx == c.ebp {
                            c.edi = ddnative_base::emit::pop(m, pt, c);
                            c.ebp = ddnative_base::emit::pop(m, pt, c);
                            c.ebx = ddnative_base::emit::pop(m, pt, c);
                            ret(c, 0);
                            return;
                        }
                    }
                }
                c.eax = r!(c.esi);
                c.edx = r!(c.eax.wrapping_add(0x40));
                c.ecx = c.esi;
                // INC preserves CF from the entry/type/id comparison.
                let next = c.edi.wrapping_add(1);
                c.fl.inc_dec(x86::INC, c.edi, next, 32);
                c.edi = next;
                call!(0xcd87c8);
                c.fl.alu_sub(c.edi, c.eax, 32);
                if c.edi >= c.eax {
                    break;
                }
            }
        }
        c.eax = c.fl.alu_xor(c.eax, c.eax, 32);
        c.edi = ddnative_base::emit::pop(m, pt, c);
    } else {
        c.eax = c.fl.alu_xor(c.eax, c.eax, 32);
    }
    c.ebp = ddnative_base::emit::pop(m, pt, c);
    c.ebx = ddnative_base::emit::pop(m, pt, c);
    ret(c, 0);
}

/// Duration readers from the animation format table at 0x01441cf0.
/// They overwrite the caller's argument slot and return an integer in ST0.
/// These entries were absent from the function inventory and emulated in c17.
macro_rules! duration_reader {
    ($name:ident, $address:literal, $kind:expr) => {
        #[doc = concat!("Translated from ", $address, " (DDDA.exe 2017-12-13 Steam).")]
        pub fn $name(m: &mut dyn Mem, c: &mut Cpu) {
            read_duration::<$kind>(m, c);
        }
    };
}
duration_reader!(duration_u16, "0x0112bb40", 0);
duration_reader!(duration_u8_at3, "0x0112bb60", 1);
duration_reader!(duration_u8_at7, "0x0112bb80", 2);
duration_reader!(duration_high_nibble32, "0x0112bba0", 3);
duration_reader!(duration_high_nibble16, "0x0112bbc0", 4);
duration_reader!(duration_high_nibble8, "0x0112bbe0", 5);

// Translated from 0x0112bb40/60/80/a0/c0/e0 (DDDA.exe 2017-12-13 Steam).
fn read_duration<const KIND: u8>(m: &mut dyn Mem, c: &mut Cpu) {
    let pt = m.page_table();
    let slot = c.esp.wrapping_add(4);
    c.eax = m.fast_u32(pt, slot);
    let value = match KIND {
        0 => {
            c.ecx = m.fast_u16(pt, c.eax.wrapping_add(6)) as u32;
            c.ecx
        }
        1 | 2 => {
            c.ecx = m.fast_u8(pt, c.eax.wrapping_add(if KIND == 1 { 3 } else { 7 })) as u32;
            c.ecx
        }
        3 => {
            c.ecx = m.fast_u32(pt, c.eax) >> 28;
            c.ecx
        }
        4 => {
            let word = m.fast_u16(pt, c.eax.wrapping_add(4)) as u32;
            c.ecx = (c.ecx & !0xffff) | (word >> 12);
            c.fl = x86::Flags::new(x86::SHR, word, 12, word >> 12, 16);
            c.edx = c.ecx & 0xffff;
            c.edx
        }
        5 => {
            let byte = m.fast_u8(pt, c.eax.wrapping_add(4)) as u32;
            c.ecx = (c.ecx & !255) | (byte >> 4);
            c.fl = x86::Flags::new(x86::SHR, byte, 4, byte >> 4, 8);
            c.edx = c.ecx & 255;
            c.edx
        }
        _ => unreachable!(),
    };
    m.fast_set_u32(pt, slot, value);
    c.st.memory::<0x00db, 4, false>(m, slot); // FILD m32int.
    if KIND == 3 {
        // The logical shift proves nonnegative: the original's FADD arm
        // after TEST/JNS cannot execute, but TEST's return flags remain.
        c.fl.alu_and(c.ecx, c.ecx, 32);
    }
    ret(c, 0);
}

// Translated from 0x00e0ec50 (DDDA.exe 2017-12-13 Steam).
/// Expand a packed dependency into the output at argument 2, returning
/// its end in EAX. ECX owns the descriptor bank at +0x28; argument 1 is
/// the value bank. Descriptor bits 20..25 count recursive dependencies,
/// bits 26..31 count terminal dependencies; +4 points to u16 indices.
/// Packed values use bits 0..11 as descriptor index and bits 12..31 as
/// the optional output value. Terminal values are emitted unchanged.
pub fn expand_dependencies(m: &mut dyn Mem, c: &mut Cpu) {
    let pt = m.page_table();
    macro_rules! r {
        ($a:expr) => {
            m.fast_u32(pt, $a)
        };
    }
    macro_rules! w {
        ($a:expr, $v:expr) => {
            m.fast_set_u32(pt, $a, $v)
        };
    }
    macro_rules! s {
        ($o:expr) => {
            c.esp.wrapping_add($o)
        };
    }
    macro_rules! descriptor {
        ($base:expr) => {
            $base.wrapping_add(c.ecx.wrapping_mul(8))
        };
    }
    macro_rules! value {
        ($cursor:expr) => {
            r!(c.ebp
                .wrapping_add((m.fast_u16(pt, $cursor) as u32).wrapping_mul(8))
                .wrapping_add(4))
        };
    }
    macro_rules! emit_value {
        ($v:expr) => {
            if $v != 0 {
                w!(c.eax, $v);
                c.eax = c.eax.wrapping_add(4);
            }
        };
    }
    c.esp = c.esp.wrapping_sub(0x2c);
    c.eax = r!(s!(0x38));
    push(m, c, c.ebx);
    push(m, c, c.ebp);
    push(m, c, c.esi);
    c.esi = r!(c.ecx.wrapping_add(0x28));
    w!(s!(0x24), c.ecx);
    c.ecx = r!(s!(0x3c));
    c.edx = c.ecx & 0xfffff000;
    w!(s!(0x0c), c.esi);
    emit_value!(c.edx);
    c.ebp = r!(s!(0x40));
    c.ecx &= 0xfff;
    let root_has_children = r!(descriptor!(c.esi)) & 0x03f00000 != 0;
    c.ebx = r!(descriptor!(c.esi).wrapping_add(4));
    c.edx = descriptor!(c.esi);
    w!(s!(0x20), c.edx);
    w!(s!(0x1c), 0);
    if root_has_children {
        push(m, c, c.edi);
        loop {
            // First unrolled child; EBX is the root's index cursor.
            c.ecx = value!(c.ebx);
            c.ebx = c.ebx.wrapping_add(2);
            c.edx = c.ecx & 0xfffff000;
            w!(s!(0x38), c.ebx);
            emit_value!(c.edx);
            c.ecx &= 0xfff;
            let has_children = r!(descriptor!(c.esi)) & 0x03f00000 != 0;
            c.edi = r!(descriptor!(c.esi).wrapping_add(4));
            c.edx = descriptor!(c.esi);
            w!(s!(0x1c), c.edx);
            w!(s!(0x18), 0);
            if has_children {
                loop {
                    // Second child; EDI walks the first child's indices.
                    c.edx = m.fast_u16(pt, c.edi) as u32;
                    c.ecx = r!(c.ebp.wrapping_add(c.edx.wrapping_mul(8)).wrapping_add(4));
                    c.edi = c.edi.wrapping_add(2);
                    c.esi = c.ecx & 0xfffff000;
                    w!(s!(0x34), c.edi);
                    emit_value!(c.esi);
                    c.edx = r!(s!(0x10));
                    c.ecx &= 0xfff;
                    let has_children = r!(descriptor!(c.edx)) & 0x03f00000 != 0;
                    c.ebx = r!(descriptor!(c.edx).wrapping_add(4));
                    c.edx = descriptor!(c.edx);
                    w!(s!(0x14), c.edx);
                    w!(s!(0x44), 0);
                    if has_children {
                        loop {
                            // Third child; EBX walks the second child's indices.
                            c.ecx = value!(c.ebx);
                            c.ebx = c.ebx.wrapping_add(2);
                            c.esi = c.ecx & 0xfffff000;
                            w!(s!(0x30), c.ebx);
                            emit_value!(c.esi);
                            c.edx = r!(s!(0x10));
                            c.ecx &= 0xfff;
                            let has_children = r!(descriptor!(c.edx)) & 0x03f00000 != 0;
                            c.esi = descriptor!(c.edx);
                            c.edx = r!(c.esi.wrapping_add(4));
                            w!(s!(0x40), c.esi);
                            w!(s!(0x48), 0);
                            if has_children {
                                loop {
                                    // Fourth child; EDX walks the third child's indices.
                                    c.ecx = value!(c.edx);
                                    c.edx = c.edx.wrapping_add(2);
                                    c.esi = c.ecx & 0xfffff000;
                                    w!(s!(0x2c), c.edx);
                                    emit_value!(c.esi);
                                    c.esi = r!(s!(0x10));
                                    c.ecx &= 0xfff;
                                    c.ebx = descriptor!(c.esi);
                                    c.esi = r!(c.ebx.wrapping_add(4));
                                    c.edi = 0;
                                    if r!(c.ebx) & 0x03f00000 != 0 {
                                        loop {
                                            c.edx = m.fast_u16(pt, c.esi) as u32;
                                            c.ecx =
                                                r!(c.ebp.wrapping_add(c.edx.wrapping_mul(8)).wrapping_add(4));
                                            push(m, c, c.eax);
                                            push(m, c, c.ebp);
                                            push(m, c, c.ecx);
                                            c.ecx = r!(s!(0x34));
                                            // ADD ESI is the last flag writer before CALL.
                                            c.esi = c.fl.alu_add(c.esi, 2, 32);
                                            push(m, c, 0x00e0edd6);
                                            x86::call_ext(m, c, 0x00e0ec50);
                                            c.ecx = (r!(c.ebx) >> 20) & 63;
                                            c.edi = c.edi.wrapping_add(1);
                                            if c.edi >= c.ecx {
                                                break;
                                            }
                                        }
                                        c.edx = r!(s!(0x2c));
                                    }
                                    c.edi = 0;
                                    if r!(c.ebx) & 0xfc000000 != 0 {
                                        loop {
                                            c.ecx = value!(c.esi);
                                            c.esi = c.esi.wrapping_add(2);
                                            emit_value!(c.ecx);
                                            c.ecx = r!(c.ebx) >> 26;
                                            c.edi = c.edi.wrapping_add(1);
                                            if c.edi >= c.ecx {
                                                break;
                                            }
                                        }
                                    }
                                    c.esi = (r!(r!(s!(0x40))) >> 20) & 63;
                                    c.ecx = r!(s!(0x48)).wrapping_add(1);
                                    w!(s!(0x48), c.ecx);
                                    if c.ecx >= c.esi {
                                        break;
                                    }
                                }
                                c.esi = r!(s!(0x40));
                                c.ebx = r!(s!(0x30));
                            }
                            c.edi = 0;
                            if r!(c.esi) & 0xfc000000 != 0 {
                                loop {
                                    c.ecx = value!(c.edx);
                                    c.edx = c.edx.wrapping_add(2);
                                    emit_value!(c.ecx);
                                    c.ecx = r!(c.esi) >> 26;
                                    c.edi = c.edi.wrapping_add(1);
                                    if c.edi >= c.ecx {
                                        break;
                                    }
                                }
                            }
                            c.edx = (r!(r!(s!(0x14))) >> 20) & 63;
                            c.ecx = r!(s!(0x44)).wrapping_add(1);
                            w!(s!(0x44), c.ecx);
                            if c.ecx >= c.edx {
                                break;
                            }
                        }
                        c.edi = r!(s!(0x34));
                        c.edx = r!(s!(0x14));
                    }
                    c.esi = 0;
                    if r!(c.edx) & 0xfc000000 != 0 {
                        loop {
                            c.ecx = value!(c.ebx);
                            c.ebx = c.ebx.wrapping_add(2);
                            emit_value!(c.ecx);
                            c.ecx = r!(c.edx) >> 26;
                            c.esi = c.esi.wrapping_add(1);
                            if c.esi >= c.ecx {
                                break;
                            }
                        }
                    }
                    c.edx = (r!(r!(s!(0x1c))) >> 20) & 63;
                    c.ecx = r!(s!(0x18)).wrapping_add(1);
                    w!(s!(0x18), c.ecx);
                    if c.ecx >= c.edx {
                        break;
                    }
                }
                c.ebx = r!(s!(0x38));
                c.edx = r!(s!(0x1c));
            }
            c.esi = 0;
            if r!(c.edx) & 0xfc000000 != 0 {
                loop {
                    c.ecx = value!(c.edi);
                    c.edi = c.edi.wrapping_add(2);
                    emit_value!(c.ecx);
                    c.ecx = r!(c.edx) >> 26;
                    c.esi = c.esi.wrapping_add(1);
                    if c.esi >= c.ecx {
                        break;
                    }
                }
            }
            c.edx = (r!(r!(s!(0x24))) >> 20) & 63;
            c.ecx = r!(s!(0x20)).wrapping_add(1);
            w!(s!(0x20), c.ecx);
            if c.ecx >= c.edx {
                break;
            }
            c.esi = r!(s!(0x10));
        }
        c.edx = r!(s!(0x24));
        c.edi = pop(m, c);
    }
    c.esi = 0;
    if r!(c.edx) & 0xfc000000 != 0 {
        loop {
            c.ecx = value!(c.ebx);
            c.ebx = c.ebx.wrapping_add(2);
            emit_value!(c.ecx);
            c.ecx = r!(c.edx) >> 26;
            c.esi = c.esi.wrapping_add(1);
            if c.esi >= c.ecx {
                break;
            }
        }
    }
    c.esi = pop(m, c);
    c.ebp = pop(m, c);
    c.ebx = pop(m, c);
    // ADD ESP overwrites all arithmetic flags before RET 12.
    c.esp = c.fl.alu_add(c.esp, 0x2c, 32);
    ret(c, 12);
}

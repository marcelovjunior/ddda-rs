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
];

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

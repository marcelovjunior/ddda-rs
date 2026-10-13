//! Packed dependency traversal used by the main/job update paths.
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
pub const PORTS: &[(u32, x86::LiftFn, u8)] = &[(0x00e0_ec50, expand_dependencies, 0xff)];

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

//! Shader-package dependency lookup leaves below the model job path.
use ddnative_base::{
    mem::{Mem, Pages},
    x86::Cpu,
};

// Translated from 0x01008200 (DDDA.exe 2017-12-13 Steam).
/// Lookup a single key. The sign bit in the record index marks a missing
/// single-key entry; the two-key variant deliberately treats it differently.
pub fn single_key(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    c.edx = p.u32(m, c.esp.wrapping_add(4));
    if c.edx != 0 {
        c.ecx = p.u32(m, c.ecx.wrapping_add(0x84));
        c.eax = c.edx & 0xfff;
        c.eax = p.u32(m, c.ecx.wrapping_add(c.eax.wrapping_mul(4)).wrapping_add(0x1c));
        while c.eax != 0 {
            if p.u32(m, c.eax) == c.edx {
                c.eax = p.u32(m, c.eax.wrapping_add(12));
                if c.eax & 0x80000000 == 0 {
                    c.eax = c.fl.alu_and(c.eax, 0x7fffffff, 32);
                    c.edx = c.eax.wrapping_mul(9);
                    c.eax = c.ecx.wrapping_add(c.edx.wrapping_mul(4)).wrapping_add(0x401c);
                    c.esp = c.esp.wrapping_add(8);
                    return;
                }
                break;
            }
            c.eax = p.u32(m, c.eax.wrapping_add(8));
            if c.eax == 0 {
                break;
            }
            c.eax = c.eax.wrapping_shl(4);
            c.eax = c.fl.alu_add(c.eax, p.u32(m, c.ecx.wrapping_add(0x18)), 32);
        }
    }
    c.eax = c.fl.alu_xor(c.eax, c.eax, 32);
    c.esp = c.esp.wrapping_add(8);
}

// Translated from 0x01008260 (DDDA.exe 2017-12-13 Steam).
/// Lookup a key pair, retaining original stack spills and access order.
pub fn pair_key(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    c.edx = p.u32(m, c.esp.wrapping_add(4));
    c.esp = c.esp.wrapping_sub(4);
    p.set_u32(m, c.esp, c.esi);
    c.esi = p.u32(m, c.esp.wrapping_add(12));
    c.eax = c.fl.alu_or(c.edx, c.esi, 32);
    if c.eax != 0 {
        c.ecx = p.u32(m, c.ecx.wrapping_add(0x84));
        c.eax = c.edx & 0xfff;
        c.eax = p.u32(m, c.ecx.wrapping_add(c.eax.wrapping_mul(4)).wrapping_add(0x1c));
        c.esp = c.esp.wrapping_sub(4);
        p.set_u32(m, c.esp, c.edi);
        c.fl.alu_and(c.eax, c.eax, 32);
        while c.eax != 0 {
            if p.u32(m, c.eax) == c.edx && p.u32(m, c.eax.wrapping_add(4)) == c.esi {
                c.eax = p.u32(m, c.eax.wrapping_add(12));
                c.eax = c.fl.alu_and(c.eax, 0x7fffffff, 32);
                c.edx = c.eax.wrapping_mul(9);
                c.edi = p.u32(m, c.esp);
                c.esp = c.esp.wrapping_add(4);
                c.eax = c.ecx.wrapping_add(c.edx.wrapping_mul(4)).wrapping_add(0x401c);
                c.esi = p.u32(m, c.esp);
                c.esp = c.esp.wrapping_add(16);
                return;
            }
            c.eax = p.u32(m, c.eax.wrapping_add(8));
            c.fl.alu_and(c.eax, c.eax, 32);
            if c.eax == 0 {
                break;
            }
            c.eax = c.eax.wrapping_shl(4);
            c.eax = c.fl.alu_add(c.eax, p.u32(m, c.ecx.wrapping_add(0x18)), 32);
        }
        c.edi = p.u32(m, c.esp);
        c.esp = c.esp.wrapping_add(4);
        c.eax = c.fl.alu_xor(c.eax, c.eax, 32);
    }
    c.esi = p.u32(m, c.esp);
    c.esp = c.esp.wrapping_add(16);
}

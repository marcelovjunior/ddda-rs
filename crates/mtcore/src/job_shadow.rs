//! State saves nested below shadow/model callbacks. Guest accesses stay ordered.
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu, Flags},
};

#[inline]
fn push(p: Pages, m: &mut dyn Mem, c: &mut Cpu, value: u32) {
    c.esp = c.esp.wrapping_sub(4);
    p.set_u32(m, c.esp, value);
}

#[inline]
fn pop(p: Pages, m: &dyn Mem, c: &mut Cpu) -> u32 {
    let value = p.u32(m, c.esp);
    c.esp = c.esp.wrapping_add(4);
    value
}

// Translated from 0x00FED480 (DDDA.exe 2017-12-13 Steam).
/// Save the context's state, including its six install-supplied key selections.
/// The 81 MOVSD operations remain sequential (including DF and overlap).
/// The full-save branch still dispatches the existing memcpy implementation.
pub fn save_state(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    c.fl.alu_sub(p.u8(m, c.esp.wrapping_add(4)) as u32, 0, 8);
    push(p, m, c, c.ebx);
    c.ebx = c.ecx;
    if !c.fl.zf() {
        c.eax = p.u32(m, c.ebx.wrapping_add(0x14));
        c.ecx = c.eax.wrapping_mul(8).wrapping_add(0x144);
        c.eax = p.u32(m, c.ebx.wrapping_add(0x30874));
        let input = c.eax;
        c.eax = input.wrapping_mul(0x8144);
        c.fl = Flags::new(x86::IMUL, input, 0x8144, c.eax, 32);
        push(p, m, c, c.ecx);
        c.edx = c.ebx.wrapping_add(0xb4);
        push(p, m, c, c.edx);
        c.ecx = c.eax.wrapping_add(c.ebx).wrapping_add(0x81f8);
        push(p, m, c, c.ecx);
        push(p, m, c, 0xfed4b5);
        x86::call_lifted(m, c, 0x012efea0);
        c.esp = c.fl.alu_add(c.esp, 12, 32);
    } else {
        c.edx = p.u32(m, c.ebx.wrapping_add(0x30874));
        let input = c.edx;
        c.edx = input.wrapping_mul(0x8144);
        c.fl = Flags::new(x86::IMUL, input, 0x8144, c.edx, 32);
        push(p, m, c, c.esi);
        push(p, m, c, c.edi);
        c.edi = c.edx.wrapping_add(c.ebx).wrapping_add(0x81f8);
        c.esi = c.ebx.wrapping_add(0xb4);
        c.ecx = 0x51;
        while c.ecx != 0 {
            let value = p.u32(m, c.esi);
            p.set_u32(m, c.edi, value);
            let step = if c.df { 0u32.wrapping_sub(4) } else { 4 };
            c.esi = c.esi.wrapping_add(step);
            c.edi = c.edi.wrapping_add(step);
            c.ecx -= 1;
        }
        for slot in 0..6u32 {
            c.ecx = p.u32(m, c.ebx.wrapping_add(0x30874));
            c.eax = p.u32(m, 0x01447b44 + slot * 4);
            c.ecx = c.ecx.wrapping_mul(0x8144);
            // Earlier flag writes are dead in the verified lifted body too.
            c.eax = if slot == 5 {
                c.fl.alu_and(c.eax, 0xfff, 32)
            } else {
                c.eax & 0xfff
            };
            c.edx = p.u32(m, c.ebx.wrapping_add(c.eax.wrapping_mul(8)).wrapping_add(0x1f8));
            if slot == 5 {
                c.edi = pop(p, m, c);
            }
            p.set_u32(
                m,
                c.ecx.wrapping_add(c.ebx).wrapping_add(0x8324 + slot * 4),
                c.edx,
            );
        }
        let value = p.u32(m, c.ebx.wrapping_add(0x30874));
        c.fl.inc_dec(x86::INC, value, value.wrapping_add(1), 32);
        p.set_u32(m, c.ebx.wrapping_add(0x30874), value.wrapping_add(1));
        c.esi = pop(p, m, c);
        c.ebx = pop(p, m, c);
        c.esp = c.esp.wrapping_add(8);
        return;
    }
    let value = p.u32(m, c.ebx.wrapping_add(0x30874));
    c.fl.inc_dec(x86::INC, value, value.wrapping_add(1), 32);
    p.set_u32(m, c.ebx.wrapping_add(0x30874), value.wrapping_add(1));
    c.ebx = pop(p, m, c);
    c.esp = c.esp.wrapping_add(8);
}

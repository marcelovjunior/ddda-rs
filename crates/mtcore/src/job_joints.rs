//! Model joint-index dispatch, retaining the original evaluator callbacks.
use ddnative_base::{mem::{Mem, Pages}, x86::{self, Cpu}};

fn push(m: &mut dyn Mem, c: &mut Cpu, p: Pages, value: u32) {
    c.esp = c.esp.wrapping_sub(4);
    p.set_u32(m, c.esp, value);
}
fn pop(m: &dyn Mem, c: &mut Cpu, p: Pages) -> u32 {
    let value = p.u32(m, c.esp);
    c.esp = c.esp.wrapping_add(4);
    value
}

// Translated from 0x01054700 (DDDA.exe 2017-12-13 Steam).
/// Walk the supplied byte indices in order. Evaluators and joint virtual
/// callbacks remain original calls, with original return PCs and spills.
pub fn evaluate_joint_indices(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    push(m, c, p, c.ecx);
    c.eax = (c.eax & !255) | u32::from(p.u8(m, c.esp.wrapping_add(16)));
    push(m, c, p, c.ebx);
    push(m, c, p, c.ebp);
    push(m, c, p, c.edi);
    c.edi = c.ecx;
    if c.eax & 2 != 0 {
        p.set_u8(m, c.esp.wrapping_add(28), 0);
    } else {
        c.ecx = (c.ecx & !255) | u32::from(p.u8(m, c.edi.wrapping_add(0x37b)));
        c.ecx = (c.ecx & !255) | (c.ecx & 1);
        p.set_u8(m, c.esp.wrapping_add(28), c.ecx as u8);
    }
    c.eax = (c.eax & !255) | (c.eax & 1);
    c.fl.alu_sub(u32::from(p.u8(m, c.edi.wrapping_add(0xe19))), 0, 8);
    p.set_u8(m, c.esp.wrapping_add(15), c.eax as u8);
    if !c.fl.zf() {
        c.ecx = c.edi;
        push(m, c, p, 0x01054738);
        x86::call_ext(m, c, 0x01054690);
        p.set_u8(m, c.edi.wrapping_add(0xe19), 0);
    }
    c.eax = p.u32(m, c.edi.wrapping_add(0x378));
    c.ebx = if c.eax & 0x08000000 != 0 {
        0
    } else if c.eax & 0x04000000 != 0 {
        0x0104ef10
    } else if c.eax & 0x40 == 0 {
        0x01051b60
    } else if c.eax & 0x100000 != 0 {
        let value = p.u32(m, c.edi.wrapping_add(0x108)) | 0x40;
        p.set_u32(m, c.edi.wrapping_add(0x108), value);
        0x01051b60
    } else {
        c.ebx = if c.eax & 0x4000 != 0 { 0x010533a0 } else { 0x01052330 };
        let value = p.u32(m, c.edi.wrapping_add(0x108)) | 0x40;
        p.set_u32(m, c.edi.wrapping_add(0x108), value);
        c.ebx
    };
    c.ebp = 0;
    c.fl.alu_sub(p.u32(m, c.esp.wrapping_add(24)), c.ebp, 32);
    if c.fl.a > c.fl.b {
        push(m, c, p, c.esi);
        loop {
            c.eax = p.u32(m, c.esp.wrapping_add(24));
            c.eax = u32::from(p.u8(m, c.eax.wrapping_add(c.ebp)));
            c.esi = c.eax.wrapping_mul(5).wrapping_shl(5);
            c.esi = c.esi.wrapping_add(p.u32(m, c.edi.wrapping_add(0x364)));
            if p.u8(m, c.esi.wrapping_add(0x90)) & 0x20 == 0
                && (p.u8(m, c.esp.wrapping_add(19)) == 0
                    || p.u8(m, c.esi.wrapping_add(0x96)) & 1 != 0)
            {
                c.fl.alu_and(c.ebx, c.ebx, 32);
                if c.ebx != 0 {
                    push(m, c, p, c.esi);
                    c.ecx = c.edi;
                    let evaluator = c.ebx;
                    push(m, c, p, 0x010547d6);
                    x86::call_ext(m, c, evaluator);
                }
                // The original rereads this caller argument after dispatch.
                // A callback may alter the reader/model/joint memory.
                if p.u8(m, c.esp.wrapping_add(32)) != 0 {
                    c.fl.alu_sub(p.u32(m, c.esi.wrapping_add(4)), 0, 32);
                    if c.fl.a != 0 {
                        c.ecx = p.u32(m, c.esi.wrapping_add(4));
                        c.edx = p.u32(m, c.ecx);
                        c.eax = p.u32(m, c.edx.wrapping_add(20));
                        push(m, c, p, c.edi);
                        push(m, c, p, c.esi);
                        let callback = c.eax;
                        push(m, c, p, 0x010547ef);
                        x86::call_ext(m, c, callback);
                    }
                }
            }
            c.ebp = c.ebp.wrapping_add(1);
            c.fl.alu_sub(c.ebp, p.u32(m, c.esp.wrapping_add(28)), 32);
            if c.fl.a >= c.fl.b { break; }
        }
        c.esi = pop(m, c, p);
    }
    c.edi = pop(m, c, p);
    c.ebp = pop(m, c, p);
    c.ebx = pop(m, c, p);
    c.ecx = pop(m, c, p);
    c.esp = c.esp.wrapping_add(16);
}

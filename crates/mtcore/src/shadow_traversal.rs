//! Original shadow traversal, including the serial nested-job path.
//! Keep guest spills, callback order and the original queue decision.
use ddnative_base::{
    mem::{Mem, Pages},
    x86::Cpu,
};

#[inline]
pub(crate) fn push(m: &mut dyn Mem, p: &Pages, c: &mut Cpu, value: u32) {
    c.esp = c.esp.wrapping_sub(4);
    p.set_u32(m, c.esp, value);
}
#[inline]
pub(crate) fn call(m: &mut dyn Mem, p: &Pages, c: &mut Cpu, target: u32, resume: u32) {
    push(m, p, c, resume);
    crate::semantic::call_guest(m, c, target);
}
#[inline]
fn finish(m: &mut dyn Mem, p: &Pages, c: &mut Cpu) {
    c.edi = p.u32(m, c.esp);
    c.esp = c.esp.wrapping_add(4);
    c.esi = p.u32(m, c.esp);
    c.esp = c.esp.wrapping_add(4);
    c.ebp = p.u32(m, c.esp);
    c.esp = c.esp.wrapping_add(4);
    c.ebx = p.u32(m, c.esp);
    c.esp = c.esp.wrapping_add(4);
    c.esp = c.fl.alu_add(c.esp, 0x1c, 32);
    c.esp = c.esp.wrapping_add(12);
}

// Translated from 0x00DB63E0 (DDDA.exe 2017-12-13 Steam), sUnit::vf16.
/// Render eligible units in list order, or enqueue the original DB7620
/// jobs when outside the scheduler's nesting guard. No nested scheduler.
pub fn traverse(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    c.esp = c.fl.alu_sub(c.esp, 0x1c, 32);
    push(m, &p, c, c.ebx);
    push(m, &p, c, c.ebp);
    push(m, &p, c, c.esi);
    push(m, &p, c, c.edi);
    c.ebp = c.ecx;
    push(m, &p, c, 4);
    c.ecx = c.esp.wrapping_add(0x2c);
    call(m, &p, c, 0x1147970, 0xdb63f4);
    c.edi = p.u32(m, c.esp.wrapping_add(0x30));
    c.ecx = p.u16(m, c.edi.wrapping_add(0x16a)) as u32;
    c.eax = p.u32(m, 0x18d0b28);
    c.ebx = p.u32(m, c.eax.wrapping_add(0x98));
    p.set_u32(m, c.esp.wrapping_add(0x18), c.ecx);
    c.ecx = (c.ecx & !255) | p.u8(m, c.edi.wrapping_add(0x168)) as u32;
    c.eax = 1u32 << (c.ecx & 31);
    c.fl.alu_sub(p.u8(m, 0x18d0bb0) as u32, 0, 8);
    p.set_u32(m, c.esp.wrapping_add(0x24), c.ebx);
    p.set_u32(m, c.esp.wrapping_add(0x1c), c.eax);
    let nested = !c.fl.zf();
    if !nested {
        c.fl.alu_sub(c.ebx, 1, 32);
    }
    if nested || c.ebx == 1 {
        serial(m, &p, c);
    } else {
        queued(m, &p, c);
    }
}

fn serial(m: &mut dyn Mem, p: &Pages, c: &mut Cpu) {
    push(m, p, c, 0);
    push(m, p, c, 0);
    push(m, p, c, 0);
    push(m, p, c, c.edi);
    call(m, p, c, 0x107ff00, 0xdb6639);
    c.eax = p.u32(m, 0x18d09e0);
    c.esi = 0;
    c.esp = c.esp.wrapping_add(16);
    p.set_u32(m, c.esp.wrapping_add(0x14), c.esi);
    c.fl.alu_sub(p.u32(m, c.eax.wrapping_add(0x620)), c.esi, 32);
    if c.fl.a > c.fl.b {
        c.ebx = 0;
        loop {
            c.ecx = p.u32(m, c.ebx.wrapping_add(c.ebp).wrapping_add(0x28)) >> 2;
            let mut enabled = c.ecx & 1 != 0;
            if enabled {
                c.ecx = p.u32(m, c.esp.wrapping_add(0x34));
                if c.ecx != 0 {
                    enabled = p.u8(m, c.esi.wrapping_add(c.ecx)) != 0;
                }
            }
            if enabled {
                c.ecx = p.u32(m, c.ebx.wrapping_add(c.eax).wrapping_add(0x2c));
                if c.ecx != 0 {
                    loop {
                        if serial_eligible(m, p, c) {
                            c.eax = p.u32(m, c.ecx);
                            c.edx = p.u32(m, c.eax.wrapping_add(0x2c));
                            push(m, p, c, c.edi);
                            call(m, p, c, c.edx, 0xdb66f8);
                        }
                        // Save-next happened before the callback, exactly as
                        // the original; flags/masks of that next node do not.
                        c.ecx = c.esi;
                        if c.esi == 0 {
                            break;
                        }
                    }
                    c.eax = p.u32(m, 0x18d09e0);
                    c.edi = p.u32(m, c.esp.wrapping_add(0x30));
                    c.esi = p.u32(m, c.esp.wrapping_add(0x14));
                }
            }
            c.esi = c.esi.wrapping_add(1);
            c.ebx = c.ebx.wrapping_add(0x18);
            p.set_u32(m, c.esp.wrapping_add(0x14), c.esi);
            c.fl.alu_sub(c.esi, p.u32(m, c.eax.wrapping_add(0x620)), 32);
            if c.fl.a >= c.fl.b {
                break;
            }
        }
    }
    push(m, p, c, 0);
    push(m, p, c, 0);
    push(m, p, c, c.edi);
    call(m, p, c, 0x107ff30, 0xdb6729);
    c.esp = c.fl.alu_add(c.esp, 12, 32);
    c.ecx = c.esp.wrapping_add(0x28);
    call(m, p, c, 0x11479a0, 0xdb6735);
    finish(m, p, c);
}

#[inline]
fn serial_eligible(m: &mut dyn Mem, p: &Pages, c: &mut Cpu) -> bool {
    c.eax = p.u32(m, c.ecx.wrapping_add(4));
    c.esi = p.u32(m, c.ecx.wrapping_add(12));
    c.edi = p.u32(m, c.ecx.wrapping_add(8));
    c.edx = (c.eax & !255) | (c.eax & 7);
    if c.edx as u8 != 2 {
        return false;
    }
    c.edx = c.eax & 0x4800;
    if c.edx != 0x4800 {
        return false;
    }
    if p.u32(m, c.esp.wrapping_add(0x18)) & c.edi == 0 {
        return false;
    }
    c.edx = p.u32(m, c.esp.wrapping_add(0x30));
    c.edx = p.u32(m, c.edx.wrapping_add(0x16c)) & c.edi;
    c.edi = p.u32(m, c.esp.wrapping_add(0x30));
    if c.edx as u16 != p.u16(m, c.edi.wrapping_add(0x16e)) {
        return false;
    }
    c.eax >>= 16;
    c.eax &= p.u32(m, c.esp.wrapping_add(0x1c));
    if c.eax & 0x3ff == 0 {
        return false;
    }
    c.eax = p.u32(m, c.ebp.wrapping_add(0xd38));
    c.edx = p.u32(m, c.ebp.wrapping_add(0xd3c));
    c.eax = !c.eax;
    c.eax &= p.u32(m, c.ecx.wrapping_add(0x18));
    c.edx = !c.edx;
    c.edx &= p.u32(m, c.ecx.wrapping_add(0x1c));
    c.eax = c.fl.alu_or(c.eax, c.edx, 32);
    c.eax == 0
}

fn queued(m: &mut dyn Mem, p: &Pages, c: &mut Cpu) {
    c.esi = c.ebp.wrapping_add(0xd40);
    push(m, p, c, c.ebx);
    push(m, p, c, c.esi);
    c.ecx = c.edi;
    p.set_u32(m, c.esp.wrapping_add(0x28), c.esi);
    call(m, p, c, 0xfed200, 0xdb644c);
    push(m, p, c, 1);
    push(m, p, c, c.ebx);
    push(m, p, c, c.esi);
    push(m, p, c, c.edi);
    call(m, p, c, 0x107ff00, 0xdb6456);
    // Original context-lane addresses, embedded LEA displacements at
    // DB6456..DB64D8. Preserve the distinct scratch register results.
    c.edx = p.u32(m, 0x18d0b28);
    p.set_u32(m, c.edx.wrapping_add(0x1bdd0), c.esi);
    c.ecx = p.u32(m, 0x18d0b28);
    c.eax = c.ebp.wrapping_add(0x349c4);
    p.set_u32(m, c.ecx.wrapping_add(0x1bdf0), c.eax);
    c.eax = p.u32(m, 0x18d0b28);
    c.edx = c.ebp.wrapping_add(0x68648);
    p.set_u32(m, c.eax.wrapping_add(0x1be10), c.edx);
    c.edx = p.u32(m, 0x18d0b28);
    c.ecx = c.ebp.wrapping_add(0x9c2cc);
    p.set_u32(m, c.edx.wrapping_add(0x1be30), c.ecx);
    c.ecx = p.u32(m, 0x18d0b28);
    c.eax = c.ebp.wrapping_add(0xcff50);
    p.set_u32(m, c.ecx.wrapping_add(0x1be50), c.eax);
    c.eax = p.u32(m, 0x18d0b28);
    c.edx = c.ebp.wrapping_add(0x103bd4);
    p.set_u32(m, c.eax.wrapping_add(0x1be70), c.edx);
    c.edx = p.u32(m, 0x18d0b28);
    c.ecx = c.ebp.wrapping_add(0x137858);
    p.set_u32(m, c.edx.wrapping_add(0x1be90), c.ecx);
    c.ecx = p.u32(m, 0x18d0b28);
    c.eax = c.ebp.wrapping_add(0x16b4dc);
    p.set_u32(m, c.ecx.wrapping_add(0x1beb0), c.eax);
    c.ecx = p.u32(m, 0x18d09e0);
    c.edx = 0;
    c.esp = c.esp.wrapping_add(16);
    p.set_u32(m, c.esp.wrapping_add(0x10), c.edx);
    c.fl.alu_sub(p.u32(m, c.ecx.wrapping_add(0x620)), c.edx, 32);
    if c.fl.a > c.fl.b {
        c.ebx = 0;
        p.set_u32(m, c.esp.wrapping_add(0x14), c.ebx);
        loop {
            c.eax = p.u32(m, c.ebx.wrapping_add(c.ebp).wrapping_add(0x28)) >> 2;
            let mut enabled = c.eax & 1 != 0;
            if enabled {
                c.eax = p.u32(m, c.esp.wrapping_add(0x34));
                if c.eax != 0 {
                    enabled = p.u8(m, c.edx.wrapping_add(c.eax)) != 0;
                }
            }
            if enabled {
                c.eax = p.u32(m, c.ebx.wrapping_add(c.ecx).wrapping_add(0x2c));
                if c.eax != 0 {
                    loop {
                        if queued_eligible(m, p, c) {
                            c.ecx = p.u32(m, 0x18d0b28);
                            c.edx = p.u32(m, c.ecx.wrapping_add(0x8c));
                            c.esi = c.ecx.wrapping_add(c.edx.wrapping_mul(8)).wrapping_add(0xc0);
                            c.edx = c.edx.wrapping_add(1);
                            p.set_u32(m, c.ecx.wrapping_add(0x8c), c.edx);
                            p.set_u32(m, c.esi, c.eax);
                            p.set_u32(m, c.esi.wrapping_add(4), 0xdb7620);
                        }
                        c.eax = c.edi;
                        if c.edi == 0 {
                            break;
                        }
                    }
                    c.ecx = p.u32(m, 0x18d09e0);
                    c.edi = p.u32(m, c.esp.wrapping_add(0x30));
                    c.edx = p.u32(m, c.esp.wrapping_add(0x10));
                    c.esi = p.u32(m, c.esp.wrapping_add(0x20));
                    c.ebx = p.u32(m, c.esp.wrapping_add(0x14));
                }
            }
            c.edx = c.edx.wrapping_add(1);
            c.ebx = c.ebx.wrapping_add(0x18);
            p.set_u32(m, c.esp.wrapping_add(0x10), c.edx);
            p.set_u32(m, c.esp.wrapping_add(0x14), c.ebx);
            c.fl.alu_sub(c.edx, p.u32(m, c.ecx.wrapping_add(0x620)), 32);
            if c.fl.a >= c.fl.b {
                break;
            }
        }
        c.ebx = p.u32(m, c.esp.wrapping_add(0x24));
    }
    c.ecx = p.u32(m, 0x18d0b28);
    push(m, p, c, 1);
    call(m, p, c, 0xdbe560, 0xdb6606);
    push(m, p, c, c.ebx);
    push(m, p, c, c.esi);
    push(m, p, c, c.edi);
    call(m, p, c, 0x107ff30, 0xdb660e);
    c.esp = c.fl.alu_add(c.esp, 12, 32);
    push(m, p, c, c.ebx);
    push(m, p, c, c.esi);
    c.ecx = c.edi;
    call(m, p, c, 0xfed3b0, 0xdb661a);
    c.ecx = c.esp.wrapping_add(0x28);
    call(m, p, c, 0x11479a0, 0xdb6623);
    finish(m, p, c);
}

#[inline]
fn queued_eligible(m: &mut dyn Mem, p: &Pages, c: &mut Cpu) -> bool {
    c.ecx = p.u32(m, c.eax.wrapping_add(4));
    c.edi = p.u32(m, c.eax.wrapping_add(12));
    c.esi = p.u32(m, c.eax.wrapping_add(8));
    c.edx = (c.ecx & !255) | (c.ecx & 7);
    if c.edx as u8 != 2 {
        return false;
    }
    c.edx = c.ecx & 0x4800;
    if c.edx != 0x4800 {
        return false;
    }
    if p.u32(m, c.esp.wrapping_add(0x18)) & c.esi == 0 {
        return false;
    }
    c.edx = p.u32(m, c.esp.wrapping_add(0x30));
    c.ebx = p.u32(m, c.edx.wrapping_add(0x16c)) & c.esi;
    if c.ebx as u16 != p.u16(m, c.edx.wrapping_add(0x16e)) {
        return false;
    }
    c.ecx >>= 16;
    c.ecx &= p.u32(m, c.esp.wrapping_add(0x1c));
    if c.ecx & 0x3ff == 0 {
        return false;
    }
    c.ecx = p.u32(m, c.ebp.wrapping_add(0xd38));
    c.edx = p.u32(m, c.ebp.wrapping_add(0xd3c));
    c.ecx = !c.ecx;
    c.ecx &= p.u32(m, c.eax.wrapping_add(0x18));
    c.edx = !c.edx;
    c.edx &= p.u32(m, c.eax.wrapping_add(0x1c));
    c.ecx = c.fl.alu_or(c.ecx, c.edx, 32);
    c.ecx == 0
}

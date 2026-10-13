//! Stable-shadow pass setup. Guest state remains visible at every callee.
use crate::shadow_traversal::{call, push};
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu},
};

// Translated from 0x00EFD240 (DDDA.exe 2017-12-13 Steam).
/// Set up a shadow pass, dispatch its units, then restore recording state.
pub fn setup(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    push(m, &p, c, c.ebp);
    c.ebp = c.esp;
    c.esp &= !15;
    c.esp = c.fl.alu_sub(c.esp, 0x14, 32);
    push(m, &p, c, c.ebx);
    push(m, &p, c, c.esi);
    c.esi = p.u32(m, c.ebp.wrapping_add(8));
    push(m, &p, c, c.edi);
    push(m, &p, c, 0);
    c.edi = c.ecx;
    push(m, &p, c, 0);
    c.ecx = c.esi;
    call(m, &p, c, 0xff1090, 0xefd25c);
    c.eax = p.u32(m, c.esi.wrapping_add(0x164));
    c.eax &= 0x1f;
    c.eax = c.fl.alu_or(c.eax, 0x20, 32);
    c.ecx = c.esi;
    p.set_u32(m, c.esi.wrapping_add(0x164), c.eax);
    push(m, &p, c, 0xefd275);
    x86::call_lifted(m, c, 0xfed8f0);
    c.xmm[0] = [p.u32(m, c.eax), 0, 0, 0];
    c.ebx = p.u32(m, c.ebp.wrapping_add(12));
    c.edx = p.u32(m, c.edi);
    p.set_u32(m, c.esp.wrapping_add(0x10), c.xmm[0][0]);
    c.xmm[0] = [p.u32(m, c.eax.wrapping_add(4)), 0, 0, 0];
    p.set_u32(m, c.esp.wrapping_add(0x14), c.xmm[0][0]);
    c.xmm[0] = [p.u32(m, c.eax.wrapping_add(8)), 0, 0, 0];
    c.eax = p.u32(m, c.edx.wrapping_add(0x74));
    push(m, &p, c, c.ebx);
    push(m, &p, c, c.esi);
    c.ecx = c.edi;
    p.set_u32(m, c.esp.wrapping_add(0x20), c.xmm[0][0]);
    call(m, &p, c, c.eax, 0xefd2a3);
    let state = p.u32(m, c.esi.wrapping_add(0x5c)) | 2;
    p.set_u32(m, c.esi.wrapping_add(0x5c), state);
    c.eax = p.u32(m, c.esi.wrapping_add(0x5c));
    if (c.eax as u8 as i8) < 0 {
        save_constants(m, &p, c);
    }
    allocate(m, &p, c, 0x914, 0x1420, 0xefd338);
    if c.eax != 0 {
        // Keep x87 loads/stores (including status and quieting behavior).
        c.st.real_memory::<0x00d9, 4, false>(m, c.edi.wrapping_add(0xb4));
        c.st.real_memory::<0x18d9, 4, true>(m, c.eax);
        c.st.real_memory::<0x00d9, 4, false>(m, c.edi.wrapping_add(0xb8));
        c.st.real_memory::<0x18d9, 4, true>(m, c.eax.wrapping_add(4));
        c.st.real_memory::<0x00d9, 4, false>(m, c.edi.wrapping_add(0xc4));
        c.st.real_memory::<0x18d9, 4, true>(m, c.eax.wrapping_add(8));
    }
    let state = p.u32(m, c.esi.wrapping_add(0x5c)) & !2;
    p.set_u32(m, c.esi.wrapping_add(0x5c), state);
    c.xmm[0] = [p.u32(m, c.edi.wrapping_add(0xb4)), 0, 0, 0];
    c.fl = x86::comiss_hw::<true>(c.xmm[0][0], p.u32(m, 0x1561738));
    // Shader keys are code immediates at EFD382/EFD389. The referenced
    // shader records and comparison constant are always guest reads.
    c.ecx = 0xed98e6cb;
    if c.fl.cf() || c.fl.zf() {
        c.ecx = 0xa97086cc;
    }
    c.edx = p.u32(m, c.esi.wrapping_add(0x20));
    c.eax = p.u32(m, c.esi.wrapping_add(0x3848));
    p.set_u32(m, c.esp.wrapping_add(12), c.ecx);
    c.ecx &= 0xfff;
    c.fl.alu_sub(c.eax, p.u32(m, c.edx.wrapping_add(c.ecx.wrapping_mul(8))), 32);
    if c.fl.a != c.fl.b {
        if p.u8(m, c.esi.wrapping_add(0x5c)) & 0x80 != 0 {
            save_shader(m, &p, c);
        }
        c.eax = p.u32(m, c.esi.wrapping_add(0x20));
        c.edx = p.u32(m, c.eax.wrapping_add(c.ecx.wrapping_mul(8)));
        c.eax = p.u32(m, c.esp.wrapping_add(12));
        c.eax = ((c.eax ^ 0x4d0b2000) & 0xfffff000) | c.ecx;
        p.set_u32(m, c.esi.wrapping_add(0x3848), c.edx);
        p.set_u32(m, c.esi.wrapping_add(0x384c), c.eax);
        let dirty = c.fl.alu_or(p.u32(m, c.esi.wrapping_add(0x154)), 3, 32);
        p.set_u32(m, c.esi.wrapping_add(0x154), dirty);
    }
    c.eax = p.u32(m, c.edi);
    c.edx = p.u32(m, c.eax.wrapping_add(0x64));
    push(m, &p, c, c.esi);
    c.ecx = c.edi;
    call(m, &p, c, c.edx, 0xefd43b);
    c.ecx = p.u8(m, c.ebx.wrapping_add(0xa0)) as u32;
    push(m, &p, c, c.ecx);
    c.ecx = p.u32(m, c.ebx.wrapping_add(4));
    c.eax = c.ebx.wrapping_add(0x90);
    push(m, &p, c, c.eax);
    c.edx = c.ebx.wrapping_add(0x50);
    push(m, &p, c, c.edx);
    c.edx = p.u32(m, c.ebx);
    c.eax = c.ebx.wrapping_add(0x10);
    push(m, &p, c, c.eax);
    push(m, &p, c, c.ecx);
    push(m, &p, c, c.edx);
    push(m, &p, c, c.esi);
    c.ecx = c.edi;
    call(m, &p, c, 0xede880, 0xefd461);
    c.eax = p.u32(m, 0x18d2ce8);
    c.ecx = p.u32(m, c.edi.wrapping_add(0xdc));
    let increment = c.eax.wrapping_add(1);
    c.fl.inc_dec(x86::INC, c.eax, increment, 32);
    c.eax = increment;
    p.set_u32(m, c.ecx.wrapping_add(4), c.eax);
    c.edx = p.u32(m, 0x18d2ce8);
    c.eax = p.u32(m, c.edi.wrapping_add(0xe0));
    c.edx = c.edx.wrapping_add(1);
    p.set_u32(m, c.eax.wrapping_add(4), c.edx);
    c.ecx = p.u32(m, 0x18d2ce8);
    c.edx = p.u32(m, c.edi.wrapping_add(0xe4));
    c.ecx = c.ecx.wrapping_add(1);
    p.set_u32(m, c.edx.wrapping_add(4), c.ecx);
    let state = p.u32(m, c.esi.wrapping_add(0x5c)) | 2;
    p.set_u32(m, c.esi.wrapping_add(0x5c), state);
    if p.u8(m, c.esi.wrapping_add(0x5c)) & 0x80 != 0 {
        save_textures(m, &p, c);
    }
    allocate(m, &p, c, 0x910, 0x1418, 0xefd524);
    if c.eax != 0 {
        c.edx = p.u32(m, c.edi.wrapping_add(0xdc));
        p.set_u32(m, c.eax, c.edx);
        c.ecx = p.u32(m, c.edi.wrapping_add(0xe0));
        p.set_u32(m, c.eax.wrapping_add(4), c.ecx);
        c.edx = p.u32(m, c.edi.wrapping_add(0xe4));
        p.set_u32(m, c.eax.wrapping_add(8), c.edx);
    }
    let state = c.fl.alu_and(p.u32(m, c.esi.wrapping_add(0x5c)), !2, 32);
    p.set_u32(m, c.esi.wrapping_add(0x5c), state);
    c.ecx = p.u32(m, c.ebx.wrapping_add(0xa4));
    c.eax = p.u32(m, c.edi);
    c.edx = p.u32(m, c.eax.wrapping_add(0x70));
    push(m, &p, c, c.ecx);
    push(m, &p, c, c.esi);
    c.ecx = c.edi;
    call(m, &p, c, c.edx, 0xefd570);
    if p.u32(m, c.ebx.wrapping_add(0xa8)) != 1 {
        c.xmm[0] = [p.u32(m, c.esp.wrapping_add(0x10)), 0, 0, 0];
        p.set_u32(m, c.esi.wrapping_add(0x184), c.xmm[0][0]);
        c.xmm[0] = [p.u32(m, c.esp.wrapping_add(0x14)), 0, 0, 0];
        p.set_u32(m, c.esi.wrapping_add(0x188), c.xmm[0][0]);
        c.xmm[0] = [p.u32(m, c.esp.wrapping_add(0x18)), 0, 0, 0];
        p.set_u32(m, c.esi.wrapping_add(0x18c), c.xmm[0][0]);
    }
    c.eax = p.u32(m, c.edi.wrapping_add(0x150));
    c.eax = c.fl.alu_add(c.eax, p.u32(m, c.ebp.wrapping_add(0x10)), 32);
    push(m, &p, c, 0);
    p.set_u32(m, c.esi.wrapping_add(0x180), c.eax);
    c.ecx = p.u32(m, 0x18d09e0);
    c.edx = p.u32(m, c.ecx);
    c.eax = p.u32(m, c.edx.wrapping_add(0x40));
    push(m, &p, c, c.esi);
    call(m, &p, c, c.eax, 0xefd5c2);
    p.set_u32(m, c.esi.wrapping_add(0x180), 0);
    c.fl.alu_sub(p.u32(m, c.edi.wrapping_add(0x64)), 4, 32);
    if c.fl.a != c.fl.b {
        c.eax = 1;
        c.fl.alu_sub(p.u32(m, c.edi.wrapping_add(0x68)), c.eax, 32);
        if c.fl.a == c.fl.b {
            c.fl.alu_sub(p.u32(m, c.ebx.wrapping_add(0xa8)), c.eax, 32);
            if c.fl.a != c.fl.b {
                c.ecx = p.u32(m, c.ebx);
                c.eax = c.ebx.wrapping_add(0x90);
                push(m, &p, c, c.eax);
                push(m, &p, c, c.ecx);
                push(m, &p, c, c.esi);
                c.ecx = c.edi;
                call(m, &p, c, 0xeea130, 0xefd5f6);
                c.eax = p.u32(m, c.edi.wrapping_add(0x34));
                c.ecx = p.u32(m, c.ebx);
                c.edx = p.u32(m, c.edi);
                c.edx = p.u32(m, c.edx.wrapping_add(0x68));
                push(m, &p, c, c.eax);
                push(m, &p, c, c.ecx);
                push(m, &p, c, c.esi);
                c.ecx = c.edi;
                call(m, &p, c, c.edx, 0xefd607);
            }
        }
    }
    c.ecx = c.esi;
    call(m, &p, c, 0xfedc00, 0xefd60e);
    c.edi = p.u32(m, c.esp);
    c.esp = c.esp.wrapping_add(4);
    c.esi = p.u32(m, c.esp);
    c.esp = c.esp.wrapping_add(4);
    c.ebx = p.u32(m, c.esp);
    c.esp = c.ebp;
    c.ebp = p.u32(m, c.esp);
    c.esp = c.esp.wrapping_add(20);
}

#[inline]
fn allocate(m: &mut dyn Mem, p: &Pages, c: &mut Cpu, descriptor: u32, destination: u32, resume: u32) {
    c.eax = p.u32(m, c.esi.wrapping_add(0x18));
    c.eax = p.u32(m, c.eax.wrapping_add(descriptor));
    c.ecx = p.u16(m, c.eax.wrapping_add(0x18)) as u32;
    c.edx = p.u32(m, c.esi.wrapping_add(0x24));
    c.ecx = (c.ecx.wrapping_mul(4).wrapping_add(15)) & !15;
    c.edx = c.edx.wrapping_add(c.ecx);
    p.set_u32(m, c.esp.wrapping_add(12), c.ecx);
    c.fl.alu_sub(c.edx, p.u32(m, c.esi.wrapping_add(0x28)), 32);
    if c.fl.a > c.fl.b {
        push(m, p, c, c.ecx);
        c.ecx = c.esi;
        call(m, p, c, 0xfed890, resume);
        c.ecx = p.u32(m, c.esp.wrapping_add(12));
    }
    c.eax = p.u32(m, c.esi.wrapping_add(0x24));
    c.ecx = c.ecx.wrapping_add(c.eax);
    p.set_u32(m, c.esi.wrapping_add(0x24), c.ecx);
    p.set_u32(m, c.esi.wrapping_add(destination), c.eax);
    let dirty = p.u32(m, c.esi.wrapping_add(0x154)) | 1;
    p.set_u32(m, c.esi.wrapping_add(0x154), dirty);
}

// The three command types use different scratch registers. Preserve them
// explicitly instead of treating the structures as host records. Offsets
// and operation codes below are instructions at EFD2AE/EFD3B0/EFD49A.
fn save_constants(m: &mut dyn Mem, p: &Pages, c: &mut Cpu) {
    c.eax = p.u32(m, c.esi.wrapping_add(0x6c));
    c.ecx = c.eax.wrapping_mul(3);
    c.edx = p.u32(m, c.esi.wrapping_add(c.ecx.wrapping_mul(4)).wrapping_add(0x30c7c));
    c.eax = c.esi.wrapping_add(c.ecx.wrapping_mul(4)).wrapping_add(0x30c7c);
    c.edx = (c.edx & 0x80000245) | 0x245;
    p.set_u32(m, c.eax, c.edx);
    c.eax = p.u32(m, c.esi.wrapping_add(0x6c));
    c.eax = c.eax.wrapping_mul(3);
    let address = c.esi.wrapping_add(c.eax.wrapping_mul(4)).wrapping_add(0x30c7c);
    let bits = c.fl.alu_and(p.u32(m, address), 0x7fffffff, 32);
    p.set_u32(m, address, bits);
    c.ecx = p.u32(m, c.esi.wrapping_add(0x1420));
    c.eax = address;
    c.eax = p.u32(m, c.esi.wrapping_add(0x6c));
    c.eax = c.eax.wrapping_mul(3);
    p.set_u32(
        m,
        c.esi.wrapping_add(c.eax.wrapping_mul(4)).wrapping_add(0x30c80),
        c.ecx,
    );
    c.edx = p.u32(m, c.esi.wrapping_add(0x1424));
    p.set_u32(
        m,
        c.esi.wrapping_add(c.eax.wrapping_mul(4)).wrapping_add(0x30c84),
        c.edx,
    );
    let count = p.u32(m, c.esi.wrapping_add(0x6c)).wrapping_add(1);
    p.set_u32(m, c.esi.wrapping_add(0x6c), count);
}

fn save_shader(m: &mut dyn Mem, p: &Pages, c: &mut Cpu) {
    c.eax = p.u32(m, c.esi.wrapping_add(0x6c));
    c.edx = c.eax.wrapping_mul(3);
    c.eax = c.esi.wrapping_add(c.edx.wrapping_mul(4)).wrapping_add(0x30c7c);
    c.edx = p.u32(m, c.eax);
    c.edx = (c.edx & 0x800006ca) | 0x6ca;
    p.set_u32(m, c.eax, c.edx);
    c.eax = p.u32(m, c.esi.wrapping_add(0x6c));
    c.eax = c.eax.wrapping_mul(3);
    let address = c.esi.wrapping_add(c.eax.wrapping_mul(4)).wrapping_add(0x30c7c);
    let bits = c.fl.alu_and(p.u32(m, address), 0x7fffffff, 32);
    p.set_u32(m, address, bits);
    c.edx = p.u32(m, c.esi.wrapping_add(0x3848));
    c.eax = address;
    c.eax = p.u32(m, c.esi.wrapping_add(0x6c));
    c.eax = c.eax.wrapping_mul(3);
    p.set_u32(
        m,
        c.esi.wrapping_add(c.eax.wrapping_mul(4)).wrapping_add(0x30c80),
        c.edx,
    );
    c.edx = p.u32(m, c.esi.wrapping_add(0x384c));
    p.set_u32(
        m,
        c.esi.wrapping_add(c.eax.wrapping_mul(4)).wrapping_add(0x30c84),
        c.edx,
    );
    let count = p.u32(m, c.esi.wrapping_add(0x6c)).wrapping_add(1);
    p.set_u32(m, c.esi.wrapping_add(0x6c), count);
}

fn save_textures(m: &mut dyn Mem, p: &Pages, c: &mut Cpu) {
    c.eax = p.u32(m, c.esi.wrapping_add(0x6c));
    c.eax = c.eax.wrapping_mul(3);
    c.ecx = p.u32(m, c.esi.wrapping_add(c.eax.wrapping_mul(4)).wrapping_add(0x30c7c));
    c.eax = c.esi.wrapping_add(c.eax.wrapping_mul(4)).wrapping_add(0x30c7c);
    c.ecx = (c.ecx & 0x80000244) | 0x244;
    p.set_u32(m, c.eax, c.ecx);
    c.eax = p.u32(m, c.esi.wrapping_add(0x6c));
    c.edx = c.eax.wrapping_mul(3);
    let address = c.esi.wrapping_add(c.edx.wrapping_mul(4)).wrapping_add(0x30c7c);
    let bits = c.fl.alu_and(p.u32(m, address), 0x7fffffff, 32);
    p.set_u32(m, address, bits);
    c.ecx = p.u32(m, c.esi.wrapping_add(0x1418));
    c.eax = address;
    c.eax = p.u32(m, c.esi.wrapping_add(0x6c));
    c.eax = c.eax.wrapping_mul(3);
    p.set_u32(
        m,
        c.esi.wrapping_add(c.eax.wrapping_mul(4)).wrapping_add(0x30c80),
        c.ecx,
    );
    c.edx = p.u32(m, c.esi.wrapping_add(0x141c));
    p.set_u32(
        m,
        c.esi.wrapping_add(c.eax.wrapping_mul(4)).wrapping_add(0x30c84),
        c.edx,
    );
    let count = p.u32(m, c.esi.wrapping_add(0x6c)).wrapping_add(1);
    p.set_u32(m, c.esi.wrapping_add(0x6c), count);
}

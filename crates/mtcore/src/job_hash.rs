//! Dependency-key hashing used below the MT job callbacks.
use ddnative_base::{mem::{Mem, Pages}, x86::Cpu};

// Translated from 0x00E0E5E0 (DDDA.exe 2017-12-13 Steam).
/// Hash floor(byte_count / 4) words, ignoring the trailing bytes. Original
/// multiply-then-XOR order (FNV-1), with a word salt before byte extraction.
/// Keep guest spills and ordered reads: the input may overlap the stack.
pub fn dependency_hash(m: &mut dyn Mem, c: &mut Cpu) {
    let pages = Pages::of(m);
    c.eax = pages.u32(m, c.esp.wrapping_add(12));
    c.esp = c.esp.wrapping_sub(4);
    pages.set_u32(m, c.esp, c.esi);
    c.esi = pages.u32(m, c.esp.wrapping_add(12)) >> 2;
    c.edx = 0;
    c.fl.alu_and(c.esi, c.esi, 32);
    if c.esi != 0 {
        c.esp = c.esp.wrapping_sub(4);
        pages.set_u32(m, c.esp, c.ebx);
        c.esp = c.esp.wrapping_sub(4);
        pages.set_u32(m, c.esp, c.edi);
        c.edi = pages.u32(m, c.esp.wrapping_add(16));
        loop {
            // Code immediates at E0E603/E0E609 (multiplier/salt). These
            // are not an embedded data table or host-selected hash policy.
            c.ecx = pages.u32(m, c.edi.wrapping_add(c.edx.wrapping_mul(4)));
            c.ecx ^= 0x14da14da;
            let h0 = c.eax.wrapping_mul(0x01000193) ^ (c.ecx & 0xff);
            let h1 = h0.wrapping_mul(0x01000193) ^ ((c.ecx >> 8) & 0xff);
            c.ebx = h1.wrapping_mul(0x01000193);
            c.eax = (c.ebx ^ ((c.ecx >> 16) & 0xff)).wrapping_mul(0x01000193);
            c.ecx >>= 24;
            c.edx = c.edx.wrapping_add(1);
            c.eax ^= c.ecx;
            // The final CMP replaces all defined arithmetic flags from
            // the inner operations, and remains visible at the next read.
            c.fl.alu_sub(c.edx, c.esi, 32);
            if c.edx >= c.esi { break; }
        }
        c.edi = pages.u32(m, c.esp);
        c.esp = c.esp.wrapping_add(4);
        c.ebx = pages.u32(m, c.esp);
        c.esp = c.esp.wrapping_add(4);
    }
    c.esi = pages.u32(m, c.esp);
    c.esp = c.esp.wrapping_add(20);
}

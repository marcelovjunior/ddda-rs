//! Models of the platform services that lifted code calls (allocators,
//! CRT memory routines). The platform layer is replaced, not translated
//! (docs/NATIVE_PIPELINE.md, "The platform layer"); these models define
//! what a replaced service does at the machine level, so code that calls
//! it can still be lifted 1:1 around the call.
//!
//! Each model starts where the original function starts (the return
//! address at [esp]) and ends with the same `ret`. research/native/
//! harness.py runs the identical model in place of the original service
//! while it records vectors, so a vector verifies the caller's own code.
//!
//! Contract shared by every model: only the registers listed change
//! (eax, and esp by the `ret`); flags, xmm and the x87 stack are kept.
use crate::mem::Mem;
use crate::x86::Cpu;

/// The services modelled here: (address, name, model). harness.PLATFORM
/// lists the same addresses.
pub const MODELS: &[(u32, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    (0x00CF73F0, "pool40_alloc", pool40_alloc),
    (0x00DE0B80, "mt_alloc", mt_alloc),
    (0x00DE0B90, "mt_free", mt_free),
    (0x012EFE20, "memset", memset),
    (0x012EFEA0, "memcpy", memcpy),
];

fn ret(m: &mut dyn Mem, cpu: &mut Cpu, pop: u32) {
    let _ = m.u32(cpu.esp); // the return address
    cpu.esp = cpu.esp.wrapping_add(4 + pop);
}

fn arg(m: &mut dyn Mem, cpu: &Cpu, k: u32) -> u32 {
    m.u32(cpu.esp.wrapping_add(4 + 4 * k))
}

/// 0x00CF73F0: the global pool of 0x28-byte nodes (free list at
/// 0x01877DA0, blocks of 0x400 nodes, a critical section around it).
/// No arguments; eax = a node.
pub fn pool40_alloc(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = m.alloc(0x28);
    ret(m, cpu, 0);
}

/// 0x00DE0B80: MT allocator, allocate `ecx` bytes (forwards to the global
/// allocator object at 0x018D0AA0). eax = the block.
pub fn mt_alloc(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = m.alloc(cpu.ecx);
    ret(m, cpu, 0);
}

/// 0x00DE0B90: MT allocator, free the block in `ecx` (the global
/// allocator's vtable slot +0x38). No observable effect in the model.
pub fn mt_free(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret(m, cpu, 0);
}

/// 0x012EFE20: CRT memset(dst, c, n), cdecl. eax = dst.
pub fn memset(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dst, c, n) = (arg(m, cpu, 0), arg(m, cpu, 1) as u8, arg(m, cpu, 2));
    for k in 0..n {
        m.set_u8(dst.wrapping_add(k), c);
    }
    cpu.eax = dst;
    ret(m, cpu, 0);
}

/// 0x012EFEA0: CRT memcpy(dst, src, n), cdecl; this CRT's memcpy handles
/// overlap like memmove. eax = dst.
pub fn memcpy(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dst, src, n) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let bytes: Vec<u8> = (0..n).map(|k| m.u8(src.wrapping_add(k))).collect();
    for (k, b) in bytes.into_iter().enumerate() {
        m.set_u8(dst.wrapping_add(k as u32), b);
    }
    cpu.eax = dst;
    ret(m, cpu, 0);
}

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

thread_local! {
    /// Which models (bit k = MODELS[k]) this thread sends to the dispatcher.
    static REDIRECTED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// For the runtime (crates/ddruntime): on this thread, calls to the models
/// at `addrs` go to `x86::call_ext` at the model's address (the original
/// body or a runtime service bound there) instead of the model. Tests and
/// the recorder never call this, so vectors keep verifying against the
/// models.
pub fn redirect(addrs: &[u32]) {
    let mask = MODELS.iter().enumerate().filter(|(_, (a, _, _))| addrs.contains(a)).fold(0, |m, (k, _)| m | 1 << k);
    REDIRECTED.with(|r| r.set(mask));
}

/// Whether the model at MODELS[k] was redirected (and the call made).
fn redirected(m: &mut dyn Mem, cpu: &mut Cpu, k: usize) -> bool {
    if REDIRECTED.with(|r| r.get()) & (1 << k) == 0 {
        return false;
    }
    crate::x86::call_ext(m, cpu, MODELS[k].0);
    true
}

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
    if redirected(m, cpu, 0) {
        return;
    }
    cpu.eax = m.alloc(0x28);
    ret(m, cpu, 0);
}

/// 0x00DE0B80: MT allocator, allocate `ecx` bytes (forwards to the global
/// allocator object at 0x018D0AA0). eax = the block.
pub fn mt_alloc(m: &mut dyn Mem, cpu: &mut Cpu) {
    if redirected(m, cpu, 1) {
        return;
    }
    cpu.eax = m.alloc(cpu.ecx);
    ret(m, cpu, 0);
}

/// 0x00DE0B90: MT allocator, free the block in `ecx` (the global
/// allocator's vtable slot +0x38). No observable effect in the model.
pub fn mt_free(m: &mut dyn Mem, cpu: &mut Cpu) {
    if redirected(m, cpu, 2) {
        return;
    }
    ret(m, cpu, 0);
}

/// 0x012EFE20: CRT memset(dst, c, n), cdecl. eax = dst.
pub fn memset(m: &mut dyn Mem, cpu: &mut Cpu) {
    if redirected(m, cpu, 3) {
        return;
    }
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
    if redirected(m, cpu, 4) {
        return;
    }
    let (dst, src, n) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let bytes: Vec<u8> = (0..n).map(|k| m.u8(src.wrapping_add(k))).collect();
    for (k, b) in bytes.into_iter().enumerate() {
        m.set_u8(dst.wrapping_add(k as u32), b);
    }
    cpu.eax = dst;
    ret(m, cpu, 0);
}

/// The answers of instructions that report the machine rather than compute
/// (L22): CPUID and RDTSC. The runtime installs its recorded machine profile
/// (crates/ddruntime/src/machine.rs, consistent with the registry's CPU
/// key); the lifted test installs the answers a vector recorded.
pub trait Machine {
    fn cpuid(&self, leaf: u32, subleaf: u32) -> [u32; 4];
    fn rdtsc(&self) -> u64;
}

thread_local! {
    static MACHINE: std::cell::RefCell<Option<std::rc::Rc<dyn Machine>>> = const { std::cell::RefCell::new(None) };
}

/// Install this thread's machine; returns the previous one.
pub fn set_machine(machine: Option<std::rc::Rc<dyn Machine>>) -> Option<std::rc::Rc<dyn Machine>> {
    MACHINE.with(|m| std::mem::replace(&mut *m.borrow_mut(), machine))
}

fn machine(what: &str) -> std::rc::Rc<dyn Machine> {
    MACHINE.with(|m| m.borrow().clone()).unwrap_or_else(|| panic!("{what}: no machine installed on this thread"))
}

/// CPUID: leaf in EAX, subleaf in ECX; results in EAX, EBX, ECX, EDX.
pub fn cpuid(cpu: &mut Cpu) {
    let [a, b, c, d] = machine("cpuid").cpuid(cpu.eax, cpu.ecx);
    (cpu.eax, cpu.ebx, cpu.ecx, cpu.edx) = (a, b, c, d);
}

/// RDTSC: the time-stamp counter in EDX:EAX.
pub fn rdtsc(cpu: &mut Cpu) {
    let t = machine("rdtsc").rdtsc();
    (cpu.eax, cpu.edx) = (t as u32, (t >> 32) as u32);
}

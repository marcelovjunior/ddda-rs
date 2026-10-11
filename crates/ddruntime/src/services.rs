//! Runtime services: Rust replacements for platform code (the C runtime
//! now; OS and device services later) bound at the original's addresses
//! (docs/RUNTIME.md, "Platform services and guest threads").
//!
//! Each service has the ABI of the function it replaces: it starts with
//! the return address at [esp], reads its arguments where the original
//! does, sets eax as the original returns it, and ends with the original's
//! `ret` (cdecl: the caller pops the arguments). Every binding cites the
//! original address and how it was identified. A service replaces the
//! CRT's internal bookkeeping (lock, encoded-pointer tables, heap headers)
//! with the runtime's own; what the guest can observe through its return
//! values and the memory it is handed is the same.
use crate::dispatch::{Binding, Conflict, Registry};
use crate::memory::{GuestMemory, Overlap, READ, WRITE};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use std::cell::RefCell;
use std::collections::HashMap;

/// Where the guest heap lives (an address range the image and stack do not
/// use). The original heap's addresses are not reproduced; nothing in the
/// game may depend on them.
pub const HEAP_BASE: u32 = 0x2000_0000;
pub const HEAP_SIZE: u32 = 0x4000_0000;

/// The guest heap: 16-byte aligned blocks from a lazily backed region,
/// with exact-size reuse of freed blocks.
#[derive(Default)]
pub struct Heap {
    next: u32,
    sizes: HashMap<u32, u32>,
    free: HashMap<u32, Vec<u32>>,
}

impl Heap {
    /// A block of `size` bytes (0 is a valid, distinct block, like malloc(0)),
    /// or None when the region is exhausted.
    pub fn alloc(&mut self, size: u32) -> Option<u32> {
        let rounded = size.max(1).checked_add(15)? & !15;
        if let Some(b) = self.free.get_mut(&rounded).and_then(|v| v.pop()) {
            self.sizes.insert(b, rounded);
            return Some(b);
        }
        if self.next == 0 {
            self.next = HEAP_BASE;
        }
        let b = self.next;
        let end = b.checked_add(rounded)?;
        if end > HEAP_BASE + HEAP_SIZE {
            return None;
        }
        self.next = end;
        self.sizes.insert(b, rounded);
        Some(b)
    }

    /// Release a block; freeing something that is not a live block is a
    /// guest bug the runtime reports instead of corrupting the heap.
    pub fn free(&mut self, block: u32) -> Result<(), u32> {
        let size = self.sizes.remove(&block).ok_or(block)?;
        self.free.entry(size).or_default().push(block);
        Ok(())
    }

    pub fn size_of(&self, block: u32) -> Option<u32> {
        self.sizes.get(&block).copied()
    }
}

#[derive(Default)]
struct State {
    heap: Heap,
    /// Functions registered with `_onexit`/`atexit`, in registration order
    /// (the CRT runs them in reverse at exit).
    exit_list: Vec<u32>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Functions registered to run at exit, in registration order.
pub fn exit_list() -> Vec<u32> {
    STATE.with(|s| s.borrow().exit_list.clone())
}

pub fn heap_size_of(block: u32) -> Option<u32> {
    STATE.with(|s| s.borrow().heap.size_of(block))
}

fn arg(m: &dyn Mem, cpu: &Cpu, k: u32) -> u32 {
    m.u32(cpu.esp.wrapping_add(4 + 4 * k))
}

/// `ret N`: pop the return address and N bytes of arguments.
fn ret(m: &dyn Mem, cpu: &mut Cpu, n: u32) {
    let _ = m.u32(cpu.esp);
    cpu.esp = cpu.esp.wrapping_add(4 + n);
}

fn zero(m: &mut dyn Mem, block: u32, size: u32) {
    for k in 0..size {
        m.set_u8(block.wrapping_add(k), 0);
    }
}

/// 0x012ED89C: CRT `__onexit` (Ghidra: library function, single match,
/// Visual Studio 2010 Release). cdecl `_onexit_t __onexit(_onexit_t func)`:
/// registers `func` and returns it (NULL only when the table cannot grow).
pub fn onexit(m: &mut dyn Mem, cpu: &mut Cpu) {
    let func = arg(m, cpu, 0);
    STATE.with(|s| s.borrow_mut().exit_list.push(func));
    cpu.eax = func;
    ret(m, cpu, 0);
}

/// 0x012F8177: CRT `__calloc_crt` (Ghidra: library function, single
/// match, Visual Studio 2010 Release). cdecl
/// `void *__calloc_crt(size_t count, size_t size)`: a zeroed block of
/// count * size bytes, or NULL (the original retries with growing waits
/// before giving up; the runtime heap either has the space or not).
pub fn calloc_crt(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (count, size) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let block = count.checked_mul(size).and_then(|n| STATE.with(|s| s.borrow_mut().heap.alloc(n)).map(|b| (b, n)));
    cpu.eax = match block {
        Some((b, n)) => {
            zero(m, b, n);
            b
        }
        None => 0,
    };
    ret(m, cpu, 0);
}

/// Map the heap region and bind every service at its address.
pub fn install(mem: &mut GuestMemory, registry: &mut Registry) -> Result<(), InstallError> {
    mem.map_lazy(HEAP_BASE, HEAP_SIZE, READ | WRITE).map_err(InstallError::Overlap)?;
    for (addr, f) in SERVICES {
        registry.insert(*addr, Binding::Service(*f)).map_err(InstallError::Conflict)?;
    }
    Ok(())
}

/// Every service and the original address it replaces.
pub const SERVICES: &[(u32, fn(&mut dyn Mem, &mut Cpu))] = &[
    (0x012E_D89C, onexit),
    (0x012F_8177, calloc_crt),
];

#[derive(Debug)]
pub enum InstallError {
    Overlap(Overlap),
    Conflict(Conflict),
}

//! Win32 API services the C runtime's start-up and the game's early code
//! use (KERNEL32 mostly). Each follows the documented contract (Microsoft
//! Learn) with stdcall cleanup; behaviour that depends on the machine is
//! chosen deliberately and labelled (code pages, processor count, the
//! absence of a console). Single guest thread until stage R05.
use crate::services::{arg, ret, with_state, zero};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;


/// Handle values the runtime gives out (any value the guest only passes
/// back is fine; these are distinct from pointers the guest might hold).
pub const PROCESS_HEAP: u32 = 0x0F00_0000;
pub const PSEUDO_PROCESS: u32 = 0xFFFF_FFFF;
pub const MAIN_PROCESS_ID: u32 = 0x0000_2000;
/// GetModuleHandle(NULL): the image base.
pub const IMAGE_BASE: u32 = 0x0040_0000;

const ERROR_PROC_NOT_FOUND: u32 = 127;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const HEAP_ZERO_MEMORY: u32 = 0x8;

type Api = fn(&mut dyn Mem, &mut Cpu);

fn set_error(e: u32) {
    crate::runtime::set_last_error(e);
}

fn write_bytes(m: &mut dyn Mem, at: u32, data: &[u8]) {
    for (k, &b) in data.iter().enumerate() {
        m.set_u8(at.wrapping_add(k as u32), b);
    }
}

/// A string the runtime hands the guest once and keeps (GetCommandLineA,
/// GetEnvironmentStringsW): allocated on the guest heap on first use.
pub(crate) fn persistent(m: &mut dyn Mem, key: &'static str, bytes: &[u8]) -> u32 {
    if let Some(p) = with_state(|s| s.strings.get(key).copied()) {
        return p;
    }
    let p = with_state(|s| s.heap.alloc(bytes.len() as u32)).expect("guest heap");
    write_bytes(m, p, bytes);
    with_state(|s| s.strings.insert(key, p));
    p
}

fn exe_path() -> String {
    with_state(|s| s.exe_path.clone())
}

// --- errors, identity, time ------------------------------------------------

pub fn get_last_error(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = crate::runtime::last_error();
    ret(m, cpu, 0);
}
pub fn set_last_error(m: &mut dyn Mem, cpu: &mut Cpu) {
    set_error(arg(m, cpu, 0));
    ret(m, cpu, 4);
}
pub fn get_current_process_id(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = MAIN_PROCESS_ID;
    ret(m, cpu, 0);
}
pub fn get_current_process(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = PSEUDO_PROCESS; // the documented pseudo handle (HANDLE)-1
    ret(m, cpu, 0);
}
pub fn is_debugger_present(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 0);
}

/// QueryPerformanceFrequency(LARGE_INTEGER*): a 10 MHz counter (the
/// frequency Windows 10 reports on most machines).
pub const QPC_FREQUENCY: u64 = 10_000_000;
pub fn query_performance_frequency(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    m.set_u32(p, QPC_FREQUENCY as u32);
    m.set_u32(p.wrapping_add(4), (QPC_FREQUENCY >> 32) as u32);
    cpu.eax = 1;
    ret(m, cpu, 4);
}
/// QueryPerformanceCounter(LARGE_INTEGER*): the runtime clock in 100 ns ticks.
pub fn query_performance_counter(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    let t = (crate::services::runtime_clock().as_nanos() / 100) as u64;
    m.set_u32(p, t as u32);
    m.set_u32(p.wrapping_add(4), (t >> 32) as u32);
    cpu.eax = 1;
    ret(m, cpu, 4);
}
/// GetSystemTimeAsFileTime(FILETIME*): host UTC time, 100 ns since 1601.
pub fn get_system_time_as_file_time(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    let since_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let t = (since_unix.as_nanos() / 100) as u64 + 116_444_736_000_000_000;
    m.set_u32(p, t as u32);
    m.set_u32(p.wrapping_add(4), (t >> 32) as u32);
    ret(m, cpu, 4);
}

/// GetSystemInfo(SYSTEM_INFO*), 36 bytes. The game sizes its worker pool
/// from the processor count. Serial mode: one processor, since one guest
/// thread runs at a time (crate::threads). Parallel mode: the host's
/// logical processors (at most 32, the width of the 32-bit mask).
pub fn get_system_info(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    let n = if crate::threads::parallel() {
        std::thread::available_parallelism().map_or(1, |n| n.get()).min(32) as u32
    } else {
        1
    };
    zero(m, p, 36);
    m.set_u16(p, 0); // PROCESSOR_ARCHITECTURE_INTEL
    m.set_u32(p + 4, 0x1000); // dwPageSize
    m.set_u32(p + 8, 0x0001_0000); // lpMinimumApplicationAddress
    // lpMaximumApplicationAddress: DDDA.exe is large-address-aware, so on
    // 64-bit Windows its user space is 4 GB (Microsoft Learn, "Memory Limits
    // for Windows and Windows Server Releases"); VIRTUAL_RANGES uses it.
    m.set_u32(p + 12, 0xFFFE_FFFF);
    m.set_u32(p + 16, (1u64 << n).wrapping_sub(1) as u32); // dwActiveProcessorMask
    m.set_u32(p + 20, n); // dwNumberOfProcessors
    m.set_u32(p + 24, 586); // dwProcessorType PROCESSOR_INTEL_PENTIUM
    m.set_u32(p + 28, 0x1_0000); // dwAllocationGranularity
    m.set_u16(p + 32, 6); // wProcessorLevel
    ret(m, cpu, 4);
}

// --- interlocked ----------------------------------------------------------

// The Interlocked functions are atomic read-modify-writes with a full
// barrier (a lock-prefixed instruction on x86): host atomics on the guest
// word (SeqCst), so they stay atomic while other guest threads run
// translated code at the same time (crate::threads' parallel mode).
// Windows requires the operand to be 32-bit aligned; an unaligned or
// faulting operand takes the ordinary accessors.
use std::sync::atomic::Ordering::SeqCst;
pub fn interlocked_increment(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    cpu.eax = match m.atomic_u32(p) {
        Some(a) => a.fetch_add(1, SeqCst).wrapping_add(1),
        None => {
            let v = m.u32(p).wrapping_add(1);
            m.set_u32(p, v);
            v
        }
    };
    ret(m, cpu, 4);
}
pub fn interlocked_decrement(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    cpu.eax = match m.atomic_u32(p) {
        Some(a) => a.fetch_sub(1, SeqCst).wrapping_sub(1),
        None => {
            let v = m.u32(p).wrapping_sub(1);
            m.set_u32(p, v);
            v
        }
    };
    ret(m, cpu, 4);
}
pub fn interlocked_exchange(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (p, v) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = match m.atomic_u32(p) {
        Some(a) => a.swap(v, SeqCst),
        None => {
            let old = m.u32(p);
            m.set_u32(p, v);
            old
        }
    };
    ret(m, cpu, 8);
}
pub fn interlocked_exchange_add(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (p, v) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = match m.atomic_u32(p) {
        Some(a) => a.fetch_add(v, SeqCst),
        None => {
            let old = m.u32(p);
            m.set_u32(p, old.wrapping_add(v));
            old
        }
    };
    ret(m, cpu, 8);
}
pub fn interlocked_compare_exchange(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (p, exchange, comparand) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    cpu.eax = match m.atomic_u32(p) {
        Some(a) => a.compare_exchange(comparand, exchange, SeqCst, SeqCst).unwrap_or_else(|old| old),
        None => {
            let old = m.u32(p);
            if old == comparand {
                m.set_u32(p, exchange);
            }
            old
        }
    };
    ret(m, cpu, 12);
}

// --- TLS --------------------------------------------------------------------

/// Allocation generations are shared, values and last-error belong to the
/// calling host/guest thread. Odd = allocated, even = free. Allocation/free
/// are cold and serialized; Get/Set need only one atomic generation read.
/// Reuse changes the generation so every thread observes an initial zero.
pub(crate) struct TlsSlots {
    allocation: std::sync::Mutex<()>,
    generation: [std::sync::atomic::AtomicU64; 1088],
}
impl Default for TlsSlots {
    fn default() -> Self {
        Self { allocation: std::sync::Mutex::new(()), generation: [const { std::sync::atomic::AtomicU64::new(0) }; 1088] }
    }
}
impl TlsSlots {
    fn alloc(&self) -> u32 {
        let _lock = self.allocation.lock().unwrap_or_else(|e| e.into_inner());
        for (i, slot) in self.generation.iter().enumerate() {
            let generation = slot.load(SeqCst);
            if generation & 1 == 0 {
                slot.store(generation.checked_add(1).expect("TLS generation exhausted"), SeqCst);
                return i as u32;
            }
        }
        u32::MAX
    }
    fn free(&self, i: usize) -> bool {
        let _lock = self.allocation.lock().unwrap_or_else(|e| e.into_inner());
        let Some(slot) = self.generation.get(i) else { return false; };
        let generation = slot.load(SeqCst);
        if generation & 1 == 0 { return false; }
        slot.store(generation.checked_add(1).expect("TLS generation exhausted"), SeqCst);
        true
    }
    fn generation(&self, i: usize) -> Option<u64> {
        self.generation.get(i).map(|g| g.load(SeqCst))
    }
}

/// TlsAlloc(): lowest free index; TLS_OUT_OF_INDEXES after 1088.
pub fn tls_alloc(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = crate::runtime::with_thread(|rt, c| {
        let index = rt.tls.alloc();
        if index == u32::MAX { c.last_error.set(ERROR_NOT_ENOUGH_MEMORY); }
        index
    });
    ret(m, cpu, 0);
}
/// Microsoft Learn, TlsGetValue: success clears last-error, including zero.
/// Range validation follows the host Win32 oracle (0..1088), including
/// unallocated slots; allocation/free generations clear prior values.
pub fn tls_get_value(m: &mut dyn Mem, cpu: &mut Cpu) {
    let i = arg(m, cpu, 0) as usize;
    cpu.eax = crate::runtime::with_thread(|rt, c| match rt.tls.generation(i) {
        Some(generation) => {
            c.last_error.set(0);
            let (epoch, value) = c.tls[i].get();
            if epoch == generation { value } else { 0 }
        }
        None => { c.last_error.set(ERROR_INVALID_PARAMETER); 0 }
    });
    ret(m, cpu, 4);
}
pub fn tls_set_value(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (i, value) = (arg(m, cpu, 0) as usize, arg(m, cpu, 1));
    cpu.eax = crate::runtime::with_thread(|rt, c| match rt.tls.generation(i) {
        Some(generation) => { c.tls[i].set((generation, value)); 1 }
        None => { c.last_error.set(ERROR_INVALID_PARAMETER); 0 }
    });
    ret(m, cpu, 8);
}
pub fn tls_free(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = crate::runtime::with_thread(|rt, c| {
        let freed = rt.tls.free(arg(m, cpu, 0) as usize);
        if !freed { c.last_error.set(ERROR_INVALID_PARAMETER); }
        u32::from(freed)
    });
    ret(m, cpu, 4);
}

// --- heaps (every Win32 heap is the one guest heap) ------------------------

pub fn get_process_heap(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = PROCESS_HEAP;
    ret(m, cpu, 0);
}
/// HeapCreate(options, initial, maximum): a handle; all heaps share the
/// guest heap (blocks are distinct either way).
pub fn heap_create(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = PROCESS_HEAP + 0x10;
    ret(m, cpu, 12);
}
pub fn heap_set_information(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1;
    ret(m, cpu, 16);
}
/// HeapAlloc(heap, flags, size): NULL on exhaustion; HEAP_ZERO_MEMORY zeroes.
pub fn heap_alloc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (flags, size) = (arg(m, cpu, 1), arg(m, cpu, 2));
    let block = with_state(|s| s.heap.alloc(size));
    if let (Some(b), true) = (block, flags & HEAP_ZERO_MEMORY != 0) {
        zero(m, b, size);
    }
    cpu.eax = block.unwrap_or(0);
    ret(m, cpu, 12);
}
/// HeapFree(heap, flags, block): TRUE; NULL is allowed; freeing anything
/// else that is not a live block stops the runtime (a guest heap bug).
pub fn heap_free(m: &mut dyn Mem, cpu: &mut Cpu) {
    let block = arg(m, cpu, 2);
    if block != 0 {
        with_state(|s| s.heap.free(block)).unwrap_or_else(|b| panic!("HeapFree of {b:#x}, not a live block"));
    }
    cpu.eax = 1;
    ret(m, cpu, 12);
}
/// HeapSize(heap, flags, block): the requested size; (SIZE_T)-1 on error.
pub fn heap_size(m: &mut dyn Mem, cpu: &mut Cpu) {
    let block = arg(m, cpu, 2);
    cpu.eax = with_state(|s| s.heap.requested_size(block)).unwrap_or(u32::MAX);
    ret(m, cpu, 12);
}
/// HeapReAlloc(heap, flags, block, size): a new block with the old contents
/// (zero-extended under HEAP_ZERO_MEMORY), the old one freed.
pub fn heap_re_alloc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (flags, block, size) = (arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    let old = with_state(|s| s.heap.requested_size(block)).unwrap_or_else(|| panic!("HeapReAlloc of {block:#x}"));
    let new = with_state(|s| s.heap.alloc(size));
    cpu.eax = match new {
        Some(n) => {
            for k in 0..old.min(size) {
                let b = m.u8(block.wrapping_add(k));
                m.set_u8(n.wrapping_add(k), b);
            }
            if flags & HEAP_ZERO_MEMORY != 0 && size > old {
                zero(m, n.wrapping_add(old), size - old);
            }
            with_state(|s| s.heap.free(block)).expect("live");
            n
        }
        None => 0,
    };
    ret(m, cpu, 16);
}

// --- virtual memory: reservations and commits ------------------------------

const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const MEM_DECOMMIT: u32 = 0x4000;
const MEM_RELEASE: u32 = 0x8000;
const ERROR_INVALID_ADDRESS: u32 = 487;
/// The allocation granularity (GetSystemInfo's dwAllocationGranularity).
const GRANULARITY: u32 = 0x1_0000;

/// The address ranges VirtualAlloc places regions in, searched bottom-up
/// (as Windows does without MEM_TOP_DOWN): below the thread stacks, and,
/// because DDDA.exe is large-address-aware (IMAGE_FILE_LARGE_ADDRESS_AWARE
/// in its PE header) and gets 4 GB of user space on 64-bit Windows, above
/// 2 GB. The rest of the space holds the runtime's own regions (image,
/// heap, stacks, thread blocks, tokens). Exact placement differs from a
/// real process (whose space also holds the system DLLs); the guest only
/// sees that regions are 64 KB-aligned and do not overlap.
pub const VIRTUAL_RANGES: [(u32, u32); 2] = [(0x6000_0000, 0x7800_0000), (0x8000_0000, 0xFF00_0000)];

/// Reserved regions: base -> size (page-rounded).
#[derive(Default)]
pub struct VirtualSpace {
    regions: std::collections::BTreeMap<u32, u32>,
}

impl VirtualSpace {
    /// The region holding [addr, addr + len), if one holds all of it.
    fn region_of(&self, addr: u32, len: u32) -> Option<(u32, u32)> {
        let (&b, &n) = self.regions.range(..=addr).next_back()?;
        (addr as u64 + len as u64 <= b as u64 + n as u64).then_some((b, n))
    }

    /// The first region overlapping [base, base + len).
    fn overlapping(&self, base: u32, len: u32) -> Option<(u32, u32)> {
        let end = base as u64 + len as u64;
        self.regions.iter().map(|(&b, &n)| (b, n)).find(|&(b, n)| (b as u64) < end && (base as u64) < b as u64 + n as u64)
    }

    fn overlaps(&self, base: u32, len: u32) -> bool {
        self.overlapping(base, len).is_some()
    }

    /// The lowest free 64 KB-aligned place for `len` bytes.
    fn find(&self, len: u32) -> Option<u32> {
        for (lo, hi) in VIRTUAL_RANGES {
            let mut at = lo as u64;
            while at + len as u64 <= hi as u64 {
                match self.overlapping(at as u32, len) {
                    None => return Some(at as u32),
                    Some((b, n)) => at = (b as u64 + n as u64).next_multiple_of(GRANULARITY as u64),
                }
            }
        }
        None
    }
}

fn with_space<R>(f: impl FnOnce(&mut crate::memory::GuestMemory) -> R) -> R {
    crate::runtime::with(|rt| rt.machine.with(|mm| {
        let machine = mm.as_mut().expect("VirtualAlloc outside a session (no guest machine)");
        f(&mut machine.memory)
    }))
}

/// Commit the pages of [addr, addr + len) that are not committed yet (new
/// pages read as zero; committed ones keep their contents).
fn commit(addr: u32, len: u32, perm: u8) {
    with_space(|mem| {
        let mut p = addr & !0xFFF;
        while (p as u64) < addr as u64 + len as u64 {
            if !mem.is_mapped(p) {
                mem.map_lazy(p, 0x1000, perm).expect("an uncommitted page of a reservation");
            }
            p += 0x1000;
        }
    });
}

/// VirtualAlloc(lpAddress, dwSize, flAllocationType, flProtect) (Microsoft
/// Learn "VirtualAlloc function (memoryapi.h)"), with the answers measured on
/// Windows 11 (research/runtime/windows/Virtual.cs):
/// - MEM_RESERVE reserves the pages spanning [lpAddress, lpAddress + dwSize)
///   from a 64 KB-aligned base (lpAddress rounded down; NULL: the lowest
///   free place, see [`VIRTUAL_RANGES`]); reserved pages are not accessible.
/// - MEM_COMMIT commits pages of an existing reservation (lpAddress rounded
///   down to its page; already committed pages keep their contents) and
///   returns that page address; with lpAddress NULL it reserves and commits
///   a new region. MEM_RESERVE | MEM_COMMIT does both.
/// - NULL with ERROR_INVALID_ADDRESS (487) for a range that is in use
///   (reserving) or not inside one reservation (committing), with
///   ERROR_INVALID_PARAMETER (87) for dwSize 0, no MEM_COMMIT/MEM_RESERVE,
///   flProtect 0. NULL with ERROR_NOT_ENOUGH_MEMORY when no place is left.
///   Success leaves the last error alone.
/// Modelled protections: PAGE_READONLY, PAGE_READWRITE; the other flags
/// (MEM_RESET, MEM_TOP_DOWN, write watch, large pages, execute and guard
/// pages) stop the session. Callers: MtVirtualAllocator 0x00D11B60/vf11/vf12
/// (reserve 512 MB), its page commits 0x00D10E90, 0x00CF5530 (commit).
pub fn virtual_alloc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (addr, size, kind, prot) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    let r = (|| -> Result<u32, u32> {
        if size == 0 || kind & (MEM_COMMIT | MEM_RESERVE) == 0 || prot == 0 {
            return Err(ERROR_INVALID_PARAMETER);
        }
        if kind & !(MEM_COMMIT | MEM_RESERVE) != 0 {
            panic!("VirtualAlloc flAllocationType {kind:#x} is not modelled (crate::win32)");
        }
        let perm = match prot {
            0x02 => crate::memory::READ,
            0x04 => crate::memory::READ | crate::memory::WRITE,
            _ => panic!("VirtualAlloc flProtect {prot:#x} is not modelled (crate::win32)"),
        };
        if kind & MEM_RESERVE != 0 || addr == 0 {
            let (base, len) = if addr == 0 {
                let len = size.div_ceil(0x1000) * 0x1000;
                let base = with_state(|s| s.virtual_space.find(len)).ok_or(ERROR_NOT_ENOUGH_MEMORY)?;
                (base, len)
            } else {
                let base = addr & !(GRANULARITY - 1);
                let end = (addr as u64 + size as u64).div_ceil(0x1000) * 0x1000;
                let len = u32::try_from(end - base as u64).map_err(|_| ERROR_INVALID_ADDRESS)?;
                let inside = VIRTUAL_RANGES.iter().any(|&(lo, hi)| base >= lo && end <= hi as u64);
                if !inside || with_state(|s| s.virtual_space.overlaps(base, len)) {
                    return Err(ERROR_INVALID_ADDRESS);
                }
                (base, len)
            };
            with_state(|s| s.virtual_space.regions.insert(base, len));
            if kind & MEM_COMMIT != 0 {
                commit(base, len, perm);
            }
            return Ok(base);
        }
        // MEM_COMMIT inside a reservation
        let page = addr & !0xFFF;
        let len = ((addr as u64 + size as u64).div_ceil(0x1000) * 0x1000 - page as u64) as u32;
        with_state(|s| s.virtual_space.region_of(page, len)).ok_or(ERROR_INVALID_ADDRESS)?;
        commit(page, len, perm);
        Ok(page)
    })();
    cpu.eax = r.unwrap_or_else(|e| {
        set_error(e);
        0
    });
    ret(m, cpu, 16);
}

/// VirtualFree(lpAddress, dwSize, dwFreeType) (Microsoft Learn "VirtualFree
/// function (memoryapi.h)"), measured on Windows 11 (Virtual.cs):
/// MEM_DECOMMIT decommits the pages spanning [lpAddress, lpAddress + dwSize)
/// of one reservation (dwSize 0 at the region's base: all of it; decommitting
/// decommitted pages succeeds); MEM_RELEASE (dwSize 0, lpAddress a region's
/// base) frees the whole region. FALSE with ERROR_INVALID_PARAMETER (87) for
/// another dwFreeType (both or neither) or MEM_RELEASE with a size,
/// ERROR_INVALID_ADDRESS (487) for an address that is no region (base).
/// Success leaves the last error alone. Callers: 0x00CF5560/0x00CF5580,
/// MtVirtualAllocator's destructor and page release 0x00D10F60.
pub fn virtual_free(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (addr, size, kind) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let r = (|| -> Result<(), u32> {
        match kind {
            MEM_RELEASE => {
                if size != 0 {
                    return Err(ERROR_INVALID_PARAMETER);
                }
                let len = with_state(|s| s.virtual_space.regions.remove(&addr)).ok_or(ERROR_INVALID_ADDRESS)?;
                with_space(|mem| mem.unmap(addr, len));
                Ok(())
            }
            MEM_DECOMMIT => {
                let (page, len) = if size == 0 {
                    let len = with_state(|s| s.virtual_space.regions.get(&addr).copied());
                    match len {
                        Some(n) => (addr, n),
                        None => panic!("VirtualFree(MEM_DECOMMIT, size 0) of {addr:#x}, not a region base, is not modelled (crate::win32)"),
                    }
                } else {
                    let page = addr & !0xFFF;
                    (page, ((addr as u64 + size as u64).div_ceil(0x1000) * 0x1000 - page as u64) as u32)
                };
                with_state(|s| s.virtual_space.region_of(page, len)).ok_or(ERROR_INVALID_ADDRESS)?;
                with_space(|mem| mem.unmap(page, len));
                Ok(())
            }
            _ => Err(ERROR_INVALID_PARAMETER),
        }
    })();
    cpu.eax = match r {
        Ok(()) => 1,
        Err(e) => {
            set_error(e);
            0
        }
    };
    ret(m, cpu, 12);
}

// --- process start-up information -----------------------------------------

/// GetStartupInfoW(STARTUPINFOW*): cb = 68, nothing else set (a normal GUI
/// launch: no inherited handles or window state).
pub fn get_startup_info_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    zero(m, p, 68);
    m.set_u32(p, 68);
    ret(m, cpu, 4);
}
/// GetCommandLineA(): "\"<exe path>\"", kept for the process lifetime.
pub fn get_command_line_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let line = format!("\"{}\"\0", exe_path());
    cpu.eax = persistent(m, "command line A", line.as_bytes());
    ret(m, cpu, 0);
}
/// GetEnvironmentStringsW(): the guest's environment block, which starts
/// empty (a terminating pair of NUL characters: the guest sees no host
/// variables) and holds what SetEnvironmentVariableA set
/// (crate::system::environment_block). Like Windows, each call returns a
/// fresh copy for FreeEnvironmentStringsW to free.
pub fn get_environment_strings_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    let block = crate::system::environment_block();
    let p = with_state(|s| s.heap.alloc(2 * block.len() as u32)).expect("guest heap");
    for (k, &u) in block.iter().enumerate() {
        m.set_u16(p + 2 * k as u32, u);
    }
    with_state(|s| s.environment_blocks.push(p));
    cpu.eax = p;
    ret(m, cpu, 0);
}
/// FreeEnvironmentStringsW(block): frees a block GetEnvironmentStringsW
/// returned; TRUE.
pub fn free_environment_strings_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    let ours = with_state(|s| match s.environment_blocks.iter().position(|&b| b == p) {
        Some(k) => {
            s.environment_blocks.swap_remove(k);
            true
        }
        None => false,
    });
    if ours {
        with_state(|s| s.heap.free(p)).expect("an environment block is a live heap block");
    }
    cpu.eax = 1;
    ret(m, cpu, 4);
}
/// GetModuleHandleA/W(name): NULL is the exe (its image base). Other
/// modules are not loaded in the runtime: NULL with ERROR_MOD_NOT_FOUND,
/// as Windows answers for a module that is not loaded.
/// The system DLLs every process has loaded (a module handle for them is
/// answered by GetModuleHandle without LoadLibrary).
const SYSTEM_MODULES: &[&str] = &["kernel32.dll", "ntdll.dll", "kernelbase.dll", "user32.dll", "gdi32.dll", "advapi32.dll"];

/// A module name as Windows compares it: the file name, lower case, with
/// ".dll" added when it has no extension.
fn module_key(name: &str) -> String {
    let file = name.rsplit(['\\', '/']).next().unwrap_or(name).to_ascii_lowercase();
    if file.contains('.') { file } else { format!("{file}.dll") }
}

/// The handle of a module the process has (a system DLL, a DLL the exe
/// imports, or one loaded with LoadLibrary when `load`).
fn module_handle(name: &str, load: bool) -> Option<u32> {
    let key = module_key(name);
    with_state(|s| {
        if let Some(k) = s.modules.iter().position(|m| *m == key) {
            return Some(MODULE_HANDLE + 0x1_0000 * (k as u32 + 1));
        }
        let known = SYSTEM_MODULES.contains(&key.as_str()) || s.exports.keys().any(|(dll, _)| *dll == key);
        (known || load).then(|| {
            s.modules.push(key);
            MODULE_HANDLE + 0x1_0000 * s.modules.len() as u32
        })
    })
}

fn module_name(h: u32) -> Option<String> {
    let k = h.checked_sub(MODULE_HANDLE)? / 0x1_0000;
    with_state(|s| s.modules.get(k.checked_sub(1)? as usize).cloned())
}

fn get_module_handle(m: &mut dyn Mem, cpu: &mut Cpu, wide: bool) {
    let p = arg(m, cpu, 0);
    cpu.eax = if p == 0 {
        IMAGE_BASE
    } else {
        let name = if wide { crate::user32::guest_str_w(m, p) } else { crate::user32::guest_str_a(m, p) };
        module_handle(&name, false).unwrap_or_else(|| {
            set_error(126); // ERROR_MOD_NOT_FOUND
            0
        })
    };
    ret(m, cpu, 4);
}
/// GetModuleHandleA/W(name): NULL is the exe; system DLLs and the DLLs the
/// exe imports have handles; anything else ERROR_MOD_NOT_FOUND.
pub fn get_module_handle_a(m: &mut dyn Mem, cpu: &mut Cpu) { get_module_handle(m, cpu, false) }
pub fn get_module_handle_w(m: &mut dyn Mem, cpu: &mut Cpu) { get_module_handle(m, cpu, true) }

/// GetProcAddress(module, name): a function the exe imports from that
/// module resolves to its bound token (the same service as the import);
/// anything else is NULL with ERROR_PROC_NOT_FOUND, as on a system without
/// that export (the CRT's FlsAlloc probe then takes its documented TLS
/// fallback). Ordinals are not resolved.
pub fn get_proc_address(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, p) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let found = if p >= 0x1_0000 {
        let name = crate::user32::guest_str_a(m, p);
        module_name(h).and_then(|dll| with_state(|s| s.exports.get(&(dll, name)).copied()))
    } else {
        None
    };
    cpu.eax = found.unwrap_or_else(|| {
        set_error(ERROR_PROC_NOT_FOUND);
        0
    });
    ret(m, cpu, 8);
}
/// GetModuleFileNameA/W(module, buffer, size) (Microsoft Learn
/// "GetModuleFileNameA function (libloaderapi.h)"): for the exe (NULL or
/// its image base) its path with a NUL; returns the characters copied and
/// sets the last error to 0. If the path does not fit, the first size - 1
/// characters and a NUL are written, the result is size and the last error
/// ERROR_INSUFFICIENT_BUFFER (size 0 writes nothing and returns 0); a
/// handle that is no module fails with ERROR_MOD_NOT_FOUND (all measured,
/// research/runtime/windows/ModuleName.cs). The DLLs' paths are not
/// modelled (stop). The CRT's `__setargv` reads argv[0] here.
fn module_file_name(m: &mut dyn Mem, cpu: &mut Cpu, wide: bool) {
    let (h, buf, size) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    if h != 0 && h != IMAGE_BASE {
        if module_name(h).is_some() {
            panic!("GetModuleFileName of module {h:#x} ({:?}) is not modelled (crate::win32)", module_name(h));
        }
        set_error(126); // ERROR_MOD_NOT_FOUND
        cpu.eax = 0;
        return ret(m, cpu, 12);
    }
    let path = exe_path();
    // the A form: code page 1252, '?' for what it cannot hold
    let units: Vec<u16> = if wide {
        path.encode_utf16().collect()
    } else {
        path.encode_utf16().map(|u| crate::text::encode_1252(u, true).unwrap_or(b'?') as u16).collect()
    };
    let put = |m: &mut dyn Mem, k: usize, u: u16| {
        if wide {
            m.set_u16(buf.wrapping_add(2 * k as u32), u);
        } else {
            m.set_u8(buf.wrapping_add(k as u32), u as u8);
        }
    };
    cpu.eax = if (units.len() as u32) < size {
        for (k, &u) in units.iter().chain([0u16].iter()).enumerate() {
            put(m, k, u);
        }
        set_error(0);
        units.len() as u32
    } else {
        if size > 0 {
            for (k, &u) in units[..size as usize - 1].iter().chain([0u16].iter()).enumerate() {
                put(m, k, u);
            }
        }
        set_error(ERROR_INSUFFICIENT_BUFFER);
        size
    };
    ret(m, cpu, 12);
}
pub fn get_module_file_name_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    module_file_name(m, cpu, false)
}
pub fn get_module_file_name_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    module_file_name(m, cpu, true)
}

// --- code pages: an English (United States) Windows -----------------------

pub fn get_acp(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1252;
    ret(m, cpu, 0);
}
pub fn get_oemcp(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 437;
    ret(m, cpu, 0);
}
/// The code pages answered, as Windows 11 (en-US) answers GetCPInfo and
/// IsValidCodePage for them (measured, research/runtime/windows/CpInfo.cs):
/// (code page, IsValidCodePage, CPINFO MaxCharSize, lead-byte ranges).
/// GetCPInfo also takes the symbolic pages CP_ACP 0 and CP_THREAD_ACP 3
/// (1252 here), CP_OEMCP 1 (437) and CP_MACCP 2 (10000, single-byte), which
/// IsValidCodePage rejects. Every page's DefaultChar is '?'.
const CODE_PAGES: &[(u32, bool, u32, &[(u8, u8)])] = &[
    (0, false, 1, &[]),
    (1, false, 1, &[]),
    (2, false, 1, &[]),
    (3, false, 1, &[]),
    (437, true, 1, &[]),
    (850, true, 1, &[]),
    (932, true, 2, &[(0x81, 0x9F), (0xE0, 0xFC)]),
    (1252, true, 1, &[]),
    (65000, true, 5, &[]),
    (65001, true, 4, &[]),
];
/// Code pages Windows does not have (measured: GetCPInfo fails with
/// ERROR_INVALID_PARAMETER, IsValidCodePage answers FALSE).
const NO_CODE_PAGES: &[u32] = &[42, 12345, 0xFFFF];

fn code_page_entry(api: &str, cp: u32) -> Option<&'static (u32, bool, u32, &'static [(u8, u8)])> {
    match CODE_PAGES.iter().find(|e| e.0 == cp) {
        Some(e) => Some(e),
        None if NO_CODE_PAGES.contains(&cp) => None,
        None => panic!("{api}({cp}): code page {cp} is not modelled (crate::win32 CODE_PAGES)"),
    }
}

/// IsValidCodePage(codepage): TRUE for an installed code page; FALSE for
/// the symbolic CP_* values and unknown pages (measured). The CRT's
/// `__setmbcp_nolock` asks it for the ANSI code page.
pub fn is_valid_code_page(m: &mut dyn Mem, cpu: &mut Cpu) {
    let cp = arg(m, cpu, 0);
    cpu.eax = code_page_entry("IsValidCodePage", cp).is_some_and(|e| e.1) as u32;
    ret(m, cpu, 4);
}
/// GetCPInfo(codepage, CPINFO*): CPINFO is MaxCharSize (DWORD), DefaultChar
/// [2], LeadByte[12] (pairs of first/last lead byte, zero-terminated), 18
/// bytes; FALSE with ERROR_INVALID_PARAMETER for an unknown page, the
/// CPINFO untouched (measured). Callers: the CRT's `__setmbcp_nolock`,
/// `setSBUpLow` 0x012FABB0, `__crtCompareStringA_stat`.
pub fn get_cp_info(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (cp, p) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = match code_page_entry("GetCPInfo", cp) {
        Some(&(_, _, max, leads)) => {
            zero(m, p, 18);
            m.set_u32(p, max);
            m.set_u8(p + 4, b'?');
            for (k, &(lo, hi)) in leads.iter().enumerate() {
                m.set_u8(p + 6 + 2 * k as u32, lo);
                m.set_u8(p + 7 + 2 * k as u32, hi);
            }
            1
        }
        None => {
            set_error(ERROR_INVALID_PARAMETER);
            0
        }
    };
    ret(m, cpu, 8);
}

// --- standard handles: a GUI process without a console ----------------------

/// GetStdHandle(nStdHandle): what SetStdHandle (crate::system) last set,
/// NULL at first (a GUI process without a console, measured);
/// INVALID_HANDLE_VALUE with ERROR_INVALID_HANDLE for another nStdHandle.
pub fn get_std_handle(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = match crate::system::std_index(arg(m, cpu, 0)) {
        Some(i) => with_state(|s| s.system.std_handles[i]),
        None => {
            set_error(6); // ERROR_INVALID_HANDLE
            u32::MAX
        }
    };
    ret(m, cpu, 4);
}
pub fn get_file_type(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0; // FILE_TYPE_UNKNOWN
    ret(m, cpu, 4);
}
pub fn set_handle_count(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = arg(m, cpu, 0);
    ret(m, cpu, 4);
}

/// The guest called ExitProcess / TerminateProcess: carried as a panic
/// payload so the host can stop cleanly with the exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit(pub u32);
pub fn exit_process(m: &mut dyn Mem, cpu: &mut Cpu) {
    std::panic::panic_any(Exit(arg(m, cpu, 0)));
}
pub fn terminate_process(m: &mut dyn Mem, cpu: &mut Cpu) {
    std::panic::panic_any(Exit(arg(m, cpu, 1)));
}

/// LoadLibraryA/W(name): the runtime loads no DLLs; it hands back a module
/// handle so presence checks pass (WinMain refuses to start without
/// WMVCORE.DLL), and GetProcAddress on it finds nothing.
pub const MODULE_HANDLE: u32 = 0x7FD0_0000;
/// LoadLibraryA/W(name): a module handle (the runtime provides the DLLs'
/// services through the import table; presence is what callers such as
/// WinMain's WMVCORE check test).
fn load_library(m: &mut dyn Mem, cpu: &mut Cpu, wide: bool) {
    let p = arg(m, cpu, 0);
    let name = if wide { crate::user32::guest_str_w(m, p) } else { crate::user32::guest_str_a(m, p) };
    cpu.eax = module_handle(&name, true).unwrap_or(0);
    ret(m, cpu, 4);
}
pub fn load_library_a(m: &mut dyn Mem, cpu: &mut Cpu) { load_library(m, cpu, false) }
pub fn load_library_w(m: &mut dyn Mem, cpu: &mut Cpu) { load_library(m, cpu, true) }
pub fn free_library(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1;
    ret(m, cpu, 4);
}
/// USER32 MessageBoxA(hwnd, text, caption, type): printed to stderr,
/// answered IDOK (1).
pub fn message_box_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let text = read_cstr(m, arg(m, cpu, 1));
    let caption = read_cstr(m, arg(m, cpu, 2));
    eprintln!("[guest MessageBox] {caption}: {text}");
    cpu.eax = 1;
    ret(m, cpu, 16);
}

fn read_cstr(m: &dyn Mem, p: u32) -> String {
    if p == 0 {
        return String::new();
    }
    (0..1024).map(|k| m.u8(p.wrapping_add(k))).take_while(|&b| b != 0).map(|b| b as char).collect()
}

/// ole32 CoInitialize(reserved): S_OK (the runtime's COM is the interface
/// objects it creates itself; there is no apartment to set up).
pub fn co_initialize(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 4);
}
pub fn co_uninitialize(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret(m, cpu, 0);
}
/// ole32 CoTaskMemAlloc(size) / CoTaskMemFree(block): the guest heap.
pub fn co_task_mem_alloc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let size = arg(m, cpu, 0);
    cpu.eax = with_state(|s| s.heap.alloc(size)).unwrap_or(0);
    ret(m, cpu, 4);
}
pub fn co_task_mem_free(m: &mut dyn Mem, cpu: &mut Cpu) {
    let block = arg(m, cpu, 0);
    if block != 0 {
        with_state(|s| s.heap.free(block)).unwrap_or_else(|b| panic!("CoTaskMemFree of {b:#x}, not a live block"));
    }
    ret(m, cpu, 4);
}

// --- USER32: a headless desktop ---------------------------------------------
// Until the presentation layer exists, windows are records the runtime
// keeps; metrics are those of Windows 10 at 96 DPI; the screen is the
// adapter's current display mode (crate::d3d9::current_mode: the host's
// desktop, or the game's full-screen mode).

const WS_THICKFRAME: u32 = 0x0004_0000;
const WS_CAPTION: u32 = 0x00C0_0000;
const WS_BORDER: u32 = 0x0080_0000;
/// The screen's size in pixels.
pub fn screen() -> (u32, u32) {
    let m = crate::d3d9::current_mode();
    (m.width, m.height)
}

/// AdjustWindowRect(RECT*, style, menu): grows the client rectangle by the
/// frame (3 px fixed, 4 px sizing frame), the caption (23 px) and a menu
/// bar (20 px) if asked. TRUE.
pub fn adjust_window_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (r, style, menu) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let mut rect = [0i32; 4];
    for (k, v) in rect.iter_mut().enumerate() {
        *v = m.u32(r + 4 * k as u32) as i32;
    }
    adjust_rect(&mut rect, style, menu != 0);
    for (k, v) in rect.into_iter().enumerate() {
        m.set_u32(r + 4 * k as u32, v as u32);
    }
    cpu.eax = 1;
    ret(m, cpu, 12);
}

/// AdjustWindowRect's arithmetic on (left, top, right, bottom).
pub(crate) fn adjust_rect(r: &mut [i32; 4], style: u32, menu: bool) {
    let frame: i32 = if style & WS_THICKFRAME != 0 { 4 } else if style & (WS_BORDER | WS_CAPTION) != 0 { 3 } else { 0 };
    let caption: i32 = if style & WS_CAPTION == WS_CAPTION { 23 } else { 0 };
    let menu_bar: i32 = if menu { 20 } else { 0 };
    r[0] -= frame;
    r[1] -= frame + caption + menu_bar;
    r[2] += frame;
    r[3] += frame;
}

/// SystemParametersInfoA(action, uparam, pvparam, winini): settings changes
/// are accepted and not applied (the host desktop is not the guest's); a
/// query fills nothing it does not know and still succeeds. TRUE.
pub fn system_parameters_info_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1;
    ret(m, cpu, 16);
}

/// GetSystemMetrics(index): the screen size; 0 for anything else.
pub fn get_system_metrics(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (w, h) = match arg(m, cpu, 0) {
        0 | 1 | 16 | 17 | 78 | 79 => screen(),
        _ => (0, 0),
    };
    cpu.eax = match arg(m, cpu, 0) {
        0 | 16 | 78 => w,                         // SM_CXSCREEN, SM_CXFULLSCREEN, SM_CXVIRTUALSCREEN
        1 | 79 => h,                              // SM_CYSCREEN, SM_CYVIRTUALSCREEN
        17 => h - 23,                             // SM_CYFULLSCREEN (minus caption)
        4 => 23,                                  // SM_CYCAPTION
        5 | 6 => 1,                               // SM_CXBORDER, SM_CYBORDER
        7 | 8 => 3,                               // SM_CXFIXEDFRAME, SM_CYFIXEDFRAME
        32 | 33 => 4,                             // SM_CXSIZEFRAME, SM_CYSIZEFRAME
        80 => 1,                                  // SM_CMONITORS
        _ => 0,
    };
    ret(m, cpu, 4);
}

pub const APIS: &[(&str, &str, Api)] = &[
    ("USER32.dll", "AdjustWindowRect", adjust_window_rect),
    ("USER32.dll", "SystemParametersInfoA", system_parameters_info_a),
    ("USER32.dll", "GetSystemMetrics", get_system_metrics),
    ("ole32.dll", "CoInitialize", co_initialize),
    ("ole32.dll", "CoUninitialize", co_uninitialize),
    ("ole32.dll", "CoTaskMemAlloc", co_task_mem_alloc),
    ("ole32.dll", "CoTaskMemFree", co_task_mem_free),
    ("KERNEL32.dll", "LoadLibraryA", load_library_a),
    ("KERNEL32.dll", "LoadLibraryW", load_library_w),
    ("KERNEL32.dll", "FreeLibrary", free_library),
    ("USER32.dll", "MessageBoxA", message_box_a),
    ("KERNEL32.dll", "GetLastError", get_last_error),
    ("KERNEL32.dll", "SetLastError", set_last_error),
    ("KERNEL32.dll", "GetCurrentProcessId", get_current_process_id),
    ("KERNEL32.dll", "GetCurrentProcess", get_current_process),
    ("KERNEL32.dll", "IsDebuggerPresent", is_debugger_present),
    ("KERNEL32.dll", "QueryPerformanceFrequency", query_performance_frequency),
    ("KERNEL32.dll", "QueryPerformanceCounter", query_performance_counter),
    ("KERNEL32.dll", "GetSystemTimeAsFileTime", get_system_time_as_file_time),
    ("KERNEL32.dll", "GetSystemInfo", get_system_info),
    ("KERNEL32.dll", "InterlockedIncrement", interlocked_increment),
    ("KERNEL32.dll", "InterlockedDecrement", interlocked_decrement),
    ("KERNEL32.dll", "InterlockedExchange", interlocked_exchange),
    ("KERNEL32.dll", "InterlockedExchangeAdd", interlocked_exchange_add),
    ("KERNEL32.dll", "InterlockedCompareExchange", interlocked_compare_exchange),
    ("KERNEL32.dll", "TlsAlloc", tls_alloc),
    ("KERNEL32.dll", "TlsGetValue", tls_get_value),
    ("KERNEL32.dll", "TlsSetValue", tls_set_value),
    ("KERNEL32.dll", "TlsFree", tls_free),
    ("KERNEL32.dll", "GetProcessHeap", get_process_heap),
    ("KERNEL32.dll", "HeapCreate", heap_create),
    ("KERNEL32.dll", "HeapSetInformation", heap_set_information),
    ("KERNEL32.dll", "HeapAlloc", heap_alloc),
    ("KERNEL32.dll", "HeapFree", heap_free),
    ("KERNEL32.dll", "HeapSize", heap_size),
    ("KERNEL32.dll", "HeapReAlloc", heap_re_alloc),
    ("KERNEL32.dll", "VirtualAlloc", virtual_alloc),
    ("KERNEL32.dll", "VirtualFree", virtual_free),
    ("KERNEL32.dll", "GetStartupInfoW", get_startup_info_w),
    ("KERNEL32.dll", "GetCommandLineA", get_command_line_a),
    ("KERNEL32.dll", "GetEnvironmentStringsW", get_environment_strings_w),
    ("KERNEL32.dll", "FreeEnvironmentStringsW", free_environment_strings_w),
    ("KERNEL32.dll", "GetModuleHandleA", get_module_handle_a),
    ("KERNEL32.dll", "GetModuleHandleW", get_module_handle_w),
    ("KERNEL32.dll", "GetProcAddress", get_proc_address),
    ("KERNEL32.dll", "GetModuleFileNameA", get_module_file_name_a),
    ("KERNEL32.dll", "GetModuleFileNameW", get_module_file_name_w),
    ("KERNEL32.dll", "GetACP", get_acp),
    ("KERNEL32.dll", "GetOEMCP", get_oemcp),
    ("KERNEL32.dll", "IsValidCodePage", is_valid_code_page),
    ("KERNEL32.dll", "GetCPInfo", get_cp_info),
    ("KERNEL32.dll", "GetStdHandle", get_std_handle),
    ("KERNEL32.dll", "GetFileType", get_file_type),
    ("KERNEL32.dll", "SetHandleCount", set_handle_count),
    ("KERNEL32.dll", "ExitProcess", exit_process),
    ("KERNEL32.dll", "TerminateProcess", terminate_process),
];

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
    /// live block -> (rounded size, requested size)
    sizes: HashMap<u32, (u32, u32)>,
    free: HashMap<u32, Vec<u32>>,
}

impl Heap {
    /// A block of `size` bytes (0 is a valid, distinct block, like malloc(0)),
    /// or None when the region is exhausted.
    pub fn alloc(&mut self, size: u32) -> Option<u32> {
        let rounded = size.max(1).checked_add(15)? & !15;
        if let Some(b) = self.free.get_mut(&rounded).and_then(|v| v.pop()) {
            self.sizes.insert(b, (rounded, size));
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
        self.sizes.insert(b, (rounded, size));
        Some(b)
    }

    /// Release a block; freeing something that is not a live block is a
    /// guest bug the runtime reports instead of corrupting the heap.
    pub fn free(&mut self, block: u32) -> Result<(), u32> {
        let (rounded, _) = self.sizes.remove(&block).ok_or(block)?;
        self.free.entry(rounded).or_default().push(block);
        Ok(())
    }

    /// The usable size of a live block (rounded to 16).
    pub fn size_of(&self, block: u32) -> Option<u32> {
        self.sizes.get(&block).map(|&(r, _)| r)
    }

    /// The size the block was requested with (what `HeapSize` reports).
    pub fn requested_size(&self, block: u32) -> Option<u32> {
        self.sizes.get(&block).map(|&(_, n)| n)
    }
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) heap: Heap,
    /// Functions registered with `_onexit`/`atexit`, in registration order
    /// (the CRT runs them in reverse at exit).
    exit_list: Vec<u32>,
    /// The guest's unhandled-exception filter (SetUnhandledExceptionFilter).
    exception_filter: u32,
    /// GetLastError / SetLastError.
    pub(crate) last_error: u32,
    /// TLS slots (TlsAlloc): None = free.
    pub(crate) tls: Vec<Option<u32>>,
    /// How often each TLS index was allocated (a thread's saved value
    /// belongs to one allocation; crate::threads).
    pub(crate) tls_epoch: Vec<u32>,
    /// VirtualAlloc's reservations (crate::win32).
    pub(crate) virtual_space: crate::win32::VirtualSpace,
    /// Strings the runtime hands the guest once (command line, environment).
    pub(crate) strings: HashMap<&'static str, u32>,
    /// Path of the exe the runtime loaded (GetModuleFileName, command line).
    pub(crate) exe_path: String,
    /// Interface objects the runtime created (crate::interfaces), by name.
    pub(crate) objects: HashMap<&'static str, u32>,
    /// Imported functions by (lower-case DLL name, function name): their
    /// tokens (GetProcAddress answers from this).
    pub(crate) exports: HashMap<(String, String), u32>,
    /// Module names the runtime handed out handles for, in handle order
    /// (crate::win32::MODULE_HANDLE + 0x10000 * (k + 1)).
    pub(crate) modules: Vec<String>,
    /// Global/Local memory, environment, standard handles (crate::system).
    pub(crate) system: crate::system::System,
    /// Open registry keys (crate::registry).
    pub(crate) registry: crate::registry::Keys,
    /// The input method state (crate::imm).
    pub(crate) ime: crate::imm::Ime,
    /// Blocks GetEnvironmentStringsW handed out and not yet freed.
    pub(crate) environment_blocks: Vec<u32>,
}

pub(crate) fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    crate::runtime::with(|rt| rt.state.with(f))
}

/// Record the loaded exe's path (GetModuleFileName, GetCommandLine).
pub fn set_exe_path(path: &str) {
    with_state(|s| s.exe_path = path.to_string());
}

/// Functions registered to run at exit, in registration order.
pub fn exit_list() -> Vec<u32> {
    with_state(|s| s.exit_list.clone())
}

pub fn heap_size_of(block: u32) -> Option<u32> {
    with_state(|s| s.heap.size_of(block))
}

pub(crate) fn arg(m: &dyn Mem, cpu: &Cpu, k: u32) -> u32 {
    m.u32(cpu.esp.wrapping_add(4 + 4 * k))
}

/// Where guest code the runtime calls (window procedures, I/O completion
/// routines, thread starts) returns to (never executed).
pub const CALLBACK_RETURN: u32 = 0xFFFF_FFE0;

/// Call guest code `target` as stdcall with `args`, from inside a service:
/// the arguments and a return address are pushed below the service's frame
/// and the dispatcher runs the target. eax is the result; ecx and edx are
/// the callee's to clobber (as across any call), the other registers are
/// preserved by the callee's own ABI and checked here.
pub fn call_guest(m: &mut dyn Mem, cpu: &mut Cpu, target: u32, args: &[u32]) -> u32 {
    let (esp, ebx, ebp, esi, edi) = (cpu.esp, cpu.ebx, cpu.ebp, cpu.esi, cpu.edi);
    for &a in args.iter().rev() {
        cpu.esp = cpu.esp.wrapping_sub(4);
        m.set_u32(cpu.esp, a);
    }
    cpu.esp = cpu.esp.wrapping_sub(4);
    m.set_u32(cpu.esp, CALLBACK_RETURN);
    ddnative_base::x86::call_ext(m, cpu, target);
    assert_eq!(cpu.esp, esp, "guest callback {target:#010x} did not return with stdcall cleanup of {} argument(s)", args.len());
    assert_eq!((cpu.ebx, cpu.ebp, cpu.esi, cpu.edi), (ebx, ebp, esi, edi), "guest callback {target:#010x} clobbered a callee-saved register");
    cpu.eax
}

/// `ret N`: pop the return address and N bytes of arguments.
pub(crate) fn ret(m: &dyn Mem, cpu: &mut Cpu, n: u32) {
    let _ = m.u32(cpu.esp);
    cpu.esp = cpu.esp.wrapping_add(4 + n);
}

pub(crate) fn zero(m: &mut dyn Mem, block: u32, size: u32) {
    for k in 0..size {
        m.set_u8(block.wrapping_add(k), 0);
    }
}

/// 0x012ED89C: CRT `__onexit` (Ghidra: library function, single match,
/// Visual Studio 2010 Release). cdecl `_onexit_t __onexit(_onexit_t func)`:
/// registers `func` and returns it (NULL only when the table cannot grow).
pub fn onexit(m: &mut dyn Mem, cpu: &mut Cpu) {
    let func = arg(m, cpu, 0);
    with_state(|s| s.exit_list.push(func));
    cpu.eax = func;
    ret(m, cpu, 0);
}

/// KERNEL32 `EncodePointer` / `DecodePointer` (stdcall, one argument): the
/// identity. The real ones XOR with a per-process secret; the guest only ever
/// passes an encoded value back through the matching call, so the identity
/// is observably the same.
pub fn encode_pointer(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = arg(m, cpu, 0);
    ret(m, cpu, 4);
}

/// The main guest thread's id (any nonzero value is a valid Windows thread
/// id; crate::threads numbers the others after it).
pub const MAIN_THREAD_ID: u32 = 0x0000_1000;

// CRITICAL_SECTION (winnt.h RTL_CRITICAL_SECTION, 24 bytes on x86):
// +0 DebugInfo, +4 LockCount, +8 RecursionCount, +12 OwningThread,
// +16 LockSemaphore, +20 SpinCount.

/// KERNEL32 `InitializeCriticalSection(cs)` (stdcall): the documented
/// initial state: unowned, LockCount -1, recursion 0. DebugInfo is set to
/// -1 as Windows 8 and later do when no debug info is allocated (not
/// observed for this game; nothing in it is known to read the field).
pub fn initialize_critical_section(m: &mut dyn Mem, cpu: &mut Cpu) {
    let cs = arg(m, cpu, 0);
    for (off, v) in [(0, u32::MAX), (4, u32::MAX), (8, 0), (12, 0), (16, 0), (20, 0)] {
        m.set_u32(cs.wrapping_add(off), v);
    }
    ret(m, cpu, 4);
}

/// KERNEL32 `InitializeCriticalSectionAndSpinCount(cs, spin)` (stdcall):
/// as InitializeCriticalSection, spin count stored; returns TRUE.
pub fn initialize_critical_section_and_spin_count(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (cs, spin) = (arg(m, cpu, 0), arg(m, cpu, 1));
    for (off, v) in [(0, u32::MAX), (4, u32::MAX), (8, 0), (12, 0), (16, 0), (20, spin)] {
        m.set_u32(cs.wrapping_add(off), v);
    }
    cpu.eax = 1;
    ret(m, cpu, 8);
}

/// KERNEL32 `EnterCriticalSection(cs)` (stdcall): takes ownership or
/// recurses; while another guest thread owns it, waits without the guest
/// lock (crate::threads) until it is released. LockCount counts the
/// owner's entries (waiters are not counted in it).
pub fn enter_critical_section(m: &mut dyn Mem, cpu: &mut Cpu) {
    let cs = arg(m, cpu, 0);
    let me = crate::threads::current_tid();
    loop {
        let owner = m.u32(cs.wrapping_add(12));
        if owner == 0 || owner == me {
            break;
        }
        crate::threads::block(None);
    }
    m.set_u32(cs.wrapping_add(4), m.u32(cs.wrapping_add(4)).wrapping_add(1));
    m.set_u32(cs.wrapping_add(8), m.u32(cs.wrapping_add(8)).wrapping_add(1));
    m.set_u32(cs.wrapping_add(12), me);
    ret(m, cpu, 4);
}

/// KERNEL32 `LeaveCriticalSection(cs)` (stdcall): undoes one Enter.
pub fn leave_critical_section(m: &mut dyn Mem, cpu: &mut Cpu) {
    let cs = arg(m, cpu, 0);
    let rec = m.u32(cs.wrapping_add(8));
    assert!(rec > 0 && m.u32(cs.wrapping_add(12)) == crate::threads::current_tid(),
            "LeaveCriticalSection({cs:#x}) without a matching Enter");
    m.set_u32(cs.wrapping_add(8), rec - 1);
    m.set_u32(cs.wrapping_add(4), m.u32(cs.wrapping_add(4)).wrapping_sub(1));
    if rec == 1 {
        m.set_u32(cs.wrapping_add(12), 0);
        crate::threads::signal();
    }
    ret(m, cpu, 4);
}

/// KERNEL32 `DeleteCriticalSection(cs)` (stdcall): nothing to release.
pub fn delete_critical_section(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret(m, cpu, 4);
}

/// KERNEL32 `IsProcessorFeaturePresent(feature)` (stdcall): the host CPU's
/// answer for the features a 32-bit x86 program asks about (winnt.h
/// PF_* values); FALSE for anything else.
pub fn is_processor_feature_present(m: &mut dyn Mem, cpu: &mut Cpu) {
    let feature = arg(m, cpu, 0);
    // winnt.h: 0 FLOATING_POINT_PRECISION_ERRATA, 1 FLOATING_POINT_EMULATED,
    // 2 COMPARE_EXCHANGE_DOUBLE, 3 MMX, 6 XMMI (SSE), 7 3DNOW, 8 RDTSC,
    // 9 PAE_ENABLED, 10 XMMI64 (SSE2), 12 NX_ENABLED, 13 SSE3,
    // 17 COMPARE_EXCHANGE128
    #[cfg(target_arch = "x86_64")]
    let present = match feature {
        2 | 3 | 6 | 8 | 9 | 10 | 12 => true, // every x86-64 CPU and Windows has these
        13 => std::arch::is_x86_feature_detected!("sse3"),
        17 => std::arch::is_x86_feature_detected!("cmpxchg16b"),
        _ => false, // errata, emulation, 3DNow!, and anything not listed
    };
    #[cfg(not(target_arch = "x86_64"))]
    let present = false;
    cpu.eax = present as u32;
    ret(m, cpu, 4);
}

/// KERNEL32 `SetUnhandledExceptionFilter(filter)` (stdcall): records the
/// filter, returns the previous one (none before). The runtime does not
/// raise guest exceptions yet, so the filter is never called.
pub fn set_unhandled_exception_filter(m: &mut dyn Mem, cpu: &mut Cpu) {
    let filter = arg(m, cpu, 0);
    cpu.eax = with_state(|s| std::mem::replace(&mut s.exception_filter, filter));
    ret(m, cpu, 4);
}

/// Where the guest's "known folders" point: a sandbox, never the user's
/// real Documents (ddda-rs must not read or write the player's saves
/// without being asked). `DDRUNTIME_HOME` overrides the default
/// `%TEMP%\ddruntime`.
pub fn sandbox_home() -> std::path::PathBuf {
    std::env::var_os("DDRUNTIME_HOME").map(Into::into)
        .unwrap_or_else(|| std::env::temp_dir().join("ddruntime"))
}

/// SHELL32 `SHGetFolderPathW(hwnd, csidl, token, flags, path)` (stdcall,
/// 5 arguments): writes the folder as UTF-16 (at most MAX_PATH including
/// the terminator) into `path` and returns S_OK. Every folder maps into the
/// sandbox, named by its CSIDL.
pub fn sh_get_folder_path_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (csidl, path) = (arg(m, cpu, 1), arg(m, cpu, 4));
    let dir = sandbox_home().join(format!("csidl-{:#06x}", csidl & 0xFF));
    let _ = std::fs::create_dir_all(&dir);
    let wide: Vec<u16> = dir.to_string_lossy().encode_utf16().take(259).chain([0]).collect();
    for (k, w) in wide.iter().enumerate() {
        m.set_u16(path.wrapping_add(2 * k as u32), *w);
    }
    cpu.eax = 0; // S_OK
    ret(m, cpu, 20);
}

/// KERNEL32 `GetTickCount()` (stdcall, no arguments): milliseconds since
/// the runtime started (Windows counts from boot; the guest only uses
/// differences). Wraps at 2^32 like the original.
pub fn get_tick_count(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = runtime_clock().as_millis() as u32;
    ret(m, cpu, 0);
}

/// Time since the runtime's first clock read.
pub fn runtime_clock() -> std::time::Duration {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed()
}

/// Windows API services, by DLL (case-insensitive) and function name.
pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("KERNEL32.dll", "EncodePointer", encode_pointer),
    ("KERNEL32.dll", "DecodePointer", encode_pointer),
    ("KERNEL32.dll", "InitializeCriticalSection", initialize_critical_section),
    ("KERNEL32.dll", "InitializeCriticalSectionAndSpinCount", initialize_critical_section_and_spin_count),
    ("KERNEL32.dll", "EnterCriticalSection", enter_critical_section),
    ("KERNEL32.dll", "LeaveCriticalSection", leave_critical_section),
    ("KERNEL32.dll", "DeleteCriticalSection", delete_critical_section),
    ("KERNEL32.dll", "IsProcessorFeaturePresent", is_processor_feature_present),
    ("KERNEL32.dll", "SetUnhandledExceptionFilter", set_unhandled_exception_filter),
    ("SHELL32.dll", "SHGetFolderPathW", sh_get_folder_path_w),
    ("KERNEL32.dll", "GetTickCount", get_tick_count),
];

/// Name every import token for diagnostics and bind the ones with a service.
/// Returns how many imports are bound.
pub fn bind_apis(registry: &mut Registry, imports: &[crate::image::Import]) -> Result<usize, Conflict> {
    let mut bound = 0;
    with_state(|s| {
        for i in imports {
            s.exports.insert((i.dll.to_ascii_lowercase(), i.name.clone()), i.token);
        }
    });
    for i in imports {
        registry.name(i.token, i.full_name());
        let found = APIS.iter().chain(crate::win32::APIS).chain(crate::files::APIS).chain(crate::steam::APIS).chain(crate::d3d9::APIS).chain(crate::user32::APIS).chain(crate::threads::APIS).chain(crate::text::APIS).chain(crate::system::APIS).chain(crate::registry::APIS).chain(crate::profile::APIS).chain(crate::imm::APIS).chain(crate::input::APIS).chain(crate::audio::APIS).find(|(d, n, _)| d.eq_ignore_ascii_case(&i.dll) && *n == i.name);
        if let Some((_, _, f)) = found {
            registry.insert(i.token, Binding::Service(*f))?;
            bound += 1;
        }
    }
    Ok(bound)
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
    // until the lifter handles its SEH frame (brief L15): the recorder
    // rejects the original body
    (0x012E_D89C, onexit),
];

#[derive(Debug)]
pub enum InstallError {
    Overlap(Overlap),
    Conflict(Conflict),
}

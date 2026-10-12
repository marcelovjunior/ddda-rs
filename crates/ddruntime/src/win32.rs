//! Win32 API services the C runtime's start-up and the game's early code
//! use (KERNEL32 mostly). Each follows the documented contract (Microsoft
//! Learn) with stdcall cleanup; behaviour that depends on the machine is
//! chosen deliberately and labelled (code pages, processor count, the
//! absence of a console). Single guest thread until stage R05.
use crate::services::{arg, ret, with_state, zero};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

/// VirtualAlloc's region: page-granular blocks from a lazily backed range.
pub const VIRTUAL_BASE: u32 = 0x6000_0000;
pub const VIRTUAL_SIZE: u32 = 0x1800_0000;

/// Handle values the runtime gives out (any value the guest only passes
/// back is fine; these are distinct from pointers the guest might hold).
pub const PROCESS_HEAP: u32 = 0x0F00_0000;
pub const PSEUDO_PROCESS: u32 = 0xFFFF_FFFF;
pub const MAIN_PROCESS_ID: u32 = 0x0000_2000;
/// GetModuleHandle(NULL): the image base.
pub const IMAGE_BASE: u32 = 0x0040_0000;

const ERROR_PROC_NOT_FOUND: u32 = 127;
const ERROR_INVALID_PARAMETER: u32 = 87;
const HEAP_ZERO_MEMORY: u32 = 0x8;

type Api = fn(&mut dyn Mem, &mut Cpu);

fn set_error(e: u32) {
    with_state(|s| s.last_error = e);
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
    cpu.eax = with_state(|s| s.last_error);
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

/// GetSystemInfo(SYSTEM_INFO*), 36 bytes. One processor: one guest
/// thread runs at a time (crate::threads); the game sizes its worker pool
/// from this.
pub fn get_system_info(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    zero(m, p, 36);
    m.set_u16(p, 0); // PROCESSOR_ARCHITECTURE_INTEL
    m.set_u32(p + 4, 0x1000); // dwPageSize
    m.set_u32(p + 8, 0x0001_0000); // lpMinimumApplicationAddress
    m.set_u32(p + 12, 0x7FFE_FFFF); // lpMaximumApplicationAddress
    m.set_u32(p + 16, 1); // dwActiveProcessorMask
    m.set_u32(p + 20, 1); // dwNumberOfProcessors
    m.set_u32(p + 24, 586); // dwProcessorType PROCESSOR_INTEL_PENTIUM
    m.set_u32(p + 28, 0x1_0000); // dwAllocationGranularity
    m.set_u16(p + 32, 6); // wProcessorLevel
    ret(m, cpu, 4);
}

// --- interlocked (one guest thread: plain read-modify-write) --------------

pub fn interlocked_increment(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    let v = m.u32(p).wrapping_add(1);
    m.set_u32(p, v);
    cpu.eax = v;
    ret(m, cpu, 4);
}
pub fn interlocked_decrement(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    let v = m.u32(p).wrapping_sub(1);
    m.set_u32(p, v);
    cpu.eax = v;
    ret(m, cpu, 4);
}
pub fn interlocked_exchange(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (p, v) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = m.u32(p);
    m.set_u32(p, v);
    ret(m, cpu, 8);
}
pub fn interlocked_exchange_add(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (p, v) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let old = m.u32(p);
    m.set_u32(p, old.wrapping_add(v));
    cpu.eax = old;
    ret(m, cpu, 8);
}
pub fn interlocked_compare_exchange(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (p, exchange, comparand) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let old = m.u32(p);
    if old == comparand {
        m.set_u32(p, exchange);
    }
    cpu.eax = old;
    ret(m, cpu, 12);
}

// --- TLS --------------------------------------------------------------------

/// TlsAlloc(): the lowest free index (TLS_OUT_OF_INDEXES 0xFFFFFFFF after 1088).
pub fn tls_alloc(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = with_state(|s| match s.tls.iter().position(Option::is_none) {
        Some(i) => {
            s.tls[i] = Some(0);
            s.tls_epoch[i] += 1;
            i as u32
        }
        None if s.tls.len() < 1088 => {
            s.tls.push(Some(0));
            s.tls_epoch.push(1);
            (s.tls.len() - 1) as u32
        }
        None => u32::MAX,
    });
    ret(m, cpu, 0);
}
/// TlsGetValue(index): the value (0 if never set); sets the last error to
/// 0 on success as documented, so callers can tell 0 from failure.
pub fn tls_get_value(m: &mut dyn Mem, cpu: &mut Cpu) {
    let i = arg(m, cpu, 0) as usize;
    cpu.eax = with_state(|s| match s.tls.get(i).copied().flatten() {
        Some(v) => {
            s.last_error = 0;
            v
        }
        None => {
            s.last_error = ERROR_INVALID_PARAMETER;
            0
        }
    });
    ret(m, cpu, 4);
}
pub fn tls_set_value(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (i, v) = (arg(m, cpu, 0) as usize, arg(m, cpu, 1));
    cpu.eax = with_state(|s| match s.tls.get_mut(i) {
        Some(slot @ Some(_)) => {
            *slot = Some(v);
            1
        }
        _ => {
            s.last_error = ERROR_INVALID_PARAMETER;
            0
        }
    });
    ret(m, cpu, 8);
}
pub fn tls_free(m: &mut dyn Mem, cpu: &mut Cpu) {
    let i = arg(m, cpu, 0) as usize;
    cpu.eax = with_state(|s| match s.tls.get_mut(i) {
        Some(slot @ Some(_)) => {
            *slot = None;
            1
        }
        _ => 0,
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

/// VirtualAlloc(address, size, type, protect): a fresh page-aligned block
/// (address hints are ignored: the guest gets memory where the runtime has
/// it). Reserve and commit are not distinguished: the region reads as zero.
pub fn virtual_alloc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let size = arg(m, cpu, 1);
    let pages = size.div_ceil(0x1000).max(1) * 0x1000;
    cpu.eax = with_state(|s| {
        if s.virtual_next == 0 {
            s.virtual_next = VIRTUAL_BASE;
        }
        let b = s.virtual_next;
        match b.checked_add(pages) {
            Some(e) if e <= VIRTUAL_BASE + VIRTUAL_SIZE => {
                s.virtual_next = e;
                b
            }
            _ => 0,
        }
    });
    ret(m, cpu, 16);
}
pub fn virtual_free(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1;
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
/// GetModuleFileNameA/W(module, buffer, size): the exe's path, truncated
/// and terminated as documented; returns the characters copied.
fn module_file_name(m: &mut dyn Mem, cpu: &mut Cpu, wide: bool) {
    let (buf, size) = (arg(m, cpu, 1), arg(m, cpu, 2));
    let path = exe_path();
    let units: Vec<u16> = if wide { path.encode_utf16().collect() } else { path.bytes().map(u16::from).collect() };
    let n = units.len().min(size.saturating_sub(1) as usize);
    for (k, &u) in units[..n].iter().chain([0u16].iter()).enumerate() {
        if wide {
            m.set_u16(buf.wrapping_add(2 * k as u32), u);
        } else {
            m.set_u8(buf.wrapping_add(k as u32), u as u8);
        }
    }
    cpu.eax = n as u32;
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
pub fn is_valid_code_page(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = matches!(arg(m, cpu, 0), 437 | 1252 | 65001) as u32;
    ret(m, cpu, 4);
}
/// GetCPInfo(codepage, CPINFO*): single-byte code pages, default char '?'.
pub fn get_cp_info(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (cp, p) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = if matches!(cp, 437 | 1252) {
        zero(m, p, 18);
        m.set_u32(p, 1); // MaxCharSize
        m.set_u8(p + 4, b'?'); // DefaultChar
        1
    } else {
        set_error(ERROR_INVALID_PARAMETER);
        0
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

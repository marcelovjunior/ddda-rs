//! Diagnostic fallback (L51): run an untranslated function of the exe in an
//! embedded x86 emulator (Unicorn) instead of stopping.
//!
//! The emulator executes the original machine code on the guest's own pages
//! (each mapped in place on first touch). The differential test checks its
//! state transfer against recorded outputs. A call or jump into anything bound in the
//! registry (a lifted function, a service, an import) leaves the emulator:
//! the registers go back to the [`Cpu`], the binding runs natively, and
//! emulation resumes at the return address. Every emulated entry is logged,
//! which turns the boot's one-stop-at-a-time loop into one run that lists
//! every function still missing on its route.
//!
//! Off unless the crate is built with the `emu` feature and the process runs
//! with `DDRUNTIME_EMU_FALLBACK=1`. `DDRUNTIME_EMU_LOG=<file>` appends one
//! line per newly emulated entry. Shipped code stays lifted and verified;
//! this only keeps a run going.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use ddnative_base::mem::{Mem, READ, WRITE};
use ddnative_base::x86::{self, Cpu};
use unicorn_engine::unicorn_const::{Arch, HookType, MemType, Mode, Permission};
use unicorn_engine::{RegisterX86, Unicorn};

use crate::dispatch::Registry;

#[path = "emu_numeric.rs"]
mod numeric;

/// DDDA.exe's .text section (0x00401000 + 0x00F9BDFB, from the PE header).
const TEXT: std::ops::Range<u32> = 0x0040_1000..0x0139_CDFB;

/// Preferred host-only pages for the state stubs. Recorded random pointers
/// can occupy this range, so an instance relocates it to unused guest pages.
const STUBS: u32 = 0xFFFF_0000;
const STUB_IN: u32 = STUBS;
const STUB_OUT: u32 = STUBS + 0x100;
const STUB_USER: u32 = STUBS + 0x200;
const SCRATCH: u32 = STUBS + 0x1000;
const FPU_IMAGE: u32 = SCRATCH; // 108 bytes (FNSAVE m108)
const MXCSR_SLOT: u32 = SCRATCH + 0x80;
const JUMP_SLOT: u32 = SCRATCH + 0xF0;
const XMM_SLOTS: u32 = SCRATCH + 0x100; // 8 x 16 bytes
/// Private GDT with room for every guest selector. Unicorn gets 32-bit
/// segment bases from descriptors, including FS's thread-local base.
const GDT: u32 = STUBS - 0x10000;
const DATA_SELECTOR: u64 = 2 << 3;

fn segments(cpu: &Cpu) -> [(RegisterX86, u16, u32, bool); 6] {
    [
        (RegisterX86::CS, cpu.seg.cs, 0, true),
        (RegisterX86::SS, cpu.seg.ss, 0, false),
        (RegisterX86::DS, cpu.seg.ds, 0, false),
        (RegisterX86::ES, cpu.seg.es, 0, false),
        (RegisterX86::GS, cpu.seg.gs, 0, false),
        (RegisterX86::FS, cpu.seg.fs, cpu.fs_base, false),
    ]
}

fn segment_descriptors(uc: &mut Emu, cpu: &Cpu) {
    let offset = uc.get_data().offset;
    let cpl = cpu.seg.cs & 3;
    assert_eq!(cpu.seg.ss & 3, cpl, "CS and SS privilege levels differ");
    for (_, selector, base, code) in segments(cpu) {
        assert_eq!(selector & 4, 0, "guest LDT is not represented by Cpu");
        if selector & !7 != 0 {
            uc.mem_write(
                (GDT + u32::from(selector & !7)).wrapping_add(offset) as u64,
                &descriptor(base, (if code { 0x9B } else { 0x93 }) | (u32::from(cpl) << 5)),
            )
            .unwrap();
        }
    }
}

/// A present, writable, flat-limit data descriptor (DPL 0, 32-bit, 4 KiB
/// granular) based at `base`.
fn data_descriptor(base: u32) -> [u8; 8] {
    descriptor(base, 0x92)
}

/// A present, flat-limit descriptor (32-bit, 4 KiB granular) of `access`.
fn descriptor(base: u32, access: u32) -> [u8; 8] {
    let low = 0xFFFF | ((base & 0xFFFF) << 16);
    let high = ((base >> 16) & 0xFF) | (access << 8) | (0xF << 16) | (0xC << 20) | (base & 0xFF00_0000);
    let mut d = [0u8; 8];
    d[..4].copy_from_slice(&low.to_le_bytes());
    d[4..].copy_from_slice(&high.to_le_bytes());
    d
}

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("DDRUNTIME_EMU_FALLBACK").is_ok_and(|v| v == "1"))
}

/// `DDRUNTIME_EMU_DEBUG`: trace the hooks to stderr.
fn debug() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("DDRUNTIME_EMU_DEBUG").is_ok())
}

pub fn in_text(addr: u32) -> bool {
    TEXT.contains(&addr)
}

/// How many times each entry ran in the emulator.
pub fn stats() -> Vec<(u32, u64)> {
    let mut v: Vec<_> = counts()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|(a, n)| (*a, *n))
        .collect();
    v.sort();
    v
}

fn counts() -> &'static Mutex<HashMap<u32, u64>> {
    static C: OnceLock<Mutex<HashMap<u32, u64>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

fn note(target: u32, caller: u32) {
    let first = {
        let mut c = counts().lock().unwrap_or_else(|e| e.into_inner());
        let n = c.entry(target).or_insert(0);
        *n += 1;
        *n == 1
    };
    if first {
        let line = format!("emulated {target:#010x} from {caller:#010x}\n");
        match std::env::var("DDRUNTIME_EMU_LOG") {
            Ok(path) => {
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                    let _ = f.write_all(line.as_bytes());
                }
            }
            Err(_) => eprint!("ddruntime: {line}"),
        }
    }
}

/// Why emulation stopped.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Exit {
    /// Control reached the caller's return address with the frame popped.
    Returned,
    /// Control reached a bound address: run it natively, then resume.
    Bound(u32),
    /// Execute one hardware-dependent floating instruction with native helpers.
    Numeric(u32, u32),
}

struct Data {
    deadline: Option<std::time::Instant>,
    steps: u32,
    hooks: Vec<*mut std::ffi::c_void>,
    cpl: u16,
    offset: u32,     // relocation of private descriptors and state stubs
    registry: usize, // the &Registry of the current run (a raw pointer)
    pages: usize,    // the guest's PageTable (a raw pointer, see `run`)
    /// Guest pages mapped into this instance: page -> (host pointer, perm),
    /// rechecked against the page table before each entry.
    mapped: HashMap<u32, (usize, u8)>,
    ret: u32,
    entry_esp: u32,
    resume: u32,
    exit: Option<Exit>,
    fault: Option<String>,
}

type Emu = Unicorn<'static, Data>;

struct Instance(Emu);
impl std::ops::Deref for Instance {
    type Target = Emu;
    fn deref(&self) -> &Emu {
        &self.0
    }
}
impl std::ops::DerefMut for Instance {
    fn deref_mut(&mut self) -> &mut Emu {
        &mut self.0
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        // unicorn-engine 2.0.1's hook wrappers own an Rc back to the engine.
        // Remove those hooks to break the cycle before dropping the engine.
        for hook in std::mem::take(&mut self.0.get_data_mut().hooks) {
            let _ = self.0.remove_hook(hook);
        }
    }
}

thread_local! {
    /// Idle emulator instances of this thread. One instance per nesting level
    /// is kept: each owns a large translation buffer, so creating one per call
    /// exhausts memory (first boot run: "Could not allocate dynamic
    /// translator buffer" after a few minutes).
    static POOL: std::cell::RefCell<Vec<Instance>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Return an instance even when a diagnostic case or native call-out unwinds.
/// Otherwise a run of terminal/failing cases repeatedly allocates Unicorn's
/// large translation buffer. State is reloaded at the next entry.
struct Lease {
    uc: Option<Instance>,
    observer: Option<*mut std::ffi::c_void>,
}
impl std::ops::Deref for Lease {
    type Target = Emu;
    fn deref(&self) -> &Emu {
        self.uc.as_ref().unwrap()
    }
}
impl std::ops::DerefMut for Lease {
    fn deref_mut(&mut self) -> &mut Emu {
        self.uc.as_mut().unwrap()
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut uc = self.uc.take().unwrap();
        if let Some(hook) = self.observer.take() {
            if uc.remove_hook(hook).is_err() {
                return;
            }
        }
        POOL.with(|p| p.borrow_mut().push(uc));
    }
}

fn registry<'r>(ptr: usize) -> &'r Registry {
    // SAFETY: `run` sets the pointer for the duration of the emulation
    unsafe { &*(ptr as *const Registry) }
}

fn is_return(data: &Data, address: u32, esp: u32) -> bool {
    // Stack-allocation helpers can return with ESP below the entry frame.
    // A bound token equal to the return address is still a nested call while
    // that frame remains on the stack (notably a recorded zero call token).
    address == data.ret && (esp > data.entry_esp || registry(data.registry).get(address).is_none())
}

fn private_range(offset: u32) -> std::ops::Range<u32> {
    GDT.wrapping_add(offset)..(STUBS + 0x2000).wrapping_add(offset)
}

fn private_free(table: &ddnative_base::mem::PageTable, offset: u32) -> bool {
    private_range(offset).step_by(4096).all(|a| {
        let page = a as usize / 4096;
        table.perm(page) == 0 && table.page(page).is_null()
    })
}

fn private_offset(table: &ddnative_base::mem::PageTable) -> u32 {
    for n in 0..=((GDT - 0x8000_0000) / 0x20000) {
        let base = GDT - n * 0x20000;
        let offset = base.wrapping_sub(GDT);
        if private_free(table, offset) {
            return offset;
        }
    }
    panic!("no unused pages for emulator state");
}

fn new_instance(offset: u32, cpl: u16) -> Instance {
    assert!(cpl == 0 || cpl == 3, "unsupported guest privilege level");
    let a = |address: u32| address.wrapping_add(offset) as u64;
    let data = Data {
        deadline: None,
        steps: 0,
        hooks: Vec::new(),
        cpl,
        offset,
        registry: 0,
        pages: 0,
        mapped: HashMap::new(),
        ret: 0,
        entry_esp: 0,
        resume: 0,
        exit: None,
        fault: None,
    };
    let mut uc = Unicorn::new_with_data(Arch::X86, Mode::MODE_32, data).expect("unicorn");
    uc.mem_map(a(STUBS), 0x2000, Permission::ALL).expect("stub pages");
    uc.mem_map(a(GDT), 0x10000, Permission::ALL).expect("GDT pages");
    let (stub_in, stub_out) = stubs(offset);
    uc.mem_write(a(STUB_IN), &stub_in).unwrap();
    uc.mem_write(a(STUB_OUT), &stub_out).unwrap();
    // GDTR (uc_x86_mmr: selector u16, base u64, limit u32, flags u32)
    let mut mmr = [0u8; 24];
    mmr[8..16].copy_from_slice(&(a(GDT)).to_le_bytes());
    mmr[16..20].copy_from_slice(&0xFFFFu32.to_le_bytes());
    uc.mem_write(a(GDT + 8), &descriptor(0, 0x9A)).unwrap();
    uc.mem_write(a(GDT + 16), &data_descriptor(0)).unwrap();
    uc.reg_write_long(RegisterX86::GDTR, &mmr).unwrap();
    for seg in [RegisterX86::SS, RegisterX86::DS, RegisterX86::ES, RegisterX86::GS] {
        uc.reg_write(seg, DATA_SELECTOR).unwrap();
    }

    if cpl == 3 {
        // Loading CS through the register API does not change Unicorn's cached
        // privilege level. IRET establishes CPL 3 so POPFD preserves IF and IOPL.
        let initial = Cpu::default();
        segment_descriptors(&mut uc, &initial);
        uc.mem_write(a(STUB_USER), &[0xCF, 0x90]).unwrap();
        let frame: Vec<u8> = [
            a(STUB_USER + 1) as u32,
            u32::from(initial.seg.cs),
            0x202,
            a(SCRATCH + 0x380) as u32,
            u32::from(initial.seg.ss),
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
        uc.mem_write(a(SCRATCH + 0x300), &frame).unwrap();
        uc.reg_write(RegisterX86::ESP, a(SCRATCH + 0x300)).unwrap();
        uc.emu_start(a(STUB_USER), a(STUB_USER + 2), 0, 0)
            .expect("enter guest user privilege level");
    }

    // guest pages, mapped in place on first touch; a fetch from a bound
    // address (an import token outside the image) is a call-out
    let memory_hook = uc
        .add_mem_hook(HookType::MEM_UNMAPPED, 1, 0, |uc, kind, addr, size, _| {
            let a = addr as u32;
            if debug() {
                eprintln!("mem {kind:?} {a:#010x} size {size}");
            }
            if kind == MemType::FETCH_UNMAPPED {
                let esp = uc.reg_read(RegisterX86::ESP).unwrap_or(0) as u32;
                let d = uc.get_data();
                if is_return(d, a, esp) {
                    uc.get_data_mut().exit = Some(Exit::Returned); // to a caller on an unmapped page
                    return false;
                }
                if registry(d.registry).get(a).is_some() {
                    uc.get_data_mut().exit = Some(Exit::Bound(a));
                    return false;
                }
            }
            // SAFETY: the page table outlives this call (page_table()'s
            // contract), and backed pages never move or free while it lives
            let table = unsafe { &*(uc.get_data().pages as *const ddnative_base::mem::PageTable) };
            let first = a / 0x1000;
            let last = a.wrapping_add(size.max(1) as u32 - 1) / 0x1000;
            for p in first..=last {
                // An unaligned operand may span a mapped page and a missing
                // page. Mapping the first one twice fails before the second is
                // reached, so only install pages not already present.
                if uc.get_data().mapped.contains_key(&p) {
                    continue;
                }
                let perm = table.perm(p as usize);
                if perm & READ == 0 {
                    uc.get_data_mut().fault = Some(format!("{kind:?} of unmapped {addr:#010x}"));
                    return false;
                }
                let ptr = table.backed_page(p as usize);
                let prot = if perm & WRITE != 0 {
                    Permission::ALL
                } else {
                    Permission::READ | Permission::EXEC
                };
                // SAFETY: a backed guest page is 4 KiB and stays put
                if unsafe { uc.mem_map_ptr(p as u64 * 0x1000, 0x1000, prot, ptr as *mut _) }.is_err() {
                    return false;
                }
                uc.get_data_mut().mapped.insert(p, (ptr as usize, perm));
            }
            true
        })
        .expect("memory hook");

    // returns and call-outs, at block starts inside the guest
    let block_hook = uc
        .add_block_hook(|uc, addr, _| {
            let a = addr as u32;
            if private_range(uc.get_data().offset).contains(&a) {
                return;
            }
            let esp = uc.reg_read(RegisterX86::ESP).unwrap_or(0) as u32;
            let d = uc.get_data();
            let exit = if is_return(d, a, esp) {
                Exit::Returned // ret or ret n
            } else if a != d.resume && registry(uc.get_data().registry).get(a).is_some() {
                Exit::Bound(a)
            } else {
                return;
            };
            if debug() {
                eprintln!("emu hook at {a:#010x}: {exit:?}");
            }
            uc.get_data_mut().exit = Some(exit);
            let _ = uc.emu_stop();
        })
        .expect("block hook");
    let numeric_hook = uc
        .add_code_hook(1, 0, |uc, address, size| {
            if uc.get_data().exit.is_some() || private_range(uc.get_data().offset).contains(&(address as u32))
            {
                return;
            }
            // Avoid a timer-thread creation/join on every short emulated
            // segment. Check long integer loops here and call-outs below.
            let d = uc.get_data_mut();
            d.steps = d.steps.wrapping_add(1);
            if d.steps & 4095 == 0 && d.deadline.is_some_and(|end| std::time::Instant::now() >= end) {
                d.fault = Some("diagnostic time limit".into());
                let _ = uc.emu_stop();
                return;
            }
            let mut code = [0u8; 15];
            if size <= 15
                && uc.mem_read(address, &mut code[..size as usize]).is_ok()
                && numeric::handles(&code[..size as usize])
            {
                uc.get_data_mut().exit = Some(Exit::Numeric(address as u32, size));
                let _ = uc.emu_stop();
            }
        })
        .expect("numeric hook");
    uc.get_data_mut().hooks = vec![memory_hook, block_hook, numeric_hook];
    Instance(uc)
}

/// Drop mappings whose guest page moved, changed permission or went away
/// (VirtualFree, VirtualProtect, a re-backed page); they map again lazily.
fn revalidate(uc: &mut Emu) {
    // SAFETY: as in the memory hook
    let table = unsafe { &*(uc.get_data().pages as *const ddnative_base::mem::PageTable) };
    let stale: Vec<u32> = uc
        .get_data()
        .mapped
        .iter()
        .filter(|(p, (ptr, perm))| {
            table.page(**p as usize) as usize != *ptr || table.perm(**p as usize) != *perm
        })
        .map(|(p, _)| *p)
        .collect();
    for p in stale {
        let _ = uc.mem_unmap(p as u64 * 0x1000, 0x1000);
        uc.get_data_mut().mapped.remove(&p);
    }
}

/// A 108-byte buffer as a `Mem`, for the x87 image (X87::fnsave/frstor).
struct Image([u8; 108]);

impl Mem for Image {
    fn u8(&self, addr: u32) -> u8 {
        self.0[addr as usize]
    }
    fn set_u8(&mut self, addr: u32, v: u8) {
        self.0[addr as usize] = v;
    }
}

fn stubs(offset: u32) -> (Vec<u8>, Vec<u8>) {
    let abs = |a: u32| a.wrapping_add(offset).to_le_bytes();
    let mut stub_in = Vec::new();
    stub_in.extend([0xDD, 0x25]); // frstor [FPU_IMAGE]
    stub_in.extend(abs(FPU_IMAGE));
    stub_in.extend([0x0F, 0xAE, 0x15]); // ldmxcsr [MXCSR_SLOT]
    stub_in.extend(abs(MXCSR_SLOT));
    for k in 0..8u32 {
        stub_in.extend([0x0F, 0x10, (0x05 | (k << 3)) as u8]); // movups xmmK, [slot]
        stub_in.extend(abs(XMM_SLOTS + 16 * k));
    }
    stub_in.extend([0xFF, 0x25]); // jmp dword [JUMP_SLOT]
    stub_in.extend(abs(JUMP_SLOT));
    let mut stub_out = Vec::new();
    stub_out.extend([0xDD, 0x35]); // fnsave [FPU_IMAGE]
    stub_out.extend(abs(FPU_IMAGE));
    stub_out.extend([0x0F, 0xAE, 0x1D]); // stmxcsr [MXCSR_SLOT]
    stub_out.extend(abs(MXCSR_SLOT));
    for k in 0..8u32 {
        stub_out.extend([0x0F, 0x11, (0x05 | (k << 3)) as u8]); // movups [slot], xmmK
        stub_out.extend(abs(XMM_SLOTS + 16 * k));
    }
    (stub_in, stub_out)
}

const GPRS: [(RegisterX86, fn(&mut Cpu) -> &mut u32); 8] = [
    (RegisterX86::EAX, |c| &mut c.eax),
    (RegisterX86::ECX, |c| &mut c.ecx),
    (RegisterX86::EDX, |c| &mut c.edx),
    (RegisterX86::EBX, |c| &mut c.ebx),
    (RegisterX86::ESP, |c| &mut c.esp),
    (RegisterX86::EBP, |c| &mut c.ebp),
    (RegisterX86::ESI, |c| &mut c.esi),
    (RegisterX86::EDI, |c| &mut c.edi),
];

fn fail(msg: String) -> ! {
    std::panic::panic_any(msg)
}

/// Run the untranslated function at `target` (its return address already
/// pushed, as for any call) in the emulator, with call-outs to `registry`.
/// Panics with a report if the emulated code faults.
pub fn run(registry: &Registry, m: &mut dyn Mem, cpu: &mut Cpu, target: u32) {
    run_inner(registry, m, cpu, target, None);
}

/// Diagnostic execution with an emulation time bound and the guest write set.
/// Call-out writes still go through `Mem`; the observer covers machine-code
/// writes, including stores that leave a byte unchanged. Normal runs install
/// no additional memory hook. The bound is checked between call-outs; it
/// cannot interrupt native code inside a call-out.
pub fn run_observed(
    registry: &Registry,
    m: &mut dyn Mem,
    cpu: &mut Cpu,
    target: u32,
    writes: std::rc::Rc<std::cell::RefCell<std::collections::BTreeSet<u32>>>,
    timeout: std::time::Duration,
) {
    run_inner(registry, m, cpu, target, Some((writes, timeout, None)));
}

/// Also observe the actual address of each emulator-to-native call-out.
pub fn run_observed_with_calls(
    registry: &Registry,
    m: &mut dyn Mem,
    cpu: &mut Cpu,
    target: u32,
    writes: std::rc::Rc<std::cell::RefCell<std::collections::BTreeSet<u32>>>,
    timeout: std::time::Duration,
    on_call: impl FnMut(u32) + 'static,
) {
    run_inner(
        registry,
        m,
        cpu,
        target,
        Some((writes, timeout, Some(Box::new(on_call)))),
    );
}

type Observation = (
    std::rc::Rc<std::cell::RefCell<std::collections::BTreeSet<u32>>>,
    std::time::Duration,
    Option<Box<dyn FnMut(u32)>>,
);

fn run_inner(
    registry: &Registry,
    m: &mut dyn Mem,
    cpu: &mut Cpu,
    target: u32,
    mut observation: Option<Observation>,
) {
    note(target, m.u32(cpu.esp));
    let Some(table) = m.page_table() else {
        fail(format!("emulated {target:#010x}: this memory has no page table"))
    };
    // SAFETY: the Mem contract keeps its page table valid during this call.
    let guest_table = unsafe { table.as_ref() };
    let pooled = POOL.with(|p| p.borrow_mut().pop());
    let mut uc = match pooled {
        Some(uc)
            if uc.get_data().cpl == cpu.seg.cs & 3 && private_free(guest_table, uc.get_data().offset) =>
        {
            uc
        }
        other => {
            drop(other);
            new_instance(private_offset(guest_table), cpu.seg.cs & 3)
        }
    };
    let offset = uc.get_data().offset;
    let a = |address: u32| address.wrapping_add(offset) as u64;
    let deadline = observation
        .as_ref()
        .map(|(_, timeout, _)| std::time::Instant::now() + *timeout);
    let observed = observation.is_some();
    let observer = observation.as_ref().map(|(writes, _, _)| {
        let writes = writes.clone();
        let private = private_range(offset);
        uc.add_mem_hook(
            HookType::MEM_WRITE,
            0,
            u32::MAX as u64,
            move |_, _, addr, size, _| {
                writes.borrow_mut().extend(
                    (0..size)
                        .map(|k| (addr as u32).wrapping_add(k as u32))
                        .filter(|a| !private.contains(a)),
                );
                true
            },
        )
        .expect("write observer")
    });
    let mut uc = Lease {
        uc: Some(uc),
        observer,
    };
    if observed || uc.get_data().pages != table.as_ptr() as usize {
        // another memory: nothing mapped applies
        let pages: Vec<u32> = uc.get_data().mapped.keys().copied().collect();
        for p in pages {
            let _ = uc.mem_unmap(p as u64 * 0x1000, 0x1000);
        }
        uc.get_data_mut().mapped.clear();
    }
    {
        let d = uc.get_data_mut();
        d.registry = registry as *const Registry as usize;
        d.deadline = deadline;
        d.steps = 0;
        d.pages = table.as_ptr() as usize;
        d.ret = m.u32(cpu.esp);
        d.entry_esp = cpu.esp;
        d.resume = target;
        d.fault = None;
    }
    let stub_out_end = a(STUB_OUT) + stubs(offset).1.len() as u64;
    loop {
        revalidate(&mut uc);
        // state in: registers, flags, FS, x87, MXCSR (live in the host while
        // lifted code runs), XMM
        for (r, f) in GPRS {
            uc.reg_write(r, *f(cpu) as u64).unwrap();
        }
        let (flags, _) = cpu.fl.snapshot();
        let eflags = (flags & 0x8D5) | ((cpu.df as u32) << 10) | 0x202 | (cpu.sys_flags & 0x24_4000);
        uc.reg_write(RegisterX86::EFLAGS, eflags as u64).unwrap();
        segment_descriptors(&mut uc, cpu);
        for (register, selector, _, _) in segments(cpu) {
            // Legacy recorder fixtures use Unicorn's initial flat CS=0.
            // Reloading a null CS clears its cached 32-bit code attribute.
            if register == RegisterX86::CS && selector == 0 {
                continue;
            }
            uc.reg_write(register, u64::from(selector)).unwrap();
        }
        let mut image = Image([0; 108]);
        cpu.st.clone().fnsave(&mut image, 0, 0, 0, 0);
        uc.mem_write(a(FPU_IMAGE), &image.0).unwrap();
        uc.mem_write(a(MXCSR_SLOT), &x86::stmxcsr().to_le_bytes())
            .unwrap();
        for k in 0..8 {
            let bytes: Vec<u8> = cpu.xmm[k].iter().flat_map(|w| w.to_le_bytes()).collect();
            uc.mem_write(a(XMM_SLOTS + 16 * k as u32), &bytes).unwrap();
        }
        let resume = uc.get_data().resume;
        uc.mem_write(a(JUMP_SLOT), &resume.to_le_bytes()).unwrap();
        uc.get_data_mut().exit = None;
        if deadline.is_some_and(|end| std::time::Instant::now() >= end) {
            fail(format!("emulated {target:#010x}: diagnostic time limit"));
        }
        let result = uc.emu_start(a(STUB_IN), 0, 0, 0);
        let mut exit = uc.get_data().exit;
        // Unicorn's `until=0` stops before its block hook at address zero.
        // Recorded indirect calls can bind zero just like any other token.
        if exit.is_none() && result.is_ok() {
            let eip = uc.reg_read(RegisterX86::EIP).unwrap() as u32;
            let esp = uc.reg_read(RegisterX86::ESP).unwrap() as u32;
            let d = uc.get_data();
            exit = if is_return(d, eip, esp) {
                Some(Exit::Returned)
            } else if eip != d.resume && registry.get(eip).is_some() {
                Some(Exit::Bound(eip))
            } else {
                None
            };
        }
        if exit.is_none() {
            let eip = uc.reg_read(RegisterX86::EIP).unwrap_or(0);
            let why = uc
                .get_data()
                .fault
                .clone()
                .unwrap_or_else(|| format!("{result:?}"));
            fail(format!("emulated {target:#010x} stopped at {eip:#010x}: {why}"));
        }

        // state out
        for (r, f) in GPRS {
            *f(cpu) = uc.reg_read(r).unwrap() as u32;
        }
        let eflags = uc.reg_read(RegisterX86::EFLAGS).unwrap() as u32;
        cpu.seg = x86::Segments {
            cs: uc.reg_read(RegisterX86::CS).unwrap() as u16,
            ss: uc.reg_read(RegisterX86::SS).unwrap() as u16,
            ds: uc.reg_read(RegisterX86::DS).unwrap() as u16,
            es: uc.reg_read(RegisterX86::ES).unwrap() as u16,
            fs: uc.reg_read(RegisterX86::FS).unwrap() as u16,
            gs: uc.reg_read(RegisterX86::GS).unwrap() as u16,
        };
        cpu.fs_base = uc.reg_read(RegisterX86::FS_BASE).unwrap() as u32;
        let esp = cpu.esp;
        uc.emu_start(a(STUB_OUT), stub_out_end, 0, 0).expect("state out");
        cpu.esp = esp;
        x86::popfd(cpu, eflags & !0x100);
        let mut image = Image([0; 108]);
        uc.mem_read(a(FPU_IMAGE), &mut image.0).unwrap();
        cpu.st.frstor(&image, 0);
        let mut mxcsr = [0u8; 4];
        uc.mem_read(a(MXCSR_SLOT), &mut mxcsr).unwrap();
        x86::ldmxcsr(u32::from_le_bytes(mxcsr));
        for k in 0..8 {
            let mut bytes = [0u8; 16];
            uc.mem_read(a(XMM_SLOTS + 16 * k as u32), &mut bytes).unwrap();
            for w in 0..4 {
                cpu.xmm[k][w] = u32::from_le_bytes(bytes[4 * w..4 * w + 4].try_into().unwrap());
            }
        }

        match exit.unwrap() {
            Exit::Numeric(pc, size) => {
                let code: Vec<u8> = (0..size).map(|k| m.u8(pc.wrapping_add(k))).collect();
                numeric::execute(&code, m, cpu);
                uc.get_data_mut().resume = pc.wrapping_add(size);
            }
            Exit::Returned => {
                // the emulated ret already popped the return address
                return;
            }
            Exit::Bound(callee) => {
                if let Some((_, _, Some(on_call))) = observation.as_mut() {
                    on_call(callee);
                }
                let back = m.u32(cpu.esp);
                if let Err(u) = registry.call(m, cpu, callee) {
                    std::panic::panic_any(u);
                }
                if is_return(uc.get_data(), back, cpu.esp) {
                    return; // a tail call out of the emulated function
                }
                uc.get_data_mut().resume = back;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dropping_an_instance_releases_its_hook_owners() {
        let mut instance = super::new_instance(0, 3);
        let owner = std::rc::Rc::new(());
        let weak = std::rc::Rc::downgrade(&owner);
        let hook = instance
            .add_block_hook(move |_, _, _| {
                let _ = &owner;
            })
            .unwrap();
        instance.get_data_mut().hooks.push(hook);
        drop(instance);
        assert!(weak.upgrade().is_none(), "hook ownership cycle must be broken");
    }
}

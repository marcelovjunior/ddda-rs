//! Diagnostic fallback (L51): run an untranslated function of the exe in an
//! embedded x86 emulator (Unicorn) instead of stopping.
//!
//! The emulator executes the original machine code on the guest's own pages
//! (each mapped in place on first touch), so it is exact by construction but
//! slow. A call or jump from emulated code into anything bound in the
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

/// DDDA.exe's .text section (0x00401000 + 0x00F9BDFB, from the PE header).
const TEXT: std::ops::Range<u32> = 0x0040_1000..0x0139_CDFB;

/// Host-only pages for the state stubs, at the top of the address space
/// where Windows never maps user memory.
const STUBS: u32 = 0xFFFF_0000;
const STUB_IN: u32 = STUBS;
const STUB_OUT: u32 = STUBS + 0x100;
const SCRATCH: u32 = STUBS + 0x1000;
const FPU_IMAGE: u32 = SCRATCH; // 108 bytes (FNSAVE m108)
const MXCSR_SLOT: u32 = SCRATCH + 0x80;
const JUMP_SLOT: u32 = SCRATCH + 0xF0;
const XMM_SLOTS: u32 = SCRATCH + 0x100; // 8 x 16 bytes
/// A GDT: entry 1 flat code, 2 flat data (SS, DS, ES, GS), 3 FS (based at
/// the guest thread's TEB). In 32-bit mode Unicorn takes segment bases only
/// from descriptors, and loading a GDT makes it reload the stack segment.
const GDT: u32 = SCRATCH + 0x400;
const DATA_SELECTOR: u64 = 2 << 3;
const FS_SELECTOR: u64 = 3 << 3;

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
    let mut v: Vec<_> = counts().lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(a, n)| (*a, *n)).collect();
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
}

struct Data {
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

thread_local! {
    /// Idle emulator instances of this thread. One instance per nesting level
    /// is kept: each owns a large translation buffer, so creating one per call
    /// exhausts memory (first boot run: "Could not allocate dynamic
    /// translator buffer" after a few minutes).
    static POOL: std::cell::RefCell<Vec<Emu>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn registry<'r>(ptr: usize) -> &'r Registry {
    // SAFETY: `run` sets the pointer for the duration of the emulation
    unsafe { &*(ptr as *const Registry) }
}

fn new_instance() -> Emu {
    let data = Data { registry: 0, pages: 0, mapped: HashMap::new(), ret: 0, entry_esp: 0, resume: 0, exit: None, fault: None };
    let mut uc = Unicorn::new_with_data(Arch::X86, Mode::MODE_32, data).expect("unicorn");
    uc.mem_map(STUBS as u64, 0x2000, Permission::ALL).expect("stub pages");
    let (stub_in, stub_out) = stubs();
    uc.mem_write(STUB_IN as u64, &stub_in).unwrap();
    uc.mem_write(STUB_OUT as u64, &stub_out).unwrap();
    // GDTR (uc_x86_mmr: selector u16, base u64, limit u32, flags u32)
    let mut mmr = [0u8; 24];
    mmr[8..16].copy_from_slice(&(GDT as u64).to_le_bytes());
    mmr[16..20].copy_from_slice(&(4 * 8 - 1u32).to_le_bytes());
    uc.mem_write((GDT + 8) as u64, &descriptor(0, 0x9A)).unwrap();
    uc.mem_write((GDT + 16) as u64, &data_descriptor(0)).unwrap();
    uc.reg_write_long(RegisterX86::GDTR, &mmr).unwrap();
    for seg in [RegisterX86::SS, RegisterX86::DS, RegisterX86::ES, RegisterX86::GS] {
        uc.reg_write(seg, DATA_SELECTOR).unwrap();
    }

    // guest pages, mapped in place on first touch; a fetch from a bound
    // address (an import token outside the image) is a call-out
    uc.add_mem_hook(HookType::MEM_UNMAPPED, 1, 0, |uc, kind, addr, size, _| {
        let a = addr as u32;
        if debug() {
            eprintln!("mem {kind:?} {a:#010x} size {size}");
        }
        if kind == MemType::FETCH_UNMAPPED {
            let esp = uc.reg_read(RegisterX86::ESP).unwrap_or(0) as u32;
            let d = uc.get_data();
            if a == d.ret && esp > d.entry_esp {
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
            let perm = table.perm(p as usize);
            if perm & READ == 0 {
                uc.get_data_mut().fault = Some(format!("{kind:?} of unmapped {addr:#010x}"));
                return false;
            }
            let ptr = table.backed_page(p as usize);
            let prot = if perm & WRITE != 0 { Permission::ALL } else { Permission::READ | Permission::EXEC };
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
    uc.add_block_hook(|uc, addr, _| {
        let a = addr as u32;
        if a >= STUBS {
            return;
        }
        let esp = uc.reg_read(RegisterX86::ESP).unwrap_or(0) as u32;
        let d = uc.get_data();
        let exit = if a == d.ret && esp > d.entry_esp {
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
    uc
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
        .filter(|(p, (ptr, perm))| table.page(**p as usize) as usize != *ptr || table.perm(**p as usize) != *perm)
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

fn stubs() -> (Vec<u8>, Vec<u8>) {
    let abs = |a: u32| a.to_le_bytes();
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
    note(target, m.u32(cpu.esp));
    let Some(table) = m.page_table() else { fail(format!("emulated {target:#010x}: this memory has no page table")) };
    let mut uc = POOL.with(|p| p.borrow_mut().pop()).unwrap_or_else(new_instance);
    if uc.get_data().pages != table.as_ptr() as usize {
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
        d.pages = table.as_ptr() as usize;
        d.ret = m.u32(cpu.esp);
        d.entry_esp = cpu.esp;
        d.resume = target;
        d.fault = None;
    }
    let stub_out_end = STUB_OUT + stubs().1.len() as u32;
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
        uc.mem_write((GDT + 3 * 8) as u64, &data_descriptor(cpu.fs_base)).unwrap();
        uc.reg_write(RegisterX86::FS, FS_SELECTOR).unwrap();
        let mut image = Image([0; 108]);
        cpu.st.clone().fnsave(&mut image, 0, 0, 0, 0);
        uc.mem_write(FPU_IMAGE as u64, &image.0).unwrap();
        uc.mem_write(MXCSR_SLOT as u64, &x86::stmxcsr().to_le_bytes()).unwrap();
        for k in 0..8 {
            let bytes: Vec<u8> = cpu.xmm[k].iter().flat_map(|w| w.to_le_bytes()).collect();
            uc.mem_write((XMM_SLOTS + 16 * k as u32) as u64, &bytes).unwrap();
        }
        let resume = uc.get_data().resume;
        uc.mem_write(JUMP_SLOT as u64, &resume.to_le_bytes()).unwrap();
        uc.get_data_mut().exit = None;
        let result = uc.emu_start(STUB_IN as u64, 0, 0, 0);
        let exit = uc.get_data().exit;
        if exit.is_none() {
            let eip = uc.reg_read(RegisterX86::EIP).unwrap_or(0);
            let why = uc.get_data().fault.clone().unwrap_or_else(|| format!("{result:?}"));
            fail(format!("emulated {target:#010x} stopped at {eip:#010x}: {why}"));
        }

        // state out
        for (r, f) in GPRS {
            *f(cpu) = uc.reg_read(r).unwrap() as u32;
        }
        let eflags = uc.reg_read(RegisterX86::EFLAGS).unwrap() as u32;
        let esp = cpu.esp;
        uc.emu_start(STUB_OUT as u64, stub_out_end as u64, 0, 0).expect("state out");
        cpu.esp = esp;
        x86::popfd(cpu, eflags & !0x100);
        let mut image = Image([0; 108]);
        uc.mem_read(FPU_IMAGE as u64, &mut image.0).unwrap();
        cpu.st.frstor(&image, 0);
        let mut mxcsr = [0u8; 4];
        uc.mem_read(MXCSR_SLOT as u64, &mut mxcsr).unwrap();
        x86::ldmxcsr(u32::from_le_bytes(mxcsr));
        for k in 0..8 {
            let mut bytes = [0u8; 16];
            uc.mem_read((XMM_SLOTS + 16 * k as u32) as u64, &mut bytes).unwrap();
            for w in 0..4 {
                cpu.xmm[k][w] = u32::from_le_bytes(bytes[4 * w..4 * w + 4].try_into().unwrap());
            }
        }

        match exit.unwrap() {
            Exit::Returned => {
                // the emulated ret already popped the return address
                POOL.with(|p| p.borrow_mut().push(uc));
                return;
            }
            Exit::Bound(callee) => {
                let back = m.u32(cpu.esp);
                if let Err(u) = registry.call(m, cpu, callee) {
                    std::panic::panic_any(u);
                }
                if back == uc.get_data().ret && cpu.esp > uc.get_data().entry_esp {
                    POOL.with(|p| p.borrow_mut().push(uc));
                    return; // a tail call out of the emulated function
                }
                uc.get_data_mut().resume = back;
            }
        }
    }
}

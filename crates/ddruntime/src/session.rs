//! A guest session: the loaded image, a stack, every binding (lifted
//! functions, CRT and Win32 services, import tokens), installed as this
//! thread's `call_ext` handler. Drivers (examples/boot.rs, winmain.rs, the
//! initializer census) start from here.
use crate::dispatch::{Registry, Unbound};
use crate::image::Import;
use crate::memory::{GuestMemory, READ, WRITE};
use crate::win32::Exit;
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use std::sync::Arc;

pub const STACK_TOP: u32 = 0x7F00_0000;

/// The platform models (ddnative_base::platform) the runtime replaces by
/// whatever the dispatcher has at their address: the game's allocators
/// must be the original bodies (they share state with the allocator
/// object at 0x018D0AA0 that other code calls directly). memset and memcpy
/// go to the runtime's page-wise services (fastmem, L61): the models copy
/// byte by byte.
pub const REDIRECTED_MODELS: [u32; 5] =
    [0x00CF_73F0, 0x00DE_0B80, 0x00DE_0B90, crate::fastmem::MEMSET, crate::fastmem::MEMCPY];
pub const STACK_SIZE: u32 = 0x0010_0000;
/// The return address a driver pushes for a top-level call: never executed
/// (a lifted `ret` returns to the Rust caller), only kept on the stack.
pub const RETURN_SENTINEL: u32 = 0xFFFF_FFF0;

pub struct Session {
    pub mem: GuestMemory,
    pub registry: Arc<Registry>,
    pub imports: Vec<Import>,
    pub exe_path: String,
}

/// How a top-level guest call ended.
#[derive(Debug)]
pub enum Outcome {
    Returned(Cpu),
    Unbound(Unbound),
    Exit(u32),
    /// message, likely return addresses, registers at the fault
    Fault(String, Vec<u32>, [u32; 8]),
}

impl Session {
    /// Load `exe_path` (the player's DDDA.exe) and bind everything.
    pub fn new(exe_path: &str) -> Result<Session, String> {
        let exe = ddexe::ExeFile::open(exe_path).map_err(|e| e.to_string())?;
        // a new session: fresh runtime state for this host thread (and the
        // guest threads it will start)
        crate::runtime::enter(std::sync::Arc::new(crate::runtime::Runtime::default()));
        let mut mem = GuestMemory::new();
        crate::image::load(&mut mem, &exe).map_err(|e| e.to_string())?;
        mem.map_lazy(STACK_TOP - STACK_SIZE, STACK_SIZE, READ | WRITE).map_err(|e| format!("{e:?}"))?;
        crate::seh::initialize_teb(&mut mem, crate::services::MAIN_THREAD_ID, STACK_TOP - STACK_SIZE, STACK_TOP);
        let mut registry = Registry::with_lifted_program();
        // guest memory only: no guest lock in parallel mode (L65)
        registry.replace(crate::fastmem::MEMCPY, crate::dispatch::Binding::LockFree(crate::fastmem::memcpy));
        registry.replace(crate::fastmem::MEMSET, crate::dispatch::Binding::LockFree(crate::fastmem::memset));
        // the CRT's string routines, ported (L67; guest memory only)
        for &(addr, f) in crate::crtfast::SERVICES {
            registry.replace(addr, crate::dispatch::Binding::LockFree(f));
        }
        crate::services::install(&mut mem, &mut registry).map_err(|e| format!("{e:?}"))?;
        crate::services::set_exe_path(exe_path);
        let install = std::path::Path::new(exe_path).parent().ok_or("exe has no directory")?;
        crate::files::set_roots(install, &crate::services::sandbox_home());
        let imports = crate::image::bind_imports(&mut mem, &exe).map_err(|e| e.to_string())?;
        crate::services::bind_apis(&mut registry, &imports).map_err(|e| format!("{e:?}"))?;
        // Interface objects and named guest method tokens.
        let mut tokens = crate::interfaces::Tokens::default();
        crate::steam::install(&mut mem, &mut registry, &mut tokens).map_err(|e| format!("{e:?}"))?;
        crate::input::install(&mut mem, &mut registry, &mut tokens).map_err(|e| format!("{e:?}"))?;
        crate::audio::install(&mut mem, &mut registry, &mut tokens).map_err(|e| format!("{e:?}"))?;
        crate::movies::install(&mut mem, &mut registry, &mut tokens).map_err(|e| format!("{e:?}"))?;
        crate::d3d9::install(&mut mem, &mut registry, &mut tokens).map_err(|e| format!("{e:?}"))?;
        mem.map(crate::interfaces::METHOD_TOKENS, tokens.span(), READ).map_err(|e| format!("{e:?}"))?;
        mem.map_lazy(crate::threads::STACKS_BASE, crate::threads::STACKS_SIZE, READ | WRITE).map_err(|e| format!("{e:?}"))?;
        let registry = Arc::new(registry);
        crate::threads::set_machine(mem.share(), registry.clone());
        registry.install();
        ddnative_base::platform::redirect(&REDIRECTED_MODELS);
        Ok(Session { mem, registry, imports, exe_path: exe_path.to_string() })
    }

    /// Call `target` like a `call` instruction would, with `args` pushed
    /// right to left (stdcall/cdecl; `ecx` for thiscall), on a fresh frame
    /// of the session stack.
    pub fn call(&mut self, target: u32, ecx: u32, args: &[u32]) -> Outcome {
        let mut cpu = Cpu { esp: STACK_TOP - 0x100, ecx, fs_base: crate::seh::TEB_BASE, ..Cpu::default() };
        for &a in args.iter().rev() {
            cpu.esp -= 4;
            self.mem.set_u32(cpu.esp, a);
        }
        cpu.esp -= 4;
        self.mem.set_u32(cpu.esp, RETURN_SENTINEL);
        let (mem, registry) = (&mut self.mem, &self.registry);
        // L22: the guest MXCSR is live while guest code runs; Rust resumes
        // under the host default (also after a panic).
        ddnative_base::x86::ldmxcsr(cpu.mxcsr.0);
        // in parallel mode the guest code runs without the guest lock
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::threads::without_lock(|| registry.call(mem, &mut cpu, target))
        }));
        cpu.mxcsr = ddnative_base::x86::Mxcsr(ddnative_base::x86::stmxcsr());
        ddnative_base::x86::ldmxcsr(ddnative_base::x86::MXCSR_DEFAULT);
        match r {
            Ok(Ok(())) => Outcome::Returned(cpu),
            Ok(Err(u)) => Outcome::Unbound(u),
            Err(e) => {
                if let Some(u) = e.downcast_ref::<Unbound>() {
                    Outcome::Unbound(u.clone())
                } else if let Some(Exit(code)) = e.downcast_ref::<Exit>() {
                    Outcome::Exit(*code)
                } else {
                    let msg = e.downcast_ref::<String>().cloned()
                        .or_else(|| e.downcast_ref::<crate::seh::Stop>().map(ToString::to_string))
                        .or_else(|| e.downcast_ref::<crate::executor::Stop>().map(|s| format!("{s:?}")))
                        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "panic".into());
                    let regs = [cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi];
                    // the guest functions from the fault hook, else the stack scan
                    let frames = GUEST_FRAMES.with(|g| std::mem::take(&mut *g.borrow_mut()));
                    Outcome::Fault(msg, if frames.is_empty() { self.backtrace(cpu.esp) } else { frames }, regs)
                }
            }
        }
    }

    /// Likely return addresses on the guest stack above `esp`: words that
    /// point into .text right after a `call` instruction (E8 rel32, or
    /// FF /2 through a register or memory operand). Lifted code keeps the
    /// registers in `Cpu`, so after a fault `esp` is the faulting frame's.
    pub fn backtrace(&self, esp: u32) -> Vec<u32> {
        const TEXT: std::ops::Range<u32> = 0x0040_1000..0x0139_D000;
        let mut out = Vec::new();
        for k in 0..0x800u32 {
            let a = esp.wrapping_add(4 * k);
            if self.mem.check(a, 4, false).is_err() {
                break;
            }
            let v = self.mem.u32(a);
            if !TEXT.contains(&v) {
                continue;
            }
            let is_call = |back: u32, ok: &dyn Fn(&[u8]) -> bool| {
                self.mem.read(v - back, back).map(|b| ok(&b)).unwrap_or(false)
            };
            let call = is_call(5, &|b: &[u8]| b[0] == 0xE8)
                || is_call(2, &|b: &[u8]| b[0] == 0xFF && (b[1] >> 3) & 7 == 2)
                || is_call(3, &|b: &[u8]| b[0] == 0xFF && (b[1] >> 3) & 7 == 2)
                || is_call(6, &|b: &[u8]| b[0] == 0xFF && (b[1] >> 3) & 7 == 2)
                || is_call(7, &|b: &[u8]| b[0] == 0xFF && (b[1] >> 3) & 7 == 2);
            if call {
                out.push(v);
                if out.len() == 12 {
                    break;
                }
            }
        }
        out
    }

    /// The name of an import token, if `addr` is one.
    pub fn import_name(&self, addr: u32) -> Option<String> {
        self.imports.iter().find(|i| i.token == addr).map(Import::full_name)
    }
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::Returned(cpu) => write!(f, "returned eax={:#x}", cpu.eax),
            Outcome::Unbound(u) => write!(f, "stopped: {u}"),
            Outcome::Exit(c) => write!(f, "ExitProcess({c:#x})"),
            Outcome::Fault(s, bt, r) => {
                write!(f, "stopped: {s}
  eax={:#x} ecx={:#x} edx={:#x} ebx={:#x} esp={:#x} ebp={:#x} esi={:#x} edi={:#x}",
                       r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7])?;
                if !bt.is_empty() {
                    write!(f, "
  guest frames (innermost first):")?;
                    for r in bt {
                        write!(f, " {r:#010x}")?;
                    }
                }
                Ok(())
            }
        }
    }
}

thread_local! {
    static GUEST_FRAMES: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Replace the panic hook (which would print every guest fault) with one
/// that silently records the translated functions on the Rust stack at the
/// fault: lifted function `f_XXXXXXXX` is the guest function at
/// 0xXXXXXXXX, so this is the guest call chain, innermost first, including
/// direct lifted-to-lifted calls the stack scan cannot see. Needs symbols
/// (debug builds have them).
pub fn install_fault_hook() {
    std::panic::set_hook(Box::new(|_| {
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let mut frames = Vec::new();
        for line in bt.lines() {
            if let Some(k) = line.find("::f_") {
                let hex = &line[k + 4..];
                if hex.len() >= 8 {
                    if let Ok(a) = u32::from_str_radix(&hex[..8], 16) {
                        if frames.last() != Some(&a) {
                            frames.push(a);
                        }
                    }
                }
            }
        }
        GUEST_FRAMES.with(|g| *g.borrow_mut() = frames);
    }));
}

/// One step of the CRT start-up that did not succeed, or WinMain's outcome.
pub struct Started {
    /// The step that stopped the start-up ("WinMain" if it got that far).
    pub step: &'static str,
    pub outcome: Outcome,
}

impl Session {
    /// Start the program the way the CRT entry does: the body of
    /// `___tmainCRTStartup`.
    ///
    /// Translated from 0x012EF8A7 (DDDA.exe 2017-12-13 Steam), without its
    /// SEH frame: `__SEH_prolog4`/`__SEH_epilog4` need the FS segment
    /// (brief L15), so the original body cannot be lifted yet. This is a
    /// temporary bridge; once L15 lands the lifted body replaces it. Each
    /// step calls the lifted CRT function at its address and checks its
    /// result as the original does (where the original would exit, the
    /// start-up stops with that step). GetStartupInfoW's flags have no
    /// STARTF_USESHOWWINDOW in this runtime, so WinMain gets SW_SHOWDEFAULT
    /// (10), as the original computes. The HeapSetInformation call at
    /// 0x012EF8CC and `__RTC_Initialize` have no effect the steps below
    /// depend on but are called for fidelity.
    pub fn start(&mut self) -> Started {
        self.start_through("WinMain")
    }

    /// [`Session::start`], ending after the step named `last` (a CRT step
    /// below or "WinMain"), so a test of the start-up does not run the
    /// game once the boot gets that far.
    pub fn start_through(&mut self, last: &str) -> Started {
        let ok = |o: &Outcome, good: &dyn Fn(u32) -> bool| matches!(o, Outcome::Returned(c) if good(c.eax));
        // (name, address, cdecl arguments, success test)
        let steps: [(&'static str, u32, &[u32], &dyn Fn(u32) -> bool); 8] = [
            ("__heap_init", 0x012F_E354, &[], &|r| r != 0),
            ("__mtinit", 0x012F_982C, &[], &|r| r != 0),
            ("__RTC_Initialize", 0x012F_99A7, &[], &|_| true),
            ("__ioinit", 0x012F_7C13, &[], &|r| (r as i32) >= 0),
            ("__setargv", 0x012F_E202, &[], &|r| (r as i32) >= 0),
            ("_setenvp", 0x012F_DF8C, &[], &|r| (r as i32) >= 0),
            ("__cinit", 0x012E_DA5E, &[1], &|r| r == 0),
            ("__wincmdln", 0x012F_DF2D, &[], &|_| true),
        ];
        let mut cmdln = 0;
        for (k, (name, addr, args, good)) in steps.iter().enumerate() {
            if k == 4 {
                // between __ioinit and __setargv: _acmdln = GetCommandLineA(),
                // _aenvptr = __crtGetEnvironmentStringsA() (0x012EF956..0x012EF966)
                let line = self.import_call("KERNEL32.dll!GetCommandLineA");
                self.mem.set_u32(0x019A_B1C4, line);
                let env = self.call(0x012F_E2BD, 0, &[]);
                let Outcome::Returned(c) = env else { return Started { step: "__crtGetEnvironmentStringsA", outcome: env } };
                self.mem.set_u32(0x018D_7A18, c.eax);
            }
            let o = self.call(*addr, 0, args);
            if !ok(&o, good) {
                return Started { step: name, outcome: o };
            }
            if let Outcome::Returned(c) = &o {
                cmdln = c.eax; // the last step's result: the WinMain command line
            }
            if *name == last {
                return Started { step: name, outcome: o };
            }
        }
        let outcome = self.call(0x0040_79E0, 0, &[0x0040_0000, 0, cmdln, 10]);
        Started { step: "WinMain", outcome }
    }

    /// Call an import's service (no arguments) and return eax.
    fn import_call(&mut self, name: &str) -> u32 {
        let token = self.imports.iter().find(|i| i.full_name() == name).unwrap_or_else(|| panic!("{name} is not imported")).token;
        match self.call(token, 0, &[]) {
            Outcome::Returned(c) => c.eax,
            o => panic!("{name}: {o}"),
        }
    }
}

/// The C and C++ static-initializer tables of this build (operands of the
/// CRT initializer driver at 0x012EDA8A/8F/94 and 0x012EDAAB/B0).
pub const INITIALIZER_TABLES: [(&str, u32, u32); 2] =
    [("C", 0x0140_34B4, 0x0140_34D0), ("C++", 0x0139_D490, 0x0140_34B0)];

impl Session {
    /// Every non-null initializer slot target, in CRT order.
    pub fn initializers(&self) -> Vec<(&'static str, u32)> {
        let mut out = Vec::new();
        for (kind, lo, hi) in INITIALIZER_TABLES {
            for slot in (lo..hi).step_by(4) {
                let t = self.mem.u32(slot);
                if t != 0 {
                    out.push((kind, t));
                }
            }
        }
        out
    }
}

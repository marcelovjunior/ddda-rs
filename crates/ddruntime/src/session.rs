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
use std::rc::Rc;

pub const STACK_TOP: u32 = 0x7F00_0000;
pub const STACK_SIZE: u32 = 0x0010_0000;
/// The return address a driver pushes for a top-level call: never executed
/// (a lifted `ret` returns to the Rust caller), only kept on the stack.
pub const RETURN_SENTINEL: u32 = 0xFFFF_FFF0;

pub struct Session {
    pub mem: GuestMemory,
    pub registry: Rc<Registry>,
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
        let mut registry = Registry::with_lifted_program();
        crate::services::install(&mut mem, &mut registry).map_err(|e| format!("{e:?}"))?;
        crate::services::set_exe_path(exe_path);
        let install = std::path::Path::new(exe_path).parent().ok_or("exe has no directory")?;
        crate::files::set_roots(install, &crate::services::sandbox_home());
        let imports = crate::image::bind_imports(&mut mem, &exe).map_err(|e| e.to_string())?;
        crate::services::bind_apis(&mut registry, &imports).map_err(|e| format!("{e:?}"))?;
        // interface objects (Steam now; COM devices later)
        let mut tokens = crate::interfaces::Tokens::default();
        crate::steam::install(&mut mem, &mut registry, &mut tokens).map_err(|e| format!("{e:?}"))?;
        crate::d3d9::install(&mut mem, &mut registry, &mut tokens).map_err(|e| format!("{e:?}"))?;
        mem.map(crate::interfaces::METHOD_TOKENS, tokens.span(), READ).map_err(|e| format!("{e:?}"))?;
        let registry = Rc::new(registry);
        registry.install();
        Ok(Session { mem, registry, imports, exe_path: exe_path.to_string() })
    }

    /// Call `target` like a `call` instruction would, with `args` pushed
    /// right to left (stdcall/cdecl; `ecx` for thiscall), on a fresh frame
    /// of the session stack.
    pub fn call(&mut self, target: u32, ecx: u32, args: &[u32]) -> Outcome {
        let mut cpu = Cpu { esp: STACK_TOP - 0x100, ecx, ..Cpu::default() };
        for &a in args.iter().rev() {
            cpu.esp -= 4;
            self.mem.set_u32(cpu.esp, a);
        }
        cpu.esp -= 4;
        self.mem.set_u32(cpu.esp, RETURN_SENTINEL);
        let (mem, registry) = (&mut self.mem, &self.registry);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| registry.call(mem, &mut cpu, target)));
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
                        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "panic".into());
                    let regs = [cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi];
                    Outcome::Fault(msg, self.backtrace(cpu.esp), regs)
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
  guest return addresses on the stack (innermost first):")?;
                    for r in bt {
                        write!(f, " {r:#010x}")?;
                    }
                }
                Ok(())
            }
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

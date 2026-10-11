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
    Fault(String),
}

impl Session {
    /// Load `exe_path` (the player's DDDA.exe) and bind everything.
    pub fn new(exe_path: &str) -> Result<Session, String> {
        let exe = ddexe::ExeFile::open(exe_path).map_err(|e| e.to_string())?;
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
                    Outcome::Fault(e.downcast_ref::<String>().cloned()
                        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "panic".into()))
                }
            }
        }
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
            Outcome::Fault(s) => write!(f, "stopped: {s}"),
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

//! The production dispatcher behind `x86::call_ext` (docs/RUNTIME.md,
//! "Dispatcher and the measured boot frontier").
//!
//! Lifted code reaches untranslated or indirectly called targets through
//! `x86::call_ext(m, cpu, target)` with the return address already pushed.
//! The [`Registry`] maps each address to exactly one binding: a lifted
//! function (every one, ext functions included: the test-only rule that
//! ext roots are mocked does not apply here) or a reviewed service adapter.
//! An address without a binding stops with an [`Unbound`] report: the
//! target, the return address on the guest stack and the registers. It
//! never resumes at an invented return.
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu, LiftFn};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

/// What runs at an address.
#[derive(Clone, Copy)]
pub enum Binding {
    /// A verified 1:1 translation.
    Lifted(LiftFn),
    /// A reviewed replacement for platform code (OS, CRT, device services):
    /// same ABI as a lifted function (starts with the return address at
    /// [esp], ends with the original's `ret`).
    Service(LiftFn),
}

impl Binding {
    fn function(self) -> LiftFn {
        match self {
            Binding::Lifted(f) | Binding::Service(f) => f,
        }
    }
    fn kind(self) -> &'static str {
        match self {
            Binding::Lifted(_) => "lifted",
            Binding::Service(_) => "service",
        }
    }
}

/// A call to an address nothing is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unbound {
    pub target: u32,
    pub name: Option<String>,
    /// The guest return address (at [esp] when the call arrives).
    pub return_to: Option<u32>,
    pub regs: [u32; 8],
}

impl fmt::Display for Unbound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "call to untranslated code at {:#010x}", self.target)?;
        if let Some(n) = &self.name {
            write!(f, " ({n})")?;
        }
        match self.return_to {
            Some(r) => write!(f, ", called from before {r:#010x}")?,
            None => write!(f, ", return address unreadable")?,
        }
        let [eax, ecx, edx, ebx, esp, ebp, esi, edi] = self.regs;
        write!(f, "; eax={eax:#x} ecx={ecx:#x} edx={edx:#x} ebx={ebx:#x} esp={esp:#x} ebp={ebp:#x} \
                   esi={esi:#x} edi={edi:#x}")
    }
}

/// Two bindings for one address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub addr: u32,
    pub existing: &'static str,
    pub new: &'static str,
}

#[derive(Default)]
pub struct Registry {
    bindings: HashMap<u32, Binding>,
    names: HashMap<u32, String>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every lifted function of the generated program.
    pub fn with_lifted_program() -> Self {
        let mut r = Self::new();
        for (addr, f) in ddnative::generated::lifted::all() {
            r.insert(addr, Binding::Lifted(f)).expect("the lifted list has one entry per address");
        }
        r
    }

    /// Add a binding; an address can have only one.
    pub fn insert(&mut self, addr: u32, b: Binding) -> Result<(), Conflict> {
        if let Some(old) = self.bindings.get(&addr) {
            return Err(Conflict { addr, existing: old.kind(), new: b.kind() });
        }
        self.bindings.insert(addr, b);
        Ok(())
    }

    /// Names for diagnostics (e.g. from the RTTI class names or the inventory).
    pub fn name(&mut self, addr: u32, name: impl Into<String>) {
        self.names.insert(addr, name.into());
    }

    pub fn get(&self, addr: u32) -> Option<Binding> {
        self.bindings.get(&addr).copied()
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Run whatever is bound at `target` (return address already pushed),
    /// or report it as unbound.
    pub fn call(&self, m: &mut dyn Mem, cpu: &mut Cpu, target: u32) -> Result<(), Unbound> {
        match self.get(target) {
            Some(b) => {
                b.function()(m, cpu);
                Ok(())
            }
            None => Err(self.unbound(m, cpu, target)),
        }
    }

    fn unbound(&self, m: &dyn Mem, cpu: &Cpu, target: u32) -> Unbound {
        let return_to = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| m.u32(cpu.esp))).ok();
        Unbound {
            target,
            name: self.names.get(&target).cloned(),
            return_to,
            regs: [cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi],
        }
    }

    /// Install this registry as this thread's `call_ext` handler. An
    /// unbound target panics with its [`Unbound`] report as the payload.
    pub fn install(self: &Rc<Self>) {
        let me = Rc::clone(self);
        x86::set_ext_calls(Some(Box::new(move |m, cpu, target| {
            if let Err(u) = me.call(m, cpu, target) {
                std::panic::panic_any(u);
            }
        })));
    }
}

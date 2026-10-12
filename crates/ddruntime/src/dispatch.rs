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
use std::sync::Arc;

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

/// Hashes a guest address: a multiply by 2^64/phi with the high half
/// folded into the low bits (the table's bucket index), so 16-byte aligned
/// function addresses spread over the buckets.
#[derive(Default)]
struct AddrHasher(u64);

impl std::hash::Hasher for AddrHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u32(self.0 as u32 ^ b as u32);
        }
    }
    #[inline]
    fn write_u32(&mut self, v: u32) {
        let h = (v as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 = h ^ (h >> 32);
    }
}

#[derive(Default)]
pub struct Registry {
    instructions: Option<ddnative_base::execution::Lookup>,
    bindings: HashMap<u32, Binding, std::hash::BuildHasherDefault<AddrHasher>>,
    names: HashMap<u32, String>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every lifted function of the generated program (none without the
    /// `game` feature: then every original address is unbound).
    pub fn with_lifted_program() -> Self {
        #[allow(unused_mut)]
        let mut r = Self::new();
        #[cfg(feature = "game")]
        r.set_instruction_lookup(ddnative::generated::lifted::instruction);
        #[cfg(feature = "game")]
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

    /// Install the generated, exact instruction-address resolver. Services
    /// retain precedence; whole-function lifts are not instruction fallbacks.
    pub fn set_instruction_lookup(&mut self, lookup: ddnative_base::execution::Lookup) {
        self.instructions = Some(lookup);
    }

    pub(crate) fn instruction(&self, ip: u32) -> Option<ddnative_base::execution::Step> {
        self.instructions.and_then(|lookup| lookup(ip))
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
        // The step executor only where gen emitted restartable instruction
        // entries (L27b, LIFT_INSTRUCTIONS); whole-function bindings run
        // directly below.
        if self.instructions.is_some() && self.instruction(target).is_some() {
            crate::executor::run(self, m, cpu, target);
            return Ok(());
        }
        match self.get(target) {
            Some(Binding::Service(f)) => {
                // L22: a service is Rust: it runs under the host default
                // MXCSR and the guest's (live while lifted code runs) waits
                // in cpu.mxcsr, so neither sees nor changes the other's
                // rounding or sticky flags.
                cpu.mxcsr = x86::Mxcsr(x86::stmxcsr());
                x86::ldmxcsr(x86::MXCSR_DEFAULT);
                // a service runs holding the guest lock in parallel mode
                // (crate::threads); an API call is also where a guest
                // thread's time slice may end
                let _lock = crate::threads::service();
                f(m, cpu);
                x86::ldmxcsr(cpu.mxcsr.0);
                Ok(())
            }
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
    pub fn install(self: &Arc<Self>) {
        // CPUID/RDTSC in lifted code answer from the recorded machine.
        ddnative_base::platform::set_machine(Some(std::rc::Rc::new(crate::machine::Recorded::new())));
        let me = Arc::clone(self);
        x86::set_ext_calls(Some(Box::new(move |m, cpu, target| {
            if let Err(u) = me.call(m, cpu, target) {
                std::panic::panic_any(u);
            }
        })));
    }
}

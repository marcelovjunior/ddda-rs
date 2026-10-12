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

    /// Bind `addr` to `b`, replacing what was there (the runtime's own
    /// services over platform models, e.g. fastmem's memcpy).
    pub fn replace(&mut self, addr: u32, b: Binding) {
        self.bindings.insert(addr, b);
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
                self.run_service(f, m, cpu, target);
                Ok(())
            }
            Some(b) => {
                b.function()(m, cpu);
                Ok(())
            }
            None => {
                // L51: the diagnostic emulator runs untranslated exe code
                #[cfg(feature = "emu")]
                if crate::emu::enabled() && crate::emu::in_text(target) {
                    crate::emu::run(self, m, cpu, target);
                    return Ok(());
                }
                Err(self.unbound(m, cpu, target))
            }
        }
    }

    /// Run the service bound at `target` (its return address pushed), the
    /// one way every caller runs a service: [`Registry::call`] and the step
    /// executor (crate::executor) alike.
    pub(crate) fn run_service(&self, f: LiftFn, m: &mut dyn Mem, cpu: &mut Cpu, target: u32) {
        // L22: a service is Rust: it runs under the host default MXCSR and
        // the guest's (live while lifted code runs) waits in cpu.mxcsr, so
        // neither sees nor changes the other's rounding or sticky flags.
        cpu.mxcsr = x86::Mxcsr(x86::stmxcsr());
        x86::ldmxcsr(x86::MXCSR_DEFAULT);
        // a service runs holding the guest lock in parallel mode
        // (crate::threads); an API call is also where a guest thread's time
        // slice may end
        let _lock = crate::threads::service();
        let traced = trace::start(self, m, cpu, target);
        let _scope = crate::threads::ServiceScope::enter();
        f(m, cpu);
        if let Some(line) = traced {
            trace::finish(line, cpu);
        }
        x86::ldmxcsr(cpu.mxcsr.0);
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
        // lifted calls are preemption points in serial mode (crate::threads)
        x86::set_preempt_hook(crate::threads::preempt);
        let me = Arc::clone(self);
        x86::set_ext_calls(Some(Box::new(move |m, cpu, target| {
            if let Err(u) = me.call(m, cpu, target) {
                std::panic::panic_any(u);
            }
        })));
    }
}

/// A service-call trace for diagnosing runs that do not stop (L21: the
/// boot's steady state). With `DDRUNTIME_TRACE_SERVICES` set to a file
/// path, each call of a named service (an import, e.g.
/// `KERNEL32.dll!ReadFile`) is written as one line: guest thread, name,
/// return address, the first four stack arguments, eax after the call.
/// A call that can block (a wait, a sleep, EnterCriticalSection,
/// GetMessage) is also written, and flushed, when it starts, ending in
/// "...": a thread blocked for good shows its last call.
/// `DDRUNTIME_TRACE_SERVICES_MAX` caps the lines (default 2,000,000); the
/// file is flushed every 1,000 lines, so a killed run keeps nearly all.
/// Unset, it costs one initialized check per service call.
mod trace {
    use super::*;
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};

    struct Trace {
        out: std::io::BufWriter<std::fs::File>,
        lines: u64,
        max: u64,
    }

    fn file() -> Option<&'static Mutex<Trace>> {
        static T: OnceLock<Option<Mutex<Trace>>> = OnceLock::new();
        T.get_or_init(|| {
            let path = std::env::var_os("DDRUNTIME_TRACE_SERVICES")?;
            let max = std::env::var("DDRUNTIME_TRACE_SERVICES_MAX").ok().and_then(|v| v.parse().ok()).unwrap_or(2_000_000);
            let f = std::fs::File::create(&path).unwrap_or_else(|e| panic!("DDRUNTIME_TRACE_SERVICES {}: {e}", path.to_string_lossy()));
            Some(Mutex::new(Trace { out: std::io::BufWriter::new(f), lines: 0, max }))
        })
        .as_ref()
    }

    /// The line's start, if this call is traced.
    pub(super) fn start(r: &Registry, m: &dyn Mem, cpu: &Cpu, target: u32) -> Option<String> {
        file()?;
        let name = r.names.get(&target)?;
        let word = |k: u32| std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| m.u32(cpu.esp.wrapping_add(4 * k)))).ok();
        let w = |k| word(k).map_or("-".to_string(), |v| format!("{v:#x}"));
        let mut line = format!("{} {} ret={} args={} {} {} {}", crate::threads::current_tid(), name, w(0), w(1), w(2), w(3), w(4));
        for k in 1..=4 {
            if let Some(s) = word(k).and_then(|p| text_at(m, p)) {
                line += &format!(" a{k}={s:?}");
            }
        }
        if ["Wait", "Sleep", "EnterCriticalSection", "GetMessage"].iter().any(|w| name.contains(w)) {
            write(&format!("{line} ..."), true);
        }
        Some(line)
    }

    /// The text an argument points to, if it looks like a string: at least
    /// three printable ASCII characters, as bytes or as UTF-16 (up to 260).
    fn text_at(m: &dyn Mem, p: u32) -> Option<String> {
        if p < 0x10000 {
            return None;
        }
        let byte = |a: u32| m.probe(a, 1, false).ok().map(|_| m.u8(a));
        let printable = |b: u8| (0x20..0x7F).contains(&b);
        let wide = byte(p.wrapping_add(1)) == Some(0);
        let step = if wide { 2 } else { 1 };
        let mut s = String::new();
        for k in 0..260u32 {
            let a = p.wrapping_add(k * step);
            match (byte(a), wide) {
                (Some(b), false) if printable(b) => s.push(b as char),
                (Some(b), true) if printable(b) && byte(a.wrapping_add(1)) == Some(0) => s.push(b as char),
                _ => break,
            }
        }
        (s.len() >= 3).then_some(s)
    }

    pub(super) fn finish(line: String, cpu: &Cpu) {
        write(&format!("{line} -> {:#x}", cpu.eax), false);
    }

    fn write(line: &str, flush: bool) {
        let Some(t) = file() else { return };
        let mut t = t.lock().unwrap_or_else(|e| e.into_inner());
        if t.lines >= t.max {
            return;
        }
        t.lines += 1;
        let _ = writeln!(t.out, "{line}");
        if flush || t.lines % 1000 == 0 || t.lines == t.max {
            let _ = t.out.flush();
        }
    }
}

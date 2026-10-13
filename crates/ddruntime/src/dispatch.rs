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
use ddnative_base::execution::{Lookup, Step};
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
    /// A locked service whose `threads::unlocked` work is its final host
    /// operation: afterward only CPU/stack return, no runtime state or
    /// callbacks. Outermost parallel calls need not reacquire after that
    /// work. Audit every unlocked call before assigning this policy.
    ServiceTail(LiftFn),
    /// A service that touches only guest memory, host atomics, the host
    /// clock or the calling thread's own id: no unprotected runtime state.
    /// A service may explicitly take `threads::with_lock` for a slow path
    /// (L82 input events/hooks); snapshots must have their own synchronization.
    /// In parallel
    /// mode it runs without the guest lock (L65; the Interlocked family,
    /// memcpy/memset, the timers); in the serial mode it is an API call
    /// like any other (a time slice may end there).
    LockFree(LiftFn),
    /// A Direct3D 9 / D3DX method (L93): runs holding the D3D lock
    /// (crate::d3d9dev::D3dCell) for its whole call, so D3D calls stay
    /// atomic against each other; with `true` it also holds the guest lock
    /// (taken first), without it (the hot device methods, which touch only
    /// the D3D objects and the backend) none in parallel mode.
    Device(LiftFn, bool),
}

impl Binding {
    fn kind(self) -> &'static str {
        match self {
            Binding::Lifted(_) => "lifted",
            Binding::Service(_) => "service",
            Binding::ServiceTail(_) => "service with host-only tail",
            Binding::LockFree(_) => "lock-free service",
            Binding::Device(..) => "D3D service",
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

/// What runs at an address: its binding, and its step entry (the program's
/// instruction lookup's answer there, resolved once).
#[derive(Clone, Copy)]
struct Entry {
    binding: Option<Binding>,
    step: Option<Step>,
}

/// The step entries from `at` on, into `entries`: one per instruction, so a
/// step-lifted body ends where 16 bytes in a row (more than the longest x86
/// instruction) hold none. Stops early at an entry already indexed (a scan
/// from there has indexed what follows).
fn index_steps(entries: &mut Entries, lookup: Lookup, mut at: u32) {
    // only a step entry starts a body (most bound addresses are not one)
    if lookup(at).is_none() {
        return;
    }
    let mut gap = 0;
    while gap < 16 {
        match lookup(at) {
            Some(s) => {
                let e = entries.entry(at).or_insert(Entry { binding: None, step: None });
                if e.step.is_some() {
                    return;
                }
                e.step = Some(s);
                gap = 0;
            }
            None => gap += 1,
        }
        at = match at.checked_add(1) {
            Some(a) => a,
            None => return,
        };
    }
}

type Entries = HashMap<u32, Entry, std::hash::BuildHasherDefault<AddrHasher>>;

/// L70: every call_ext reaches [`Registry::call`], so the address is looked
/// up once, in one table holding both the bindings and the step entries.
/// Before, the generated `instruction` lookup asked all 32 shards in turn
/// (32 calls, each a binary search over its shard's table) on every call,
/// for the few hundred step-lifted functions, and then the bindings were
/// probed separately. The table holds every bound address with its step
/// entry resolved, and the interior step entries of step-lifted bodies
/// ([`index_steps`]); an address it lacks asks the program's lookup, so the
/// answers are exactly the program's.
#[derive(Default)]
pub struct Registry {
    instructions: Option<Lookup>,
    entries: Entries,
    bound: usize,
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
        // the readable ports and mods over their lifted bodies (L60)
        #[cfg(feature = "game")]
        ddports::ensure_with(|ports| {
            crate::archive_inflate::install(ports);
            crate::procedural_geometry::install(ports);
            if crate::ndraw_stats::enabled() {
                crate::ndraw_stats::wrap_executor(ports, &ddnative::generated::lifted::bodies());
            }
            if crate::load_log::enabled() {
                crate::load_log::wrap_archive(ports, &ddnative::generated::lifted::bodies());
            }
        });
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
        if let Some(old) = self.get(addr) {
            return Err(Conflict { addr, existing: old.kind(), new: b.kind() });
        }
        self.replace(addr, b);
        Ok(())
    }

    /// Bind `addr` to `b`, replacing what was there (the runtime's own
    /// services over platform models, e.g. fastmem's memcpy).
    pub fn replace(&mut self, addr: u32, b: Binding) {
        match self.entries.get_mut(&addr) {
            Some(e) => {
                // its step entry is resolved already (with the lookup set)
                self.bound += e.binding.is_none() as usize;
                e.binding = Some(b);
            }
            None => {
                self.entries.insert(addr, Entry { binding: Some(b), step: None });
                self.bound += 1;
                if let Some(lookup) = self.instructions {
                    index_steps(&mut self.entries, lookup, addr);
                }
            }
        }
    }

    /// Names for diagnostics (e.g. from the RTTI class names or the inventory).
    pub fn name(&mut self, addr: u32, name: impl Into<String>) {
        self.names.insert(addr, name.into());
    }

    pub fn get(&self, addr: u32) -> Option<Binding> {
        self.entries.get(&addr).and_then(|e| e.binding)
    }

    /// Install the generated, exact instruction-address resolver. Services
    /// retain precedence; whole-function lifts are not instruction fallbacks.
    pub fn set_instruction_lookup(&mut self, lookup: Lookup) {
        self.instructions = Some(lookup);
        // resolve the step entries again, from every bound address
        self.entries.retain(|_, e| e.binding.is_some());
        for e in self.entries.values_mut() {
            e.step = None;
        }
        let bound: Vec<u32> = self.entries.keys().copied().collect();
        for addr in bound {
            index_steps(&mut self.entries, lookup, addr);
        }
    }

    pub(crate) fn instruction(&self, ip: u32) -> Option<Step> {
        let lookup = self.instructions?;
        match self.entries.get(&ip) {
            Some(e) => e.step,
            None => lookup(ip),
        }
    }

    pub fn len(&self) -> usize {
        self.bound
    }

    pub fn is_empty(&self) -> bool {
        self.bound == 0
    }

    /// Run whatever is bound at `target` (return address already pushed),
    /// or report it as unbound.
    pub fn call(&self, m: &mut dyn Mem, cpu: &mut Cpu, target: u32) -> Result<(), Unbound> {
        // The step executor only where gen emitted restartable instruction
        // entries (L27b, LIFT_INSTRUCTIONS); whole-function bindings run
        // directly below.
        let binding = match self.entries.get(&target) {
            Some(Entry { step: Some(_), .. }) => None,
            Some(Entry { binding: Some(b), .. }) => Some(*b),
            // not indexed: no binding, and a step entry only if the
            // program's lookup has one (a body index_steps did not reach)
            _ => match self.instructions {
                Some(lookup) if lookup(target).is_some() => None,
                _ => return self.call_unbound(m, cpu, target),
            },
        };
        match binding {
            None => crate::executor::run(self, m, cpu, target),
            Some(Binding::Service(f)) => self.run_service(f, m, cpu, target, true, false),
            Some(Binding::ServiceTail(f)) => self.run_service(f, m, cpu, target, true, true),
            Some(Binding::LockFree(f)) => self.run_service(f, m, cpu, target, false, false),
            Some(Binding::Device(f, guest)) => self.run_service_as(f, m, cpu, target, guest, false, true),
            Some(Binding::Lifted(f)) => f(m, cpu),
        }
        Ok(())
    }

    #[cold]
    fn call_unbound(&self, m: &mut dyn Mem, cpu: &mut Cpu, target: u32) -> Result<(), Unbound> {
        // L51: the diagnostic emulator runs untranslated exe code
        #[cfg(feature = "emu")]
        if crate::emu::enabled() && crate::emu::in_text(target) {
            crate::emu::run(self, m, cpu, target);
            return Ok(());
        }
        Err(self.unbound(m, cpu, target))
    }

    /// Run the service bound at `target` (its return address pushed), the
    /// one way every caller runs a service: [`Registry::call`] and the step
    /// executor (crate::executor) alike.
    /// `locked`: whether it takes the guest lock in parallel mode (a
    /// [`Binding::Service`]; a [`Binding::LockFree`] does not).
    pub(crate) fn run_service(&self, f: LiftFn, m: &mut dyn Mem, cpu: &mut Cpu, target: u32, locked: bool, tail: bool) {
        self.run_service_as(f, m, cpu, target, locked, tail, false)
    }

    /// [`run_service`](Self::run_service); `device`: holding the D3D lock
    /// for the call (a [`Binding::Device`]), taken after the guest lock and
    /// after the time-slice yield, so the two locks are always taken in that
    /// order.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_service_as(&self, f: LiftFn, m: &mut dyn Mem, cpu: &mut Cpu, target: u32, locked: bool, tail: bool, device: bool) {
        // L22: a service is Rust: it runs under the host default MXCSR and
        // the guest's (live while lifted code runs) waits in cpu.mxcsr, so
        // neither sees nor changes the other's rounding or sticky flags.
        cpu.mxcsr = x86::Mxcsr(x86::stmxcsr());
        x86::ldmxcsr(x86::MXCSR_DEFAULT);
        // L62: the guest x87 state leaves the host FPU, so host code
        // (drivers, libraries) sees the thread's own FPU
        ddnative_base::x87::park();
        // a service runs holding the guest lock in parallel mode
        // (crate::threads); an API call is also where a guest thread's time
        // slice may end
        let stats = crate::threads::stats::on().then(|| {
            let name = self.names.get(&target).cloned().unwrap_or_else(|| format!("{target:#010x}"));
            crate::threads::stats::start(name)
        });
        let lock = if locked { crate::threads::service() } else { crate::threads::service_lock_free() };
        let traced = trace::start(self, m, cpu, target);
        let scope = crate::threads::ServiceScope::enter_with_tail(tail);
        let d3d = device.then(crate::d3d9dev::device_lock);
        f(m, cpu);
        drop(d3d);
        if let Some(line) = traced {
            trace::finish(line, cpu);
        }
        drop(scope);
        drop(lock);
        drop(stats);
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
        // the lifted code's slow paths (dispatch cache, x87 state), and
        // lifted calls are preemption points in serial mode (crate::threads)
        ddnative_host::install();
        ddnative_host::set_preempt_hook(crate::threads::preempt);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{GuestMemory, READ, WRITE};
    use ddnative_base::execution::{Flow, Step};
    use std::sync::OnceLock;
    use std::time::Instant;

    const FUNCTIONS: u32 = 56_000;
    const TEXT: u32 = 0x0040_1000;
    const STEP5: u32 = 0x012E_D1F5;
    const STEP6: u32 = 0x0130_4000;

    fn ret(_: &mut dyn Mem, c: &mut Cpu) {
        c.esp += 4;
    }
    fn step(_: &mut dyn Mem, _: &mut Cpu, ip: u32) -> Flow {
        Flow::Next(ip + 1)
    }

    /// A model of the generated facade's `instruction` (ddnative
    /// generated/lifted.rs): each of the 32 shards binary-searches its own
    /// table, in turn. As in the current gen, two shards (the CRT's) hold
    /// step entries, 2,157 and 823 of them, one per instruction (5 bytes
    /// apart here).
    fn shard_tables() -> &'static [Vec<(u32, Step)>] {
        static T: OnceLock<Vec<Vec<(u32, Step)>>> = OnceLock::new();
        T.get_or_init(|| {
            (0..32)
                .map(|k| match k {
                    5 => (0..2157).map(|i| (STEP5 + 5 * i, step as Step)).collect(),
                    6 => (0..823).map(|i| (STEP6 + 5 * i, step as Step)).collect(),
                    _ => Vec::new(),
                })
                .collect()
        })
    }
    #[inline(never)]
    fn shard_instruction(k: usize, ip: u32) -> Option<Step> {
        let t = &shard_tables()[k];
        t.binary_search_by_key(&ip, |e| e.0).ok().map(|i| t[i].1)
    }
    fn modeled_instruction(ip: u32) -> Option<Step> {
        (0..32).find_map(|k| shard_instruction(k, ip))
    }

    /// 56,000 lifted functions (the program's count) and the step-lifted
    /// functions' entries (every 40th step entry here).
    fn registry(lookup_first: bool) -> Registry {
        let mut r = Registry::new();
        if lookup_first {
            r.set_instruction_lookup(modeled_instruction);
        }
        for k in 0..FUNCTIONS {
            r.insert(TEXT + 16 * k, Binding::Lifted(ret)).unwrap();
        }
        for i in (0..2157).step_by(40) {
            r.insert(STEP5 + 5 * i, Binding::Lifted(ret)).unwrap();
        }
        for i in (0..823).step_by(40) {
            r.insert(STEP6 + 5 * i, Binding::Lifted(ret)).unwrap();
        }
        if !lookup_first {
            r.set_instruction_lookup(modeled_instruction);
        }
        r
    }

    fn same(a: Option<Step>, b: Option<Step>) -> bool {
        a.map(|f| f as usize) == b.map(|f| f as usize)
    }

    /// The registry answers exactly as the program's lookup and its
    /// bindings do, at every address: function entries, step entries
    /// (entries and interiors), the bytes between them, and outside; also
    /// when the lookup is installed after the bindings, and after a
    /// replace.
    #[test]
    fn lookups_answer_as_the_program_does() {
        for lookup_first in [true, false] {
            let mut r = registry(lookup_first);
            let ranges = [(TEXT - 64, TEXT + 4096), (TEXT + 16 * FUNCTIONS - 64, TEXT + 16 * FUNCTIONS + 64),
                          (STEP5 - 64, STEP5 + 5 * 2157 + 64), (STEP6 - 64, STEP6 + 5 * 823 + 64), (0, 64),
                          (u32::MAX - 64, u32::MAX)];
            for &(lo, hi) in &ranges {
                for a in lo..=hi {
                    assert!(same(r.instruction(a), modeled_instruction(a)), "instruction({a:#x})");
                    let bound = (a >= TEXT && a < TEXT + 16 * FUNCTIONS && (a - TEXT) % 16 == 0)
                        || (a >= STEP5 && a < STEP5 + 5 * 2157 && (a - STEP5) % 200 == 0)
                        || (a >= STEP6 && a < STEP6 + 5 * 823 && (a - STEP6) % 200 == 0);
                    assert_eq!(r.get(a).is_some(), bound, "get({a:#x})");
                }
            }
            r.replace(TEXT, Binding::LockFree(ret));
            assert!(matches!(r.get(TEXT), Some(Binding::LockFree(_))));
            r.replace(STEP5 + 5, Binding::Service(ret));
            assert!(matches!(r.get(STEP5 + 5), Some(Binding::Service(_))));
            assert!(same(r.instruction(STEP5 + 5), modeled_instruction(STEP5 + 5)));
        }
    }

    /// The CRT's two shards (crt-check): with their real functions and step
    /// entries, the registry answers as their lookups at every address of
    /// their range, and indexes (rather than falls back for) their step
    /// entries.
    #[cfg(feature = "crt-check")]
    #[test]
    fn lookups_answer_as_the_crt_shards_do() {
        fn lookup(ip: u32) -> Option<Step> {
            ddlift_05::instruction(ip).or_else(|| ddlift_06::instruction(ip))
        }
        let mut r = Registry::new();
        r.set_instruction_lookup(lookup);
        for &(a, f) in ddlift_05::FUNCTIONS.iter().chain(ddlift_06::FUNCTIONS) {
            r.insert(a, Binding::Lifted(f)).unwrap();
        }
        let all: Vec<u32> = ddlift_05::INSTRUCTIONS.iter().chain(ddlift_06::INSTRUCTIONS).map(|e| e.0).collect();
        let functions = ddlift_05::FUNCTIONS.iter().chain(ddlift_06::FUNCTIONS).map(|e| e.0);
        let lo = functions.clone().chain(all.iter().copied()).min().unwrap();
        let hi = functions.chain(all.iter().copied()).max().unwrap();
        for a in lo - 64..=hi + 64 {
            assert!(same(r.instruction(a), lookup(a)), "instruction({a:#x})");
        }
        let indexed = all.iter().filter(|a| r.entries.get(a).is_some_and(|e| e.step.is_some())).count();
        eprintln!("{indexed} of {} step entries indexed, {:#x}..{:#x} checked", all.len(), lo, hi);
        assert_eq!(indexed, all.len());
    }

    /// Calls per second through Registry::call to lifted targets (as
    /// call_ext's virtual calls arrive), and the step executor's lookups at
    /// interior step entries. Numbers with `-- --nocapture`.
    #[test]
    fn dispatch_speed() {
        let t0 = Instant::now();
        let r = registry(true);
        let build = t0.elapsed();
        let mut m = GuestMemory::new();
        m.map(0x0010_0000, 0x1000, READ | WRITE).unwrap();
        let mut c = Cpu::default();
        let n = 1_000_000u32;
        let mut x = 1u32;
        let t = Instant::now();
        for _ in 0..n {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            c.esp = 0x0010_0800;
            r.call(&mut m, &mut c, TEXT + 16 * ((x >> 8) % FUNCTIONS)).unwrap();
        }
        let calls = n as f64 / t.elapsed().as_secs_f64();
        let t = Instant::now();
        let mut found = 0;
        for k in 0..n {
            found += r.instruction(STEP5 + 5 * (k % 2157)).is_some() as u32;
        }
        let steps = n as f64 / t.elapsed().as_secs_f64();
        assert_eq!(found, n);
        eprintln!("Registry::call to a lifted function: {:.1} M calls/s ({:.1} ns each)", calls / 1e6, 1e9 / calls);
        eprintln!("instruction() at a step entry:       {:.1} M lookups/s ({:.1} ns each)", steps / 1e6, 1e9 / steps);
        eprintln!("registry built in {:.1} ms", build.as_secs_f64() * 1e3);
    }
}

//! ddperf: fixed workloads that measure the lifted guest code's speed
//! (brief L25, part 3). Build with the release profile:
//!
//!     cargo run --release -p ddperf -- init [path\to\DDDA.exe] [--count]
//!     cargo run --release -p ddperf -- replay [--limit N] [--passes P] [--count] [--vectors FILE]
//!
//! `init` runs DDDA.exe's static initializers in CRT order on a full
//! runtime session, like examples/initializers.rs, and times them.
//! `replay` runs every recorded vector (the required-vector set,
//! `vectors/lift.json`) through its lifted function on the runtime's own
//! guest memory (`ddruntime::memory::GuestMemory`), `--passes` times, and
//! checks each result against the original's (registers, xmm0, x87 state,
//! calls, and every byte the original wrote). Only the lifted function's
//! own run is timed; seeding memory and checking are not.
//!
//! `micro` times single operations behind those paths: a `call_ext`
//! round trip through the runtime's Registry to a service that only
//! returns, and a guest memory read through `&dyn Mem` against the same
//! read called directly (what devirtualising `Mem` could save), and
//! resolving a direct cross-shard call's target with and without the
//! per-thread cache.
//!
//! `--count` reports how often the guest touches memory (by width, and
//! accesses that cross a page) and calls `call_ext`, through a counting
//! `Mem` wrapper and a counting `call_ext` handler. Counting changes the
//! timings, so counted runs are for the counts only.
use ddnative_base::mem::{Mem, HEAP_BASE};
use ddnative_base::x86::{self, Cpu, Entry, LiftFn};
use ddruntime::memory::{GuestMemory, PAGE, READ, WRITE};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::time::{Duration, Instant};

// --- counting ------------------------------------------------------------------

#[derive(Default, Clone, Copy)]
struct Counts {
    reads: [u64; 3],
    writes: [u64; 3],
    crossing: u64,
    ext: u64,
}

thread_local! {
    static COUNTS: Cell<Counts> = Cell::new(Counts::default());
    static EXT_TARGETS: RefCell<HashMap<u32, u64>> = RefCell::new(HashMap::new());
}

fn count(f: impl FnOnce(&mut Counts)) {
    COUNTS.with(|c| {
        let mut v = c.get();
        f(&mut v);
        c.set(v);
    });
}

fn width_slot(w: u32) -> usize {
    match w {
        1 => 0,
        2 => 1,
        _ => 2,
    }
}

/// The guest memory as lifted code sees it, optionally counting, with the
/// fixtures' bump allocator when `alloc` is set (the vectors were recorded
/// with harness.py's allocator models; the runtime keeps the trait default).
struct Probe<'a, M: Mem> {
    inner: &'a mut M,
    counting: bool,
    alloc: bool,
    heap: u32,
}

impl<M: Mem> Probe<'_, M> {
    #[inline]
    fn note(&self, addr: u32, width: u32, write: bool) {
        if self.counting {
            count(|c| {
                if write { c.writes[width_slot(width)] += 1 } else { c.reads[width_slot(width)] += 1 }
                if (addr % PAGE) + width > PAGE {
                    c.crossing += 1;
                }
            });
        }
    }
}

impl<M: Mem> Mem for Probe<'_, M> {
    fn u8(&self, addr: u32) -> u8 {
        self.note(addr, 1, false);
        self.inner.u8(addr)
    }
    fn set_u8(&mut self, addr: u32, v: u8) {
        self.note(addr, 1, true);
        self.inner.set_u8(addr, v)
    }
    fn u16(&self, addr: u32) -> u16 {
        self.note(addr, 2, false);
        self.inner.u16(addr)
    }
    fn u32(&self, addr: u32) -> u32 {
        self.note(addr, 4, false);
        self.inner.u32(addr)
    }
    fn set_u16(&mut self, addr: u32, v: u16) {
        self.note(addr, 2, true);
        self.inner.set_u16(addr, v)
    }
    fn set_u32(&mut self, addr: u32, v: u32) {
        self.note(addr, 4, true);
        self.inner.set_u32(addr, v)
    }
    fn alloc(&mut self, size: u32) -> u32 {
        if !self.alloc {
            return self.inner.alloc(size);
        }
        // the same bump allocator as ddnative_base::mem::Sparse
        if self.heap == 0 {
            self.heap = HEAP_BASE;
        }
        let a = self.heap;
        self.heap = self.heap.wrapping_add(size.max(1)).wrapping_add(15) & !15;
        a
    }
}

fn print_counts(elapsed: Duration) {
    let c = COUNTS.with(|c| c.get());
    let reads: u64 = c.reads.iter().sum();
    let writes: u64 = c.writes.iter().sum();
    println!("memory: {reads} reads (u8 {}, u16 {}, u32 {}), {writes} writes (u8 {}, u16 {}, u32 {}), {} cross a page",
             c.reads[0], c.reads[1], c.reads[2], c.writes[0], c.writes[1], c.writes[2], c.crossing);
    println!("call_ext: {} calls", c.ext);
    let secs = elapsed.as_secs_f64().max(1e-9);
    println!("per second (counted run): {:.1} M accesses, {:.2} M call_ext", (reads + writes) as f64 / secs / 1e6, c.ext as f64 / secs / 1e6);
    let mut top: Vec<(u32, u64)> = EXT_TARGETS.with(|t| t.borrow().iter().map(|(a, n)| (*a, *n)).collect());
    top.sort_by(|a, b| b.1.cmp(&a.1));
    if !top.is_empty() {
        println!("most frequent call_ext targets:");
        for (a, n) in top.iter().take(15) {
            println!("  {n:10} x {a:#010x}");
        }
    }
}

// --- init: the static initializers ---------------------------------------------------

fn run_init(exe: &str, counting: bool) {
    use ddruntime::session::{Session, RETURN_SENTINEL, STACK_TOP};
    let t = Instant::now();
    let mut s = Session::new(exe).unwrap_or_else(|e| panic!("{e}"));
    let load = t.elapsed();
    // silent faults (they are counted); no backtrace capture inside the timing
    std::panic::set_hook(Box::new(|_| {}));
    if counting {
        let reg = s.registry.clone();
        x86::set_ext_calls(Some(Box::new(move |m, cpu, target| {
            count(|c| c.ext += 1);
            EXT_TARGETS.with(|t| *t.borrow_mut().entry(target).or_default() += 1);
            if let Err(u) = reg.call(m, cpu, target) {
                std::panic::panic_any(u);
            }
        })));
    }
    let inits = s.initializers();
    let registry = s.registry.clone();
    let (mut done, mut stopped, mut skipped) = (0, 0, 0);
    let mut eax_sum: u64 = 0;
    let t = Instant::now();
    for (_, target) in &inits {
        if registry.get(*target).is_none() {
            skipped += 1;
            continue;
        }
        let mut cpu = Cpu { esp: STACK_TOP - 0x100 - 4, ..Cpu::default() };
        s.mem.set_u32(cpu.esp, RETURN_SENTINEL);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if counting {
                registry.call(&mut Probe { inner: &mut s.mem, counting, alloc: false, heap: 0 }, &mut cpu, *target)
            } else {
                registry.call(&mut s.mem, &mut cpu, *target)
            }
        }));
        match r {
            Ok(Ok(())) if cpu.esp == STACK_TOP - 0x100 => {
                done += 1;
                eax_sum = eax_sum.wrapping_add(cpu.eax as u64);
            }
            _ => stopped += 1,
        }
    }
    let run = t.elapsed();
    println!("session load {:.1} ms", load.as_secs_f64() * 1e3);
    println!("{} initializers: {done} ran to completion, {skipped} not translated, {stopped} stopped; eax sum {eax_sum:#x}",
             inits.len());
    println!("initializers: {:.3} ms", run.as_secs_f64() * 1e3);
    println!("{} guest pages backed", s.mem.backed_pages());
    if counting {
        print_counts(run);
    }
}

// --- replay: the recorded vectors ---------------------------------------------------

#[derive(Clone, Copy)]
struct Call {
    target: u32,
    regs: [u32; 8],
    eax: u32,
    ecx: u32,
    edx: u32,
    xmm0: [u32; 4],
    pop: u32,
    st_push: Option<u32>,
}

struct Case {
    addr: u32,
    regs_in: [u32; 8],
    reads: Vec<(u32, Vec<u8>)>,
    writes: Vec<(u32, Vec<u8>)>,
    regs_out: [u32; 8],
    xmm0_out: u32,
    st0_out: Option<u32>,
    /// x87 control word, depth and extended st0, when recorded
    x87: Option<(u32, u32, Option<(u64, u16)>)>,
    calls: Vec<Call>,
}

const REGS: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];

fn u(v: &serde_json::Value) -> u32 {
    v.as_u64().unwrap() as u32
}

fn hex_runs(v: &serde_json::Value) -> Vec<(u32, Vec<u8>)> {
    v.as_array().unwrap().iter().map(|run| {
        let h = run[1].as_str().unwrap();
        (u(&run[0]), (0..h.len() / 2).map(|k| u8::from_str_radix(&h[2 * k..2 * k + 2], 16).unwrap()).collect())
    }).collect()
}

fn case_of(c: &serde_json::Value) -> Case {
    let regs = |r: &serde_json::Value| REGS.map(|k| u(&r[k]));
    Case {
        addr: u(&c["addr"]),
        regs_in: regs(&c["regs_in"]),
        reads: hex_runs(&c["reads"]),
        writes: hex_runs(&c["writes"]),
        regs_out: regs(&c["regs_out"]),
        xmm0_out: u(&c["xmm0_out"]),
        st0_out: c["st0_out"].as_u64().map(|v| v as u32),
        x87: c.get("x87_control").is_some().then(|| {
            let ext = c["st0_ext"].as_array().map(|v| (v[0].as_u64().unwrap(), u(&v[1]) as u16));
            (u(&c["x87_control"]), u(&c["x87_depth"]), ext)
        }),
        calls: c.get("calls").and_then(|v| v.as_array()).map(|a| a.iter().map(|e| {
            let o = &e["out"];
            Call {
                target: u(&e["target"]),
                regs: regs(&e["regs"]),
                eax: u(&o["eax"]),
                ecx: u(&o["ecx"]),
                edx: u(&o["edx"]),
                xmm0: [0, 1, 2, 3].map(|k| u(&o["xmm0"][k])),
                pop: u(&o["pop"]),
                st_push: o.get("st_push").and_then(|v| v.as_u64()).map(|v| v as u32),
            }
        }).collect()).unwrap_or_default(),
    }
}

/// Parse the vector file one case at a time (the whole file as JSON values
/// would take several GB).
fn load_cases(path: &str, limit: usize) -> Vec<Case> {
    struct Seq(usize);
    impl<'de> serde::de::Visitor<'de> for Seq {
        type Value = Vec<Case>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an array of vectors")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<Case>, A::Error> {
            let mut out = Vec::new();
            while let Some(v) = seq.next_element::<serde_json::Value>()? {
                if out.len() < self.0 {
                    out.push(case_of(&v));
                }
            }
            Ok(out)
        }
    }
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut de = serde_json::Deserializer::from_slice(&bytes);
    serde::Deserializer::deserialize_seq(&mut de, Seq(limit)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Replays the calls a vector recorded to untranslated code, like the
/// generated test (crates/ddnative/tests/lifted.rs): lifted functions and
/// platform models run; anything else returns what the recording saw.
fn replay_handler(calls: Rc<RefCell<VecDeque<Call>>>, table: Rc<HashMap<u32, LiftFn>>, counting: bool) -> Box<x86::ExtCall> {
    Box::new(move |m: &mut dyn Mem, cpu: &mut Cpu, target: u32| {
        if counting {
            count(|c| c.ext += 1);
            EXT_TARGETS.with(|t| *t.borrow_mut().entry(target).or_default() += 1);
        }
        if let Some(f) = table.get(&target) {
            return f(m, cpu);
        }
        let ev = calls.borrow_mut().pop_front().unwrap_or_else(|| panic!("unrecorded call to {target:#x}"));
        assert_eq!(target, ev.target, "call target");
        assert_eq!([cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi], ev.regs, "registers at the call");
        let _ = m.u32(cpu.esp);
        cpu.eax = ev.eax;
        cpu.ecx = ev.ecx;
        cpu.edx = ev.edx;
        cpu.xmm[0] = ev.xmm0;
        cpu.esp = cpu.esp.wrapping_add(4 + ev.pop);
        if let Some(v) = ev.st_push {
            cpu.st.push(v);
        }
    })
}

fn map_runs(mem: &mut GuestMemory, runs: &[(u32, Vec<u8>)]) {
    for (a, b) in runs {
        let end = a.wrapping_add(b.len().max(1) as u32 - 1);
        let mut p = a / PAGE;
        loop {
            if !mem.is_mapped(p * PAGE) {
                mem.map(p * PAGE, PAGE, READ | WRITE).unwrap();
            }
            if p == end / PAGE {
                break;
            }
            p = (p + 1) % (1 << 20);
        }
    }
}

fn run_replay(path: &str, limit: usize, passes: usize, counting: bool) {
    let t = Instant::now();
    let cases = load_cases(path, limit);
    println!("{} vectors loaded in {:.1} s", cases.len(), t.elapsed().as_secs_f64());
    let all: HashMap<u32, LiftFn> = ddnative::generated::lifted::all().into_iter().collect();
    let table = Rc::new(ddnative::generated::lifted::dispatch_table());
    std::panic::set_hook(Box::new(|_| {}));
    // the timer's own cost per measured call (reported, not subtracted)
    let mut probe = Duration::ZERO;
    for _ in 0..100_000 {
        let s = Instant::now();
        probe += s.elapsed();
    }
    println!("timer: {:.1} ns per Instant pair", probe.as_secs_f64() * 1e9 / 100_000.0);

    const CHUNK: usize = 256;
    let mut per_fn: HashMap<u32, Duration> = HashMap::new();
    let mut total = Duration::ZERO;
    let mut failures: Vec<u32> = Vec::new();
    for pass in 0..passes {
        for chunk in cases.chunks(CHUNK) {
            let mut mem = GuestMemory::new();
            for c in chunk {
                map_runs(&mut mem, &c.reads);
                map_runs(&mut mem, &c.writes);
            }
            for c in chunk {
                let f = all[&c.addr];
                for (a, b) in &c.reads {
                    mem.poke(*a, b).unwrap();
                }
                let calls = Rc::new(RefCell::new(c.calls.iter().copied().collect::<VecDeque<_>>()));
                x86::set_ext_calls(Some(replay_handler(calls.clone(), table.clone(), counting)));
                let [eax, ecx, edx, ebx, esp, ebp, esi, edi] = c.regs_in;
                let mut cpu = Cpu::from(Entry { eax, ecx, edx, ebx, esp, ebp, esi, edi, xmm0: 0 });
                cpu.fs_base = 0x7FFD_E000; // harness.TEB_BASE
                let mut probe = Probe { inner: &mut mem, counting, alloc: true, heap: 0 };
                let start = Instant::now();
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut probe, &mut cpu)));
                let took = start.elapsed();
                total += took;
                *per_fn.entry(c.addr).or_default() += took;
                if pass == 0 {
                    let ok = r.is_ok() && calls.borrow().is_empty() && {
                        let x = cpu.exit();
                        let x87 = c.x87.is_none_or(|(control, depth, ext)| {
                            cpu.st.control() as u32 == control && cpu.st.depth() == depth && cpu.st.top_extended() == ext
                        });
                        [x.eax, x.ecx, x.edx, x.ebx, x.esp, x.ebp, x.esi, x.edi] == c.regs_out
                            && x.xmm0 == c.xmm0_out && (c.st0_out.is_none() || x.st0 == c.st0_out) && x87
                            && c.writes.iter().all(|(a, b)| mem.read(*a, b.len() as u32).ok().as_deref() == Some(&b[..]))
                    };
                    if !ok {
                        failures.push(c.addr);
                    }
                }
            }
        }
    }
    x86::set_ext_calls(None);
    failures.sort();
    failures.dedup();
    println!("{} cases x {passes} passes: lifted code {:.3} ms in total, {:.3} ms per pass, {:.1} ns per case",
             cases.len(), total.as_secs_f64() * 1e3, total.as_secs_f64() * 1e3 / passes as f64,
             total.as_secs_f64() * 1e9 / (cases.len() * passes).max(1) as f64);
    println!("{} functions differ from the recording on guest memory: {:x?}", failures.len(), &failures[..failures.len().min(20)]);
    let mut top: Vec<(u32, Duration)> = per_fn.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    println!("functions taking the most time:");
    for (a, d) in top.iter().take(15) {
        println!("  {:10.3} ms {a:#010x}", d.as_secs_f64() * 1e3);
    }
    if counting {
        print_counts(total);
    }
}

// --- micro: single operations ------------------------------------------------------

/// `ret`: what the cheapest service does.
fn just_return(m: &mut dyn Mem, cpu: &mut Cpu) {
    let _ = m.u32(cpu.esp);
    cpu.esp = cpu.esp.wrapping_add(4);
}

#[inline(never)]
fn read_dyn(m: &dyn Mem, base: u32, n: u32) -> u32 {
    (0..n).fold(0u32, |acc, k| acc.wrapping_add(m.u32(base.wrapping_add((k & 0x3FF) * 4))))
}

/// The lifted code's lowered access (lift.py fast_memory): inline fast
/// path through the page table, the trait call otherwise.
#[inline(never)]
fn read_fast(m: &dyn Mem, base: u32, n: u32) -> u32 {
    let pt = m.page_table();
    (0..n).fold(0u32, |acc, k| acc.wrapping_add(m.fast_u32(pt, base.wrapping_add((k & 0x3FF) * 4))))
}

#[inline(never)]
fn read_direct(m: &GuestMemory, base: u32, n: u32) -> u32 {
    (0..n).fold(0u32, |acc, k| acc.wrapping_add(Mem::u32(m, base.wrapping_add((k & 0x3FF) * 4))))
}

fn run_micro() {
    use ddruntime::dispatch::{Binding, Registry};
    const N: u32 = 10_000_000;
    let mut mem = GuestMemory::new();
    mem.map(0x1000_0000, 0x10_0000, READ | WRITE).unwrap();
    let mut registry = Registry::new();
    registry.insert(0x0100_0000, Binding::Service(just_return)).unwrap();
    let registry = std::sync::Arc::new(registry);
    registry.install();
    let mut cpu = Cpu { esp: 0x1008_0000, ..Cpu::default() };
    let t = Instant::now();
    for _ in 0..N {
        cpu.esp -= 4;
        mem.set_u32(cpu.esp, 0x1234_5678);
        x86::call_ext(&mut mem, &mut cpu, 0x0100_0000);
    }
    let ns = t.elapsed().as_secs_f64() * 1e9 / N as f64;
    println!("call_ext -> Registry -> service (push return address, ret): {ns:.2} ns per call");
    x86::set_ext_calls(None);
    for (name, f) in [("dyn Mem", 0), ("direct", 1), ("the inline fast path", 2)] {
        let t = Instant::now();
        let mut sum = 0u32;
        for _ in 0..10 {
            sum = sum.wrapping_add(match f {
                0 => read_dyn(&mem, 0x1000_0000, N),
                1 => read_direct(&mem, 0x1000_0000, N),
                _ => read_fast(&mem, 0x1000_0000, N),
            });
        }
        let ns = t.elapsed().as_secs_f64() * 1e9 / (10 * N) as f64;
        println!("guest u32 read through {name}: {ns:.2} ns ({sum:#x})");
    }
    // resolving a direct cross-shard call's target (call_lifted): 2048
    // real lifted addresses, called round robin, cached or searched
    let all = ddnative::generated::lifted::all();
    let targets: Vec<u32> = all.iter().step_by((all.len() / 2048).max(1)).map(|e| e.0).take(2048).collect();
    for (name, cached) in [("cached", true), ("searched", false)] {
        let t = Instant::now();
        let mut acc = 0usize;
        for k in 0..N as usize {
            let a = targets[k % targets.len()];
            let f = if cached { x86::lifted_target(a) } else { x86::lifted_target_uncached(a) };
            acc = acc.wrapping_add(f as usize);
        }
        let ns = t.elapsed().as_secs_f64() * 1e9 / N as f64;
        println!("call_lifted target resolution, {name}: {ns:.2} ns ({acc:#x})");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|k| args.get(k + 1)).cloned();
    let counting = flag("--count");
    match args.first().map(String::as_str) {
        Some("init") => {
            let exe = args.get(1).filter(|a| !a.starts_with("--")).cloned()
                .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
            run_init(&exe, counting);
        }
        Some("micro") => run_micro(),
        Some("replay") => {
            let dir = std::env::var("LIFT_DATA").unwrap_or_else(|_| "E:/ddda-re/notes/native".into());
            let path = value("--vectors").unwrap_or(format!("{dir}/vectors/lift.json"));
            let limit = value("--limit").map(|v| v.parse().unwrap()).unwrap_or(usize::MAX);
            let passes = value("--passes").map(|v| v.parse().unwrap()).unwrap_or(5);
            run_replay(&path, limit, passes, counting);
        }
        _ => eprintln!("usage: ddperf init [exe] [--count] | ddperf micro | ddperf replay [--limit N] [--passes P] [--count] [--vectors FILE]"),
    }
}

//! Opt-in L172 proof runner. Real execution stays serial while original-body
//! replays use private memory and private helper threads. No speculative
//! guest writes or host services can be published by these replays.
#![cfg_attr(not(feature = "game"), allow(dead_code))]

use crate::parallel_shadow::{self as lanes, LaneMemory, Reject, Snapshot};
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use mtcore::shadow_lanes::{capture_context, Limits, LANE_BYTES};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    ops::Range,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering::Relaxed},
        Arc, OnceLock,
    },
};

const SHADOW: u32 = 0x00ee2c90;
const TRAVERSE: u32 = 0x00db63e0;
const SCHEDULE: u32 = 0x00dbe560;
const SCHEDULER_GLOBAL: u32 = 0x018d0b28;
const NESTING: u32 = 0x018d0bb0;
const NESTING_OTHER: u32 = 0x018d0404;
const MAX_PAGES: usize = 16_384; // Diagnostic memory bound: 64 MiB.
const ACCESS_LIMIT: usize = 40_000_000;
static ORIGINAL: OnceLock<[LiftFn; 3]> = OnceLock::new();
static RAW: OnceLock<Arc<BTreeMap<u32, LiftFn>>> = OnceLock::new();
static SAMPLE: AtomicU64 = AtomicU64::new(0);
#[path = "shadow_replay_file.rs"]
mod capture_file;
thread_local! {
    static IN_SHADOW: Cell<bool> = const { Cell::new(false) };
    static REPLAY: RefCell<Option<Rc<Replay>>> = const { RefCell::new(None) };
}

pub(crate) fn enabled() -> bool {
    let requested = std::env::var("DDRUNTIME_PARALLEL_SHADOW").as_deref() == Ok("1");
    assert!(
        !requested || cfg!(feature = "parallel-shadow"),
        "DDRUNTIME_PARALLEL_SHADOW requires the ddruntime/parallel-shadow Cargo feature"
    );
    requested
}

pub(crate) fn install(ports: &mut Vec<(u32, LiftFn)>, bodies: &[(u32, LiftFn)]) {
    let originals = [SHADOW, TRAVERSE, SCHEDULE].map(|address| {
        ports
            .iter()
            .chain(bodies)
            .find(|(a, _)| *a == address)
            .expect("parallel shadow body")
            .1
    });
    ORIGINAL.set(originals).expect("install parallel shadow once");
    RAW.set(Arc::new(bodies.iter().copied().collect()))
        .expect("install raw shadow bodies once");
    for (address, body) in [
        (SHADOW, shadow as LiftFn),
        (TRAVERSE, traverse),
        (SCHEDULE, schedule),
    ] {
        ports.retain(|(a, _)| *a != address);
        ports.push((address, body));
    }
    eprintln!(
        "parallel-shadow installed capture=v2 first={} count={} dump={:?}",
        std::env::var("DDRUNTIME_PARALLEL_SHADOW_FIRST").unwrap_or_else(|_| "0".into()),
        std::env::var("DDRUNTIME_PARALLEL_SHADOW_COUNT").unwrap_or_else(|_| "1".into()),
        std::env::var_os("DDRUNTIME_PARALLEL_SHADOW_DUMP")
    );
}

struct ShadowScope(bool);
impl Drop for ShadowScope {
    fn drop(&mut self) {
        IN_SHADOW.with(|v| v.set(self.0));
    }
}
fn shadow(memory: &mut dyn Mem, cpu: &mut Cpu) {
    static ENTERED: std::sync::Once = std::sync::Once::new();
    ENTERED.call_once(|| eprintln!("parallel-shadow entered shadow={SHADOW:#x}"));
    let _scope = ShadowScope(IN_SHADOW.with(|v| v.replace(true)));
    ORIGINAL.get().unwrap()[0](memory, cpu);
}
fn schedule(memory: &mut dyn Mem, cpu: &mut Cpu) {
    // Registering this address also routes in-shard calls through the private
    // resolver. Ordinary execution retains the previous exact binding.
    ORIGINAL.get().unwrap()[2](memory, cpu);
}

/// Journal the real execution once, preserving native access widths. A
/// serial read inconsistent with captured input plus this execution's writes
/// rejects replay; it never changes the original execution's result.
struct Journal<'a> {
    live: &'a mut dyn Mem,
    input: RefCell<Snapshot>,
    writes: RefCell<BTreeMap<u32, u8>>,
    failure: Cell<Option<u32>>,
}
impl Journal<'_> {
    fn page(&self, address: u32) {
        if self.failure.get().is_some() || self.input.borrow().has_page(address) {
            return;
        }
        let base = address & !4095;
        if self.input.borrow().page_count() == MAX_PAGES || self.live.probe(base, 4096, false).is_err() {
            self.failure.set(Some(address));
            return;
        }
        let permissions = 1 | if self.live.probe(base, 4096, true).is_ok() {
            2
        } else {
            0
        };
        self.input.borrow_mut().capture_page(self.live, base, permissions);
    }
    fn seed(&self, start: u32, length: u32) {
        let Some(end) = start.checked_add(length) else {
            self.failure.set(Some(start));
            return;
        };
        let mut page = start & !4095;
        while page < end {
            self.page(page);
            if self.failure.get().is_some() {
                return;
            }
            page += 4096;
        }
    }
    fn before(&self, address: u32, width: u32) {
        for offset in 0..width {
            self.page(address.wrapping_add(offset));
        }
    }
    fn observed(&self, address: u32, bytes: &[u8], write: bool) {
        if self.failure.get().is_some() {
            return;
        }
        let mut writes = self.writes.borrow_mut();
        for (offset, &value) in bytes.iter().enumerate() {
            let at = address.wrapping_add(offset as u32);
            if write {
                if writes.len() == lanes::MAX_TRACKED_BYTES && !writes.contains_key(&at) {
                    self.failure.set(Some(at));
                    return;
                }
                writes.insert(at, value);
            } else if writes
                .get(&at)
                .copied()
                .unwrap_or_else(|| self.input.borrow().u8(at))
                != value
            {
                self.failure.set(Some(at));
            }
        }
    }
    fn seed_primitives(&self) {
        // sPrimitive ctor E60DE0; manager ctors 11927F0/11925E0/11929A0.
        let primitive = self.u32(0x018d1848);
        self.seed(primitive, 0x1d0);
        let pools = self.u32(primitive + 0xd0);
        if pools > 8 {
            self.failure.set(Some(primitive + 0xd0));
            return;
        }
        for lane in 0..pools {
            let pool = self.u32(primitive + 0x30 + lane * 4);
            self.seed(pool, 0x280);
            self.seed(self.u32(pool + 4), 0x84);
            self.seed(self.u32(pool + 8), 0x18);
        }
        let bytes = self.u32(primitive + 0xd8);
        self.seed(bytes, 0x14);
        self.seed(self.u32(bytes + 4), self.u32(bytes + 0xc));
        let tags = self.u32(primitive + 0xdc);
        self.seed(tags, 0x14);
        let count = self.u32(tags + 0x10);
        if let (Some(descriptors), Some(entries)) = (count.checked_mul(0x28), count.checked_mul(8)) {
            self.seed(self.u32(tags + 4), descriptors);
            self.seed(self.u32(tags + 8), entries);
        } else {
            self.failure.set(Some(tags));
        }
        let textures = self.u32(primitive + 0xd4);
        self.seed(textures, 0x10);
        if let Some(bytes) = self.u32(textures + 8).checked_mul(0x14) {
            self.seed(self.u32(textures + 4), bytes);
        } else {
            self.failure.set(Some(textures));
        }
    }
}
impl Mem for Journal<'_> {
    fn probe(&self, a: u32, n: u32, w: bool) -> Result<(), u32> {
        self.live.probe(a, n, w)
    }
    fn u8(&self, a: u32) -> u8 {
        self.before(a, 1);
        let v = self.live.u8(a);
        self.observed(a, &[v], false);
        v
    }
    fn u16(&self, a: u32) -> u16 {
        self.before(a, 2);
        let v = self.live.u16(a);
        self.observed(a, &v.to_le_bytes(), false);
        v
    }
    fn u32(&self, a: u32) -> u32 {
        self.before(a, 4);
        let v = self.live.u32(a);
        self.observed(a, &v.to_le_bytes(), false);
        v
    }
    fn read_bytes(&self, a: u32, out: &mut [u8]) {
        self.before(a, out.len() as u32);
        self.live.read_bytes(a, out);
        self.observed(a, out, false);
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.before(a, 1);
        self.live.set_u8(a, v);
        self.observed(a, &[v], true);
    }
    fn set_u16(&mut self, a: u32, v: u16) {
        self.before(a, 2);
        self.live.set_u16(a, v);
        self.observed(a, &v.to_le_bytes(), true);
    }
    fn set_u32(&mut self, a: u32, v: u32) {
        self.before(a, 4);
        self.live.set_u32(a, v);
        self.observed(a, &v.to_le_bytes(), true);
    }
    fn alloc(&mut self, size: u32) -> u32 {
        self.failure.set(Some(0));
        self.live.alloc(size)
    }
}

#[derive(Clone)]
struct PrivateMemory(Rc<RefCell<LaneMemory>>);
impl Mem for PrivateMemory {
    fn probe(&self, a: u32, n: u32, w: bool) -> Result<(), u32> {
        self.0.borrow().probe(a, n, w)
    }
    fn u8(&self, a: u32) -> u8 {
        self.0.borrow().u8(a)
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.0.borrow_mut().set_u8(a, v);
    }
}
struct Replay {
    raw: Arc<BTreeMap<u32, LiftFn>>,
    memory: PrivateMemory,
    stack: Range<u32>,
    lane_count: usize,
    depth: usize,
}
fn current() -> Rc<Replay> {
    REPLAY.with(|v| v.borrow().as_ref().expect("private replay scope").clone())
}
fn resolve(target: u32) -> LiftFn {
    match target {
        SCHEDULE => private_schedule,
        crate::fastmem::MEMCPY => crate::fastmem::memcpy,
        crate::fastmem::MEMSET => crate::fastmem::memset,
        _ => {
            let replay = current();
            if let Some(body) = replay.raw.get(&target) {
                return *body;
            }
            // Import slots referenced by 0107FB57/0107FBAE (DDDA.exe
            // 2017-12-13 Steam). Read the captured binding, never assume
            // a loader token number or invoke the live service registry.
            for (slot, service) in [(0x0139d160, private_enter as LiftFn), (0x0139d164, private_leave)] {
                if replay.memory.probe(slot, 4, false).is_ok()
                    && replay.memory.u32(slot) == target
                    && target != 0
                {
                    return service;
                }
            }
            std::panic::panic_any(Reject::UnsupportedTarget(target))
        }
    }
}

/// Exact uncontended guest-memory contract of threads::{enter,leave}_critical_section.
/// This is runtime service isolation, not a replacement game allocator. Every
/// lock word remains tracked; shared allocator dependencies still refuse merge.
/// Foreign owners, waiter counts, malformed state and unmatched leave refuse.
fn private_section(m: &mut dyn Mem, cpu: &mut Cpu, enter: bool) {
    let address = m.u32(cpu.esp.wrapping_add(4));
    let reject = || std::panic::panic_any(Reject::UnsupportedLock(address));
    // Inverse of seh::initialize_teb's runtime-owned TEB/thread mapping.
    let Some(offset) = cpu.fs_base.checked_sub(crate::seh::TEB_BASE) else {
        reject();
        return;
    };
    if offset & 4095 != 0
        || cpu.fs_base >= crate::session::STACK_TOP - crate::session::STACK_SIZE
        || m.u32(cpu.fs_base + 24) != cpu.fs_base
    {
        reject();
    }
    let me = crate::services::MAIN_THREAD_ID + (offset / 4096) * 4;
    let lock = m.u32(address.wrapping_add(4));
    let recursion = m.u32(address.wrapping_add(8));
    let owner = m.u32(address.wrapping_add(12));
    if lock != recursion.wrapping_sub(1)
        || recursion >= i32::MAX as u32
        || (recursion == 0 && owner != 0)
        || (recursion != 0 && owner != me)
        || (!enter && recursion == 0)
    {
        reject();
    }
    if enter {
        m.set_u32(address.wrapping_add(4), lock.wrapping_add(1));
        m.set_u32(address.wrapping_add(8), recursion + 1);
        m.set_u32(address.wrapping_add(12), me);
    } else {
        m.set_u32(address.wrapping_add(8), recursion - 1);
        m.set_u32(address.wrapping_add(4), lock.wrapping_sub(1));
        if recursion == 1 {
            m.set_u32(address.wrapping_add(12), 0);
        }
    }
    crate::services::ret(m, cpu, 4);
}
fn private_enter(m: &mut dyn Mem, cpu: &mut Cpu) {
    private_section(m, cpu, true);
}
fn private_leave(m: &mut dyn Mem, cpu: &mut Cpu) {
    private_section(m, cpu, false);
}

/// Called only on newly created audit threads. No Runtime is entered and no
/// production Registry is installed: unknown services cannot reach the host.
fn with_replay<T>(replay: Replay, run: impl FnOnce(&mut PrivateMemory) -> T) -> T {
    ddnative_host::install();
    ddnative_base::platform::redirect(&crate::session::REDIRECTED_MODELS);
    let mut memory = replay.memory.clone();
    REPLAY.with(|v| {
        assert!(v.borrow().is_none());
        *v.borrow_mut() = Some(Rc::new(replay));
    });
    let _scope = x86::private_calls::enter(resolve);
    let previous = x86::set_ext_calls(Some(Box::new(|m, c, target| resolve(target)(m, c))));
    let machine = ddnative_base::platform::set_machine(None);
    struct Clear {
        previous: Option<Rc<x86::ExtCall>>,
        machine: Option<Rc<dyn ddnative_base::platform::Machine>>,
    }
    impl Drop for Clear {
        fn drop(&mut self) {
            x86::set_ext_calls(self.previous.take().map(|old| {
                Box::new(move |m: &mut dyn Mem, c: &mut Cpu, t| old(m, c, t)) as Box<x86::ExtCall>
            }));
            ddnative_base::platform::set_machine(self.machine.take());
            REPLAY.with(|v| *v.borrow_mut() = None);
            ddnative_base::x87::park();
        }
    }
    let _clear = Clear { previous, machine };
    run(&mut memory)
}

fn push(memory: &mut dyn Mem, cpu: &mut Cpu, value: u32) {
    cpu.esp = cpu.esp.wrapping_sub(4);
    memory.set_u32(cpu.esp, value);
}

/// Private replacement for DBE560's queue dispatch, using DBD8D0's callback
/// ABI (00DBD9CB/00DBDA4C/00DBDABC). OS event creation, signals and waits are
/// deliberately absent; all jobs finish before the caller resumes.
fn private_schedule(memory: &mut dyn Mem, cpu: &mut Cpu) {
    let replay = current();
    if replay.depth != 0 {
        std::panic::panic_any(Reject::UnsupportedScheduler(0xffff_ffff));
    }
    let scheduler = cpu.ecx;
    let count = memory.u32(scheduler + 0x8c) as usize;
    let mode = memory.u32(cpu.esp + 4);
    if mode == 0 || mode > 3 || count > 8192 {
        std::panic::panic_any(Reject::UnsupportedScheduler(mode));
    }
    if memory.u32(scheduler + 0x1c14c) != 0 {
        std::panic::panic_any(Reject::UnsupportedScheduler(mode));
    }
    if count == 0 {
        memory.set_u32(scheduler + 0x1c148, 0);
        memory.set_u32(scheduler + 0x1c14c, 0);
        cpu.esp = cpu.esp.wrapping_add(8);
        return;
    }
    let participants = memory.u32(scheduler + 0x98);
    let multiple = count > 1 && participants > 1;
    if !multiple {
        memory.set_u32(scheduler + 0x98, 1);
    }
    let nesting = [memory.u8(NESTING), memory.u8(NESTING_OTHER)];
    if multiple {
        memory.set_u8(NESTING, 1);
        memory.set_u8(NESTING_OTHER, 1);
        memory.set_u8(scheduler + 0xb8, 1);
    }
    memory.set_u32(scheduler + 0x90, 0);
    memory.set_u32(scheduler + 0x94, mode);
    let input = Arc::new(replay.memory.0.borrow().frozen());
    let mut incoming = cpu.clone();
    incoming.mxcsr.0 = x86::stmxcsr();
    let scratch = replay.stack.start..incoming.esp;
    let raw = replay.raw.clone();
    let lane_count = if multiple { replay.lane_count } else { 1 };
    let results = lanes::execute(
        input.clone(),
        count,
        lane_count,
        ACCESS_LIMIT,
        |lane, jobs, lane_memory| {
            // Move the lane transaction into a TLS-owned handle, returning it to
            // the executor only after every callback and private resolver ends.
            let owned = std::mem::replace(lane_memory, LaneMemory::new(input.clone(), ACCESS_LIMIT));
            let shared = PrivateMemory(Rc::new(RefCell::new(owned)));
            let mut callback_cpu = incoming.clone();
            callback_cpu.mxcsr.0 = incoming.mxcsr.0;
            x86::ldmxcsr(incoming.mxcsr.0);
            with_replay(
                Replay {
                    raw: raw.clone(),
                    memory: shared.clone(),
                    stack: scratch.clone(),
                    lane_count: 1,
                    depth: 1,
                },
                |m| {
                    let context = m.u32(scheduler + 0x1bdd0 + lane as u32 * 0x20);
                    for job in jobs {
                        let entry = scheduler + 0xc0 + job as u32 * if mode < 2 { 8 } else { 12 };
                        callback_cpu.esp = incoming.esp;
                        callback_cpu.ecx = m.u32(entry);
                        let target = m.u32(entry + 4);
                        callback_cpu.esi = entry;
                        callback_cpu.edi = count as u32;
                        if mode == 1 || mode == 3 {
                            callback_cpu.ebx = context;
                            callback_cpu.ebp = scheduler;
                        } else {
                            callback_cpu.ebx = scheduler;
                        }
                        if mode >= 2 {
                            callback_cpu.edx = m.u32(entry + 8);
                            callback_cpu.eax = target;
                            let argument = callback_cpu.edx;
                            push(m, &mut callback_cpu, argument);
                        }
                        if mode == 1 || mode == 3 {
                            push(m, &mut callback_cpu, context);
                        }
                        if mode == 1 {
                            callback_cpu.edx = target;
                        }
                        push(
                            m,
                            &mut callback_cpu,
                            match mode {
                                1 => 0xdbd9cb,
                                2 => 0xdbda4c,
                                3 => 0xdbdabc,
                                _ => unreachable!(),
                            },
                        );
                        resolve(target)(m, &mut callback_cpu);
                        assert_eq!(callback_cpu.esp, incoming.esp, "private job callback stack ABI");
                    }
                },
            );
            *lane_memory = Rc::try_unwrap(shared.0)
                .ok()
                .expect("private memory handles released")
                .into_inner();
        },
    )
    .unwrap_or_else(|error| std::panic::panic_any(error));
    lanes::merge(&input, &results, &[scratch.clone()]).unwrap_or_else(|error| std::panic::panic_any(error));
    replay.memory.0.borrow_mut().absorb(&results, scratch);
    if multiple {
        memory.set_u8(NESTING, nesting[0]);
        memory.set_u8(NESTING_OTHER, nesting[1]);
        memory.set_u8(scheduler + 0xb8, 0);
    } else {
        memory.set_u32(scheduler + 0x98, participants);
    }
    memory.set_u32(scheduler + 0x90, count as u32);
    for offset in [0x8c, 0x1c148, 0x1c14c] {
        memory.set_u32(scheduler + offset, 0);
    }
    cpu.esp = cpu.esp.wrapping_add(8);
}

fn traverse(memory: &mut dyn Mem, cpu: &mut Cpu) {
    let original = ORIGINAL.get().unwrap()[1];
    if !IN_SHADOW.with(Cell::get) {
        static OUTSIDE: std::sync::Once = std::sync::Once::new();
        OUTSIDE.call_once(|| eprintln!("parallel-shadow traversal outside shadow scope"));
        return original(memory, cpu);
    }
    let index = SAMPLE.fetch_add(1, Relaxed);
    if index == 0 {
        eprintln!("parallel-shadow first scoped traversal");
    }
    let first = std::env::var("DDRUNTIME_PARALLEL_SHADOW_FIRST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let count = std::env::var("DDRUNTIME_PARALLEL_SHADOW_COUNT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1)
        .min(8);
    if index < first || index - first >= count {
        return original(memory, cpu);
    }
    eprintln!("parallel-shadow sample={index} capture begin");
    let incoming = cpu.clone();
    let mxcsr = x86::stmxcsr();
    let context = memory.u32(cpu.esp + 4);
    let mut journal = Journal {
        live: memory,
        input: RefCell::new(Snapshot::default()),
        writes: RefCell::new(BTreeMap::new()),
        failure: Cell::new(None),
    };
    // Source: DB63E0's eight inline contexts and DBE560's scheduler fields.
    journal.seed(incoming.ecx + 0xd40, 8 * LANE_BYTES);
    let scheduler = journal.u32(SCHEDULER_GLOBAL);
    journal.seed(scheduler, 0x1c150);
    // FED200 partitions the secondary root-entry arena at context+54.
    // Both entry arenas have the source capacity at context+4c.
    let preseed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        journal.seed(context, LANE_BYTES);
        let capacity = journal
            .u32(context + 0x4c)
            .checked_mul(8)
            .expect("entry arena size");
        journal.seed(journal.u32(context + 0x44), capacity);
        journal.seed(journal.u32(context + 0x54), capacity);
        journal.seed_primitives();
    }));
    if preseed.is_err() {
        journal.failure.set(Some(context));
    }
    let stack = if incoming.fs_base != 0 {
        journal.u32(incoming.fs_base + 8)..journal.u32(incoming.fs_base + 4)
    } else {
        incoming.esp..incoming.esp
    };
    if stack.start < incoming.esp && incoming.esp < stack.end && stack.end - stack.start <= 8 << 20 {
        journal.seed(stack.start, stack.end - stack.start);
    } else {
        journal.failure.set(Some(incoming.esp));
    }
    original(&mut journal, cpu);
    let outgoing = cpu.clone();
    let result_mxcsr = x86::stmxcsr();
    ddnative_base::x87::park();
    x86::ldmxcsr(x86::MXCSR_DEFAULT);
    let observed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        capture_context(&journal, context, Limits::default())
    }));
    let result = if let Some(address) = journal.failure.get() {
        Err(format!("incomplete/incoherent capture at {address:#x}"))
    } else if let Ok(Ok(observed)) = observed {
        let input = Arc::new(journal.input.into_inner());
        let writes = journal.writes.into_inner();
        if let Some(directory) = std::env::var_os("DDRUNTIME_PARALLEL_SHADOW_DUMP") {
            let save = || -> std::io::Result<()> {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory)?;
                let path = directory.join(format!("shadow-{}-{index}.ddshadow", std::process::id()));
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                let mut file = std::io::BufWriter::new(file);
                capture_file::Fixture {
                    input: input.clone(),
                    incoming: incoming.clone(),
                    outgoing: outgoing.clone(),
                    mxcsr,
                    result_mxcsr,
                    context,
                    scheduler,
                    stack: stack.clone(),
                    writes: writes.clone(),
                }
                .write(&mut file)?;
                std::io::Write::flush(&mut file)?;
                eprintln!("parallel-shadow capture={}", path.display());
                Ok(())
            };
            if let Err(error) = save() {
                eprintln!("parallel-shadow capture failed: {error}");
            }
        }
        compare_replays(
            input,
            incoming,
            outgoing,
            mxcsr,
            result_mxcsr,
            context,
            scheduler,
            stack,
            observed,
            writes,
            RAW.get().unwrap().clone(),
        )
    } else {
        Err("serial stream capture refused".into())
    };
    eprintln!("parallel-shadow sample={index} audit={result:?} publish=serial");
    x86::ldmxcsr(result_mxcsr);
}

/// Offline proof only: no Session, live game, services or graphics device is
/// created. The caller builds this with the head's existing game-code train.
#[cfg(all(feature = "game", feature = "parallel-shadow"))]
pub fn replay_file(path: &std::path::Path) -> Result<usize, String> {
    if !enabled() {
        return Err("set DDRUNTIME_PARALLEL_SHADOW=1 before startup".into());
    }
    let _registry = crate::dispatch::Registry::with_lifted_program();
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let fixture =
        capture_file::Fixture::read(&mut std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    let final_input = fixture.input.with_writes(&fixture.writes);
    let observed = capture_context(&final_input, fixture.context, Limits::default())
        .map_err(|e| format!("recorded stream: {e:?}"))?;
    compare_replays(
        fixture.input,
        fixture.incoming,
        fixture.outgoing,
        fixture.mxcsr,
        fixture.result_mxcsr,
        fixture.context,
        fixture.scheduler,
        fixture.stack,
        observed,
        fixture.writes,
        RAW.get().unwrap().clone(),
    )
}

fn compare_replays(
    input: Arc<Snapshot>,
    incoming: Cpu,
    outgoing: Cpu,
    mxcsr: u32,
    result_mxcsr: u32,
    context: u32,
    scheduler: u32,
    stack: Range<u32>,
    observed: mtcore::shadow_lanes::StreamSnapshot,
    observed_writes: BTreeMap<u32, u8>,
    raw: Arc<BTreeMap<u32, LiftFn>>,
) -> Result<usize, String> {
    let mut results = Vec::new();
    for parallel in [false, true] {
        let (input, raw, mut cpu, stack) = (input.clone(), raw.clone(), incoming.clone(), stack.clone());
        let handle = std::thread::Builder::new()
            .name(format!("shadow-replay-{parallel}"))
            .stack_size(64 << 20)
            .spawn(move || {
                let memory = PrivateMemory(Rc::new(RefCell::new(LaneMemory::new(input, ACCESS_LIMIT))));
                with_replay(
                    Replay {
                        raw,
                        memory: memory.clone(),
                        stack,
                        lane_count: if parallel { 2 } else { 1 },
                        depth: 0,
                    },
                    |m| {
                        if parallel {
                            m.set_u8(NESTING, 0);
                            m.set_u8(NESTING_OTHER, 0);
                            m.set_u32(scheduler + 0x98, 2);
                            m.set_u32(scheduler + 0x8c, 0);
                            m.set_u32(scheduler + 0x1c14c, 0);
                        }
                        x86::ldmxcsr(mxcsr);
                        resolve(TRAVERSE)(m, &mut cpu);
                        let final_mxcsr = x86::stmxcsr();
                        ddnative_base::x87::park();
                        x86::ldmxcsr(x86::MXCSR_DEFAULT);
                        let stream = capture_context(m, context, Limits::default())
                            .map_err(|e| format!("stream: {e:?}"))?;
                        let writes = memory.0.borrow().writes().clone();
                        Ok::<_, String>((cpu.clone(), stream, writes, final_mxcsr))
                    },
                )
            })
            .map_err(|e| format!("start replay: {e}"))?;
        let result = handle.join().map_err(|error| {
            error.downcast_ref::<Reject>().map_or_else(
                || "replay panic".to_string(),
                |e| format!("replay rejected: {e:?}"),
            )
        })??;
        results.push(result);
    }
    let abi = |c: &Cpu| [c.eax, c.ebx, c.esi, c.edi, c.ebp, c.esp];
    if abi(&results[0].0) != abi(&outgoing) {
        return Err("original serial replay ABI differs from live serial".into());
    }
    compare_control_state(&outgoing, result_mxcsr, &results[0].0, results[0].3)
        .map_err(|field| format!("live/original serial replay {field} mismatch"))?;
    if observed.first_difference(&results[0].1).is_some() {
        return Err("original serial replay stream differs from live serial".into());
    }
    let scratch = stack.start..outgoing.esp;
    for address in observed_writes.keys().chain(results[0].2.keys()) {
        if scratch.contains(address) {
            continue;
        }
        let a = observed_writes
            .get(address)
            .copied()
            .unwrap_or_else(|| input.u8(*address));
        let b = results[0]
            .2
            .get(address)
            .copied()
            .unwrap_or_else(|| input.u8(*address));
        if a != b {
            return Err(format!(
                "live/original serial replay memory mismatch at {address:#x}"
            ));
        }
    }
    if let Some(diff) = results[0].1.first_difference(&results[1].1) {
        return Err(format!("serial/parallel stream mismatch: {diff:?}"));
    }
    // Conservative proof gate: no differing memory accepted yet. Private
    // scheduler/context relocation classification needs separate evidence.
    for address in results[0].2.keys().chain(results[1].2.keys()) {
        if scratch.contains(address) {
            continue;
        }
        let a = results[0]
            .2
            .get(address)
            .copied()
            .unwrap_or_else(|| input.u8(*address));
        let b = results[1]
            .2
            .get(address)
            .copied()
            .unwrap_or_else(|| input.u8(*address));
        if a != b {
            return Err(format!(
                "streams matched {} entries; serial/parallel memory mismatch at {address:#x}",
                observed.entries.len()
            ));
        }
    }
    if abi(&results[0].0) != abi(&results[1].0) {
        return Err("serial/parallel ABI mismatch".into());
    }
    compare_control_state(&results[0].0, results[0].3, &results[1].0, results[1].3)
        .map_err(|field| format!("serial/parallel {field} mismatch"))?;
    if observed.entries.is_empty() {
        return Err("empty stream is not shadow acceptance".into());
    }
    Ok(observed.entries.len())
}

/// Conservative state gate. Do not discard x87 status/register contents or
/// control changes merely because the command bytes happened to match.
fn compare_control_state(a: &Cpu, a_mxcsr: u32, b: &Cpu, b_mxcsr: u32) -> Result<(), &'static str> {
    if a_mxcsr != b_mxcsr || a.mxcsr.0 != b.mxcsr.0 {
        return Err("MXCSR");
    }
    if a.st.clone().state != b.st.clone().state {
        return Err("x87 image");
    }
    if a.df != b.df
        || a.fs_base != b.fs_base
        || a.sys_flags != b.sys_flags
        || [a.seg.es, a.seg.cs, a.seg.ss, a.seg.ds, a.seg.fs, a.seg.gs]
            != [b.seg.es, b.seg.cs, b.seg.ss, b.seg.ds, b.seg.fs, b.seg.gs]
    {
        return Err("thread control state");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{GuestMemory, READ, WRITE};

    #[test]
    fn private_sections_match_runtime_recursive_guest_memory_contract() {
        let rt = Arc::new(crate::runtime::Runtime::default());
        crate::runtime::enter(rt);
        let tid = crate::services::MAIN_THREAD_ID + 12;
        crate::runtime::local(|local| local.guest_tid.set(tid));
        crate::threads::set_parallel(true);
        let mut live = GuestMemory::new();
        live.map(0x1000, 4096, READ | WRITE).unwrap();
        live.set_u32(0x1104, u32::MAX);
        live.set_u32(0x1800, 0x12345678);
        live.set_u32(0x1804, 0x1100);
        let teb = crate::seh::initialize_teb(&mut live, tid, 0x1000, 0x2000);
        let mut input = Snapshot::default();
        for page in [0x1000, teb] {
            input.capture_page(&live, page, READ | WRITE);
        }
        let mut private = LaneMemory::new(Arc::new(input), 10000);
        for enter in [true, true, true, false, true, false, false, false] {
            let initial = Cpu {
                esp: 0x1800,
                fs_base: teb,
                eax: 0xabcdef01,
                ..Cpu::default()
            };
            let mut actual = initial.clone();
            let mut expected = initial;
            private_section(&mut private, &mut actual, enter);
            if enter {
                crate::threads::enter_critical_section(&mut live, &mut expected);
            } else {
                crate::threads::leave_critical_section(&mut live, &mut expected);
            }
            for at in 0x1100..0x1118 {
                assert_eq!(private.u8(at), live.u8(at), "lock byte {at:#x}");
            }
            assert_eq!(actual.esp, expected.esp);
            assert_eq!(actual.eax, expected.eax);
        }
        assert_eq!(private.u32(0x1104), u32::MAX);
        assert_eq!(private.u32(0x1108), 0);
        assert_eq!(private.u32(0x110c), 0);
    }

    #[test]
    fn private_sections_reject_foreign_owners_waiters_and_unmatched_leave() {
        let mut live = GuestMemory::new();
        live.map(0x1000, 4096, READ | WRITE).unwrap();
        let tid = crate::services::MAIN_THREAD_ID;
        let teb = crate::seh::initialize_teb(&mut live, tid, 0x1000, 0x2000);
        live.set_u32(0x1804, 0x1100);
        for (lock, recursion, owner, enter) in [
            (0, 1, tid + 4, true),
            (1, 1, tid, true),
            (u32::MAX, 0, 0, false),
            (u32::MAX, 0, tid, true),
        ] {
            live.set_u32(0x1104, lock);
            live.set_u32(0x1108, recursion);
            live.set_u32(0x110c, owner);
            let mut cpu = Cpu {
                esp: 0x1800,
                fs_base: teb,
                ..Cpu::default()
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                private_section(&mut live, &mut cpu, enter)
            }));
            assert_eq!(
                result.unwrap_err().downcast_ref::<Reject>(),
                Some(&Reject::UnsupportedLock(0x1100))
            );
            assert_eq!(
                [live.u32(0x1104), live.u32(0x1108), live.u32(0x110c)],
                [lock, recursion, owner]
            );
            assert_eq!(cpu.esp, 0x1800);
        }
    }

    #[test]
    fn private_imports_use_captured_slots_without_live_registry() {
        let mut live = GuestMemory::new();
        live.map(0x1000, 4096, READ | WRITE).unwrap();
        live.map(0x0139d000, 4096, READ | WRITE).unwrap();
        let teb = crate::seh::initialize_teb(&mut live, crate::services::MAIN_THREAD_ID, 0x1000, 0x2000);
        live.set_u32(0x0139d160, 0x7ff01230);
        live.set_u32(0x0139d164, 0x7ff04560);
        live.set_u32(0x1104, u32::MAX);
        live.set_u32(0x1804, 0x1100);
        let mut input = Snapshot::default();
        for page in [0x1000, 0x0139d000, teb] {
            input.capture_page(&live, page, READ | WRITE);
        }
        let memory = PrivateMemory(Rc::new(RefCell::new(LaneMemory::new(Arc::new(input), 10000))));
        let replay = Replay {
            raw: Arc::new(BTreeMap::new()),
            memory: memory.clone(),
            stack: 0x1000..0x2000,
            lane_count: 1,
            depth: 0,
        };
        with_replay(replay, |m| {
            for (target, recursion) in [(0x7ff01230, 1), (0x7ff01230, 2), (0x7ff04560, 1), (0x7ff04560, 0)] {
                let mut cpu = Cpu {
                    esp: 0x1800,
                    fs_base: teb,
                    ..Cpu::default()
                };
                x86::call_ext(m, &mut cpu, target);
                assert_eq!(cpu.esp, 0x1808);
                assert_eq!(m.u32(0x1108), recursion);
                assert_eq!(live.u32(0x1108), 0);
            }
        });
        assert_eq!(memory.u32(0x1104), u32::MAX);
        assert_eq!(memory.u32(0x110c), 0);
    }

    #[test]
    fn private_lock_model_does_not_hide_shared_allocator_dependencies() {
        let mut live = GuestMemory::new();
        live.map(0x1000, 4096, READ | WRITE).unwrap();
        let teb = crate::seh::initialize_teb(&mut live, crate::services::MAIN_THREAD_ID, 0x1000, 0x2000);
        live.set_u32(0x1104, u32::MAX);
        live.set_u32(0x1300, 0x2000);
        live.set_u32(0x1804, 0x1100);
        let mut input = Snapshot::default();
        for page in [0x1000, teb] {
            input.capture_page(&live, page, READ | WRITE);
        }
        let input = Arc::new(input);
        let results = lanes::execute(input.clone(), 2, 2, 10000, |_, _, m| {
            let mut cpu = Cpu {
                esp: 0x1800,
                fs_base: teb,
                ..Cpu::default()
            };
            private_enter(m, &mut cpu);
            let allocated = m.u32(0x1300);
            m.set_u32(0x1300, allocated + 16);
            cpu.esp = 0x1800;
            private_leave(m, &mut cpu);
            allocated
        })
        .unwrap();
        assert!(matches!(
            lanes::merge(&input, &results, &[0x1800..0x1900]),
            Err(Reject::ReadDependency {
                lane: 1,
                address: 0x1300,
                ..
            })
        ));
        assert_eq!(live.u32(0x1300), 0x2000);
        assert_eq!(live.u32(0x1104), u32::MAX);
    }

    fn synthetic_job(m: &mut dyn Mem, cpu: &mut Cpu) {
        let job = (cpu.ecx - 0x3000) / 0x10;
        let entry = 0x2000 + job * 8;
        let record = 0x2100 + job * 16;
        let second_lane = m.u32(cpu.esp + 4) == 0x7100;
        let mutation = m.u32(0x1010);
        m.set_u32(entry, job);
        m.set_u32(entry + 4, record);
        m.set_u32(record, 7 << 16);
        m.set_u32(record + 4, job + 100);
        m.set_u32(record + 8, if second_lane && mutation == 1 { 9 } else { 1 });
        m.set_u8(0x1400 + job, if second_lane && mutation == 2 { 9 } else { 1 });
        cpu.esp += 8;
    }

    fn synthetic_traverse(m: &mut dyn Mem, cpu: &mut Cpu) {
        let parallel = current().lane_count > 1;
        cpu.ecx = 0x40000;
        m.set_u32(0x4008c, 4);
        push(m, cpu, 1);
        push(m, cpu, 0x1234);
        private_schedule(m, cpu);
        m.set_u32(0x1048, 4);
        // Synthetic fixture's own postcondition, not a game-state exclusion.
        m.set_u8(NESTING, 1);
        m.set_u8(NESTING_OTHER, 1);
        if parallel {
            match m.u32(0x1010) {
                3 => x86::ldmxcsr(0x3f80),
                4 => resolve(0x12345678)(m, cpu),
                _ => {}
            }
        }
        cpu.esp += 8;
    }

    /// Exercises capture serialization and the complete replay gate without
    /// linking shards. The expected stream/state is constructed independently
    /// from these synthetic callbacks; this is not original-game evidence.
    fn synthetic_fixture(mutation: u32) -> capture_file::Fixture {
        ddnative_host::install();
        let mut live = GuestMemory::new();
        let ranges = [
            (0x1000, 0x3000),
            (0x40000, 0x1d000),
            (0x80000, 0x2000),
            (0x18d0000, 0x2000),
        ];
        for (base, len) in ranges {
            live.map(base, len, READ | WRITE).unwrap();
        }
        live.set_u32(0x1010, mutation);
        live.set_u32(0x1044, 0x2000);
        live.set_u32(SCHEDULER_GLOBAL, 0x40000);
        live.set_u8(NESTING, 1);
        live.set_u8(NESTING_OTHER, 1);
        live.set_u32(0x4008c, 4);
        live.set_u32(0x40098, 2);
        live.set_u32(0x5bdd0, 0x7000);
        live.set_u32(0x5bdf0, 0x7100);
        for job in 0..4 {
            live.set_u32(0x400c0 + job * 8, 0x3000 + job * 16);
            live.set_u32(0x400c4 + job * 8, 0x400010);
        }
        let mut input = Snapshot::default();
        for (base, len) in ranges {
            for page in (base..base + len).step_by(4096) {
                input.capture_page(&live, page, READ | WRITE);
            }
        }
        let input = Arc::new(input);
        let mut expected = LaneMemory::new(input.clone(), ACCESS_LIMIT);
        for job in 0..4 {
            for (address, value) in [
                (0x2000 + job * 8, job),
                (0x2004 + job * 8, 0x2100 + job * 16),
                (0x2100 + job * 16, 7 << 16),
                (0x2104 + job * 16, job + 100),
                (0x2108 + job * 16, 1),
            ] {
                expected.set_u32(address, value);
            }
            expected.set_u8(0x1400 + job, 1);
        }
        for (address, value) in [(0x1048, 4), (0x4008c, 0), (0x40090, 4), (0x40094, 1)] {
            expected.set_u32(address, value);
        }
        let incoming = Cpu {
            esp: 0x81000,
            ..Cpu::default()
        };
        let outgoing = Cpu {
            esp: 0x81008,
            ..incoming.clone()
        };
        capture_file::Fixture {
            input,
            incoming,
            outgoing,
            mxcsr: 0x1f80,
            result_mxcsr: 0x1f80,
            context: 0x1000,
            scheduler: 0x40000,
            stack: 0x80000..0x82000,
            writes: expected.writes().clone(),
        }
    }

    #[test]
    fn serialized_synthetic_capture_checks_complete_replay_and_refusals() {
        for (mutation, error) in [
            (0, None),
            (1, Some("stream mismatch")),
            (2, Some("memory mismatch")),
            (3, Some("MXCSR mismatch")),
            (4, Some("UnsupportedTarget")),
        ] {
            let fixture = synthetic_fixture(mutation);
            let mut bytes = Vec::new();
            fixture.write(&mut bytes).unwrap();
            let fixture = capture_file::Fixture::read(&mut &bytes[..]).unwrap();
            let observed = capture_context(
                &fixture.input.with_writes(&fixture.writes),
                fixture.context,
                Limits::default(),
            )
            .unwrap();
            let result = compare_replays(
                fixture.input,
                fixture.incoming,
                fixture.outgoing,
                fixture.mxcsr,
                fixture.result_mxcsr,
                fixture.context,
                fixture.scheduler,
                fixture.stack,
                observed,
                fixture.writes,
                Arc::new(BTreeMap::from([
                    (TRAVERSE, synthetic_traverse as LiftFn),
                    (0x400010, synthetic_job as LiftFn),
                ])),
            );
            match error {
                None => assert_eq!(result.unwrap(), 4),
                Some(expected) => assert!(
                    result.as_ref().unwrap_err().contains(expected),
                    "{mutation}: {result:?}"
                ),
            }
        }
    }

    #[test]
    fn proof_rejects_fp_and_thread_control_changes() {
        let cpu = Cpu::default();
        assert_eq!(compare_control_state(&cpu, 0x1f80, &cpu.clone(), 0x1f80), Ok(()));
        assert_eq!(compare_control_state(&cpu, 0x1f80, &cpu, 0x3f80), Err("MXCSR"));
        let mut changed = cpu.clone();
        changed.mxcsr.0 ^= 0x2000;
        assert_eq!(
            compare_control_state(&cpu, 0x1f80, &changed, 0x1f80),
            Err("MXCSR")
        );
        for byte in [0, 4, 8, 28, 107] {
            let mut changed = cpu.clone();
            changed.st.state[byte] ^= 1;
            assert_eq!(
                compare_control_state(&cpu, 0x1f80, &changed, 0x1f80),
                Err("x87 image")
            );
        }
        for field in 0..4 {
            let mut changed = cpu.clone();
            match field {
                0 => changed.df = !changed.df,
                1 => changed.fs_base ^= 4096,
                2 => changed.sys_flags ^= 1,
                _ => changed.seg.fs ^= 1,
            }
            assert_eq!(
                compare_control_state(&cpu, 0x1f80, &changed, 0x1f80),
                Err("thread control state")
            );
        }
    }

    #[test]
    fn journal_is_observational_and_rejects_concurrent_input_changes() {
        let mut memory = GuestMemory::new();
        memory.map(0x1000, 4096, READ | WRITE).unwrap();
        memory.set_u32(0x1000, 7);
        let mut concurrent = memory.share();
        let mut journal = Journal {
            live: &mut memory,
            input: RefCell::new(Snapshot::default()),
            writes: RefCell::new(BTreeMap::new()),
            failure: Cell::new(None),
        };
        assert_eq!(journal.u32(0x1000), 7);
        journal.set_u32(0x1000, 9);
        assert_eq!(journal.u32(0x1000), 9);
        assert_eq!(journal.input.borrow().u32(0x1000), 7);
        assert_eq!(concurrent.u32(0x1000), 9);
        concurrent.set_u32(0x1000, 11);
        assert_eq!(journal.u32(0x1000), 11);
        assert_eq!(journal.failure.get(), Some(0x1000));
    }

    #[test]
    fn private_queue_modes_use_lane_context_and_original_argument_order() {
        fn callback(memory: &mut dyn Mem, cpu: &mut Cpu) {
            let mode = memory.u32(0x1000);
            let job = (cpu.ecx - 0x3000) / 0x10;
            if mode == 1 || mode == 3 {
                assert_eq!(memory.u32(cpu.esp + 4), if job < 2 { 0x7000 } else { 0x7100 });
            }
            if mode >= 2 {
                assert_eq!(memory.u32(cpu.esp + if mode == 2 { 4 } else { 8 }), 100 + job);
            }
            memory.set_u32(0x1100 + job * 4, job + 1);
            cpu.esp += if mode == 3 { 12 } else { 8 };
        }
        for mode in 1..=3 {
            let mut memory = GuestMemory::new();
            for (base, length) in [
                (0x1000, 0x3000),
                (0x40000, 0x1d000),
                (0x80000, 0x2000),
                (0x18d0000, 0x2000),
            ] {
                memory.map(base, length, READ | WRITE).unwrap();
            }
            memory.set_u32(0x1000, mode);
            memory.set_u32(SCHEDULER_GLOBAL, 0x40000);
            memory.set_u32(0x4008c, 4);
            memory.set_u32(0x40098, 2);
            memory.set_u32(0x5bdd0, 0x7000);
            memory.set_u32(0x5bdf0, 0x7100);
            memory.set_u32(0x81004, mode);
            for job in 0..4 {
                let entry = 0x400c0 + job * if mode == 1 { 8 } else { 12 };
                memory.set_u32(entry, 0x3000 + job * 0x10);
                memory.set_u32(entry + 4, 0x400010);
                if mode >= 2 {
                    memory.set_u32(entry + 8, 100 + job);
                }
            }
            let mut input = Snapshot::default();
            for (base, length) in [
                (0x1000, 0x3000),
                (0x40000, 0x1d000),
                (0x80000, 0x2000),
                (0x18d0000, 0x2000),
            ] {
                for page in (base..base + length).step_by(4096) {
                    input.capture_page(&memory, page, READ | WRITE);
                }
            }
            let private = PrivateMemory(Rc::new(RefCell::new(LaneMemory::new(
                Arc::new(input),
                ACCESS_LIMIT,
            ))));
            let replay = Replay {
                raw: Arc::new(BTreeMap::from([(0x400010, callback as LiftFn)])),
                memory: private.clone(),
                stack: 0x80000..0x82000,
                lane_count: 2,
                depth: 0,
            };
            let mut cpu = Cpu {
                ecx: 0x40000,
                esp: 0x81000,
                ebx: 11,
                esi: 12,
                edi: 13,
                ebp: 14,
                ..Cpu::default()
            };
            with_replay(replay, |m| private_schedule(m, &mut cpu));
            assert_eq!(
                [cpu.ebx, cpu.esi, cpu.edi, cpu.ebp, cpu.esp],
                [11, 12, 13, 14, 0x81008]
            );
            for job in 0..4 {
                assert_eq!(private.u32(0x1100 + job * 4), job + 1);
                assert_eq!(memory.u32(0x1100 + job * 4), 0);
            }
            assert_eq!(private.u32(0x4008c), 0);
            assert_eq!(memory.u32(0x4008c), 4);
        }
    }

    #[test]
    fn unknown_external_call_is_rejected_before_host_dispatch() {
        let memory = PrivateMemory(Rc::new(RefCell::new(LaneMemory::new(
            Arc::new(Snapshot::default()),
            10,
        ))));
        let replay = Replay {
            raw: Arc::new(BTreeMap::new()),
            memory,
            stack: 0..0,
            lane_count: 1,
            depth: 0,
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_replay(replay, |m| {
                x86::call_ext(m, &mut Cpu::default(), 0x1234);
            })
        }));
        assert_eq!(
            result.err().unwrap().downcast_ref::<Reject>(),
            Some(&Reject::UnsupportedTarget(0x1234))
        );
        assert!(REPLAY.with(|v| v.borrow().is_none()));
        assert!(!x86::private_calls::dispatch(
            &mut GuestMemory::new(),
            &mut Cpu::default(),
            0x1234
        ));
    }
}

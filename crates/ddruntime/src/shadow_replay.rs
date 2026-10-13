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
        Arc, Mutex, OnceLock,
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
static CAPTURE_START: OnceLock<std::time::Instant> = OnceLock::new();
#[path = "shadow_replay_file.rs"]
mod capture_file;
#[path = "shadow_replay_sequences.rs"]
mod sequences;
thread_local! {
    static IN_SHADOW: Cell<bool> = const { Cell::new(false) };
    static REPLAY: RefCell<Option<Rc<Replay>>> = const { RefCell::new(None) };
    static UNRESOLVED: Cell<u32> = const { Cell::new(0) };
    static COUNTER_CAPACITY_READ: Cell<bool> = const { Cell::new(false) };
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
    CAPTURE_START.get_or_init(std::time::Instant::now);
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
        "parallel-shadow installed capture=v4 first={} count={} after_ms={} dump={:?}",
        std::env::var("DDRUNTIME_PARALLEL_SHADOW_FIRST").unwrap_or_else(|_| "0".into()),
        std::env::var("DDRUNTIME_PARALLEL_SHADOW_COUNT").unwrap_or_else(|_| "1".into()),
        std::env::var("DDRUNTIME_PARALLEL_SHADOW_AFTER_MS").unwrap_or_else(|_| "0".into()),
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
    failure: Cell<Option<CaptureFailure>>,
    stage: Cell<&'static str>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CaptureFailure {
    stage: &'static str,
    reason: &'static str,
    address: u32,
}
impl Journal<'_> {
    fn fail(&self, address: u32, reason: &'static str) {
        if self.failure.get().is_none() {
            self.failure.set(Some(CaptureFailure {
                stage: self.stage.get(),
                reason,
                address,
            }));
        }
    }
    fn page(&self, address: u32) {
        if self.input.borrow().has_page(address) {
            return;
        }
        let base = address & !4095;
        if self.input.borrow().page_count() == MAX_PAGES {
            self.fail(address, "page limit");
            return;
        }
        if self.live.probe(base, 4096, false).is_err() {
            self.fail(address, "unreadable page");
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
            self.fail(start, "range overflow");
            return;
        };
        let mut page = start & !4095;
        while page < end {
            self.page(page);
            if !self.input.borrow().has_page(page) {
                eprintln!(
                    "parallel-shadow seed refusal stage={} start={start:#x} length={length:#x}",
                    self.stage.get()
                );
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
        let mut writes = self.writes.borrow_mut();
        for (offset, &value) in bytes.iter().enumerate() {
            let at = address.wrapping_add(offset as u32);
            if !self.input.borrow().has_page(at) {
                self.fail(at, "observed byte lacks initial captured page");
                continue;
            }
            if write {
                if writes.len() == lanes::MAX_TRACKED_BYTES && !writes.contains_key(&at) {
                    self.fail(at, "tracked write limit");
                    return;
                }
                writes.insert(at, value);
            } else if writes
                .get(&at)
                .copied()
                .unwrap_or_else(|| self.input.borrow().u8(at))
                != value
            {
                self.fail(at, "read differs from frozen input plus journal writes");
            }
        }
    }
    fn seed_primitives(&self) {
        // sPrimitive ctor E60DE0; manager ctors 11927F0/11925E0/11929A0.
        let primitive = self.u32(0x018d1848);
        self.stage.set("primitive singleton");
        self.seed(primitive, 0x1d0);
        let pools = self.u32(primitive + 0xd0);
        if pools > 8 {
            self.fail(primitive + 0xd0, "primitive pool count exceeds eight");
            return;
        }
        for lane in 0..pools {
            self.stage.set("primitive pool/list/buffer headers");
            let pool = self.u32(primitive + 0x30 + lane * 4);
            self.seed(pool, 0x280);
            self.seed(self.u32(pool + 4), 0x84);
            self.seed(self.u32(pool + 8), 0x18);
        }
        let bytes = self.u32(primitive + 0xd8);
        self.stage.set("primitive byte manager");
        self.seed(bytes, 0x14);
        self.stage.set("primitive byte arena");
        self.seed(self.u32(bytes + 4), self.u32(bytes + 0xc));
        let tags = self.u32(primitive + 0xdc);
        self.stage.set("primitive tag manager");
        self.seed(tags, 0x14);
        let count = self.u32(tags + 0x10);
        if let (Some(descriptors), Some(entries)) = (count.checked_mul(0x28), count.checked_mul(8)) {
            self.stage.set("primitive descriptor arena");
            self.seed(self.u32(tags + 4), descriptors);
            self.stage.set("primitive tag entry arena");
            self.seed(self.u32(tags + 8), entries);
        } else {
            self.fail(tags, "primitive tag size overflow");
        }
        let textures = self.u32(primitive + 0xd4);
        self.stage.set("primitive texture manager");
        self.seed(textures, 0x10);
        if let Some(bytes) = self.u32(textures + 8).checked_mul(0x14) {
            self.stage.set("primitive texture arena");
            self.seed(self.u32(textures + 4), bytes);
        } else {
            self.fail(textures, "primitive texture size overflow");
        }
    }
    fn seed_render_commands(&self, render: u32) {
        // DA7D70 (DDDA.exe 2017-12-13 Steam): command allocation returns
        // ring_base[ring_index] + old cursor when old+size < capacity.
        // FED890 requests fresh pages for FED200's empty child cursors even
        // when the serial parent fits inside its already allocated page.
        self.stage.set("DA7D70 command arena");
        let ring = self.u32(render.wrapping_add(0x33db8));
        let slot = render.wrapping_add(0x33dc0).wrapping_add(ring.wrapping_mul(4));
        let base = self.u32(slot);
        let capacity = self.u32(render.wrapping_add(0x33dcc));
        self.seed(base, capacity);
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
        let address = self.live.alloc(size);
        self.fail(address, "host allocation during serial traversal");
        eprintln!("parallel-shadow capture allocation address={address:#x} size={size:#x}");
        address
    }
}

#[derive(Clone)]
struct PrivateMemory(Rc<RefCell<LaneMemory>>);
fn reject_unsynchronized_counter_access(address: u32) {
    let counter = REPLAY.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|replay| replay.command_counter.as_ref().map(|counter| counter.address))
    });
    if counter.is_some_and(|base| address.wrapping_sub(base) < 4) {
        std::panic::panic_any(Reject::AtomicConflict(counter.unwrap()));
    }
}
impl Mem for PrivateMemory {
    fn probe(&self, a: u32, n: u32, w: bool) -> Result<(), u32> {
        self.0.borrow().probe(a, n, w)
    }
    fn u8(&self, a: u32) -> u8 {
        reject_unsynchronized_counter_access(a);
        self.0.borrow().u8(a)
    }
    fn u32(&self, address: u32) -> u32 {
        if COUNTER_CAPACITY_READ.with(Cell::get) {
            let counter = current().command_counter.clone();
            if let Some(counter) = counter.filter(|counter| counter.address == address) {
                return *counter.value.lock().unwrap();
            }
        }
        u32::from_le_bytes(std::array::from_fn(|offset| {
            self.u8(address.wrapping_add(offset as u32))
        }))
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        reject_unsynchronized_counter_access(a);
        self.0.borrow_mut().set_u8(a, v);
    }
}
/// A completed stream must not consume the temporary secondary sort arena.
/// FED3B0 copies roots and nested arrays to the parent before returning.
struct PublishedView<'a> {
    memory: &'a dyn Mem,
    private: Vec<Range<u32>>,
}
impl Mem for PublishedView<'_> {
    fn probe(&self, a: u32, n: u32, w: bool) -> Result<(), u32> {
        for range in &self.private {
            for offset in 0..n {
                let at = a.wrapping_add(offset);
                if range.contains(&at) {
                    return Err(at);
                }
            }
        }
        self.memory.probe(a, n, w)
    }
    fn u8(&self, a: u32) -> u8 {
        if self.private.iter().any(|range| range.contains(&a)) {
            std::panic::panic_any(Reject::PrivateOutputEscape(a));
        }
        self.memory.u8(a)
    }
    fn set_u8(&mut self, _: u32, _: u8) {
        panic!("read-only published view");
    }
}
struct Replay {
    raw: Arc<BTreeMap<u32, LiftFn>>,
    memory: PrivateMemory,
    stack: Range<u32>,
    lane_count: usize,
    depth: usize,
    command_counter: Option<Arc<CommandCounter>>,
}

struct CommandCounter {
    address: u32,
    value: Mutex<u32>,
}

fn private_exchange_add(m: &mut dyn Mem, cpu: &mut Cpu) {
    let replay = current();
    let address = crate::services::arg(m, cpu, 0);
    if let Some(counter) = &replay.command_counter {
        if address == counter.address {
            // DA7D80 (DDDA.exe 2017-12-13 Steam) atomically claims bytes
            // from sRender+33DC8. This mutex is confined to one replay batch;
            // no live memory/OS service participates. Preserve fetch-add wrap
            // and return-old semantics, including failed capacity requests.
            let delta = crate::services::arg(m, cpu, 1);
            let mut value = counter.value.lock().unwrap();
            cpu.eax = *value;
            *value = value.wrapping_add(delta);
            crate::services::ret(m, cpu, 8);
            return;
        }
    }
    crate::win32::interlocked_exchange_add(m, cpu);
}
fn current() -> Rc<Replay> {
    REPLAY.with(|v| v.borrow().as_ref().expect("private replay scope").clone())
}
fn private_command_capacity(m: &mut dyn Mem, cpu: &mut Cpu) {
    // Original DA7DB0 (DDDA.exe 2017-12-13 Steam), called by DB7620,
    // checks capacity against cursor+headroom. Run the original body so all
    // flags/registers remain exact; only its aligned counter load is shared.
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            COUNTER_CAPACITY_READ.with(|permit| permit.set(self.0));
        }
    }
    let _restore = Restore(COUNTER_CAPACITY_READ.with(|permit| permit.replace(true)));
    let original = *current()
        .raw
        .get(&0x00da7db0)
        .expect("original command capacity helper");
    original(m, cpu);
}
fn resolve(target: u32) -> LiftFn {
    match target {
        SCHEDULE => private_schedule,
        0x00da7db0 => private_command_capacity,
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
            // DA7D80 calls InterlockedExchangeAdd through this captured IAT slot.
            // PrivateMemory has no page table: the existing service uses tracked
            // Mem reads/writes, never a raw host atomic or live service state.
            for (slot, service) in [
                (0x0139d160, private_enter as LiftFn),
                (0x0139d164, private_leave),
                (0x0139d0a8, private_exchange_add),
            ] {
                if replay.memory.probe(slot, 4, false).is_ok()
                    && replay.memory.u32(slot) == target
                    && target != 0
                {
                    return service;
                }
            }
            UNRESOLVED.with(|slot| slot.set(target));
            unsupported_call
        }
    }
}

fn unsupported_call(m: &mut dyn Mem, cpu: &mut Cpu) {
    let target = UNRESOLVED.with(Cell::get);
    let caller = if m.probe(cpu.esp, 4, false).is_ok() {
        m.u32(cpu.esp)
    } else {
        0
    };
    eprintln!(
        "parallel-shadow unsupported target={target:#x} return={caller:#x} ecx={:#x} esp={:#x}",
        cpu.ecx, cpu.esp
    );
    std::panic::panic_any(Reject::UnsupportedTarget(target));
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
    eprintln!("parallel-shadow private schedule mode={mode} jobs={count}");
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
    let command_counter = if memory.probe(0x018d08f4, 4, false).is_ok() && memory.u32(0x018d08f4) != 0 {
        let address = memory.u32(0x018d08f4).wrapping_add(0x33dc8);
        Some(Arc::new(CommandCounter {
            address,
            value: Mutex::new(memory.u32(address)),
        }))
    } else {
        None
    };
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
                    command_counter: command_counter.clone(),
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
    if let Some(counter) = &command_counter {
        let end = counter
            .address
            .checked_add(4)
            .unwrap_or_else(|| std::panic::panic_any(Reject::AtomicConflict(counter.address)));
        if results.iter().any(|lane| lane.touches(counter.address..end)) {
            std::panic::panic_any(Reject::AtomicConflict(counter.address));
        }
    }
    lanes::merge(&input, &results, &[scratch.clone()]).unwrap_or_else(|error| std::panic::panic_any(error));
    replay.memory.0.borrow_mut().absorb(&results, scratch);
    if let Some(counter) = &command_counter {
        memory.set_u32(counter.address, *counter.value.lock().unwrap());
    }
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
    let after_ms = std::env::var("DDRUNTIME_PARALLEL_SHADOW_AFTER_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    if CAPTURE_START
        .get()
        .is_some_and(|start| start.elapsed().as_millis() < u128::from(after_ms))
    {
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
        stage: Cell::new("inline recording lanes"),
    };
    // Source: DB63E0's eight inline contexts and DBE560's scheduler fields.
    journal.seed(incoming.ecx, 0xd40 + 8 * LANE_BYTES);
    journal.stage.set("scheduler");
    let scheduler = journal.u32(SCHEDULER_GLOBAL);
    journal.seed(scheduler, 0x1c150);
    // FED200 partitions the secondary root-entry arena at context+54.
    // Both entry arenas have the source capacity at context+4c.
    let preseed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        journal.stage.set("root recording context");
        journal.seed(context, LANE_BYTES);
        let capacity = journal
            .u32(context + 0x4c)
            .checked_mul(8)
            .expect("entry arena size");
        journal.stage.set("primary entry arena");
        journal.seed(journal.u32(context + 0x44), capacity);
        journal.stage.set("secondary entry arena");
        // FED200 deliberately sets a child lane's +54 to null. It is not
        // serial input; a nested parallel replay supplies private sort storage.
        let secondary = journal.u32(context + 0x54);
        if secondary != 0 {
            journal.seed(secondary, capacity);
        }
        // Parallel FED200 alone reads these frame-ring/shader headers. The
        // values stay install/runtime data, never inferred from other lanes.
        journal.stage.set("FED200 renderer metadata");
        let render = journal.u32(0x018d08f4);
        journal.seed(render.wrapping_add(0x33dbc), 4);
        journal.seed(render.wrapping_add(0x433dec), 0x20);
        journal.seed_render_commands(render);
        journal.stage.set("FED200 shader metadata");
        let shader = journal.u32(0x018d12cc);
        journal.seed(shader.wrapping_add(0x24), 0x1c);
        journal.seed_primitives();
    }));
    if preseed.is_err() {
        journal.fail(context, "preseed panic");
    }
    journal.stage.set("guest stack");
    let stack = if incoming.fs_base != 0 {
        journal.u32(incoming.fs_base + 8)..journal.u32(incoming.fs_base + 4)
    } else {
        incoming.esp..incoming.esp
    };
    if stack.start < incoming.esp && incoming.esp < stack.end && stack.end - stack.start <= 8 << 20 {
        journal.seed(stack.start, stack.end - stack.start);
    } else {
        journal.fail(incoming.esp, "invalid stack bounds");
    }
    journal.stage.set("serial traversal");
    original(&mut journal, cpu);
    let outgoing = cpu.clone();
    let result_mxcsr = x86::stmxcsr();
    ddnative_base::x87::park();
    x86::ldmxcsr(x86::MXCSR_DEFAULT);
    journal.stage.set("serial stream capture");
    let mut shader_handles = std::collections::BTreeSet::new();
    let observed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        mtcore::shadow_lanes::capture_stream(
            &journal,
            journal.u32(context + 0x44),
            journal.u32(context + 0x48),
            Limits::default(),
            &mut |m, binding| {
                // Original consumer DAA8B0, also decoded by mtdraw::decode_shader.
                let metadata = m.u32(binding + 8);
                for offset in [0x10, 0x14] {
                    let handle = m.u32(metadata + offset);
                    if handle != 0 {
                        shader_handles.insert(handle);
                    }
                }
                mtcore::shadow_lanes::capture_shader(m, binding, Limits::default().bytes)
            },
        )
    }));
    // Clone immutable host bytecode under the D3D object lock. No host service
    // executes during offline replay; absent objects remain an explicit refusal.
    let shaders = crate::d3d9dev::objects(|objects| {
        shader_handles
            .iter()
            .filter_map(
                |handle| match objects.live.get(handle).map(|object| &object.kind) {
                    Some(crate::d3d9dev::Kind::Shader { tokens, .. }) => Some((*handle, tokens.to_vec())),
                    _ => None,
                },
            )
            .collect::<BTreeMap<_, _>>()
    });
    let rejection = journal
        .failure
        .get()
        .map(|failure| format!("incomplete/incoherent capture: {failure:#x?}"))
        .or_else(|| {
            if matches!(observed, Ok(Ok(_))) {
                None
            } else {
                Some("serial stream capture refused".into())
            }
        })
        .or_else(|| (shaders.len() != shader_handles.len()).then(|| "captured shader object missing".into()));
    let fixture = capture_file::Fixture {
        input: Arc::new(journal.input.into_inner()),
        incoming,
        outgoing,
        mxcsr,
        result_mxcsr,
        context,
        scheduler,
        stack,
        writes: journal.writes.into_inner(),
        rejection,
        shaders,
    };
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
            fixture.write(&mut file)?;
            std::io::Write::flush(&mut file)?;
            eprintln!(
                "parallel-shadow capture={} rejected={}",
                path.display(),
                fixture.rejection.is_some()
            );
            Ok(())
        };
        if let Err(error) = save() {
            eprintln!("parallel-shadow capture failed: {error}");
        }
    }
    let result = if let Some(reason) = &fixture.rejection {
        Err(reason.clone())
    } else {
        replay_fixture(fixture, RAW.get().unwrap().clone())
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
    replay_fixture(fixture, RAW.get().unwrap().clone())
}

fn replay_fixture(fixture: capture_file::Fixture, raw: Arc<BTreeMap<u32, LiftFn>>) -> Result<usize, String> {
    let rejection = fixture.rejection.clone();
    eprintln!(
        "parallel-shadow captured shader objects={}",
        fixture.shaders.len()
    );
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
            raw,
            &fixture.shaders,
        )
    }))
    .unwrap_or_else(|payload| {
        Err(payload.downcast_ref::<Reject>().map_or_else(
            || "recorded input panic".into(),
            |reason| format!("recorded input: {reason:#x?}"),
        ))
    });
    match rejection {
        Some(reason) => Err(format!(
            "REJECTED capture ({reason}); diagnostic replay only: {result:?}"
        )),
        None => result,
    }
}

type ReplayResult = (
    Cpu,
    mtcore::shadow_lanes::StreamSnapshot,
    BTreeMap<u32, u8>,
    u32,
    Vec<Range<u32>>,
);

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
    shaders: &BTreeMap<u32, Vec<u32>>,
) -> Result<usize, String> {
    if input.u32(SCHEDULER_GLOBAL) != scheduler {
        return Err("capture scheduler pointer disagrees with recorded memory".into());
    }
    if observed.entries.len() <= input.u32(context + 0x48) as usize {
        return Err("traversal appended no commands; existing stream is not shadow acceptance".into());
    }
    let abi = |c: &Cpu| [c.eax, c.ebx, c.esi, c.edi, c.ebp, c.esp];
    let scratch = stack.start..outgoing.esp;
    let verify_serial = |result: &ReplayResult| -> Result<(), String> {
        if abi(&result.0) != abi(&outgoing) {
            return Err("original serial replay ABI differs from live serial".into());
        }
        compare_control_state(&outgoing, result_mxcsr, &result.0, result.3)
            .map_err(|field| format!("live/original serial replay {field} mismatch"))?;
        if observed.first_difference(&result.1).is_some() {
            return Err("original serial replay stream differs from live serial".into());
        }
        for address in observed_writes.keys().chain(result.2.keys()) {
            if scratch.contains(address) {
                continue;
            }
            let a = observed_writes
                .get(address)
                .copied()
                .unwrap_or_else(|| input.u8(*address));
            let b = result
                .2
                .get(address)
                .copied()
                .unwrap_or_else(|| input.u8(*address));
            if a != b {
                return Err(format!(
                    "live/original serial replay memory mismatch at {address:#x}: live={a:#04x} replay={b:#04x}"
                ));
            }
        }
        Ok(())
    };
    let mut results = Vec::new();
    for parallel in [false, true] {
        let (input, raw, mut cpu, stack) = (input.clone(), raw.clone(), incoming.clone(), stack.clone());
        let handle = std::thread::Builder::new()
            .name(format!("shadow-replay-{parallel}"))
            .stack_size(64 << 20)
            .spawn(move || {
                let mut private_ranges = Vec::new();
                let input = if parallel {
                    // DB63E0 reads the manager's category flags/masks and uses
                    // its eight inline contexts. A nested caller can itself be
                    // one of those contexts; FED200 must not overwrite it.
                    // Retarget only this method's manager, preserving the root
                    // context argument and every original object/list pointer.
                    let (copy, range) = input
                        .private_copy(cpu.ecx, 0xd40 + 8 * LANE_BYTES)
                        .map_err(|error| format!("private recording manager: {error:?}"))?;
                    cpu.ecx = range.start;
                    private_ranges.push(range);
                    Arc::new(copy)
                } else {
                    input
                };
                // DB63E0 and the primitive callbacks find their queue through
                // 018D0B28. The outer queue is still active during this nested
                // traversal: redirect it only inside this private transaction.
                // DBE560/DBD8D0 access fields through +1C14C (captured below).
                let (input, private_scheduler) = if parallel {
                    let (copy, range) = input
                        .private_copy(scheduler, 0x1c150)
                        .map_err(|error| format!("private scheduler: {error:?}"))?;
                    let base = range.start;
                    private_ranges.push(range);
                    (Arc::new(copy), base)
                } else {
                    (input, scheduler)
                };
                let (input, arena) = if parallel && input.u32(context + 0x54) == 0 {
                    let size = input
                        .u32(context + 0x4c)
                        .checked_mul(8)
                        .ok_or_else(|| "secondary arena size overflow".to_string())?;
                    let (extended, range) = input
                        .private_arena(size)
                        .map_err(|error| format!("private secondary arena: {error:?}"))?;
                    (Arc::new(extended), Some(range))
                } else {
                    (input, None)
                };
                if let Some(range) = &arena {
                    private_ranges.push(range.clone());
                }
                let memory = PrivateMemory(Rc::new(RefCell::new(LaneMemory::new(input, ACCESS_LIMIT))));
                with_replay(
                    Replay {
                        raw,
                        memory: memory.clone(),
                        stack,
                        lane_count: if parallel { 2 } else { 1 },
                        depth: 0,
                        command_counter: None,
                    },
                    |m| {
                        let nesting = [m.u8(NESTING), m.u8(NESTING_OTHER)];
                        let initial_counter = m.u32(context + 0x74);
                        let initial_roots = m.u32(context + 0x48);
                        let lane_base = cpu.ecx + 0xd40;
                        let initial_lanes: Vec<_> = if parallel {
                            (0..2)
                                .map(|lane| m.u32(lane_base + lane * LANE_BYTES + 0x74))
                                .collect()
                        } else {
                            Vec::new()
                        };
                        if parallel {
                            if let Some(range) = &arena {
                                m.set_u32(context + 0x54, range.start);
                            }
                            m.set_u8(NESTING, 0);
                            m.set_u8(NESTING_OTHER, 0);
                            m.set_u32(SCHEDULER_GLOBAL, private_scheduler);
                            m.set_u32(private_scheduler + 0x98, 2);
                            m.set_u32(private_scheduler + 0x8c, 0);
                            m.set_u32(private_scheduler + 0x1c14c, 0);
                        }
                        x86::ldmxcsr(mxcsr);
                        resolve(TRAVERSE)(m, &mut cpu);
                        let final_mxcsr = x86::stmxcsr();
                        ddnative_base::x87::park();
                        x86::ldmxcsr(x86::MXCSR_DEFAULT);
                        if parallel {
                            if [m.u8(NESTING), m.u8(NESTING_OTHER)] != [0, 0]
                                || m.u32(SCHEDULER_GLOBAL) != private_scheduler
                            {
                                return Err("private scheduler scope was not balanced".into());
                            }
                            m.set_u32(SCHEDULER_GLOBAL, scheduler);
                            m.set_u8(NESTING, nesting[0]);
                            m.set_u8(NESTING_OTHER, nesting[1]);
                        }
                        if arena.is_some() {
                            m.set_u32(context + 0x54, 0);
                        }
                        if parallel {
                            let count = sequences::rebase(
                                m,
                                context,
                                lane_base,
                                &initial_lanes,
                                initial_counter,
                                initial_roots,
                            )?;
                            eprintln!("parallel-shadow recording counters rebased records={count}");
                        }
                        let published = PublishedView {
                            memory: m,
                            private: private_ranges.clone(),
                        };
                        let stream = capture_context(&published, context, Limits::default())
                            .map_err(|e| format!("stream: {e:?}"))?;
                        let writes = memory.0.borrow().writes().clone();
                        Ok::<_, String>((cpu.clone(), stream, writes, final_mxcsr, private_ranges.clone()))
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
        if !parallel {
            verify_serial(&result)?;
            eprintln!(
                "parallel-shadow original serial/live verified entries={}",
                result.1.entries.len()
            );
        }
        results.push(result);
    }
    // Diagnose the independent state gate even when a stream difference stops
    // acceptance first. These categories never exempt bytes from comparison.
    let mut state_differences = BTreeMap::<&str, (usize, u32)>::new();
    let mut other_words = std::collections::BTreeSet::new();
    let render = input
        .probe(0x018d08f4, 4, false)
        .ok()
        .map(|_| input.u32(0x018d08f4));
    let command_arena = render.filter(|render| *render != 0).and_then(|render| {
        let index = input.u32(render + 0x33db8);
        let base = input.u32(render + 0x33dc0 + index * 4);
        base.checked_add(input.u32(render + 0x33dcc)).map(|end| base..end)
    });
    for address in results[0]
        .2
        .keys()
        .chain(results[1].2.keys())
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
    {
        if scratch.contains(&address) || results[1].4.iter().any(|range| range.contains(&address)) {
            continue;
        }
        let a = results[0]
            .2
            .get(&address)
            .copied()
            .unwrap_or_else(|| input.u8(address));
        let b = results[1]
            .2
            .get(&address)
            .copied()
            .unwrap_or_else(|| input.u8(address));
        if a != b {
            let category = if (context..context + LANE_BYTES).contains(&address) {
                "root context"
            } else if command_arena
                .as_ref()
                .is_some_and(|range| range.contains(&address))
            {
                "command arena"
            } else if (scheduler..scheduler + 0x1c150).contains(&address) {
                "outer scheduler"
            } else {
                "other state"
            };
            let row = state_differences.entry(category).or_insert((0, address));
            row.0 += 1;
            if category == "other state" && other_words.len() < 24 {
                other_words.insert(address & !3);
            }
        }
    }
    eprintln!("parallel-shadow state differences (byte count, first address)={state_differences:#x?}");
    for address in other_words {
        let word = |writes: &BTreeMap<u32, u8>| {
            u32::from_le_bytes(std::array::from_fn(|i| {
                let at = address + i as u32;
                writes.get(&at).copied().unwrap_or_else(|| input.u8(at))
            }))
        };
        eprintln!(
            "parallel-shadow other state {address:#x} serial={:#x} parallel={:#x}",
            word(&results[0].2),
            word(&results[1].2)
        );
    }
    // Uninitialized constant components (claude-9, docs/tasks/L172): a shader
    // constant byte may differ only when neither traversal wrote its source
    // (both executions upload arena residue the producer never set, e.g.
    // CBSkinning's w, which 0x00FE4960's caller leaves unwritten) and the
    // bound shader of that stage provably never reads the component
    // (d3d9shader::Shader::float_reads, conservative). The original's own
    // parallel branch of DB63E0 allocates from the lanes too, so this residue
    // differs between its serial and parallel executions as well.
    let reads: BTreeMap<u32, Option<d3d9shader::FloatReads>> = shaders
        .iter()
        .map(|(&handle, tokens)| (handle, d3d9shader::parse(tokens).ok().map(|s| s.float_reads())))
        .collect();
    let mut residue: Vec<(Vec<usize>, mtcore::shadow_lanes::ConstByte, u8, u8)> = Vec::new();
    let mut refused = 0;
    let mut residue_allowed = |path: &[usize],
                     a: &mtcore::shadow_lanes::Binding,
                     b: &mtcore::shadow_lanes::Binding,
                     offset: usize| -> Result<(), String> {
        let at = offset as u32;
        let origin = |x: &mtcore::shadow_lanes::Binding| x.constants.iter().find(|(o, _)| *o == at).map(|&(_, c)| c);
        let (Some(ca), Some(cb)) = (origin(a), origin(b)) else {
            return Err(format!("not an uploaded constant byte (layouts {} / {})", a.constants.len(), b.constants.len()));
        };
        if ca.section != 1 || (ca.stage, ca.register, ca.byte) != (cb.stage, cb.register, cb.byte) {
            return Err(format!("origins differ or not float: {ca:x?} / {cb:x?}"));
        }
        if results[0].2.contains_key(&ca.source) || results[1].2.contains_key(&cb.source) {
            return Err(format!(
                "written by a traversal: serial {:#x} {} parallel {:#x} {}",
                ca.source, results[0].2.contains_key(&ca.source), cb.source, results[1].2.contains_key(&cb.source)
            ));
        }
        // the stage's shader handle: metadata (+0x10 vertex, +0x14 pixel),
        // encoded after the binding's 12 header bytes (each block after its
        // size word), equal on both sides
        let handle_at = 4 + 12 + 4 + 0x10 + 4 * ca.stage as usize;
        let handle = |x: &mtcore::shadow_lanes::Binding| {
            x.bytes.get(handle_at..handle_at + 4).map(|w| u32::from_le_bytes(w.try_into().unwrap()))
        };
        let (Some(handle), Some(other)) = (handle(a), handle(b)) else { return Err("no shader handle".into()) };
        if handle != other {
            return Err(format!("shader handles differ {handle:#x} / {other:#x}"));
        }
        let Some(Some(r)) = reads.get(&handle) else { return Err(format!("shader {handle:#x} not captured or not parsed")) };
        if r.reads(ca.register as usize, (ca.byte / 4) as usize) {
            return Err(format!("shader {handle:#x} may read c{} component {}", ca.register, ca.byte / 4));
        }
        residue.push((path.to_vec(), ca, a.bytes[offset], b.bytes[offset]));
        Ok(())
    };
    let mut allow = |path: &[usize],
                     a: &mtcore::shadow_lanes::Binding,
                     b: &mtcore::shadow_lanes::Binding,
                     offset: usize| {
        let verdict = residue_allowed(path, a, b, offset);
        if let Err(why) = &verdict {
            refused += 1;
            if refused <= 4 {
                eprintln!("parallel-shadow constant byte {offset:#x} at {path:?} refused: {why}");
            }
        }
        verdict.is_ok()
    };
    let first = results[0].1.first_difference_allowing(&results[1].1, &mut allow);
    if !residue.is_empty() {
        let first = &residue[0];
        eprintln!(
            "parallel-shadow unread residue bytes allowed={} (first: path {:?} stage {} c{} byte {} serial={:#04x} parallel={:#04x})",
            residue.len(), first.0, first.1.stage, first.1.register, first.1.byte, first.2, first.3
        );
    }
    if let Some(diff) = first {
        fn entry<'a>(
            stream: &'a mtcore::shadow_lanes::StreamSnapshot,
            path: &[usize],
        ) -> Option<&'a mtcore::shadow_lanes::Entry> {
            let mut entries = &stream.entries;
            for (depth, &index) in path.iter().enumerate() {
                let entry = entries.get(index)?;
                if depth + 1 == path.len() {
                    return Some(entry);
                }
                entries = &entry.record.children;
            }
            None
        }
        let a = entry(&results[0].1, &diff.path);
        let b = entry(&results[1].1, &diff.path);
        let binding = a.zip(b).and_then(|(a, b)| {
            a.record
                .bindings
                .iter()
                .zip(&b.record.bindings)
                .find(|(a, b)| a != b)
                .map(|(a, b)| {
                    let byte = a
                        .bytes
                        .iter()
                        .zip(&b.bytes)
                        .enumerate()
                        .find(|(_, (a, b))| a != b)
                        .map(|(offset, (av, bv))| {
                            let word = offset & !3;
                            let a_word = a
                                .bytes
                                .get(word..word + 4)
                                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()));
                            let b_word = b
                                .bytes
                                .get(word..word + 4)
                                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()));
                            (offset, *av, *bv, a_word, b_word)
                        });
                    (a.kind, a.identity, b.identity, a.bytes.len(), b.bytes.len(), byte)
                })
        });
        return Err(format!(
            "serial/parallel stream mismatch: {diff:?}; key/header serial={:#x?} parallel={:#x?}; binding(kind,identities,lengths,first_byte)={binding:#x?}; entries={}/{}",
            a.map(|entry| (entry.key, entry.record.header)), b.map(|entry| (entry.key, entry.record.header)),
            results[0].1.entries.len(), results[1].1.entries.len(),
        ));
    }
    // Conservative proof gate: no differing memory accepted yet. Private
    // scheduler/context relocation classification needs separate evidence.
    for address in results[0].2.keys().chain(results[1].2.keys()) {
        // Only newly allocated private sort storage is discarded. FED3B0's
        // consumed roots/children must already be outside it (PublishedView),
        // and the complete stream comparison above must have passed.
        if scratch.contains(address) || results[1].4.iter().any(|range| range.contains(address)) {
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
                "streams matched {} entries; serial/parallel memory mismatch at {address:#x}: serial={a:#04x} parallel={b:#04x}",
                observed.entries.len()
            ));
        }
    }
    if abi(&results[0].0) != abi(&results[1].0) {
        return Err("serial/parallel ABI mismatch".into());
    }
    compare_control_state(&results[0].0, results[0].3, &results[1].0, results[1].3)
        .map_err(|field| format!("serial/parallel {field} mismatch"))?;
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
    fn merged_stream_must_not_retain_private_secondary_storage() {
        let mut live = GuestMemory::new();
        live.map(0x1000, 0x2000, READ | WRITE).unwrap();
        live.set_u32(0x1048, 1);
        live.set_u32(0x2100, 7 << 16);
        live.set_u32(0x2104, 42);
        live.set_u32(0x2108, 1);
        let mut input = Snapshot::default();
        for page in [0x1000, 0x2000] {
            input.capture_page(&live, page, READ | WRITE);
        }
        let (input, range) = input.private_arena(8).unwrap();
        let mut memory = LaneMemory::new(Arc::new(input), 10000);
        memory.set_u32(0x1044, range.start);
        memory.set_u32(range.start, 5);
        memory.set_u32(range.start + 4, 0x2100);
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            capture_context(
                &PublishedView {
                    memory: &memory,
                    private: vec![range.clone()],
                },
                0x1000,
                Limits::default(),
            )
        }));
        assert_eq!(
            rejected.unwrap_err().downcast_ref::<Reject>(),
            Some(&Reject::PrivateOutputEscape(range.start))
        );
        memory.set_u32(0x2000, memory.u32(range.start));
        memory.set_u32(0x2004, memory.u32(range.start + 4));
        memory.set_u32(0x1044, 0x2000);
        let stream = capture_context(
            &PublishedView {
                memory: &memory,
                private: vec![range],
            },
            0x1000,
            Limits::default(),
        )
        .unwrap();
        assert_eq!(stream.entries.len(), 1);
        assert_eq!(stream.entries[0].key, 5);
    }

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
        live.set_u32(0x0139d0a8, 0x7ff07890);
        live.set_u32(0x1300, u32::MAX - 2);
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
            command_counter: None,
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
            m.set_u32(0x1804, 0x1300);
            m.set_u32(0x1808, 7);
            let mut cpu = Cpu {
                esp: 0x1800,
                ebx: 42,
                ..Cpu::default()
            };
            assert!((m as &mut dyn Mem).atomic_u32(0x1300).is_none());
            x86::call_ext(m, &mut cpu, 0x7ff07890);
            assert_eq!(cpu.eax, u32::MAX - 2);
            assert_eq!(cpu.esp, 0x180c);
            assert_eq!(cpu.ebx, 42);
            assert_eq!(m.u32(0x1300), 4);
            assert_eq!(live.u32(0x1300), u32::MAX - 2);
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
            cpu.esp = 0x1800;
            m.set_u32(0x1804, 0x1300);
            m.set_u32(0x1808, 16);
            crate::win32::interlocked_exchange_add(m, &mut cpu);
            let allocated = cpu.eax;
            cpu.esp = 0x1800;
            m.set_u32(0x1804, 0x1100);
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
        let context = m.u32(cpu.esp + 4);
        let second_lane = m.u32(context + 0x58) == 1;
        let mutation = m.u32(0x1010);
        m.set_u32(entry, job);
        m.set_u32(entry + 4, record);
        let counter = m.u32(context + 0x74);
        m.set_u32(record, (7 << 16) | (counter & 0x7fff));
        m.set_u32(context + 0x74, counter.wrapping_add(1));
        m.set_u32(context + 0x48, m.u32(context + 0x48) + 1);
        m.set_u32(record + 4, job + 100);
        m.set_u32(record + 8, if second_lane && mutation == 1 { 9 } else { 1 });
        m.set_u8(0x1400 + job, if second_lane && mutation == 2 { 9 } else { 1 });
        cpu.esp += 8;
    }

    fn synthetic_traverse(m: &mut dyn Mem, cpu: &mut Cpu) {
        let parallel = current().lane_count > 1;
        assert_eq!(m.u32(cpu.ecx + 0x20), 0x12345678);
        if parallel {
            assert_ne!(cpu.ecx, 0x200000);
            m.set_u8(cpu.ecx + 0xd40 + LANE_BYTES, 99);
        }
        if parallel {
            let manager = cpu.ecx;
            cpu.ecx = m.u32(SCHEDULER_GLOBAL);
            assert_ne!(cpu.ecx, 0x40000);
            for lane in 0..2 {
                let context = manager + 0xd40 + lane * LANE_BYTES;
                m.set_u32(cpu.ecx + 0x1bdd0 + lane * 0x20, context);
                m.set_u32(context + 0x44, 0x2000 + lane * 16);
                m.set_u32(context + 0x48, 0);
                m.set_u32(context + 0x58, lane);
            }
            m.set_u32(cpu.ecx + 0x8c, 4);
            push(m, cpu, 1);
            push(m, cpu, 0x1234);
            private_schedule(m, cpu);
        } else {
            for job in 0..4 {
                cpu.ecx = 0x3000 + job * 16;
                push(m, cpu, 0x1000);
                push(m, cpu, 0x1234);
                synthetic_job(m, cpu);
            }
        }
        m.set_u32(0x1048, 4);
        if parallel {
            match m.u32(0x1010) {
                3 => x86::ldmxcsr(0x3f80),
                4 => resolve(0x12345678)(m, cpu),
                5 => m.set_u8(NESTING, 1),
                6 => m.set_u32(SCHEDULER_GLOBAL, 0x40000),
                7 => {
                    let record = m.u32(0x2004);
                    let forbidden = m.u32(SCHEDULER_GLOBAL);
                    for offset in [0, 4, 8] {
                        m.set_u32(forbidden + offset, m.u32(record + offset));
                    }
                    m.set_u32(0x2004, forbidden);
                }
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
            (0x200000, 0x1a0000),
        ];
        for (base, len) in ranges {
            live.map(base, len, READ | WRITE).unwrap();
        }
        live.set_u32(0x1010, mutation);
        live.set_u32(0x200020, 0x12345678);
        live.set_u32(0x200000 + 0xd40 + 0x74, 11);
        live.set_u32(0x200000 + 0xd40 + LANE_BYTES + 0x74, 23);
        live.set_u32(0x1074, 5);
        live.set_u32(0x1044, 0x2000);
        live.set_u32(0x104c, 16);
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
                (0x2100 + job * 16, (7 << 16) | (5 + job)),
                (0x2104 + job * 16, job + 100),
                (0x2108 + job * 16, 1),
            ] {
                expected.set_u32(address, value);
            }
            expected.set_u8(0x1400 + job, 1);
        }
        for (address, value) in [(0x1048, 4), (0x1074, 9)] {
            expected.set_u32(address, value);
        }
        let incoming = Cpu {
            esp: 0x81000,
            ecx: 0x200000,
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
            rejection: None,
            shaders: BTreeMap::new(),
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
            (5, Some("scheduler scope was not balanced")),
            (6, Some("scheduler scope was not balanced")),
            (7, Some("PrivateOutputEscape")),
        ] {
            let fixture = synthetic_fixture(mutation);
            let mut bytes = Vec::new();
            fixture.write(&mut bytes).unwrap();
            let fixture = capture_file::Fixture::read(&mut &bytes[..]).unwrap();
            let result = replay_fixture(
                fixture,
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
    fn rejected_capture_cannot_be_accepted_even_when_diagnostic_replay_matches() {
        let mut fixture = synthetic_fixture(0);
        fixture.rejection = Some("unreadable preseed page".into());
        let mut bytes = Vec::new();
        fixture.write(&mut bytes).unwrap();
        let fixture = capture_file::Fixture::read(&mut &bytes[..]).unwrap();
        let error = replay_fixture(
            fixture,
            Arc::new(BTreeMap::from([
                (TRAVERSE, synthetic_traverse as LiftFn),
                (0x400010, synthetic_job as LiftFn),
            ])),
        )
        .unwrap_err();
        assert!(error.contains("REJECTED capture"));
        assert!(error.contains("diagnostic replay only: Ok(4)"));
    }

    #[test]
    fn serial_live_mismatch_is_reported_before_parallel_execution() {
        let mut fixture = synthetic_fixture(4); // Parallel body would refuse its import.
        fixture.outgoing.eax ^= 1;
        let error = replay_fixture(
            fixture,
            Arc::new(BTreeMap::from([
                (TRAVERSE, synthetic_traverse as LiftFn),
                (0x400010, synthetic_job as LiftFn),
            ])),
        )
        .unwrap_err();
        assert_eq!(error, "original serial replay ABI differs from live serial");
    }

    #[test]
    fn unchanged_existing_commands_do_not_count_as_shadow_work() {
        let mut fixture = synthetic_fixture(0);
        fixture.input = Arc::new(fixture.input.with_writes(&BTreeMap::from([(0x1048, 4)])));
        let error = replay_fixture(
            fixture,
            Arc::new(BTreeMap::from([
                (TRAVERSE, synthetic_traverse as LiftFn),
                (0x400010, synthetic_job as LiftFn),
            ])),
        )
        .unwrap_err();
        assert!(error.contains("traversal appended no commands"));
    }

    #[test]
    fn rejected_preseed_retains_later_bounded_serial_evidence() {
        let mut live = GuestMemory::new();
        live.map(0x1000, 4096, READ | WRITE).unwrap();
        live.set_u32(0x1000, 7);
        let mut journal = Journal {
            live: &mut live,
            input: RefCell::new(Snapshot::default()),
            writes: RefCell::new(BTreeMap::new()),
            failure: Cell::new(None),
            stage: Cell::new("preseed"),
        };
        journal.seed(0, 4096);
        journal.stage.set("serial");
        assert_eq!(journal.u32(0x1000), 7);
        journal.set_u32(0x1000, 9);
        assert_eq!(journal.input.borrow().u32(0x1000), 7);
        assert_eq!(journal.live.u32(0x1000), 9);
        assert_eq!(journal.writes.borrow().get(&0x1000), Some(&9));
        assert_eq!(
            journal.failure.get(),
            Some(CaptureFailure {
                stage: "preseed",
                reason: "unreadable page",
                address: 0
            })
        );
    }

    #[test]
    fn command_arena_preseed_includes_unvisited_allocation_pages() {
        let mut live = GuestMemory::new();
        live.map(0x100000, 0x34000, READ | WRITE).unwrap();
        live.map(0x200000, 0x3000, READ | WRITE).unwrap();
        live.set_u32(0x133db8, 1);
        live.set_u32(0x133dc0, 0); // Inactive ring must not be read as the arena.
        live.set_u32(0x133dc4, 0x200820);
        live.set_u32(0x133dc8, 0x100);
        live.set_u32(0x133dcc, 0x1800);
        live.set_u8(0x20201f, 43);
        let journal = Journal {
            live: &mut live,
            input: RefCell::new(Snapshot::default()),
            writes: RefCell::new(BTreeMap::new()),
            failure: Cell::new(None),
            stage: Cell::new("test"),
        };
        journal.seed_render_commands(0x100000);
        assert!(journal.failure.get().is_none());
        assert!(journal.input.borrow().has_page(0x202000));
        assert_eq!(journal.input.borrow().u8(0x20201f), 43);
        assert!(journal.writes.borrow().is_empty());
        assert_eq!(journal.live.u32(0x133dc8), 0x100);
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
            stage: Cell::new("test"),
        };
        assert_eq!(journal.u32(0x1000), 7);
        journal.set_u32(0x1000, 9);
        assert_eq!(journal.u32(0x1000), 9);
        assert_eq!(journal.input.borrow().u32(0x1000), 7);
        assert_eq!(concurrent.u32(0x1000), 9);
        concurrent.set_u32(0x1000, 11);
        assert_eq!(journal.u32(0x1000), 11);
        assert_eq!(journal.failure.get().unwrap().address, 0x1000);
        journal.fail(0, "later error");
        assert_eq!(journal.failure.get().unwrap().address, 0x1000);
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
                command_counter: None,
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
    fn shared_command_counter_claims_disjoint_bytes_and_rejects_ordinary_access() {
        fn capacity_body(m: &mut dyn Mem, cpu: &mut Cpu) {
            cpu.eax = m.u32(cpu.ecx + 0x33dc8);
            cpu.esp += 4;
        }
        fn allocate(m: &mut dyn Mem, cpu: &mut Cpu) {
            let job = (cpu.ecx - 0x3000) / 0x10;
            let address = m.u32(0x018d08f4) + 0x33dc8;
            push(m, cpu, 16);
            push(m, cpu, address);
            push(m, cpu, 0xda7d86);
            private_exchange_add(m, cpu);
            let allocated = cpu.eax;
            cpu.ecx = m.u32(0x018d08f4);
            push(m, cpu, 0xdb762e);
            resolve(0x00da7db0)(m, cpu);
            assert_ne!(
                cpu.eax,
                m.u32(0x1020),
                "capacity reads the updated shared counter"
            );
            if m.u32(0x1010) != 0 {
                let _ = m.u32(address); // The helper's read permission has ended.
            }
            m.set_u32(0x1400 + job * 4, allocated);
            cpu.esp += 8;
        }
        for (direct_access, initial) in [(false, 0x100u32), (true, 0x100), (false, u32::MAX - 31)] {
            let fixture = synthetic_fixture(0);
            let (input, render) = fixture.input.private_arena(0x34000).unwrap();
            let address = render.start + 0x33dc8;
            let mut setup = LaneMemory::new(Arc::new(input), ACCESS_LIMIT);
            setup.set_u32(0x018d08f4, render.start);
            setup.set_u32(address, initial);
            setup.set_u32(0x1010, u32::from(direct_access));
            setup.set_u32(0x1020, initial);
            setup.set_u32(0x81004, 1);
            let input = Arc::new(setup.frozen());
            let private = PrivateMemory(Rc::new(RefCell::new(LaneMemory::new(
                input.clone(),
                ACCESS_LIMIT,
            ))));
            let replay = Replay {
                raw: Arc::new(BTreeMap::from([
                    (0x400010, allocate as LiftFn),
                    (0x00da7db0, capacity_body as LiftFn),
                ])),
                memory: private.clone(),
                stack: fixture.stack,
                lane_count: 2,
                depth: 0,
                command_counter: None,
            };
            let mut cpu = Cpu {
                ecx: 0x40000,
                esp: 0x81000,
                ..Cpu::default()
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                with_replay(replay, |m| private_schedule(m, &mut cpu));
            }));
            if direct_access {
                assert_eq!(
                    result.unwrap_err().downcast_ref::<Reject>(),
                    Some(&Reject::AtomicConflict(address))
                );
                assert_eq!(private.u32(address), initial);
                assert_eq!(private.u32(0x1400), 0);
            } else {
                result.unwrap();
                let mut allocations: Vec<_> = (0..4).map(|job| private.u32(0x1400 + job * 4)).collect();
                allocations.sort();
                let mut expected: Vec<_> = (0..4).map(|job| initial.wrapping_add(job * 16)).collect();
                expected.sort();
                assert_eq!(allocations, expected);
                assert_eq!(private.u32(address), initial.wrapping_add(64));
            }
            assert_eq!(input.u32(address), initial);
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
            command_counter: None,
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

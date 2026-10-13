//! L172 private-lane execution prerequisite. This module does not replace
//! live shadow traversal: direct lifted calls and sPrimitive's nested jobs
//! still need a proven side-effect boundary before real callbacks are safe.
//!
//! Original lane order: 00DB63E0 -> 00FED200 / 00FED3B0 (DDDA.exe
//! 2017-12-13 Steam). The host executor below is new infrastructure, not a
//! translation of the game's shared, dynamically claimed job queue.
#![allow(dead_code)]

use ddnative_base::mem::{Mem, READ, WRITE};
use std::{cell::RefCell, collections::BTreeMap, ops::Range, sync::Arc};

const PAGE: u32 = 4096;
const MAX_LANES: usize = 8; // Original DB63E0 publishes eight lane slots.
pub(crate) const MAX_TRACKED_BYTES: usize = 2 * 1024 * 1024; // Diagnostic host-memory bound.

#[derive(Clone)]
struct Page {
    bytes: Box<[u8; PAGE as usize]>,
    permissions: u8,
}

/// Explicit immutable input. No lazy reads from the running guest: missing
/// capture pages reject execution rather than mixing two frame states.
#[derive(Clone, Default)]
pub(crate) struct Snapshot {
    pages: BTreeMap<u32, Arc<Page>>,
}

impl Snapshot {
    pub(crate) fn with_writes(&self, writes: &BTreeMap<u32, u8>) -> Self {
        let mut result = self.clone();
        result.apply(writes);
        result
    }
    pub(crate) fn write_to(&self, out: &mut impl std::io::Write) -> std::io::Result<()> {
        out.write_all(&(self.pages.len() as u32).to_le_bytes())?;
        for (&base, page) in &self.pages {
            out.write_all(&base.to_le_bytes())?;
            out.write_all(&[page.permissions])?;
            out.write_all(&page.bytes[..])?;
        }
        Ok(())
    }
    pub(crate) fn read_from(input: &mut impl std::io::Read) -> std::io::Result<Self> {
        fn word(input: &mut impl std::io::Read) -> std::io::Result<u32> {
            let mut bytes = [0; 4];
            input.read_exact(&mut bytes)?;
            Ok(u32::from_le_bytes(bytes))
        }
        let invalid = || std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid shadow snapshot");
        let count = word(input)?;
        if count > 16_384 {
            return Err(invalid());
        }
        let mut result = Self::default();
        for _ in 0..count {
            let base = word(input)?;
            let mut permission = [0];
            input.read_exact(&mut permission)?;
            if base & (PAGE - 1) != 0
                || ![READ, READ | WRITE].contains(&permission[0])
                || result.has_page(base)
            {
                return Err(invalid());
            }
            let mut bytes = Box::new([0; PAGE as usize]);
            input.read_exact(&mut bytes[..])?;
            result.pages.insert(
                base,
                Arc::new(Page {
                    bytes,
                    permissions: permission[0],
                }),
            );
        }
        Ok(result)
    }
    pub(crate) fn has_page(&self, address: u32) -> bool {
        self.pages.contains_key(&(address & !(PAGE - 1)))
    }
    pub(crate) fn page_count(&self) -> usize {
        self.pages.len()
    }
    /// Capture must be called at an externally established stable boundary.
    /// Reading one page does not establish a coherent concurrent snapshot.
    pub(crate) fn capture_page(&mut self, m: &dyn Mem, address: u32, permissions: u8) {
        let base = address & !(PAGE - 1);
        assert_eq!(permissions & !(READ | WRITE), 0);
        m.probe(base, PAGE, false).expect("readable capture page");
        if permissions & WRITE != 0 {
            m.probe(base, PAGE, true).expect("writable capture page");
        }
        let mut bytes = Box::new([0; PAGE as usize]);
        m.read_bytes(base, &mut bytes[..]);
        self.pages.insert(base, Arc::new(Page { bytes, permissions }));
    }

    fn byte(&self, address: u32, write: bool) -> Result<u8, Reject> {
        let page = self
            .pages
            .get(&(address & !(PAGE - 1)))
            .ok_or(Reject::MissingPage(address))?;
        if page.permissions & if write { WRITE } else { READ } == 0 {
            return Err(Reject::Permission { address, write });
        }
        Ok(page.bytes[(address & (PAGE - 1)) as usize])
    }

    fn apply(&mut self, writes: &BTreeMap<u32, u8>) {
        for (&address, &value) in writes {
            let page = Arc::make_mut(self.pages.get_mut(&(address & !(PAGE - 1))).unwrap());
            page.bytes[(address & (PAGE - 1)) as usize] = value;
        }
    }
}

impl Mem for Snapshot {
    fn probe(&self, address: u32, width: u32, write: bool) -> Result<(), u32> {
        for offset in 0..width {
            let at = address.wrapping_add(offset);
            self.byte(at, write).map_err(|_| at)?;
        }
        Ok(())
    }
    fn u8(&self, address: u32) -> u8 {
        self.byte(address, false)
            .unwrap_or_else(|e| std::panic::panic_any(e))
    }
    fn set_u8(&mut self, _: u32, _: u8) {
        panic!("immutable shadow input")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reject {
    MissingPage(u32),
    Permission {
        address: u32,
        write: bool,
    },
    AccessLimit,
    TrackedByteLimit,
    CallbackPanic,
    InvalidLanes,
    InvalidPrivateRange,
    InputMismatch,
    LaneOrder,
    ReadDependency {
        lane: usize,
        address: u32,
        observed: u8,
        ordered: u8,
    },
    WriteConflict {
        lane: usize,
        address: u32,
        previous: u8,
        next: u8,
    },
    ThreadStart,
    UnsupportedTarget(u32),
    UnsupportedScheduler(u32),
    IncoherentInput(u32),
    PublicationConflict(u32),
    UnsupportedLock(u32),
}

/// Each lane owns its writes and reads the same immutable input. Keeping
/// page_table() unavailable is deliberate: Pages::of must use these hooks,
/// otherwise dependency tracking could silently miss a native fast access.
pub(crate) struct LaneMemory {
    input: Arc<Snapshot>,
    reads: RefCell<BTreeMap<u32, u8>>,
    writes: BTreeMap<u32, u8>,
    accesses: std::cell::Cell<usize>,
    limit: usize,
}

impl LaneMemory {
    pub(crate) fn new(input: Arc<Snapshot>, limit: usize) -> Self {
        Self {
            input,
            reads: RefCell::new(BTreeMap::new()),
            writes: BTreeMap::new(),
            accesses: 0.into(),
            limit,
        }
    }
    pub(crate) fn frozen(&self) -> Snapshot {
        let mut result = (*self.input).clone();
        result.apply(&self.writes);
        result
    }
    pub(crate) fn writes(&self) -> &BTreeMap<u32, u8> {
        &self.writes
    }

    /// Publish a proven candidate at an externally established exclusive guest
    /// boundary. The caller must keep every checked location stable through
    /// completion; this preflight is NOT a lock or a concurrent transaction.
    /// `baseline` contains the serial writes in same-run check mode, or is
    /// empty for candidate-only execution. Dependency and stream proof happen
    /// before calling this method. Popped callback stack bytes stay private.
    /// All permissions and expected values are checked before the first store.
    pub(crate) fn publish(
        &self,
        live: &mut dyn Mem,
        baseline: &BTreeMap<u32, u8>,
        scratch: Range<u32>,
    ) -> Result<usize, Reject> {
        if scratch.start > scratch.end {
            return Err(Reject::InvalidPrivateRange);
        }
        let reads = self.reads.borrow();
        for &address in reads.keys().chain(self.writes.keys()).chain(baseline.keys()) {
            if scratch.contains(&address) {
                continue;
            }
            live.probe(address, 1, false)
                .map_err(|address| Reject::Permission {
                    address,
                    write: false,
                })?;
            if self.writes.contains_key(&address) {
                live.probe(address, 1, true)
                    .map_err(|address| Reject::Permission { address, write: true })?;
            }
            let expected = match baseline.get(&address) {
                Some(value) => *value,
                None => self.input.byte(address, false)?,
            };
            if live.u8(address) != expected {
                return Err(Reject::PublicationConflict(address));
            }
        }
        let mut bytes = 0;
        for (&address, &value) in &self.writes {
            if !scratch.contains(&address) {
                live.set_u8(address, value);
                bytes += 1;
            }
        }
        Ok(bytes)
    }

    /// Incorporate a fully validated child batch into the parent's transaction.
    /// Reads not satisfied by parent writes remain dependencies of the parent.
    pub(crate) fn absorb<T>(&mut self, lanes: &[LaneResult<T>], scratch: Range<u32>) {
        for lane in lanes {
            for (&address, _) in lane.memory.reads.borrow().iter() {
                if !scratch.contains(&address) {
                    let _ = self.u8(address);
                }
            }
            for (&address, &value) in &lane.memory.writes {
                if !scratch.contains(&address) {
                    self.set_u8(address, value);
                }
            }
        }
    }
    fn access(&self) {
        let count = self.accesses.get();
        if count == self.limit {
            std::panic::panic_any(Reject::AccessLimit);
        }
        self.accesses.set(count + 1);
    }
}

impl Mem for LaneMemory {
    fn probe(&self, address: u32, width: u32, write: bool) -> Result<(), u32> {
        self.input.probe(address, width, write)
    }
    fn u8(&self, address: u32) -> u8 {
        self.access();
        if let Some(&v) = self.writes.get(&address) {
            return v;
        }
        let value = self.input.u8(address);
        if !self.reads.borrow().contains_key(&address)
            && self.reads.borrow().len() + self.writes.len() >= MAX_TRACKED_BYTES
        {
            std::panic::panic_any(Reject::TrackedByteLimit);
        }
        self.reads.borrow_mut().entry(address).or_insert(value);
        value
    }
    fn set_u8(&mut self, address: u32, value: u8) {
        self.access();
        self.input
            .byte(address, true)
            .unwrap_or_else(|e| std::panic::panic_any(e));
        if !self.writes.contains_key(&address)
            && self.reads.borrow().len() + self.writes.len() >= MAX_TRACKED_BYTES
        {
            std::panic::panic_any(Reject::TrackedByteLimit);
        }
        self.writes.insert(address, value);
    }
    fn alloc(&mut self, _: u32) -> u32 {
        // A host allocation has effects outside Mem and is not replayable.
        std::panic::panic_any(Reject::CallbackPanic)
    }
}

pub(crate) struct LaneResult<T> {
    pub(crate) range: Range<usize>,
    pub(crate) output: T,
    memory: LaneMemory,
}

/// Ordered contiguous partitions, not work stealing: FED3B0 concatenates
/// lane command arrays, so disjoint cyclic assignments change command order.
pub(crate) fn partitions(jobs: usize, lanes: usize) -> Result<Vec<Range<usize>>, Reject> {
    if lanes == 0 || lanes > MAX_LANES {
        return Err(Reject::InvalidLanes);
    }
    let count = lanes.min(jobs);
    if count == 0 {
        return Ok(Vec::new());
    }
    let quotient = jobs / count;
    let remainder = jobs % count;
    let mut begin = 0;
    Ok((0..count)
        .map(|lane| {
            let end = begin + quotient + usize::from(lane < remainder);
            let range = begin..end;
            begin = end;
            range
        })
        .collect())
}

/// Execute a confined callback on private host threads. The callback contract
/// is intentionally explicit: it must access guest state only through Mem,
/// and perform no live services, global overrides or native sink calls.
/// Arbitrary production LiftFn pointers do not satisfy that contract yet.
pub(crate) fn execute<T: Send, F>(
    input: Arc<Snapshot>,
    jobs: usize,
    lanes: usize,
    access_limit: usize,
    callback: F,
) -> Result<Vec<LaneResult<T>>, Reject>
where
    F: Fn(usize, Range<usize>, &mut LaneMemory) -> T + Sync,
{
    let ranges = partitions(jobs, lanes)?;
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        let mut failure = None;
        for (lane, range) in ranges.into_iter().enumerate() {
            let input = input.clone();
            let callback = &callback;
            let spawned = std::thread::Builder::new()
                .name(format!("shadow-audit-{lane}"))
                .stack_size(8 << 20)
                .spawn_scoped(scope, move || {
                    let mut memory = LaneMemory::new(input, access_limit);
                    let output = callback(lane, range.clone(), &mut memory);
                    LaneResult {
                        range,
                        output,
                        memory,
                    }
                });
            match spawned {
                Ok(handle) => handles.push(handle),
                Err(_) => {
                    failure = Some(Reject::ThreadStart);
                    break;
                }
            }
        }
        // Explicitly join ALL workers even after the first failure. No
        // worker may retain input or finish writes after the caller returns.
        let mut results = Vec::new();
        for handle in handles {
            match handle.join() {
                Ok(result) => results.push(result),
                Err(payload) => {
                    if failure.is_none() {
                        failure = Some(
                            payload
                                .downcast_ref::<Reject>()
                                .cloned()
                                .unwrap_or(Reject::CallbackPanic),
                        );
                    }
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(results),
        }
    })
}

/// Validate every lane before publishing any writes, including byte-level
/// read dependencies across lanes. Same-value write-only resource stamps
/// commute; a read followed by an identical write is still a dependency.
/// Private ranges are exclusively proven popped-stack scratch, not outputs.
/// This gate is conservative: it rejects conflicting write-only stores too.
pub(crate) fn merge<T>(
    input: &Snapshot,
    lanes: &[LaneResult<T>],
    private: &[Range<u32>],
) -> Result<Snapshot, Reject> {
    if private.iter().any(|r| r.start > r.end) {
        return Err(Reject::InvalidPrivateRange);
    }
    let scratch = |address: &u32| private.iter().any(|r| r.contains(address));
    let mut writes = BTreeMap::new();
    let mut next_job = 0;
    for (lane, result) in lanes.iter().enumerate() {
        if !std::ptr::eq(input, &*result.memory.input) {
            return Err(Reject::InputMismatch);
        }
        if result.range.start != next_job || result.range.is_empty() {
            return Err(Reject::LaneOrder);
        }
        next_job = result.range.end;
        for (&address, &observed) in result.memory.reads.borrow().iter() {
            if scratch(&address) {
                continue;
            }
            let ordered = writes
                .get(&address)
                .copied()
                .unwrap_or(input.byte(address, false)?);
            if ordered != observed {
                return Err(Reject::ReadDependency {
                    lane,
                    address,
                    observed,
                    ordered,
                });
            }
        }
        for (&address, &next) in &result.memory.writes {
            if scratch(&address) {
                continue;
            }
            if let Some(&previous) = writes.get(&address) {
                if previous != next {
                    return Err(Reject::WriteConflict {
                        lane,
                        address,
                        previous,
                        next,
                    });
                }
            }
            input.byte(address, true)?;
            writes.insert(address, next);
        }
    }
    let mut output = input.clone();
    output.apply(&writes);
    Ok(output)
}

#[derive(Debug)]
pub(crate) enum AuditError {
    Execution(Reject),
    Capture(mtcore::shadow_lanes::CaptureError),
    Stream(mtcore::shadow_lanes::Difference),
    Memory(u32),
}

/// Run both paths from ONE immutable input and compare consumed command
/// payloads, not pointer addresses or hashes. `finish` performs the original
/// ordered lane merge on private memory. A successful result is a command
/// stream and non-scratch memory check; it is not an ABI or gameplay acceptance.
/// All three closures must satisfy execute()'s side-effect confinement rule.
pub(crate) fn audit<T: Send, S, F, J>(
    input: Arc<Snapshot>,
    jobs: usize,
    lane_count: usize,
    access_limit: usize,
    scratch: &[Range<u32>],
    serial: S,
    callback: F,
    finish: J,
) -> Result<usize, AuditError>
where
    S: FnOnce(&mut LaneMemory) -> u32,
    F: Fn(usize, Range<usize>, &mut LaneMemory) -> T + Sync,
    J: FnOnce(&[LaneResult<T>], &mut LaneMemory) -> u32,
{
    use mtcore::shadow_lanes::{capture_context, Limits};
    let checked = || {
        let mut serial_memory = LaneMemory::new(input.clone(), access_limit);
        let serial_context = serial(&mut serial_memory);
        let expected = capture_context(&serial_memory, serial_context, Limits::default())
            .map_err(AuditError::Capture)?;
        let lanes = execute(input.clone(), jobs, lane_count, access_limit, callback)
            .map_err(AuditError::Execution)?;
        let merged = merge(&input, &lanes, scratch).map_err(AuditError::Execution)?;
        let mut final_memory = LaneMemory::new(Arc::new(merged), access_limit);
        let parallel_context = finish(&lanes, &mut final_memory);
        let actual = capture_context(&final_memory, parallel_context, Limits::default())
            .map_err(AuditError::Capture)?;
        if let Some(difference) = expected.first_difference(&actual) {
            return Err(AuditError::Stream(difference));
        }
        // Equal resource IDs are insufficient if their contents changed.
        // Include every lane write, not just the finishing merger's writes.
        let final_snapshot = final_memory.frozen();
        for address in serial_memory
            .writes
            .keys()
            .chain(final_memory.writes.keys())
            .chain(lanes.iter().flat_map(|lane| lane.memory.writes.keys()))
        {
            if scratch.iter().any(|range| range.contains(address)) {
                continue;
            }
            if serial_memory.u8(*address) != final_snapshot.u8(*address) {
                return Err(AuditError::Memory(*address));
            }
        }
        Ok(expected.entries.len())
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(checked)).unwrap_or_else(|payload| {
        Err(AuditError::Execution(
            payload
                .downcast_ref::<Reject>()
                .cloned()
                .unwrap_or(Reject::CallbackPanic),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::GuestMemory;
    use ddnative_base::mem::Pages;
    fn input() -> Arc<Snapshot> {
        let mut m = GuestMemory::new();
        m.map(0x1000, PAGE, READ | WRITE).unwrap();
        m.map(0x3000, PAGE, READ).unwrap();
        let mut snapshot = Snapshot::default();
        snapshot.capture_page(&m, 0x1000, READ | WRITE);
        snapshot.capture_page(&m, 0x3000, READ);
        Arc::new(snapshot)
    }

    #[test]
    fn publication_preflights_all_dependencies_before_any_store() {
        let mut live = GuestMemory::new();
        live.map(0x1000, PAGE * 2, READ | WRITE).unwrap();
        let mut snapshot = Snapshot::default();
        for page in [0x1000, 0x2000] {
            snapshot.capture_page(&live, page, READ | WRITE);
        }
        let mut candidate = LaneMemory::new(Arc::new(snapshot), 100);
        assert_eq!(candidate.u8(0x2fff), 0);
        candidate.set_u32(0x1000, 0x12345678);
        candidate.set_u8(0x1800, 99); // Popped private callback stack.
        live.set_u8(0x2fff, 1);
        assert_eq!(
            candidate.publish(&mut live, &BTreeMap::new(), 0x1800..0x1900),
            Err(Reject::PublicationConflict(0x2fff))
        );
        assert_eq!(live.u32(0x1000), 0);
        assert_eq!(live.u8(0x1800), 0);
        live.set_u8(0x2fff, 0);
        assert_eq!(
            candidate.publish(&mut live, &BTreeMap::new(), 0x1800..0x1900),
            Ok(4)
        );
        assert_eq!(live.u32(0x1000), 0x12345678);
        assert_eq!(live.u8(0x1800), 0);
    }

    #[test]
    fn publication_checks_write_only_locations_and_serial_baseline() {
        let mut live = GuestMemory::new();
        live.map(0x1000, PAGE, READ | WRITE).unwrap();
        let mut snapshot = Snapshot::default();
        snapshot.capture_page(&live, 0x1000, READ | WRITE);
        let mut candidate = LaneMemory::new(Arc::new(snapshot), 100);
        candidate.set_u8(0x1000, 3);
        candidate.set_u8(0x1001, 4);
        live.set_u8(0x1001, 4);
        assert_eq!(
            candidate.publish(&mut live, &BTreeMap::new(), 0..0),
            Err(Reject::PublicationConflict(0x1001))
        );
        assert_eq!(live.u8(0x1000), 0);
        let baseline = BTreeMap::from([(0x1001, 4), (0x1002, 7)]);
        assert_eq!(
            candidate.publish(&mut live, &baseline, 0..0),
            Err(Reject::PublicationConflict(0x1002))
        );
        assert_eq!(live.u8(0x1000), 0);
        live.set_u8(0x1002, 7);
        assert_eq!(candidate.publish(&mut live, &baseline, 0..0), Ok(2));
        assert_eq!([live.u8(0x1000), live.u8(0x1001), live.u8(0x1002)], [3, 4, 7]);
    }

    #[test]
    fn publication_checks_late_permissions_before_early_store() {
        let mut writable = GuestMemory::new();
        writable.map(0x1000, PAGE * 2, READ | WRITE).unwrap();
        let mut snapshot = Snapshot::default();
        for page in [0x1000, 0x2000] {
            snapshot.capture_page(&writable, page, READ | WRITE);
        }
        let mut candidate = LaneMemory::new(Arc::new(snapshot), 100);
        candidate.set_u8(0x1000, 3);
        candidate.set_u8(0x2000, 4);
        let mut live = GuestMemory::new();
        live.map(0x1000, PAGE, READ | WRITE).unwrap();
        live.map(0x2000, PAGE, READ).unwrap();
        assert_eq!(
            candidate.publish(&mut live, &BTreeMap::new(), 0..0),
            Err(Reject::Permission {
                address: 0x2000,
                write: true
            })
        );
        assert_eq!(live.u8(0x1000), 0);
        assert_eq!(live.u8(0x2000), 0);
    }

    #[test]
    fn contiguous_partition_preserves_every_job_once() {
        for jobs in 0..129 {
            for lanes in 1..=8 {
                let parts = partitions(jobs, lanes).unwrap();
                assert!(parts.iter().all(|r| !r.is_empty()));
                assert_eq!(
                    parts.into_iter().flatten().collect::<Vec<_>>(),
                    (0..jobs).collect::<Vec<_>>()
                );
            }
        }
        assert_eq!(partitions(2, 0), Err(Reject::InvalidLanes));
        assert_eq!(partitions(2, 9), Err(Reject::InvalidLanes));
        assert_eq!(partitions(usize::MAX, 8).unwrap().last().unwrap().end, usize::MAX);
    }

    #[test]
    fn real_parallel_execution_merges_in_serial_command_order() {
        let input = input();
        let barrier = std::sync::Barrier::new(4);
        let results = execute(input.clone(), 19, 4, 1000, |_, range, m| {
            barrier.wait(); // Cannot complete if lanes execute serially.
            let p = Pages::of(m);
            range
                .map(|job| {
                    p.set_u32(m, 0x1000 + job as u32 * 4, job as u32 + 1);
                    job
                })
                .collect::<Vec<_>>()
        })
        .unwrap();
        assert_eq!(
            results.iter().flat_map(|r| r.output.clone()).collect::<Vec<_>>(),
            (0..19).collect::<Vec<_>>()
        );
        let merged = merge(&input, &results, &[]).unwrap();
        let mut serial = LaneMemory::new(input.clone(), 10_000);
        for job in 0..19 {
            serial.set_u32(0x1000 + job * 4, job + 1);
        }
        for address in 0x1000..0x2000 {
            assert_eq!(merged.u8(address), serial.u8(address));
        }
        assert_eq!(input.u32(0x1000), 0); // Original remains untouched.
    }

    #[test]
    fn cross_lane_read_dependency_rejects_without_partial_publication() {
        let input = input();
        let results = execute(input.clone(), 2, 2, 100, |lane, _, m| {
            if lane == 0 {
                m.set_u8(0x1000, 1);
            } else {
                let old = m.u8(0x1000);
                m.set_u8(0x1001, old);
            }
        })
        .unwrap();
        assert!(matches!(
            merge(&input, &results, &[]),
            Err(Reject::ReadDependency {
                lane: 1,
                address: 0x1000,
                ..
            })
        ));
        assert_eq!(input.u16(0x1000), 0);
    }

    #[test]
    fn equal_write_only_stamps_commute_but_read_modify_write_does_not() {
        let input = input();
        let results = execute(input.clone(), 8, 8, 100, |_, _, m| m.set_u32(0x1000, 123)).unwrap();
        assert_eq!(merge(&input, &results, &[]).unwrap().u32(0x1000), 123);
        let results = execute(input.clone(), 2, 2, 100, |_, _, m| {
            let old = m.u32(0x1000);
            m.set_u32(0x1000, old + 1);
        })
        .unwrap();
        assert!(matches!(
            merge(&input, &results, &[]),
            Err(Reject::ReadDependency { .. })
        ));
    }

    #[test]
    fn scratch_is_private_but_returned_stack_and_write_conflicts_are_checked() {
        let input = input();
        let results = execute(input.clone(), 2, 2, 100, |lane, _, m| {
            m.set_u32(0x1100, lane as u32);
        })
        .unwrap();
        assert!(matches!(
            merge(&input, &results, &[]),
            Err(Reject::WriteConflict { .. })
        ));
        assert!(merge(&input, &results, &[0x1000..0x1100]).is_err());
        assert_eq!(merge(&input, &results, &[0x1000..0x1104]).unwrap().u32(0x1100), 0);
    }

    #[test]
    fn faults_limits_and_worker_panics_never_publish_writes() {
        let input = input();
        for (address, expected) in [
            (0x2000, Reject::MissingPage(0x2000)),
            (
                0x3000,
                Reject::Permission {
                    address: 0x3000,
                    write: true,
                },
            ),
        ] {
            let result = execute(input.clone(), 1, 1, 100, |_, _, m| m.set_u8(address, 1));
            assert_eq!(result.err(), Some(expected));
        }
        assert_eq!(
            execute(input.clone(), 1, 1, 1, |_, _, m| m.u32(0x1000)).err(),
            Some(Reject::AccessLimit)
        );
        assert_eq!(
            execute(input.clone(), 1, 1, 10, |_, _, _| panic!("worker failed")).err(),
            Some(Reject::CallbackPanic)
        );
        assert_eq!(input.u8(0x1000), 0);
    }

    #[test]
    fn merge_rejects_wrong_snapshot_and_permuted_lanes() {
        let original = input();
        let mut results = execute(original.clone(), 2, 2, 100, |_, _, _| ()).unwrap();
        let other = (*original).clone();
        assert!(matches!(merge(&other, &results, &[]), Err(Reject::InputMismatch)));
        results.swap(0, 1);
        assert!(matches!(merge(&original, &results, &[]), Err(Reject::LaneOrder)));
    }

    #[test]
    fn same_input_serial_parallel_stream_audit_catches_order_and_payload_changes() {
        // Synthetic kind7 command records exercise the real strict decoder.
        // This is not an original game's captured shadow draw corpus.
        fn record(m: &mut LaneMemory, job: u32, value: u32) {
            let entry = 0x1200 + job * 8;
            let command = 0x1300 + job * 16;
            m.set_u32(entry, job);
            m.set_u32(entry + 4, command);
            m.set_u32(command, 7 << 16);
            m.set_u32(command + 4, value);
            m.set_u32(command + 8, 0);
        }
        for mutation in 0..4 {
            let result = audit(
                input(),
                4,
                2,
                10_000,
                &[],
                |m| {
                    for job in 0..4 {
                        record(m, job, job + 20);
                    }
                    m.set_u32(0x1044, 0x1200);
                    m.set_u32(0x1048, 4);
                    0x1000
                },
                |_, range, m| {
                    for job in range {
                        record(m, job as u32, job as u32 + 20);
                    }
                },
                |_, m| {
                    if mutation == 1 {
                        m.set_u32(0x1304, 99);
                    }
                    if mutation == 2 {
                        m.set_u32(0x1204, 0x1310);
                        m.set_u32(0x120c, 0x1300);
                    }
                    if mutation == 3 {
                        m.set_u32(0x1800, 99);
                    }
                    m.set_u32(0x1044, 0x1200);
                    m.set_u32(0x1048, 4);
                    0x1000
                },
            );
            if mutation == 0 {
                assert_eq!(result.unwrap(), 4);
            } else if mutation == 3 {
                assert!(matches!(result, Err(AuditError::Memory(0x1800))));
            } else {
                assert!(matches!(result, Err(AuditError::Stream(_))));
            }
        }
    }
}

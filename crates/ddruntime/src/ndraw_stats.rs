//! Opt-in, read-only nDraw command telemetry. See docs/NDRAW.md (L111).
//! Counts submitted records, NOT post-coalescing D3D draw calls. No wrapper
//! is installed when DDRUNTIME_NDRAW_STATS is not exactly "1".
#![cfg_attr(not(feature = "game"), allow(dead_code))]
use ddnative_base::{
    mem::Mem,
    x86::{Cpu, LiftFn},
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, OnceLock};

const EXECUTOR: u32 = 0x00DAB480;
// Diagnostic limits, not recovered engine constants. Never affect execution.
const MAX_RECORDS: usize = 1_000_000;
const MAX_DEPTH: usize = 256;

pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("DDRUNTIME_NDRAW_STATS").as_deref() == Ok("1"))
}

fn interval() -> u64 {
    static INTERVAL: OnceLock<u64> = OnceLock::new();
    *INTERVAL.get_or_init(|| {
        std::env::var("DDRUNTIME_NDRAW_STATS_EVERY")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|n| *n > 0)
            .unwrap_or(60)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DecodeError {
    Unreadable(u32),
    Overflow,
    Null,
    Depth,
    Budget,
}
type Result<T> = std::result::Result<T, DecodeError>;

// Checked reads are deliberately separate from execution's fault path.
struct Reader<'a>(&'a dyn Mem);
impl Reader<'_> {
    fn at(&self, base: u32, offset: u32, width: u32) -> Result<u32> {
        let p = base.checked_add(offset).ok_or(DecodeError::Overflow)?;
        p.checked_add(width - 1).ok_or(DecodeError::Overflow)?;
        if base == 0 {
            return Err(DecodeError::Null);
        }
        self.0.probe(p, width, false).map_err(DecodeError::Unreadable)?;
        Ok(p)
    }
    fn word(&self, base: u32, offset: u32) -> Result<u32> {
        Ok(self.0.u32(self.at(base, offset, 4)?))
    }
    fn byte(&self, base: u32, offset: u32) -> Result<u8> {
        Ok(self.0.u8(self.at(base, offset, 1)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Draw {
    target: u32,
    state: u32,
    shader: u32,
    streams: u32,
    elements: u32,
    start: u32,
    base_vertex: Option<u32>,
    instances: Option<(u32, u32)>,
}

// Translated read-only layout from 0x00DAB480 (DDDA.exe 2017-12-13 Steam).
fn draw(r: &Reader<'_>, p: u32, kind: u8) -> Result<Draw> {
    Ok(Draw {
        target: r.word(p, 4)?,
        state: r.word(p, 8)?,
        shader: r.word(p, 12)?,
        streams: r.word(p, 16)?,
        elements: r.word(p, 20)?,
        start: r.word(p, 24)?,
        base_vertex: if kind == 1 || kind == 3 {
            Some(r.word(p, 28)?)
        } else {
            None
        },
        instances: match kind {
            2 => Some((r.word(p, 28)?, r.word(p, 32)?)),
            3 => Some((r.word(p, 32)?, r.word(p, 36)?)),
            _ => None,
        },
    })
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Pass {
    /// D3D surface handles (RT0..RT3, depth), comparable to ddframe IDs.
    surfaces: [u32; 5],
    draws: u64,
    clears: u64,
    copies: u64,
}

#[derive(Debug, Default)]
struct Counts {
    by_type: [u64; 16],
    draws: u64,
    elements: u64,
    state_groups: BTreeSet<u32>,
    states: BTreeSet<u32>,
    shader_groups: BTreeSet<u32>,
    vs: BTreeSet<u32>,
    ps: BTreeSet<u32>,
    samplers: BTreeSet<u32>,
    textures: BTreeSet<u32>,
    layouts: BTreeSet<u32>,
    stream_groups: BTreeSet<u32>,
    buffers: BTreeSet<u32>,
    targets: BTreeMap<[u32; 5], u64>,
    passes: Vec<Pass>,
    lists: u64,
    errors: Vec<DecodeError>,
}

fn nonzero(set: &mut BTreeSet<u32>, p: u32) {
    if p != 0 {
        set.insert(p);
    }
}

impl Counts {
    // Layout sources: 0x00DA6FE0 (targets), 0x00DAA5F0 (states),
    // 0x00DAA7C0 (buffers), 0x00DAA8B0 (shader metadata), same Steam exe.
    fn target(&mut self, r: &Reader<'_>, p: u32) -> Result<&mut Pass> {
        let mut surfaces = [0; 5];
        for (i, surface) in surfaces.iter_mut().enumerate() {
            let view = r.word(p, i as u32 * 4)?;
            if view != 0 {
                *surface = r.word(view, 0x14)?;
            }
        }
        if self.passes.last().is_none_or(|pass| pass.surfaces != surfaces) {
            self.passes.push(Pass {
                surfaces,
                ..Pass::default()
            });
        }
        Ok(self.passes.last_mut().unwrap())
    }

    fn state(&mut self, r: &Reader<'_>, p: u32) -> Result<()> {
        nonzero(&mut self.state_groups, p);
        for offset in [8, 12, 16] {
            nonzero(&mut self.states, r.word(p, offset)?);
        }
        Ok(())
    }

    fn draw(&mut self, r: &Reader<'_>, d: Draw) -> Result<()> {
        self.draws += 1;
        self.elements += u64::from(d.elements);
        let pass = self.target(r, d.target)?;
        pass.draws += 1;
        let surfaces = pass.surfaces;
        *self.targets.entry(surfaces).or_default() += 1;
        self.state(r, d.state)?;
        nonzero(&mut self.shader_groups, d.shader);
        let meta = r.word(d.shader, 8)?;
        nonzero(&mut self.vs, r.word(meta, 0x10)?);
        nonzero(&mut self.ps, r.word(meta, 0x14)?);
        nonzero(&mut self.layouts, r.word(meta, 0x18)?);
        let descriptors = r.word(meta, 4)?;
        let vertex_counts = r.word(meta, 8)?;
        let pixel_counts = r.word(meta, 12)?;
        // 0x00DAA8B0: sampler descriptors lead each stage's descriptors.
        // VS total descriptor count occupies the low byte of metadata+8.
        for (base, count) in [
            (0, (vertex_counts >> 8) & 15),
            ((vertex_counts & 255) * 4, (pixel_counts >> 8) & 15),
        ] {
            for i in 0..count {
                let descriptor = r.word(descriptors, base + i * 4)?;
                let sampler = r.word(d.shader, 12 + ((descriptor >> 12) & 0x3ff) * 4)?;
                nonzero(&mut self.samplers, sampler);
                nonzero(&mut self.states, sampler);
                nonzero(&mut self.textures, r.word(d.shader, 12 + (descriptor >> 22) * 4)?);
            }
        }
        nonzero(&mut self.stream_groups, d.streams);
        for offset in [0, 4, 8, 12, 0x28] {
            nonzero(&mut self.buffers, r.word(d.streams, offset)?);
        }
        Ok(())
    }
}

// Translated traversal from 0x00DAB480; root selection from 0x00DA6DA0
// (DDDA.exe 2017-12-13 Steam). Deliberately visits RAW submitted records:
// indexed-draw coalescing and adjacent-copy suppression are not simulated.
fn decode(m: &dyn Mem, render: u32, counts: &mut Counts) -> Result<()> {
    let r = Reader(m);
    let slot = r.word(render, 0x33DB8)? ^ u32::from(r.byte(render, 0x26)? != 0);
    let offset = slot.checked_mul(4).ok_or(DecodeError::Overflow)?;
    let entries = r.word(
        render,
        0x433DD8u32.checked_add(offset).ok_or(DecodeError::Overflow)?,
    )?;
    let count = r.word(
        render,
        0x433DE0u32.checked_add(offset).ok_or(DecodeError::Overflow)?,
    )?;
    walk(&r, entries, count, counts)
}

fn walk(r: &Reader<'_>, entries: u32, count: u32, counts: &mut Counts) -> Result<()> {
    fn range(entries: u32, count: u32) -> Result<(u32, u32)> {
        let end = entries
            .checked_add(count.checked_mul(8).ok_or(DecodeError::Overflow)?)
            .ok_or(DecodeError::Overflow)?;
        Ok((entries, end))
    }
    let mut stack = vec![range(entries, count)?];
    let mut visited = 0;
    counts.lists += 1;
    while let Some((cursor, end)) = stack.last_mut() {
        if *cursor >= *end {
            stack.pop();
            continue;
        }
        if *cursor == 0 {
            break;
        } // executor terminates on null entry cursor
        if visited == MAX_RECORDS {
            return Err(DecodeError::Budget);
        }
        visited += 1;
        let record = r.word(*cursor, 4)?;
        *cursor += 8;
        let kind = r.byte(record, 2)? & 15;
        counts.by_type[kind as usize] += 1;
        match kind {
            0..=3 => counts.draw(r, draw(r, record, kind)?)?,
            4 => {
                let fallback = r.word(record, 0x20)?;
                counts.target(r, r.word(fallback, 4)?)?.clears += 1;
                counts.state(r, r.word(fallback, 8)?)?;
                // The original chooses Clear or a fallback draw from float
                // representability and mask. This remains a clear RECORD.
            }
            5 => {
                let context = r.word(record, 0x28)?;
                counts.target(r, r.word(context, 4)?)?.copies += 1;
            }
            6 => {
                if stack.len() == MAX_DEPTH {
                    return Err(DecodeError::Depth);
                }
                stack.push(range(r.word(record, 8)?, r.word(record, 12)?)?);
                counts.lists += 1;
            }
            _ => {} // 7 query marker; 8..15 ignored by executor switch
        }
    }
    Ok(())
}

#[derive(Default)]
struct Frame {
    number: u64,
    counts: Counts,
}
impl Frame {
    fn finish(&mut self) -> Counts {
        self.number += 1;
        std::mem::take(&mut self.counts)
    }
}
static FRAMES: Mutex<BTreeMap<u32, Frame>> = Mutex::new(BTreeMap::new());
static ORIGINAL: OnceLock<LiftFn> = OnceLock::new();

/// Called before ddports freezes overrides. Preserve an existing port/mod;
/// otherwise capture the raw body (the generated entry wrapper would recurse).
pub(crate) fn wrap_executor(ports: &mut Vec<(u32, LiftFn)>, bodies: &[(u32, LiftFn)]) {
    if !enabled() {
        return;
    }
    let Some(&(_, original)) = ports
        .iter()
        .chain(bodies)
        .find(|(address, _)| *address == EXECUTOR)
    else {
        eprintln!("ndraw stats: executor 0x{EXECUTOR:08x} has no lifted body; observer unavailable");
        return;
    };
    ORIGINAL.get_or_init(|| original);
    ports.retain(|(address, _)| *address != EXECUTOR);
    ports.push((EXECUTOR, execute));
}

fn observe(m: &dyn Mem, render: u32) {
    let r = Reader(m);
    let Ok(device) = r.word(render, 0x98) else {
        return;
    };
    let mut frames = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    let counts = &mut frames.entry(device).or_default().counts;
    if let Err(error) = decode(m, render, counts) {
        counts.errors.push(error);
    }
}

fn execute(m: &mut dyn Mem, cpu: &mut Cpu) {
    // Call the saved body/port directly: no dispatch or override recursion.
    execute_with(
        m,
        cpu,
        *ORIGINAL.get().expect("nDraw observer installed with original"),
    );
}

fn execute_with(m: &mut dyn Mem, cpu: &mut Cpu, original: LiftFn) {
    observe(m, cpu.ecx);
    original(m, cpu);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{GuestMemory, READ, WRITE};

    const RENDER: u32 = 0x1000;
    fn memory() -> GuestMemory {
        let mut m = GuestMemory::new();
        m.map(0x1000, 0x4000, READ | WRITE).unwrap();
        m.map(RENDER + 0x33000, 0x1000, READ | WRITE).unwrap();
        m.map(RENDER + 0x433000, 0x1000, READ | WRITE).unwrap();
        m.set_u32(RENDER + 0x433DD8, 0x2000);
        m.set_u32(RENDER + 0x433DE0, 1);
        m.set_u32(RENDER + 0x98, 0x1234);
        m
    }
    fn record(m: &mut GuestMemory, entry: u32, p: u32, kind: u8) {
        m.set_u32(entry, 0xfeed0000); // sort key is NOT a record pointer
        m.set_u32(entry + 4, p);
        m.set_u8(p + 2, 0xa0 | kind);
    }
    fn draw_record(m: &mut GuestMemory, entry: u32, p: u32, kind: u8) {
        record(m, entry, p, kind);
        for (offset, value) in [
            (4, 0x3800),
            (8, 0x3900),
            (12, 0x3a00),
            (16, 0x3b00),
            (20, 12),
            (24, 7),
            (28, 2),
            (32, 9),
            (36, 4),
        ] {
            m.set_u32(p + offset, value);
        }
        m.set_u32(0x3800, 0x3c00);
        m.set_u32(0x3c14, 0xdead);
        for (offset, value) in [(8, 0x91), (12, 0x92), (16, 0x93)] {
            m.set_u32(0x3900 + offset, value);
        }
        m.set_u32(0x3a08, 0x3d00);
        m.set_u32(0x3d10, 0x81);
        m.set_u32(0x3d14, 0x82);
        m.set_u32(0x3d18, 0x83);
        m.set_u32(0x3b00, 0x71);
        m.set_u32(0x3b28, 0x72);
    }

    #[test]
    fn all_draw_payloads_and_distinct_identities() {
        let mut m = memory();
        m.set_u32(RENDER + 0x433DE0, 4);
        for kind in 0..4 {
            let p = 0x3000 + 0x40 * u32::from(kind);
            draw_record(&mut m, 0x2000 + 8 * u32::from(kind), p, kind);
            let d = draw(&Reader(&m), p, kind).unwrap();
            assert_eq!((d.elements, d.start), (12, 7));
            assert_eq!(d.base_vertex, if kind % 2 == 1 { Some(2) } else { None });
            assert_eq!(
                d.instances,
                match kind {
                    2 => Some((2, 9)),
                    3 => Some((9, 4)),
                    _ => None,
                }
            );
        }
        let mut c = Counts::default();
        decode(&m, RENDER, &mut c).unwrap();
        assert_eq!(&c.by_type[..4], &[1, 1, 1, 1]);
        assert_eq!((c.draws, c.elements, c.passes.len()), (4, 48, 1));
        assert_eq!(c.states, BTreeSet::from([0x91, 0x92, 0x93]));
        assert_eq!(c.buffers, BTreeSet::from([0x71, 0x72]));
        assert_eq!(c.vs, BTreeSet::from([0x81]));
        assert_eq!(c.ps, BTreeSet::from([0x82]));
        assert_eq!(c.targets[&[0xdead, 0, 0, 0, 0]], 4);
    }

    #[test]
    fn nested_lists_return_to_parent_and_preserve_target_order() {
        let mut m = memory();
        m.set_u32(RENDER + 0x433DE0, 3);
        draw_record(&mut m, 0x2000, 0x3000, 0);
        record(&mut m, 0x2008, 0x3040, 6);
        m.set_u32(0x3048, 0x2100);
        m.set_u32(0x304c, 2);
        draw_record(&mut m, 0x2100, 0x3080, 1);
        m.set_u32(0x3084, 0x3e00); // null surfaces, a different target set
        record(&mut m, 0x2108, 0x30c0, 8);
        draw_record(&mut m, 0x2010, 0x3100, 3);
        let mut c = Counts::default();
        decode(&m, RENDER, &mut c).unwrap();
        assert_eq!((c.lists, c.draws, c.by_type.iter().sum::<u64>()), (2, 3, 5));
        assert_eq!(
            c.passes.iter().map(|p| p.surfaces[0]).collect::<Vec<_>>(),
            [0xdead, 0, 0xdead]
        );
        assert_eq!(c.targets[&[0xdead, 0, 0, 0, 0]], 2);
    }

    #[test]
    fn root_slot_xor_and_clear_copy_query_ignored_types() {
        let mut m = memory();
        m.set_u8(RENDER + 0x26, 1);
        m.set_u32(RENDER + 0x433DDc, 0x2200);
        m.set_u32(RENDER + 0x433DE4, 4);
        draw_record(&mut m, 0x2000, 0x3200, 0);
        for (i, kind) in [4, 5, 7, 15].into_iter().enumerate() {
            let p = 0x3000 + i as u32 * 0x40;
            record(&mut m, 0x2200 + i as u32 * 8, p, kind);
            m.set_u32(p + 0x20, 0x3200);
            m.set_u32(p + 0x28, 0x3200);
        }
        let mut c = Counts::default();
        decode(&m, RENDER, &mut c).unwrap();
        assert_eq!((c.draws, c.passes[0].clears, c.passes[0].copies), (0, 1, 1));
        for kind in [4, 5, 7, 15] {
            assert_eq!(c.by_type[kind], 1);
        }
        m.set_u32(RENDER + 0x33DB8, 1);
        let mut c = Counts::default();
        decode(&m, RENDER, &mut c).unwrap();
        assert_eq!(c.draws, 1);
    }

    #[test]
    fn malformed_and_recursive_lists_stop_only_the_observer() {
        let mut m = memory();
        record(&mut m, 0x2000, 0x3000, 6);
        m.set_u32(0x3008, 0x2000);
        m.set_u32(0x300c, 1);
        assert_eq!(
            decode(&m, RENDER, &mut Counts::default()),
            Err(DecodeError::Depth)
        );
        m.set_u32(0x3008, 0xfffffff8);
        m.set_u32(0x300c, 2);
        assert_eq!(
            decode(&m, RENDER, &mut Counts::default()),
            Err(DecodeError::Overflow)
        );
        m.set_u32(0x2004, 0xdead0000);
        assert_eq!(
            decode(&m, RENDER, &mut Counts::default()),
            Err(DecodeError::Unreadable(0xdead0002))
        );
    }

    #[test]
    fn empty_and_null_root_do_not_read_entries() {
        let m = memory();
        let mut c = Counts::default();
        walk(&Reader(&m), 0xdead0000, 0, &mut c).unwrap();
        walk(&Reader(&m), 0, 9, &mut c).unwrap();
        assert_eq!(c.by_type.iter().sum::<u64>(), 0);
    }

    #[test]
    fn vertex_and_pixel_sampler_descriptor_offsets_and_frame_reset() {
        let mut m = memory();
        draw_record(&mut m, 0x2000, 0x3000, 0);
        m.set_u32(0x3d04, 0x3e00);
        m.set_u32(0x3d08, 0x105); // one VS sampler, five total VS descriptors
        m.set_u32(0x3d0c, 0x100); // one PS sampler
        m.set_u32(0x3e00, (1 << 12) | (2 << 22));
        m.set_u32(0x3e14, (3 << 12) | (4 << 22));
        for (offset, pointer) in [(16, 0x51), (20, 0x61), (24, 0x52), (28, 0x62)] {
            m.set_u32(0x3a00 + offset, pointer);
        }
        let mut frame = Frame::default();
        decode(&m, RENDER, &mut frame.counts).unwrap();
        let c = frame.finish();
        assert_eq!(c.samplers, BTreeSet::from([0x51, 0x52]));
        assert_eq!(c.textures, BTreeSet::from([0x61, 0x62]));
        assert_eq!(c.states.len(), 5);
        assert_eq!(frame.number, 1);
        assert_eq!(frame.finish().draws, 0);
        assert!(frame.counts.states.is_empty());
        assert!(frame.counts.passes.is_empty());
        assert_eq!(frame.number, 2);
    }

    #[test]
    fn wrapper_preserves_cpu_memory_and_calls_original_once_even_on_decode_error() {
        fn original(m: &mut dyn Mem, cpu: &mut Cpu) {
            m.set_u32(0x4000, m.u32(0x4000) + 1);
            cpu.eax = cpu.ecx ^ 0x55;
            cpu.esp += 4;
        }
        let mut m = memory(); // bad null record pointer: observer reports, still calls
        let before = m.read(0x1000, 0x3000).unwrap();
        let mut cpu = Cpu {
            ecx: RENDER,
            esp: 0x4200,
            ..Cpu::default()
        };
        let mut expected = cpu.clone();
        original(&mut m, &mut expected);
        m.set_u32(0x4000, 0);
        execute_with(&mut m, &mut cpu, original);
        assert_eq!(format!("{cpu:?}"), format!("{expected:?}"));
        assert_eq!(m.u32(0x4000), 1);
        assert_eq!(m.read(0x1000, 0x3000).unwrap(), before);
    }
}

/// Called only by the opt-in Present wrappers, once per successful Present.
pub(crate) fn present(device: u32) {
    let mut frames = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    let frame = frames.entry(device).or_default();
    let c = frame.finish();
    if frame.number % interval() != 0 {
        return;
    }
    eprintln!("ndraw stats frame={}: device={:#x} cmds={} draws={} (raw-records) by_type={:?} lists={} state_groups={} states={} shader_groups={} vs={} ps={} layouts={} samplers={} textures={} stream_groups={} buffers={} elements={} target_draws={:?} passes={:?} errors={:?}",
        frame.number, device, c.by_type.iter().sum::<u64>(), c.draws, c.by_type, c.lists,
        c.state_groups.len(), c.states.len(), c.shader_groups.len(), c.vs.len(), c.ps.len(),
        c.layouts.len(), c.samplers.len(), c.textures.len(), c.stream_groups.len(), c.buffers.len(), c.elements,
        c.targets, c.passes, c.errors);
}

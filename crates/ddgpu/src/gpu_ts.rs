//! GPU time measurement (`DDGPU_TIMESTAMPS=1`, off by default, one cached bool
//! check when off). A timestamp query is written between the command buffers
//! of every `vk::submit` (one start marker, then one marker after each `Ready`
//! run of passes); at each present the range is resolved, read back a few
//! frames later and printed once per second: GPU ms per frame (first to last
//! timestamp = span; sum of run times = busy; the rest is GPU idle between
//! submits) and the top runs by GPU ms grouped by render-target size.
//! Assumes one submitting thread (the render worker). Run times are the
//! delta between consecutive timestamps (bottom of pipe), so they include
//! the transitions that precede a run's passes.
use std::{collections::HashMap, sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, Arc, Mutex, OnceLock}, time::Instant};

const CAP: u32 = 8192;
const SLOTS: usize = 4;

pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("DDGPU_TIMESTAMPS").as_deref() == Ok("1"))
}

pub(crate) fn features() -> wgpu::Features {
    if enabled() { wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS } else { wgpu::Features::empty() }
}

struct Slot { buf: wgpu::Buffer, busy: Arc<AtomicBool> }
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    qs: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: Vec<Slot>,
    period_ns: f64,
}
#[derive(Default)]
struct Agg {
    since: Option<Instant>,
    frames: u64,
    span_ns: f64,
    busy_ns: f64,
    dropped: u64,
    by_label: HashMap<String, (f64, u64)>,
}
static GPU: OnceLock<Gpu> = OnceLock::new();
/// None = submit start marker, Some(label) = end of one run
static FRAME: Mutex<Vec<Option<String>>> = Mutex::new(Vec::new());
static AGG: Mutex<Option<Agg>> = Mutex::new(None);

pub(crate) fn init(device: &wgpu::Device, queue: &wgpu::Queue) {
    if !enabled() { return; }
    if !device.features().contains(features()) {
        eprintln!("gpu-ts: TIMESTAMP_QUERY unsupported on this device; disabled");
        return;
    }
    let qs = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("gpu-ts"), ty: wgpu::QueryType::Timestamp, count: CAP });
    let resolve = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu-ts resolve"), size: CAP as u64 * 8,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false,
    });
    let slots = (0..SLOTS).map(|_| Slot {
        buf: device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu-ts readback"), size: CAP as u64 * 8,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false,
        }),
        busy: Default::default(),
    }).collect();
    let period_ns = queue.get_timestamp_period() as f64;
    let _ = GPU.set(Gpu { device: device.clone(), queue: queue.clone(), qs, resolve, slots, period_ns });
    eprintln!("gpu-ts: enabled (period {period_ns} ns)");
}

fn stamp(g: &Gpu, label: Option<String>) -> Option<wgpu::CommandBuffer> {
    let mut f = FRAME.lock().unwrap_or_else(|e| e.into_inner());
    if f.len() as u32 >= CAP - 2 { return None; }
    let mut enc = g.device.create_command_encoder(&Default::default());
    enc.write_timestamp(&g.qs, f.len() as u32);
    f.push(label);
    Some(enc.finish())
}

/// Wrap the runs of one submit: `runs` = (label, command buffers).
pub(crate) fn wrap(runs: Vec<(String, Vec<wgpu::CommandBuffer>)>) -> Vec<wgpu::CommandBuffer> {
    let Some(g) = GPU.get() else { return runs.into_iter().flat_map(|r| r.1).collect() };
    let mut out = vec![];
    out.extend(stamp(g, None));
    for (label, commands) in runs {
        out.extend(commands);
        out.extend(stamp(g, Some(label)));
    }
    out
}

/// Call right after the present submit: resolve this frame's timestamps.
pub(crate) fn end_frame() {
    let Some(g) = (if enabled() { GPU.get() } else { None }) else { return };
    let entries = std::mem::take(&mut *FRAME.lock().unwrap_or_else(|e| e.into_inner()));
    if entries.len() < 2 { return; }
    static N: AtomicUsize = AtomicUsize::new(0);
    let slot = &g.slots[N.fetch_add(1, Ordering::Relaxed) % SLOTS];
    if slot.busy.swap(true, Ordering::AcqRel) {
        AGG.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Agg::default).dropped += 1;
        return;
    }
    let n = entries.len() as u32;
    let mut enc = g.device.create_command_encoder(&Default::default());
    enc.resolve_query_set(&g.qs, 0..n, &g.resolve, 0);
    enc.copy_buffer_to_buffer(&g.resolve, 0, &slot.buf, 0, n as u64 * 8);
    g.queue.submit([enc.finish()]);
    let buf = slot.buf.clone();
    let busy = slot.busy.clone();
    let period = g.period_ns;
    slot.buf.slice(..n as u64 * 8).map_async(wgpu::MapMode::Read, move |r| {
        if r.is_ok() {
            let ts: Vec<u64> = {
                let view = buf.slice(..n as u64 * 8).get_mapped_range();
                view.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect()
            };
            buf.unmap();
            digest(&entries, &ts, period);
        }
        busy.store(false, Ordering::Release);
    });
}

fn digest(entries: &[Option<String>], ts: &[u64], period: f64) {
    let mut guard = AGG.lock().unwrap_or_else(|e| e.into_inner());
    let a = guard.get_or_insert_with(Agg::default);
    let now = Instant::now();
    let since = *a.since.get_or_insert(now);
    a.frames += 1;
    a.span_ns += ts[ts.len() - 1].saturating_sub(ts[0]) as f64 * period;
    for k in 1..ts.len() {
        if let Some(label) = &entries[k] {
            let d = ts[k].saturating_sub(ts[k - 1]) as f64 * period;
            a.busy_ns += d;
            let e = a.by_label.entry(label.clone()).or_default();
            e.0 += d;
            e.1 += 1;
        }
    }
    if now.duration_since(since).as_secs_f64() >= 1.0 {
        let f = a.frames.max(1) as f64;
        let mut top: Vec<_> = a.by_label.iter().collect();
        top.sort_by(|x, y| y.1 .0.total_cmp(&x.1 .0));
        let top = top.iter().take(8).map(|(l, (ns, n))| format!("{l}: {:.2} ms ({:.1} runs)", ns / 1e6 / f, *n as f64 / f)).collect::<Vec<_>>().join(" | ");
        eprintln!("gpu-ts: {} frames, span {:.2} ms/frame, busy {:.2} ms/frame, dropped {} | {top}", a.frames, a.span_ns / 1e6 / f, a.busy_ns / 1e6 / f, a.dropped);
        *a = Agg::default();
    }
}

pub(crate) fn pass_label(extent: wgpu::Extent3d, colors: usize, depth: bool) -> String {
    format!("{}x{} c{}{}", extent.width, extent.height, colors, if depth { "d" } else { "" })
}

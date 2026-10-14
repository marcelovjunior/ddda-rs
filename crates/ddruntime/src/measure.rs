//! L192 measurement (`DDRUNTIME_MEASURE=1`, off by default; no behaviour change):
//! (a) inclusive time of the main-thread frame and its halves: the sim half
//! (frame entry to `sSceneExt::vf06` 0x0049B030, FRAME.md step 56) and the
//! production half (the rest), plus shadow 0xE277A0, sRender 0xDAF000, 0x4B9660
//! and the unit passes A/B/C; (b) the render worker's native-sink draws and
//! decode time per target size; (c) draw-for-draw stability of consecutive
//! frames' draw streams (state hash without the shader constants, and with).
//! One report every 240 frames on stderr.
#![cfg_attr(not(feature = "game"), allow(dead_code))]
use ddnative_base::{mem::Mem, x86::{Cpu, LiftFn}};
use std::cell::Cell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("DDRUNTIME_MEASURE").as_deref() == Ok("1"))
}

const ADDR: [u32; 8] = [0x41dd60, 0x49b030, 0xe277a0, 0xdaf000, 0x4b9660, 0xdb6740, 0xdb5700, 0xdb5690];
const NAME: [&str; 8] = ["frame", "scene(vf06 0x49B030)", "shadow(0xE277A0)", "sRender(0xDAF000)", "queue(0x4B9660)", "passA(0xDB6740)", "passB(0xDB5700)", "passC(0xDB5690)"];
static BODY: [OnceLock<LiftFn>; 8] = [const { OnceLock::new() }; 8];
static NS: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
static SIM_NS: AtomicU64 = AtomicU64::new(0);
static FRAMES: AtomicU64 = AtomicU64::new(0);
thread_local! { static T0: Cell<Option<Instant>> = const { Cell::new(None) }; }

pub(crate) fn install(ports: &mut Vec<(u32, LiftFn)>, bodies: &[(u32, LiftFn)]) {
    let table: [LiftFn; 8] = [w0, w1, w2, w3, w4, w5, w6, w7];
    for (i, addr) in ADDR.iter().enumerate() {
        if let Some(body) = ports.iter().chain(bodies).find(|(a, _)| a == addr).map(|(_, f)| *f) {
            if BODY[i].set(body).is_ok() {
                ports.retain(|(a, _)| a != addr);
                ports.push((*addr, table[i]));
            }
        }
    }
}

fn run(i: usize, m: &mut dyn Mem, c: &mut Cpu) {
    let t = Instant::now();
    if i == 0 {
        T0.with(|v| v.set(Some(t)));
    }
    if i == 1 {
        if let Some(t0) = T0.with(Cell::get) {
            SIM_NS.fetch_add(t.duration_since(t0).as_nanos() as u64, Relaxed);
        }
    }
    BODY[i].get().unwrap()(m, c);
    NS[i].fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
    if i == 0 && FRAMES.fetch_add(1, Relaxed) % 240 == 239 {
        report_main();
    }
}
fn w0(m: &mut dyn Mem, c: &mut Cpu) { run(0, m, c) }
fn w1(m: &mut dyn Mem, c: &mut Cpu) { run(1, m, c) }
fn w2(m: &mut dyn Mem, c: &mut Cpu) { run(2, m, c) }
fn w3(m: &mut dyn Mem, c: &mut Cpu) { run(3, m, c) }
fn w4(m: &mut dyn Mem, c: &mut Cpu) { run(4, m, c) }
fn w5(m: &mut dyn Mem, c: &mut Cpu) { run(5, m, c) }
fn w6(m: &mut dyn Mem, c: &mut Cpu) { run(6, m, c) }
fn w7(m: &mut dyn Mem, c: &mut Cpu) { run(7, m, c) }

fn report_main() {
    let frames = FRAMES.load(Relaxed).max(1) as f64;
    let ms = |ns: u64| ns as f64 / 1e6 / frames;
    let total = ms(NS[0].load(Relaxed));
    let sim = ms(SIM_NS.load(Relaxed));
    let mut line = format!("measure main (cumulative mean ms/frame, {frames} frames): frame {total:.2} = sim half {sim:.2} (entry..scene) + production half {:.2}", total - sim);
    for i in 1..8 {
        line += &format!(" | {} {:.2}", NAME[i], ms(NS[i].load(Relaxed)));
    }
    eprintln!("{line}");
    report_worker();
}

// ---- render worker ------------------------------------------------------------

#[derive(Default)]
struct Size { total: u64, taken: u64, ns: u64, same_pos: u64, same_both: u64, multiset: u64, compared: u64 }
#[derive(Default)]
struct Worker {
    sizes: HashMap<(u32, u32), Size>,
    cur: HashMap<(u32, u32), Vec<(u64, u64)>>,
    prev: HashMap<(u32, u32), Vec<(u64, u64)>>,
    frames: u64,
}
static WORKER: Mutex<Option<Worker>> = Mutex::new(None);
fn with<R>(f: impl FnOnce(&mut Worker) -> R) -> R {
    let mut g = WORKER.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Worker::default))
}

pub(crate) fn note_total(size: (u32, u32)) {
    with(|w| w.sizes.entry(size).or_default().total += 1);
}

pub(crate) fn note_time(size: (u32, u32), d: Duration) {
    with(|w| w.sizes.entry(size).or_default().ns += d.as_nanos() as u64);
}

fn shader_key(s: &Option<std::sync::Arc<[u32]>>) -> u64 {
    s.as_ref().map_or(0, |a| {
        let mut x = crate::fasthash::FxHasher::default();
        a.hash(&mut x);
        x.finish()
    })
}

/// A taken draw: its state key (everything but the shader constants) and its constants.
pub(crate) fn note_draw(size: (u32, u32), d: &d3d9cmd::DrawCall) {
    static STREAM: OnceLock<bool> = OnceLock::new();
    if !*STREAM.get_or_init(|| std::env::var("DDRUNTIME_MEASURE_STREAM").as_deref() == Ok("1")) {
        with(|w| w.sizes.entry(size).or_default().taken += 1);
        return;
    }
    // loose key: what the draw IS (shaders, declaration, primitive type and size, textures
    // by resource, index format); strict key: also stream buffers/offsets, states, targets, viewport
    let mut s = crate::fasthash::FxHasher::default();
    (d.primitive, d.primitives, d.vertex_count, d.fvf).hash(&mut s);
    (shader_key(&d.vertex_shader), shader_key(&d.pixel_shader)).hash(&mut s);
    d.declaration[..].hash(&mut s);
    for t in d.textures.iter() {
        (t.sampler, t.resource).hash(&mut s);
    }
    let shape = s.finish();
    let mut c = crate::fasthash::FxHasher::default();
    shape.hash(&mut c);
    (d.start, d.base_vertex, d.min_index, d.viewport, d.scissor).hash(&mut c);
    format!("{:?}{:?}{:?}{:?}{:?}{:?}", d.vertices, d.indices, d.textures, d.render_states, d.targets, d.depth).hash(&mut c);
    let consts = c.finish();
    with(|w| {
        w.sizes.entry(size).or_default().taken += 1;
        w.cur.entry(size).or_default().push((shape, consts));
    });
}

/// The worker begins a frame: compare the finished one with its predecessor.
pub(crate) fn frame_boundary() {
    with(|w| {
        w.frames += 1;
        let cur = std::mem::take(&mut w.cur);
        for (size, now) in &cur {
            if let Some(before) = w.prev.get(size) {
                let s = w.sizes.entry(*size).or_default();
                s.compared += now.len().max(before.len()) as u64;
                for (a, b) in now.iter().zip(before) {
                    s.same_pos += (a.0 == b.0) as u64;
                    s.same_both += (a == b) as u64;
                }
                let mut counts: HashMap<u64, u32> = HashMap::new();
                for b in before {
                    *counts.entry(b.0).or_default() += 1;
                }
                for a in now {
                    if let Some(n) = counts.get_mut(&a.0) {
                        if *n > 0 {
                            *n -= 1;
                            s.multiset += 1;
                        }
                    }
                }
            }
        }
        w.prev = cur;
    });
}

fn report_worker() {
    with(|w| {
        let frames = w.frames.max(1) as f64;
        let mut rows: Vec<_> = w.sizes.iter().filter(|(_, s)| s.total > 0).collect();
        rows.sort_by_key(|(_, s)| std::cmp::Reverse(s.total));
        let mut line = format!("measure worker ({} frames; per frame: draws seen / taken / sink-decode ms; vs previous frame: loose key at same index % / strict key at same index % / loose multiset %)", w.frames);
        for (size, s) in rows.iter().take(6) {
            let c = s.compared.max(1) as f64;
            line += &format!(
                " | {}x{}: {:.0} / {:.0} / {:.2} ms; {:.1}% / {:.1}% / {:.1}%",
                size.0, size.1, s.total as f64 / frames, s.taken as f64 / frames, s.ns as f64 / 1e6 / frames,
                s.same_pos as f64 / c * 100.0, s.same_both as f64 / c * 100.0, s.multiset as f64 / c * 100.0
            );
        }
        eprintln!("{line}");
    });
}

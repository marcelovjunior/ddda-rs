//! The quad dispatches (DDGPU_NATIVE_SIM, L113) against the draw path on a
//! captured session:
//!
//!     cargo run --release -p ddgpu --example sim_oracle -- capture.ddframe FIRST LAST
//!
//! Two backends replay the capture: the draw path, and one with
//! DDGPU_NATIVE_SIM=1. They get every upload, clear and release, and the
//! draws into float render-target textures of at most 64x64 (the soft-body
//! solver's passes, docs/SOFTBODY.md, which read only uploads and each
//! other); other draws are left out of both. At the end of each frame
//! FIRST..=LAST every such target is read back from both and compared bit
//! for bit. Prints the dispatches and differences per frame.
//!
//! `--bench N` then replays the last frame's small float draws N more times
//! into each backend (the same draw values, as the game's stable shader
//! objects give) and prints the time to record them and to run them.
//!
//! Memory is bounded: the capture's blobs stay in the file (an indexed
//! reader with a 256 MB cache), uploads before the first such draw are held
//! only as the latest per resource and level, and a watchdog ends the
//! process above 4 GB of private memory. The peak is printed at the end.
use d3d9cmd::{DrawCall, GraphicsBackend, Target, Upload};
use d3d9frame::{Command, Reader};
use std::collections::{BTreeMap, HashSet};

const F32X4: u32 = 116;
/// The cache of inflated blobs.
const CACHE: usize = 256 << 20;
/// The private memory the process may use.
const CAP: u64 = 4 << 30;

/// This process's private memory (bytes).
fn private_bytes() -> u64 {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        faults: u32,
        sizes: [usize; 8],
        private: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, size: u32) -> i32;
    }
    let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, ..Default::default() };
    // SAFETY: a plain Win32 query into a local struct of the size passed
    if unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) } == 0 {
        return 0;
    }
    c.private as u64
}

/// End the process when it uses more than CAP; the peak seen.
fn watchdog() -> std::sync::Arc<std::sync::atomic::AtomicU64> {
    let peak = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let p = peak.clone();
    std::thread::spawn(move || loop {
        let now = private_bytes();
        p.fetch_max(now, std::sync::atomic::Ordering::Relaxed);
        if now > CAP {
            eprintln!("sim_oracle: {} MB private memory, over the {} MB cap: stopping", now >> 20, CAP >> 20);
            std::process::exit(3);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    });
    peak
}

fn small_float(d: &DrawCall) -> bool {
    d.targets[0].as_ref().is_some_and(|t| t.format == F32X4 && t.texture.is_some() && t.width <= 64 && t.height <= 64)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("capture path");
    let first: usize = args.get(2).and_then(|a| a.parse().ok()).expect("first frame");
    let last: usize = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(first);
    let peak = watchdog();
    let bench: usize = args.iter().position(|a| a == "--bench").and_then(|k| args.get(k + 1)).and_then(|n| n.parse().ok()).unwrap_or(0);
    // the last frame's small float draws, for --bench
    let mut last_draws: Vec<DrawCall> = Vec::new();
    // the resources the small float draws touch (sampled, written, vertices)
    let mut used: HashSet<u32> = HashSet::new();
    let mut frame = 0;
    for c in Reader::open_indexed(path, CACHE).expect("open") {
        let c = c.expect("read");
        match &c {
            Command::Present(_) => {
                frame += 1;
                if frame > last {
                    break;
                }
            }
            Command::Draw(d) if small_float(d) => {
                used.extend(d.textures.iter().map(|t| t.resource));
                used.extend(d.targets.iter().flatten().filter_map(|t| t.texture.map(|x| x.resource)));
                if let d3d9cmd::Vertices::Streams(v) = &d.vertices {
                    used.extend(v.iter().map(|s| s.1));
                }
            }
            _ => {}
        }
    }
    eprintln!("{} resources", used.len());
    std::env::remove_var("DDGPU_NATIVE_SIM");
    let mut a = ddgpu::WgpuBackend::headless().expect("wgpu adapter");
    std::env::set_var("DDGPU_NATIVE_SIM", "1");
    let mut b = ddgpu::WgpuBackend::headless().expect("wgpu adapter");
    let backends: [&mut dyn GraphicsBackend; 2] = [&mut a, &mut b];
    let [a, b] = backends;
    let mut targets: BTreeMap<u32, Target> = BTreeMap::new();
    let mut frame = 0;
    let (mut draws, mut total_diff) = (0usize, 0usize);
    // uploads before the first small float draw: the latest per (resource, level)
    let mut held: BTreeMap<(u32, u32), Command> = BTreeMap::new();
    // time spent recording (apply) and waiting for the GPU, per backend, in
    // the compared frames
    let mut record = [std::time::Duration::ZERO; 2];
    let mut wait = [std::time::Duration::ZERO; 2];
    let mut started_drawing = false;
    let started = std::time::Instant::now();
    for c in Reader::open_indexed(path, CACHE).expect("open") {
        let c = c.expect("read");
        let keep = match &c {
            Command::Upload(Upload::Image { resource, .. } | Upload::Buffer { resource, .. }) => used.contains(resource),
            Command::Draw(d) => small_float(d),
            Command::Clear(cl) => cl.targets.iter().flatten().any(|t| t.texture.is_some_and(|x| used.contains(&x.resource))),
            Command::Release(r) => used.contains(r),
            Command::Present(_) => {
                if frame >= first {
                    let mut diff = 0;
                    for (r, t) in &targets {
                        let (x, y) = (a.read_back(t), b.read_back(t));
                        if x != y {
                            diff += 1;
                            if diff <= 5 {
                                let n = x.as_ref().zip(y.as_ref()).map_or(0, |(x, y)| x.chunks(4).zip(y.chunks(4)).filter(|(p, q)| p != q).count());
                                println!("  frame {frame}: tex{r:#x} {}x{} differs in {n} floats", t.width, t.height);
                            }
                        }
                    }
                    total_diff += diff;
                    println!("frame {frame}: {draws} small float draws, {} targets compared, {diff} differ; dispatches so far {}", targets.len(), b_dispatches(b));
                }
                if started_drawing {
                    for (k, x) in [&mut *a, &mut *b].into_iter().enumerate() {
                        if let Some(w) = (x as &mut dyn std::any::Any).downcast_mut::<ddgpu::WgpuBackend>() {
                            let t = std::time::Instant::now();
                            w.wait_idle();
                            if frame >= first {
                                wait[k] += t.elapsed();
                            }
                        }
                    }
                    if frame % 10 == 0 {
                        eprintln!("frame {frame}: {draws} small float draws, {:.0} s, {} MB", started.elapsed().as_secs_f64(), private_bytes() >> 20);
                    }
                }
                draws = 0;
                frame += 1;
                if frame > last {
                    break;
                }
                false
            }
            _ => false,
        };
        if !keep {
            continue;
        }
        if !started_drawing {
            match &c {
                Command::Upload(Upload::Image { resource, index, .. }) => {
                    held.insert((*resource, *index), c);
                    continue;
                }
                Command::Upload(Upload::Buffer { resource, .. }) => {
                    held.insert((*resource, u32::MAX), c);
                    continue;
                }
                Command::Release(r) => {
                    held.retain(|k, _| k.0 != *r);
                    continue;
                }
                Command::Draw(_) => {
                    started_drawing = true;
                    for h in std::mem::take(&mut held).into_values() {
                        h.apply(a);
                        h.apply(b);
                    }
                }
                _ => continue,
            }
        }
        if let Command::Draw(d) = &c {
            draws += 1;
            if frame == last && bench > 0 {
                last_draws.push(d.clone());
            }
            for t in d.targets.iter().flatten() {
                if let Some(x) = t.texture {
                    targets.insert(x.resource, Target { level: 0, ..t.clone() });
                }
            }
        }
        if let Command::Release(r) = &c {
            targets.remove(r);
        }
        let t = std::time::Instant::now();
        c.apply(a);
        let t1 = std::time::Instant::now();
        c.apply(b);
        if frame >= first {
            record[0] += t1 - t;
            record[1] += t1.elapsed();
        }
    }
    if let Some(w) = (b as &mut dyn std::any::Any).downcast_mut::<ddgpu::WgpuBackend>() {
        for e in w.errors.iter().take(10) {
            println!("native backend: {e}");
        }
    }
    if bench > 0 {
        let only = args.iter().position(|a| a == "--bench-only").and_then(|k| args.get(k + 1)).cloned();
        for (name, x) in [("draw path", &mut *a), ("quad dispatches", &mut *b)] {
            if only.as_ref().is_some_and(|o| !name.starts_with(o.as_str())) {
                continue;
            }
            let w = (x as &mut dyn std::any::Any).downcast_mut::<ddgpu::WgpuBackend>().expect("wgpu backend");
            w.wait_idle();
            eprintln!("bench start: {name}");
            let (mut rec, mut run) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
            for _ in 0..bench {
                let t = std::time::Instant::now();
                for d in &last_draws {
                    w.draw(d);
                }
                let t1 = std::time::Instant::now();
                w.wait_idle();
                rec += t1 - t;
                run += t1.elapsed();
            }
            println!("{name}: {} draws, record {:.2} ms, submit and run {:.2} ms per frame ({bench} frames)",
                last_draws.len(), rec.as_secs_f64() * 1e3 / bench as f64, run.as_secs_f64() * 1e3 / bench as f64);
        }
    }
    let n = (last + 1 - first) as f64;
    println!("per frame: draw path records {:.2} ms, waits {:.2} ms; quad dispatches record {:.2} ms, wait {:.2} ms",
        record[0].as_secs_f64() * 1e3 / n, wait[0].as_secs_f64() * 1e3 / n, record[1].as_secs_f64() * 1e3 / n, wait[1].as_secs_f64() * 1e3 / n);
    println!("{} frames compared, {total_diff} target differences, {:.1} s, peak private memory {} MB",
        last + 1 - first, started.elapsed().as_secs_f64(), peak.load(std::sync::atomic::Ordering::Relaxed).max(private_bytes()) >> 20);
}

fn b_dispatches(b: &mut dyn GraphicsBackend) -> u64 {
    (b as &mut dyn std::any::Any).downcast_mut::<ddgpu::WgpuBackend>().map_or(0, |w| w.quad_dispatches())
}

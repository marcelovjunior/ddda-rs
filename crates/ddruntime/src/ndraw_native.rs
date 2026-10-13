//! The native path through nDraw's command executor (L119, step N2 of
//! docs/NATIVE_RENDERER.md): a `mtdraw::sink::CommandSink` installed over
//! the executor port.
//!
//! `DDDRAW_NATIVE=census` installs a sink that takes no command (every one
//! runs the default D3D9 path, unchanged) and measures what a native path
//! would take: per frame, the commands by type, the draws into the frame's
//! largest colour target (the main scene passes) and into other targets,
//! and the distinct state, shader-binding, stream and target objects.
//! It prints a summary to stderr every `DDDRAW_NATIVE_EVERY` seconds
//! (default 10). Native drawing itself comes with the native device
//! (L110) and the decoded state objects (L117).
//!
//! Layouts read (docs/NDRAW.md, sections 2 and 9): a draw record's +4 is
//! its target binding (+0..+0xC the colour views, +0x10 the depth view); a
//! view's +0x18 is its width (u16) and +0x1A its height (u16, the top bit
//! an sRGB flag).

use crate::fasthash::{FxMap, FxSet};
use ddnative_base::mem::Mem;
use mtdraw::sink::{CommandSink, Taken};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What `DDDRAW_NATIVE` asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    /// count, take nothing
    Census,
}

pub fn mode() -> Mode {
    match std::env::var("DDDRAW_NATIVE").as_deref() {
        Ok("census") => Mode::Census,
        Ok("1") => {
            // the native draw path is not here yet: measure instead
            eprintln!("DDDRAW_NATIVE=1: native drawing is not implemented yet; counting (census) instead");
            Mode::Census
        }
        _ => Mode::Off,
    }
}

/// Install the sink `mode()` asks for (before the program runs).
pub fn install() {
    match mode() {
        Mode::Off => {}
        Mode::Census => {
            let every = std::env::var("DDDRAW_NATIVE_EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(10u64);
            mtdraw::sink::set_sink(Some(Arc::new(Census::new(Duration::from_secs(every.max(1))))));
        }
    }
}

/// One frame's counts.
#[derive(Default)]
struct Frame {
    kinds: [u64; 16],
    /// draws (types 0..3) by their first colour target's size
    draws_by_size: FxMap<(u32, u32), u64>,
    states: FxSet<u32>,
    shaders: FxSet<u32>,
    streams: FxSet<u32>,
    targets: FxSet<u32>,
}

/// Totals since the last report.
#[derive(Default)]
struct Totals {
    frames: u64,
    kinds: [u64; 16],
    scene_draws: u64,
    other_draws: u64,
    states: u64,
    shaders: u64,
    streams: u64,
    targets: u64,
    sizes: FxMap<(u32, u32), u64>,
}

struct Census {
    frame: Mutex<(Frame, Totals, Instant)>,
    every: Duration,
}

impl Census {
    fn new(every: Duration) -> Self {
        Census { frame: Mutex::new((Frame::default(), Totals::default(), Instant::now())), every }
    }
}

/// The first colour target's (width, height) of a draw record; (0, 0)
/// without one.
fn target_size(m: &dyn Mem, record: u32) -> (u32, u32) {
    let binding = m.u32(record.wrapping_add(4));
    if binding == 0 {
        return (0, 0);
    }
    let view = m.u32(binding);
    if view == 0 {
        return (0, 0);
    }
    (m.u16(view.wrapping_add(0x18)) as u32, m.u16(view.wrapping_add(0x1A)) as u32 & 0x7FFF)
}

impl CommandSink for Census {
    fn command(&self, m: &mut dyn Mem, _context: u32, record: u32, kind: u32) -> Taken {
        let mut g = self.frame.lock().unwrap_or_else(|e| e.into_inner());
        let f = &mut g.0;
        f.kinds[kind as usize & 15] += 1;
        if kind <= 3 {
            *f.draws_by_size.entry(target_size(m, record)).or_default() += 1;
            f.targets.insert(m.u32(record.wrapping_add(4)));
            f.states.insert(m.u32(record.wrapping_add(8)));
            f.shaders.insert(m.u32(record.wrapping_add(0xC)));
            f.streams.insert(m.u32(record.wrapping_add(0x10)));
        }
        Taken::Default
    }

    fn frame_end(&self, _m: &mut dyn Mem, _render: u32, _context: u32) {
        let mut g = self.frame.lock().unwrap_or_else(|e| e.into_inner());
        let (f, t, since) = &mut *g;
        let f = std::mem::take(f);
        t.frames += 1;
        for k in 0..16 {
            t.kinds[k] += f.kinds[k];
        }
        // the main scene passes: the frame's largest colour target
        let scene = f.draws_by_size.keys().copied().max_by_key(|&(w, h)| w as u64 * h as u64);
        for (&size, &n) in &f.draws_by_size {
            if Some(size) == scene {
                t.scene_draws += n;
            } else {
                t.other_draws += n;
            }
            *t.sizes.entry(size).or_default() += n;
        }
        t.states += f.states.len() as u64;
        t.shaders += f.shaders.len() as u64;
        t.streams += f.streams.len() as u64;
        t.targets += f.targets.len() as u64;
        if since.elapsed() >= self.every {
            report(t);
            *t = Totals::default();
            *since = Instant::now();
        }
    }
}

fn report(t: &Totals) {
    if t.frames == 0 {
        return;
    }
    let per = |n: u64| n as f64 / t.frames as f64;
    let names = ["draw", "indexed", "instanced", "instanced indexed", "clear", "copy", "nested", "span"];
    let mut out = format!("ndraw native census: {} frames, per frame:\n", t.frames);
    for (k, name) in names.iter().enumerate() {
        out += &format!("  {name:>18}: {:8.1}\n", per(t.kinds[k]));
    }
    let other: u64 = t.kinds[8..].iter().sum();
    out += &format!("  {:>18}: {:8.1}\n", "types 8-15", per(other));
    out += &format!("  draws into the largest colour target {:.1}, into others {:.1}\n", per(t.scene_draws), per(t.other_draws));
    out += &format!("  distinct objects: states {:.1}, shader bindings {:.1}, stream bindings {:.1}, target bindings {:.1}\n",
                    per(t.states), per(t.shaders), per(t.streams), per(t.targets));
    let mut sizes: Vec<_> = t.sizes.iter().collect();
    sizes.sort_by(|a, b| b.1.cmp(a.1));
    out += "  draws by first colour target size:";
    for ((w, h), n) in sizes.into_iter().take(8) {
        out += &format!(" {w}x{h} {:.1};", per(*n));
    }
    eprintln!("{out}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct TestMem(HashMap<u32, u8>);

    impl Mem for TestMem {
        fn u8(&self, a: u32) -> u8 {
            self.0.get(&a).copied().unwrap_or(0)
        }
        fn set_u8(&mut self, a: u32, v: u8) {
            self.0.insert(a, v);
        }
    }

    /// A draw record with a target binding whose first view is w x h
    /// (the height's top bit set, as an sRGB view has it).
    fn record(m: &mut TestMem, at: u32, (w, h): (u16, u16), state: u32) -> u32 {
        let (binding, view) = (at + 0x100, at + 0x200);
        m.set_u32(at + 4, binding);
        m.set_u32(at + 8, state);
        m.set_u32(binding, view);
        m.set_u16(view + 0x18, w);
        m.set_u16(view + 0x1A, h | 0x8000);
        at
    }

    #[test]
    fn the_census_takes_nothing_and_counts_scene_draws() {
        let mut m = TestMem::default();
        let scene = record(&mut m, 0x1000, (1920, 1080), 0x50);
        let shadow = record(&mut m, 0x2000, (2048, 512), 0x60);
        let small = record(&mut m, 0x3000, (480, 270), 0x50);
        assert_eq!(target_size(&m, scene), (1920, 1080));
        let census = Census::new(Duration::from_secs(3600));
        for (r, kind) in [(scene, 1), (scene, 1), (shadow, 0), (small, 3), (scene, 4)] {
            assert_eq!(census.command(&mut m, 0, r, kind), Taken::Default);
        }
        census.frame_end(&mut m, 0, 0);
        let g = census.frame.lock().unwrap();
        let t = &g.1;
        assert_eq!(t.frames, 1);
        assert_eq!((t.kinds[0], t.kinds[1], t.kinds[3], t.kinds[4]), (1, 2, 1, 1));
        // the largest colour target here is 1920x1080: two indexed draws
        assert_eq!((t.scene_draws, t.other_draws), (2, 2));
        assert_eq!(t.states, 2);
    }
}

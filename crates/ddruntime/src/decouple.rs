//! L193 decoupled-render experiments (all off by default, no behaviour change unless set).
//!
//! Stage 1 `DDRUNTIME_FIXED_TICK=<hz>` (60 or 30): after `sMain::vf06` 0x00DBFA90 (the clock,
//! FRAME.md step 6) the step `mDeltaTime` (+0x70) is overwritten with the constant
//! `30/hz x mGlobalSpeed` (base frames are 1/30 s), so every simulated frame advances exactly one
//! tick whatever the wall time. `DDRUNTIME_FIXED_TICK_PACE=1` also holds each frame to `1/hz` s.
//! The original frame cap/spin inside vf06 still runs (mMaxFps 150).
//!
//! Stage 2 `DDRUNTIME_SIM_EVERY=<n>` (needs the fixed tick): only every n-th displayed frame
//! simulates. The others use the original's own capture pause (`sMain+0x6c` mPause = 1 with the
//! frame-step counter `+0x68` = 0): the unit passes A/B/C, sAI, sAreaHit, sWorkRate,
//! sRecognition, sObjCollision and the main body of sGameSys::vf06 return at once (FRAME.md
//! section 3), everything else (input, effects, camera, sound, weather, network, and the whole
//! production half: scene, shadow, sRender, draw jobs) still runs on the unchanged sim state.
#![cfg_attr(not(feature = "game"), allow(dead_code))]
use ddnative_base::{mem::Mem, x86::{Cpu, LiftFn}};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const CLOCK: u32 = 0x00DB_FA90;
const SMAIN: u32 = 0x018D_0B28;
static BODY: OnceLock<LiftFn> = OnceLock::new();

struct Cfg { dt: f32, interval: Duration, pace: bool, every: u64 }
fn cfg() -> &'static Option<Cfg> {
    static C: OnceLock<Option<Cfg>> = OnceLock::new();
    C.get_or_init(|| {
        let hz: f32 = std::env::var("DDRUNTIME_FIXED_TICK").ok()?.parse().ok().filter(|h: &f32| *h > 0.0)?;
        let every = std::env::var("DDRUNTIME_SIM_EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(1u64).max(1);
        let pace = std::env::var("DDRUNTIME_FIXED_TICK_PACE").as_deref() == Ok("1");
        eprintln!("decouple: fixed tick {hz} Hz (mDeltaTime {} base frames), sim every {every} displayed frame(s), pace {pace}", 30.0 / hz);
        Some(Cfg { dt: 30.0 / hz, interval: Duration::from_secs_f64(1.0 / hz as f64), pace, every })
    })
}

pub(crate) fn enabled() -> bool {
    cfg().is_some()
}

pub(crate) fn install(ports: &mut Vec<(u32, LiftFn)>, bodies: &[(u32, LiftFn)]) {
    if let Some(body) = ports.iter().chain(bodies).find(|(a, _)| *a == CLOCK).map(|(_, f)| *f) {
        if BODY.set(body).is_ok() {
            ports.retain(|(a, _)| *a != CLOCK);
            ports.push((CLOCK, clock as LiftFn));
        }
    }
    gates_install(ports, bodies);
}

// ---- stage 2b: gates on the non-simulating frames (DDRUNTIME_GATE, DDRUNTIME_GATE_MEASURE) -------------
// `DDRUNTIME_GATE=all|name,name,...` makes these sim-half functions return at once (callee pops its
// arguments) on frames that do not simulate; `DDRUNTIME_GATE_MEASURE=1` times every gate candidate on the
// frames where it runs (works without any gating: a plain per-function inclusive-cost table).
struct Gate { name: &'static str, addr: u32, pop: u32 }
const GATES: [Gate; 14] = [
    Gate { name: "effect", addr: 0x00FB5E00, pop: 0 },   // sEffect::vf09 (step 11)
    Gate { name: "prim", addr: 0x00E60D40, pop: 0 },     // sPrimitive::vf09 (15)
    Gate { name: "gpupart9", addr: 0x00EB26C0, pop: 0 }, // sGpuParticle::vf09 (15)
    Gate { name: "gui", addr: 0x00DE11A0, pop: 0 },      // sGUI::vf10 (17)
    Gate { name: "area", addr: 0x00419910, pop: 0 },     // sAreaExt::vf06 (20)
    Gate { name: "zone", addr: 0x00F74590, pop: 0 },     // sZone::vf06 (28)
    Gate { name: "item", addr: 0x0045CF60, pop: 0 },     // sItemManager::vf06 (29)
    Gate { name: "msg", addr: 0x00455280, pop: 0 },      // sIdMessage::vf06 (45)
    Gate { name: "cockpit", addr: 0x00451F80, pop: 0 },  // sIdCockpit::vf06 (45)
    Gate { name: "camera", addr: 0x00420910, pop: 0 },   // sCameraExt::vf06 (51)
    Gate { name: "sound", addr: 0x004AD440, pop: 0 },    // sSoundExt::vf06 (52)
    Gate { name: "weather", addr: 0x004C0C70, pop: 0 },  // sWeatherManager::vf06 (53)
    Gate { name: "net", addr: 0x00467F50, pop: 0 },      // sNetworkExt::vf06 (54)
    Gate { name: "softbody", addr: 0x00F20640, pop: 4 }, // uSimSoftBody::vf11 (cloth/hair passes)
];
static GATE_BODY: [OnceLock<LiftFn>; 14] = [const { OnceLock::new() }; 14];
static GATE_NS: [AtomicU64; 14] = [const { AtomicU64::new(0) }; 14];
static GATE_CALLS: [AtomicU64; 14] = [const { AtomicU64::new(0) }; 14];
static GATE_SKIPPED: [AtomicU64; 14] = [const { AtomicU64::new(0) }; 14];
static SKIP_NOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn gate_on(i: usize) -> bool {
    static ON: OnceLock<[bool; 14]> = OnceLock::new();
    ON.get_or_init(|| {
        let v = std::env::var("DDRUNTIME_GATE").unwrap_or_default();
        std::array::from_fn(|k| GATES[k].name != "sound" && (v == "all" || v.split(',').any(|n| n == GATES[k].name)))
    })[i]
}
/// DDRUNTIME_GATE_MEASURE=1|name,name: time (and install a wrapper on) all or only the named gates.
fn measure_on(i: usize) -> bool {
    static ON: OnceLock<[bool; 14]> = OnceLock::new();
    ON.get_or_init(|| {
        let v = std::env::var("DDRUNTIME_GATE_MEASURE").unwrap_or_default();
        std::array::from_fn(|k| v == "1" || v.split(',').any(|n| n == GATES[k].name))
    })[i]
}
fn gate(i: usize, m: &mut dyn Mem, c: &mut Cpu) {
    if SKIP_NOW.load(Relaxed) && gate_on(i) {
        GATE_SKIPPED[i].fetch_add(1, Relaxed);
        c.esp = c.esp.wrapping_add(4 + GATES[i].pop);
        return;
    }
    if measure_on(i) {
        let t = Instant::now();
        GATE_BODY[i].get().unwrap()(m, c);
        GATE_NS[i].fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
        GATE_CALLS[i].fetch_add(1, Relaxed);
    } else {
        GATE_BODY[i].get().unwrap()(m, c);
    }
}
macro_rules! gate_fns { ($($n:ident $i:expr),*) => { $(fn $n(m: &mut dyn Mem, c: &mut Cpu) { gate($i, m, c) })* } }
gate_fns!(g0 0, g1 1, g2 2, g3 3, g4 4, g5 5, g6 6, g7 7, g8 8, g9 9, g10 10, g11 11, g12 12, g13 13);
fn gates_install(ports: &mut Vec<(u32, LiftFn)>, bodies: &[(u32, LiftFn)]) {
    let any = (0..GATES.len()).any(|i| gate_on(i) || measure_on(i));
    if !any || cfg().is_none() {
        return;
    }
    let table: [LiftFn; 14] = [g0, g1, g2, g3, g4, g5, g6, g7, g8, g9, g10, g11, g12, g13];
    for (i, g) in GATES.iter().enumerate() {
        if !gate_on(i) && !measure_on(i) {
            continue;
        }
        if let Some(body) = ports.iter().chain(bodies).find(|(a, _)| *a == g.addr).map(|(_, f)| *f) {
            if GATE_BODY[i].set(body).is_ok() {
                ports.retain(|(a, _)| *a != g.addr);
                ports.push((g.addr, table[i]));
            }
        }
    }
}

// ---- forced full frames ---------------------------------------------------------------------------
// A frame always simulates when the game is not in free play: no player yet (boot, title, loading), an
// event running (sEventManager +0x20), the unit group mask's bit 57 cleared (events and menus freeze the
// world with it, FRAME.md 3), a large allowed step (mDeltaTimeLimite +0x78 > 2.5: the 16.0 states of
// step 4), and for HOLD frames after any of these (area changes and fades follow).
const HOLD: u64 = 240;
static FULL_UNTIL: AtomicU64 = AtomicU64::new(0);
static FORCED: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
fn force_full(m: &dyn Mem, sm: u32, n: u64) -> bool {
    let pm = m.u32(0x018F_A4EC);
    let last = crate::movies::LAST_SAMPLE_MS.load(Relaxed);
    let movie = last != 0 && (crate::services::runtime_clock().as_millis() as u64 + 1).saturating_sub(last) < 1000;
    let r = if movie {
        5
    } else if pm == 0 || m.u32(pm + 0x99C) == 0 {
        0
    } else if { let em = m.u32(0x018F_A4B8); em != 0 && m.u32(em + 0x20) != 0 } {
        1
    } else if { let su = m.u32(0x018D_09E0); su != 0 && m.u32(su + 0xD34) & (1 << 25) == 0 } {
        2
    } else if f32at(m, sm + 0x78) > 2.5 {
        3
    } else {
        4
    };
    if r < 4 || r == 5 {
        FORCED[r].fetch_add(1, Relaxed);
        FULL_UNTIL.store(n + HOLD, Relaxed);
        return true;
    }
    if n < FULL_UNTIL.load(Relaxed) {
        FORCED[4].fetch_add(1, Relaxed);
        return true;
    }
    false
}

fn gate_report(frames: u64) {
    let f = frames.max(1) as f64;
    let mut line = format!("decouple gates (per displayed frame over {frames} frames; ms inclusive / calls / skipped):");
    for (i, g) in GATES.iter().enumerate() {
        let c = GATE_CALLS[i].load(Relaxed);
        if c > 0 || GATE_SKIPPED[i].load(Relaxed) > 0 {
            line += &format!(" {} {:.3}/{:.2}/{}", g.name, GATE_NS[i].load(Relaxed) as f64 / 1e6 / f, c as f64 / f, GATE_SKIPPED[i].load(Relaxed));
        }
    }
    line += &format!(" | forced-full frames: noplayer {} event {} mask57 {} limite {} hold {} movie {}", FORCED[0].load(Relaxed), FORCED[1].load(Relaxed), FORCED[2].load(Relaxed), FORCED[3].load(Relaxed), FORCED[4].load(Relaxed), FORCED[5].load(Relaxed));
    eprintln!("{line}");
}

static FRAME: AtomicU64 = AtomicU64::new(0);
static SIMS: AtomicU64 = AtomicU64::new(0);
static LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

fn force_full_off() -> bool {
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("DDRUNTIME_FORCE_FULL").as_deref() == Ok("0"))
}

fn f32at(m: &dyn Mem, a: u32) -> f32 {
    f32::from_bits(m.u32(a))
}

fn clock(m: &mut dyn Mem, c: &mut Cpu) {
    BODY.get().unwrap()(m, c);
    let Some(cfg) = cfg() else { return };
    let sm = m.u32(SMAIN);
    if sm == 0 {
        return;
    }
    let n = FRAME.fetch_add(1, Relaxed);
    let sim = n % cfg.every == 0 || (cfg.every > 1 && !force_full_off() && force_full(m, sm, n));
    if n % 600 == 599 {
        static T: std::sync::Mutex<Option<(Instant, u64)>> = std::sync::Mutex::new(None);
        let mut g = T.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if let Some((t, f)) = *g {
            let secs = now.duration_since(t).as_secs_f64();
            eprintln!("decouple: {:.1} displayed fps, {:.1} sim ticks/s, dt {:.3}, game speed {:.2}x (sims total {})", (n - f) as f64 / secs, (n - f) as f64 / secs / cfg.every as f64, cfg.dt, (n - f) as f64 / secs / cfg.every as f64 * cfg.dt as f64 / 30.0, SIMS.load(Relaxed));
        }
        *g = Some((now, n));
        gate_report(n);
    }
    if cfg.pace {
        let mut g = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = *g {
            let due = t + cfg.interval;
            let now = Instant::now();
            if due > now + Duration::from_millis(2) {
                std::thread::sleep(due - now - Duration::from_millis(2));
            }
            while Instant::now() < due {
                std::hint::spin_loop();
            }
        }
        *g = Some(Instant::now());
    }
    // mDeltaTime: constant tick (a paused sMain, dt 0, stays 0)
    if f32at(m, sm + 0x70) > 0.0 {
        m.set_u32(sm + 0x70, (cfg.dt * f32at(m, sm + 0x80)).to_bits());
    }
    if cfg.every > 1 {
        SKIP_NOW.store(!sim, Relaxed);
        if sim {
            m.set_u8(sm + 0x6c, 0);
            SIMS.fetch_add(1, Relaxed);
        } else {
            m.set_u32(sm + 0x68, 0);
            m.set_u8(sm + 0x6c, 1);
        }
    }
}

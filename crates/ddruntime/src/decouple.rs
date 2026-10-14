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
}

static FRAME: AtomicU64 = AtomicU64::new(0);
static SIMS: AtomicU64 = AtomicU64::new(0);
static LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

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
    let sim = n % cfg.every == 0;
    if n % 600 == 599 {
        static T: std::sync::Mutex<Option<(Instant, u64)>> = std::sync::Mutex::new(None);
        let mut g = T.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if let Some((t, f)) = *g {
            let secs = now.duration_since(t).as_secs_f64();
            eprintln!("decouple: {:.1} displayed fps, {:.1} sim ticks/s, dt {:.3}, game speed {:.2}x (sims total {})", (n - f) as f64 / secs, (n - f) as f64 / secs / cfg.every as f64, cfg.dt, (n - f) as f64 / secs / cfg.every as f64 * cfg.dt as f64 / 30.0, SIMS.load(Relaxed));
        }
        *g = Some((now, n));
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
        if sim {
            m.set_u8(sm + 0x6c, 0);
            SIMS.fetch_add(1, Relaxed);
        } else {
            m.set_u32(sm + 0x68, 0);
            m.set_u8(sm + 0x6c, 1);
        }
    }
}

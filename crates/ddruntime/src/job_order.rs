//! L172 frame-bound experiment: the shadow job (uSkyStableShadow, vtable
//! 0x0160DF98, callback DB7620) is the LAST entry of the frame's job batch
//! (L167: last in 150/150 frames, 3.9 ms/frame alone at the join), so it
//! starts only after the other jobs were claimed. `DDRUNTIME_JOB_SHADOW_FIRST=1`
//! moves it to the front of a mode-1 batch before DBE560 runs (the entries
//! between shift down by one, their order is kept). Jobs of a batch run
//! concurrently on all participants, so no job can rely on running after another.
#![cfg_attr(not(feature = "game"), allow(dead_code))]
use ddnative_base::{mem::Mem, x86::{Cpu, LiftFn}};
use std::sync::OnceLock;

static BODY: OnceLock<LiftFn> = OnceLock::new();
const SHADOW_VTABLE: u32 = 0x0160_df98;

pub(crate) fn enabled() -> bool {
    std::env::var("DDRUNTIME_JOB_SHADOW_FIRST").as_deref() == Ok("1") || stats()
}

/// `DDRUNTIME_JOB_STATS=1`: time every DBE560 batch by queue size (count/total us).
fn stats() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("DDRUNTIME_JOB_STATS").as_deref() == Ok("1"))
}
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
// [mode 1..3][bucket 0..7] -> (n, us)
static BUCKETS: [[[AtomicU64; 2]; 8]; 4] = [const { [const { [const { AtomicU64::new(0) }; 2] }; 8] }; 4];
static TOTAL: AtomicU64 = AtomicU64::new(0);
fn bucket(n: u32) -> usize { match n { 0 => 0, 1 => 1, 2..=3 => 2, 4..=7 => 3, 8..=31 => 4, 32..=127 => 5, 128..=511 => 6, _ => 7 } }

pub(crate) fn install(ports: &mut Vec<(u32, LiftFn)>, bodies: &[(u32, LiftFn)]) {
    let body = ports.iter().chain(bodies).find(|(a, _)| *a == 0xdbe560).map(|(_, f)| *f).expect("scheduler body");
    BODY.set(body).ok();
    ports.retain(|(a, _)| *a != 0xdbe560);
    ports.push((0xdbe560, batch));
}

fn batch(m: &mut dyn Mem, cpu: &mut Cpu) {
    if m.u32(cpu.esp + 4) == 1 {
        let scheduler = cpu.ecx;
        let count = m.u32(scheduler + 0x8c).min(8192);
        let base = scheduler + 0xc0;
        for i in 1..count {
            let object = m.u32(base + i * 8);
            if object != 0 && m.probe(object, 4, false).is_ok() && m.u32(object) == SHADOW_VTABLE {
                let moved = (m.u32(base + i * 8), m.u32(base + i * 8 + 4));
                for k in (0..i).rev() {
                    let (a, b) = (m.u32(base + k * 8), m.u32(base + k * 8 + 4));
                    m.set_u32(base + (k + 1) * 8, a);
                    m.set_u32(base + (k + 1) * 8 + 4, b);
                }
                m.set_u32(base, moved.0);
                m.set_u32(base + 4, moved.1);
                static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 3 {
                    eprintln!("job-order: shadow job moved from {i} of {count}");
                }
                break;
            }
        }
    }
    if stats() {
        let mode = m.u32(cpu.esp + 4).min(3) as usize;
        let count = m.u32(cpu.ecx + 0x8c);
        let nested = m.u8(0x018d0bb0) != 0;
        let start = std::time::Instant::now();
        BODY.get().unwrap()(m, cpu);
        if !nested {
            let c = &BUCKETS[mode][bucket(count)];
            c[0].fetch_add(1, Relaxed);
            c[1].fetch_add(start.elapsed().as_micros() as u64, Relaxed);
        }
        if TOTAL.fetch_add(1, Relaxed) % 8192 == 8191 {
            let mut line = String::from("job-stats [mode/bucket(0,1,2-3,4-7,8-31,32-127,128-511,512+): n us] (non-nested)");
            for (mo, row) in BUCKETS.iter().enumerate() {
                for (b, c) in row.iter().enumerate() {
                    if c[0].load(Relaxed) > 0 {
                        line += &format!(" | {mo}/{b}: {} {}", c[0].load(Relaxed), c[1].load(Relaxed));
                    }
                }
            }
            eprintln!("{line}");
        }
        return;
    }
    BODY.get().unwrap()(m, cpu);
}

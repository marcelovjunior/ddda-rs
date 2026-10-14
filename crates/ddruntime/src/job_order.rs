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
    std::env::var("DDRUNTIME_JOB_SHADOW_FIRST").as_deref() == Ok("1")
}

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
                break;
            }
        }
    }
    BODY.get().unwrap()(m, cpu);
}

//! Opt-in creation events. Frame 0 is before the first backend Present.
//! Phase boundaries are supplied by the run operator, never guessed from FPS.
use std::sync::{OnceLock, atomic::{AtomicU64, Ordering}};
use std::time::Instant;

static FRAME: AtomicU64 = AtomicU64::new(0);
static CONFIG: OnceLock<Option<Vec<(u64, String)>>> = OnceLock::new();

fn config() -> &'static Option<Vec<(u64, String)>> {
    CONFIG.get_or_init(|| {
        (std::env::var("DDGPU_PIPELINE_LOG").as_deref() == Ok("1")).then(|| {
            boundaries(&std::env::var("DDGPU_PIPELINE_PHASES").unwrap_or_default())
        })
    })
}

fn boundaries(value: &str) -> Vec<(u64, String)> {
    let mut phases = vec![(0, "boot".into()), (1, "unknown".into())];
    for item in value.split(',') {
        let Some((frame, phase)) = item.split_once(':') else { continue };
        if !matches!(phase, "boot" | "title" | "menu" | "loading" | "gameplay") { continue; }
        if let Ok(frame) = frame.parse() { phases.push((frame, phase.into())); }
    }
    phases.sort_by_key(|p| p.0);
    phases
}

pub(crate) fn present() { FRAME.fetch_add(1, Ordering::Relaxed); }

pub(crate) fn timed<T>(kind: &str, cache: &str, work: impl FnOnce() -> T) -> T {
    let Some(_) = config() else { return work() };
    let started = Instant::now();
    let result = work();
    event(kind, cache, started);
    result
}

pub(crate) fn event(kind: &str, cache: &str, started: Instant) {
    let Some(phases) = config() else { return };
    let frame = FRAME.load(Ordering::Relaxed);
    let phase = &phases.iter().rev().find(|p| p.0 <= frame).unwrap().1;
    let thread = std::thread::current();
    eprintln!("DDGPU_PIPELINE kind={kind} cache={cache} frame={frame} phase={phase} ms={:.6} thread={:?}",
        started.elapsed().as_secs_f64() * 1000.0, thread.name().unwrap_or("unnamed"));
}

#[cfg(test)]
mod tests {
    #[test]
    fn explicit_boundaries_allow_repeated_phases() {
        let p = super::boundaries("10:title,20:menu,30:loading,40:gameplay,50:loading,bad,1:invalid");
        assert_eq!(p.len(), 7);
        assert_eq!(p.iter().rev().find(|p| p.0 <= 35).unwrap().1, "loading");
        assert_eq!(p.iter().rev().find(|p| p.0 <= 2).unwrap().1, "unknown");
    }
}

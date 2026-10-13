//! L170: which services the guest threads call, how often, from where and
//! for how long, and how often a thread's `call_ext` binding cache misses.
//! With `DDRUNTIME_SERVICE_CENSUS=1` a reporter thread prints, every
//! `DDRUNTIME_SERVICE_CENSUS_EVERY` seconds (default 10), the calls per
//! second of the busiest (service, return address) pairs over that
//! interval, their time inside the service (the dispatch prologue
//! included), and the calls per guest thread. Off, it costs one relaxed
//! load per service call.
use crate::fasthash::FxMap;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 0: not read yet, 1: off, 2: on.
static ON: AtomicU8 = AtomicU8::new(0);
static MISSES: AtomicU64 = AtomicU64::new(0);

/// Calls and nanoseconds.
type Counts = (u64, u64);

#[derive(Default)]
struct Totals {
    /// by (service, return address)
    sites: FxMap<(u32, u32), Counts>,
    /// by guest thread id
    threads: FxMap<u32, Counts>,
}

fn global() -> &'static Mutex<Totals> {
    static T: OnceLock<Mutex<Totals>> = OnceLock::new();
    T.get_or_init(Default::default)
}

/// Service names for the report (the registry's), set at install.
fn names() -> &'static OnceLock<std::collections::HashMap<u32, String>> {
    static N: OnceLock<std::collections::HashMap<u32, String>> = OnceLock::new();
    &N
}

thread_local! {
    /// this thread's counts since its last merge into the global table
    static LOCAL: RefCell<(Totals, u32)> = RefCell::new((Totals::default(), 0));
}

#[inline]
pub(crate) fn on() -> bool {
    match ON.load(Relaxed) {
        0 => start(),
        v => v == 2,
    }
}

#[cold]
fn start() -> bool {
    let on = std::env::var("DDRUNTIME_SERVICE_CENSUS").is_ok_and(|v| v == "1");
    if ON.compare_exchange(0, if on { 2 } else { 1 }, Relaxed, Relaxed).is_ok() && on {
        let every = std::env::var("DDRUNTIME_SERVICE_CENSUS_EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(10u64).max(1);
        let _ = std::thread::Builder::new().name("service-census".into()).spawn(move || report_every(Duration::from_secs(every)));
    }
    ON.load(Relaxed) == 2
}

/// The registry's service names, for the report.
pub(crate) fn set_names(n: &std::collections::HashMap<u32, String>) {
    if on() {
        let _ = names().set(n.clone());
    }
}

/// A miss of a thread's `call_ext` binding cache.
pub(crate) fn miss() {
    MISSES.fetch_add(1, Relaxed);
}

/// Run service `target` (called from `return_to`), counted.
pub(crate) fn service(target: u32, return_to: u32, run: impl FnOnce()) {
    let start = Instant::now();
    run();
    let ns = start.elapsed().as_nanos() as u64;
    let tid = crate::threads::current_tid();
    LOCAL.with(|l| {
        let mut l = l.borrow_mut();
        let (t, n) = &mut *l;
        add(t.sites.entry((target, return_to)).or_default(), ns);
        add(t.threads.entry(tid).or_default(), ns);
        *n += 1;
        if *n >= 4096 {
            *n = 0;
            merge(std::mem::take(t));
        }
    });
}

fn add(c: &mut Counts, ns: u64) {
    c.0 += 1;
    c.1 += ns;
}

fn merge(t: Totals) {
    let mut g = global().lock().unwrap_or_else(|e| e.into_inner());
    for (k, v) in t.sites {
        let c = g.sites.entry(k).or_default();
        c.0 += v.0;
        c.1 += v.1;
    }
    for (k, v) in t.threads {
        let c = g.threads.entry(k).or_default();
        c.0 += v.0;
        c.1 += v.1;
    }
}

fn report_every(every: Duration) {
    let mut since = Instant::now();
    let mut misses = MISSES.load(Relaxed);
    loop {
        std::thread::sleep(every);
        let t = std::mem::take(&mut *global().lock().unwrap_or_else(|e| e.into_inner()));
        let secs = since.elapsed().as_secs_f64();
        since = Instant::now();
        let m = MISSES.load(Relaxed);
        eprintln!("{}", report(&t, secs, m - misses, names().get()));
        misses = m;
    }
}

/// The report of `t` over `secs` seconds.
fn report(t: &Totals, secs: f64, misses: u64, names: Option<&std::collections::HashMap<u32, String>>) -> String {
    let name = |a: u32| names.and_then(|n| n.get(&a)).cloned().unwrap_or_else(|| format!("{a:#010x}"));
    let calls: u64 = t.sites.values().map(|c| c.0).sum();
    let ns: u64 = t.sites.values().map(|c| c.1).sum();
    let mut out = format!(
        "service census over {secs:.1} s: {:.0} service calls/s, {:.1} ms/s inside services, {:.0} call_ext cache misses/s\n",
        calls as f64 / secs,
        ns as f64 / 1e6 / secs,
        misses as f64 / secs
    );
    let mut by_service: FxMap<u32, Counts> = FxMap::default();
    for (&(s, _), c) in &t.sites {
        let e = by_service.entry(s).or_default();
        e.0 += c.0;
        e.1 += c.1;
    }
    let mut services: Vec<_> = by_service.into_iter().collect();
    services.sort_by_key(|(_, c)| std::cmp::Reverse(c.1));
    out += "  by service (time):\n";
    for (s, c) in services.iter().take(30) {
        out += &format!("    {:>10.0}/s {:>8.2} ms/s {:>7.0} ns/call  {}\n", c.0 as f64 / secs, c.1 as f64 / 1e6 / secs, c.1 as f64 / c.0 as f64, name(*s));
    }
    let mut sites: Vec<_> = t.sites.iter().collect();
    sites.sort_by_key(|(_, c)| std::cmp::Reverse(c.0));
    out += "  by call site (calls):\n";
    for ((s, r), c) in sites.iter().take(40) {
        out += &format!("    {:>10.0}/s {:>8.2} ms/s  {} from {r:#010x}\n", c.0 as f64 / secs, c.1 as f64 / 1e6 / secs, name(*s));
    }
    let mut threads: Vec<_> = t.threads.iter().collect();
    threads.sort_by_key(|(_, c)| std::cmp::Reverse(c.0));
    out += "  by guest thread (calls):\n";
    for (tid, c) in threads.iter().take(16) {
        out += &format!("    {:>10.0}/s {:>8.2} ms/s  tid {tid:#x}\n", c.0 as f64 / secs, c.1 as f64 / 1e6 / secs);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_ranks_services_sites_and_threads() {
        let mut t = Totals::default();
        t.sites.insert((0x100, 0x401000), (300, 3_000_000));
        t.sites.insert((0x100, 0x402000), (100, 1_000_000));
        t.sites.insert((0x200, 0x403000), (1000, 500_000));
        t.threads.insert(0x1070, (1400, 4_500_000));
        let names = std::collections::HashMap::from([(0x100, "KERNEL32.dll!Sleep".to_string())]);
        let r = report(&t, 2.0, 50, Some(&names));
        assert!(r.contains("700 service calls/s, 2.2 ms/s inside services, 25 call_ext cache misses/s"), "{r}");
        // by time: Sleep (4 ms) before 0x200 (0.5 ms); by calls: 0x200 first
        let by_service = r.split("by call site").next().unwrap();
        assert!(by_service.find("Sleep").unwrap() < by_service.find("0x00000200").unwrap(), "{r}");
        let by_site = r.split("by call site").nth(1).unwrap();
        assert!(by_site.find("0x00000200 from 0x00403000").unwrap() < by_site.find("Sleep from 0x00401000").unwrap(), "{r}");
        assert!(r.contains("10000 ns/call  KERNEL32.dll!Sleep"), "{r}");
        assert!(r.contains("tid 0x1070"), "{r}");
    }

    #[test]
    fn a_threads_calls_reach_the_table_every_4096() {
        let mut ran = 0;
        for _ in 0..4096 {
            service(0x77, 0x88, || ran += 1);
        }
        assert_eq!(ran, 4096);
        assert_eq!(global().lock().unwrap().sites[&(0x77, 0x88)].0, 4096);
    }

    #[test]
    fn merged_counts_add_up() {
        let mut a = Totals::default();
        a.sites.insert((1, 2), (3, 4));
        a.threads.insert(9, (3, 4));
        merge(a);
        let mut b = Totals::default();
        b.sites.insert((1, 2), (1, 1));
        merge(b);
        let g = global().lock().unwrap();
        assert_eq!(g.sites[&(1, 2)], (4, 5));
        assert_eq!(g.threads[&9], (3, 4));
    }
}

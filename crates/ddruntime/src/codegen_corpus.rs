//! Bounded, opt-in real-call inputs for the offline codegen yardstick.
//! Depends on shadow_fee320's shared recorder (711b9576). No game behavior
//! replacement: the unmodified lifted body executes once for every call.
//! Until complete local/lifted seams are installed, some captures will be
//! rejected by the offline replay. Capture timings are never FPS evidence.
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use std::{
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
        OnceLock,
    },
};

struct Target {
    entry: u32,
    body: LiftFn,
    calls: AtomicUsize,
}
struct Config {
    directory: PathBuf,
    limit: usize,
    targets: Vec<Target>,
}
static CONFIG: OnceLock<Config> = OnceLock::new();
static ARMED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
struct Active;
impl Drop for Active {
    fn drop(&mut self) {
        ACTIVE.store(false, Relaxed);
    }
}
struct Host(u32);
impl Host {
    fn enter() -> Self {
        let saved = x86::stmxcsr();
        ddnative_base::x87::park();
        x86::ldmxcsr(x86::MXCSR_DEFAULT);
        Self(saved)
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        x86::ldmxcsr(self.0);
    }
}

const WRAPPERS: [LiftFn; 50] = [
    capture::<0>,
    capture::<1>,
    capture::<2>,
    capture::<3>,
    capture::<4>,
    capture::<5>,
    capture::<6>,
    capture::<7>,
    capture::<8>,
    capture::<9>,
    capture::<10>,
    capture::<11>,
    capture::<12>,
    capture::<13>,
    capture::<14>,
    capture::<15>,
    capture::<16>,
    capture::<17>,
    capture::<18>,
    capture::<19>,
    capture::<20>,
    capture::<21>,
    capture::<22>,
    capture::<23>,
    capture::<24>,
    capture::<25>,
    capture::<26>,
    capture::<27>,
    capture::<28>,
    capture::<29>,
    capture::<30>,
    capture::<31>,
    capture::<32>,
    capture::<33>,
    capture::<34>,
    capture::<35>,
    capture::<36>,
    capture::<37>,
    capture::<38>,
    capture::<39>,
    capture::<40>,
    capture::<41>,
    capture::<42>,
    capture::<43>,
    capture::<44>,
    capture::<45>,
    capture::<46>,
    capture::<47>,
    capture::<48>,
    capture::<49>,
];

fn entries(text: &str) -> Vec<u32> {
    let values: Vec<_> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).expect("corpus hexadecimal entry"))
        .collect();
    assert!(
        !values.is_empty() && values.len() <= 50,
        "corpus needs 1..50 entries"
    );
    let mut unique = values.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), values.len(), "duplicate corpus entries");
    values
}

/// Called before dispatch freezes, beside other diagnostic installations.
/// No environment setting means no wrappers and no monitoring thread.
pub(crate) fn install(ports: &mut Vec<(u32, LiftFn)>, bodies: &[(u32, LiftFn)]) {
    let Some(directory) = std::env::var_os("DDRUNTIME_CORPUS_DIR") else {
        return;
    };
    let directory = PathBuf::from(directory);
    assert!(
        directory.is_absolute(),
        "private absolute corpus directory required"
    );
    let list = std::env::var_os("DDRUNTIME_CORPUS_LIST").expect("corpus entry list path");
    let addresses = entries(&std::fs::read_to_string(list).expect("read corpus entry list"));
    let limit = std::env::var("DDRUNTIME_CORPUS_CALLS")
        .map(|s| s.parse::<usize>().expect("corpus call count"))
        .unwrap_or(2);
    assert!((1..=16).contains(&limit), "diagnostic call budget must be 1..16");
    std::fs::create_dir_all(&directory).expect("create private corpus directory");
    let start = directory.join("START");
    assert!(!start.exists(), "use a fresh corpus directory; arm after loading");
    let targets = addresses
        .iter()
        .map(|&entry| {
            assert!(
                !ports.iter().any(|(a, _)| *a == entry),
                "corpus entry {entry:x} conflicts with an override"
            );
            let body = bodies
                .iter()
                .find(|(a, _)| *a == entry)
                .map(|(_, f)| *f)
                .expect("corpus original lifted entry");
            Target {
                entry,
                body,
                calls: AtomicUsize::new(0),
            }
        })
        .collect();
    assert!(
        CONFIG
            .set(Config {
                directory,
                limit,
                targets
            })
            .is_ok(),
        "corpus installed once"
    );
    for (i, entry) in addresses.into_iter().enumerate() {
        ports.push((entry, WRAPPERS[i]));
    }
    std::thread::Builder::new()
        .name("corpus-arm".into())
        .spawn(move || {
            while !start.exists() {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            ARMED.store(true, Relaxed);
        })
        .expect("start corpus marker monitor");
}

fn capture<const INDEX: usize>(m: &mut dyn Mem, c: &mut Cpu) {
    let config = CONFIG.get().expect("corpus installed");
    let target = &config.targets[INDEX];
    if !ARMED.load(Relaxed)
        || target.calls.load(Relaxed) >= config.limit
        || crate::shadow_fee320::capturing()
        || ACTIVE.compare_exchange(false, true, Relaxed, Relaxed).is_err()
    {
        (target.body)(m, c);
        return;
    }
    let _active = Active;
    let number = target.calls.fetch_add(1, Relaxed);
    let (call, panic) = crate::shadow_fee320::capture_call(target.body, m, c);
    {
        let _host = Host::enter();
        let tid = crate::threads::current_tid();
        let path = config
            .directory
            .join(format!("corpus-{:08x}-{tid:08x}-{number}.json", target.entry));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("new corpus record");
        let origin = if cfg!(test) {
            "synthetic-lift"
        } else {
            "runtime-lift"
        };
        write!(file,"{{\"version\":1,\"origin\":\"{origin}\",\"entry\":{},\"tid\":{tid},\"capture\":true,\"panicked\":{},\"overflow\":false,\"records\":[",target.entry,panic.is_some()).unwrap();
        crate::shadow_fee320::write_call(&mut file, &call).expect("write corpus call");
        writeln!(file, "]}}").unwrap();
    }
    if let Some(panic) = panic {
        std::panic::resume_unwind(panic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn addresses_are_bounded_and_unique() {
        assert_eq!(entries("0xfee320, 1054700\n"), [0xfee320, 0x1054700]);
        for text in ["", "fee320 fee320", "not-an-address"] {
            assert!(std::panic::catch_unwind(|| entries(text)).is_err());
        }
        assert!(std::panic::catch_unwind(|| entries(
            &(0..51).map(|x| format!("{x:x}")).collect::<Vec<_>>().join(" ")
        ))
        .is_err());
    }
    #[test]
    #[ignore = "standalone capture checker supplies private artifacts and isolates global configuration"]
    fn bounded_capture_executes_original_once_per_call() {
        struct Memory(u32);
        impl Mem for Memory {
            fn u8(&self, a: u32) -> u8 {
                assert!((0x1000..0x1004).contains(&a));
                self.0.to_le_bytes()[(a - 0x1000) as usize]
            }
            fn set_u8(&mut self, a: u32, b: u8) {
                assert!((0x1000..0x1004).contains(&a));
                let mut bytes = self.0.to_le_bytes();
                bytes[(a - 0x1000) as usize] = b;
                self.0 = u32::from_le_bytes(bytes);
            }
        }
        fn body(m: &mut dyn Mem, c: &mut Cpu) {
            c.eax = m.u32(0x1000) + 1;
            m.set_u32(0x1000, c.eax);
        }
        ddnative_host::install();
        let root =
            PathBuf::from(std::env::var_os("DDNATIVE_CORPUS_SMOKE_DIR").expect("private smoke directory"));
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = root.join(format!("smoke-{nonce}"));
        std::fs::create_dir_all(&directory).unwrap();
        let list = directory.join("entries.txt");
        std::fs::write(&list, "fee320\n").unwrap();
        std::env::set_var("DDRUNTIME_CORPUS_DIR", &directory);
        std::env::set_var("DDRUNTIME_CORPUS_LIST", &list);
        std::env::set_var("DDRUNTIME_CORPUS_CALLS", "2");
        let mut ports = vec![];
        install(&mut ports, &[(0xfee320, body)]);
        assert_eq!(ports.len(), 1);
        x86::set_ext_calls(Some(Box::new(|_, _, _| panic!("unexpected synthetic callee"))));
        let mut m = Memory(0);
        let mut cpu = Cpu::default();
        ports[0].1(&mut m, &mut cpu);
        assert_eq!(m.0, 1);
        assert_eq!(CONFIG.get().unwrap().targets[0].calls.load(Relaxed), 0);
        std::fs::write(directory.join("START"), "").unwrap();
        ARMED.store(true, Relaxed);
        for _ in 0..3 {
            ports[0].1(&mut m, &mut cpu);
        }
        assert_eq!(m.0, 4);
        assert_eq!(cpu.eax, 4);
        assert_eq!(CONFIG.get().unwrap().targets[0].calls.load(Relaxed), 2);
        let records: Vec<_> = std::fs::read_dir(&directory)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
            .collect();
        assert_eq!(records.len(), 2);
        for record in records {
            let text = std::fs::read_to_string(record.path()).unwrap();
            assert!(text.contains("\"origin\":\"synthetic-lift\""));
            assert!(text.contains("\"panicked\":false"));
        }
        x86::set_ext_calls(None);
        for name in [
            "DDRUNTIME_CORPUS_DIR",
            "DDRUNTIME_CORPUS_LIST",
            "DDRUNTIME_CORPUS_CALLS",
        ] {
            std::env::remove_var(name);
        }
    }
}

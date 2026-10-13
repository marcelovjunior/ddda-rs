//! rustc_gate: a RUSTC_WRAPPER that limits how many ddlift shard compilers run
//! at once by memory, not by cargo's job count (L75).
//!
//! A shard rustc peaks at 2-3 GB, so a cold play build cannot run 16 of them.
//! Cargo's `-j` limits both the number of rustc processes and the jobserver
//! tokens that rustc's threads (`-Z threads`, LLVM codegen units) need, so a
//! memory-safe `-j 8` also leaves half the CPU idle. With this wrapper cargo
//! runs with `-j 16` (all hardware threads as tokens), and the wrapper holds back
//! shard compilers beyond `DDDA_GATE_SHARDS` (default 6). A held-back shard
//! lends its job token to the pool while it waits, so the running compilers'
//! threads can use it, and takes a token back before it starts. Opt-level 1
//! shards (the slowest) are admitted before opt-level 0 ones. A shard also
//! waits while less than `DDDA_GATE_FREE_MB` (default 3500) of RAM is free.
//! Everything else runs straight through.
//!
//! Build: `rustc -O scripts/rustc_gate.rs -o <dir>/rustc_gate.exe`
//! (scripts/build_play.sh does this). Use: `RUSTC_WRAPPER=<dir>/rustc_gate.exe`.
//! Cargo does not fingerprint the wrapper, so it does not trigger rebuilds.

use std::ffi::{c_void, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::process::{exit, Command};
use std::time::Duration;
use std::{env, fs, thread};

type Handle = *mut c_void;
const SEMAPHORE_ALL_ACCESS: u32 = 0x1F0003;
const WAIT_OBJECT_0: u32 = 0;
const INFINITE: u32 = 0xFFFF_FFFF;

#[repr(C)]
struct MemoryStatusEx {
    length: u32,
    memory_load: u32,
    total_phys: u64,
    avail_phys: u64,
    total_page_file: u64,
    avail_page_file: u64,
    total_virtual: u64,
    avail_virtual: u64,
    avail_extended_virtual: u64,
}

#[link(name = "kernel32")]
extern "system" {
    fn OpenSemaphoreW(access: u32, inherit: i32, name: *const u16) -> Handle;
    fn CreateSemaphoreW(attrs: *mut c_void, initial: i32, maximum: i32, name: *const u16) -> Handle;
    fn ReleaseSemaphore(sem: Handle, count: i32, previous: *mut i32) -> i32;
    fn WaitForSingleObject(handle: Handle, millis: u32) -> u32;
    fn GlobalMemoryStatusEx(status: *mut MemoryStatusEx) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
}

/// Whether process `pid` still runs (a queued opt-level 1 shard's gate).
fn alive(pid: u32) -> bool {
    const SYNCHRONIZE: u32 = 0x0010_0000;
    let h = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
    if h.is_null() {
        return false;
    }
    let running = unsafe { WaitForSingleObject(h, 0) } != WAIT_OBJECT_0;
    unsafe { CloseHandle(h) };
    running
}

/// Whether an opt-level 1 shard is queued (stale markers of dead gates are removed).
fn optimised_waiting(marker: &std::path::Path) -> bool {
    let Ok(dir) = fs::read_dir(marker) else { return false };
    let mut any = false;
    for e in dir.flatten() {
        let pid = e.file_name().to_string_lossy().strip_prefix("o1-").and_then(|p| p.parse().ok());
        match pid {
            Some(pid) if alive(pid) => any = true,
            _ => {
                let _ = fs::remove_file(e.path());
            }
        }
    }
    any
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn free_mb() -> u64 {
    let mut m = MemoryStatusEx { length: std::mem::size_of::<MemoryStatusEx>() as u32, memory_load: 0, total_phys: 0,
        avail_phys: 0, total_page_file: 0, avail_page_file: 0, total_virtual: 0, avail_virtual: 0, avail_extended_virtual: 0 };
    if unsafe { GlobalMemoryStatusEx(&mut m) } == 0 {
        return u64::MAX;
    }
    m.avail_phys >> 20
}

/// Appends an event to `DDDA_GATE_LOG`, if set.
fn log(event: &str) {
    if let Ok(path) = env::var("DDDA_GATE_LOG") {
        use std::io::Write;
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{t:.1} {event}");
        }
    }
}

fn env_num(name: &str, default: u64) -> u64 {
    env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Cargo's jobserver (a named semaphore on Windows), from CARGO_MAKEFLAGS.
fn jobserver() -> Option<Handle> {
    let flags = env::var("CARGO_MAKEFLAGS").ok()?;
    let name = flags.split_whitespace().find_map(|a| a.strip_prefix("--jobserver-auth="))?;
    let h = unsafe { OpenSemaphoreW(SEMAPHORE_ALL_ACCESS, 0, wide(name).as_ptr()) };
    (!h.is_null()).then_some(h)
}

fn run(args: &[String]) -> ! {
    let status = Command::new(&args[0]).args(&args[1..]).status().unwrap_or_else(|e| {
        eprintln!("rustc_gate: cannot run {}: {e}", args[0]);
        exit(1)
    });
    exit(status.code().unwrap_or(1))
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: RUSTC_WRAPPER=rustc_gate.exe cargo ...");
        exit(2);
    }
    let shard = args.windows(2).any(|w| w[0] == "--crate-name" && w[1].starts_with("ddlift_"));
    if !shard {
        run(&args);
    }
    let optimised = args.windows(2).any(|w| w[0] == "-C" && w[1].starts_with("opt-level=") && w[1] != "opt-level=0");
    let slots = env_num("DDDA_GATE_SHARDS", 6).max(1) as i32;
    let min_free = env_num("DDDA_GATE_FREE_MB", 3500);
    // one gate per target directory, so separate builds do not share slots
    let key: u32 = env::var("CARGO_TARGET_DIR").unwrap_or_default().bytes()
        .fold(2166136261u32, |h, b| (h ^ b.to_ascii_lowercase() as u32).wrapping_mul(16777619));
    let gate = unsafe { CreateSemaphoreW(std::ptr::null_mut(), slots, slots, wide(&format!("ddda-rustc-gate-{key:08x}")).as_ptr()) };
    if gate.is_null() {
        run(&args);
    }
    // lend this job's token while waiting: release one, take one back before starting
    let js = jobserver();
    let lent = js.is_some_and(|h| unsafe { ReleaseSemaphore(h, 1, std::ptr::null_mut()) } != 0);
    // opt-level 1 shards first: an opt-level 0 shard waits while one is queued
    let marker = env::temp_dir().join(format!("ddda-rustc-gate-{key:08x}"));
    let mine = marker.join(format!("o1-{}", std::process::id()));
    if optimised {
        let _ = fs::create_dir_all(&marker);
        let _ = fs::write(&mine, b"");
    } else {
        // cargo starts ready units together: give their opt-level 1 gates time to queue
        thread::sleep(Duration::from_secs(2));
    }
    loop {
        if !optimised && optimised_waiting(&marker) {
            thread::sleep(Duration::from_millis(500));
            continue;
        }
        if unsafe { WaitForSingleObject(gate, 500) } != WAIT_OBJECT_0 {
            continue;
        }
        if free_mb() >= min_free {
            break;
        }
        unsafe { ReleaseSemaphore(gate, 1, std::ptr::null_mut()) };
        thread::sleep(Duration::from_secs(2));
    }
    if optimised {
        let _ = fs::remove_file(&mine);
    }
    if lent {
        unsafe { WaitForSingleObject(js.unwrap(), INFINITE) };
    }
    let name = args.windows(2).find(|w| w[0] == "--crate-name").map_or("?", |w| w[1].as_str());
    log(&format!("start {name} o{} lent={lent} free={}MB", optimised as u8, free_mb()));
    let status = Command::new(&args[0]).args(&args[1..]).status();
    unsafe { ReleaseSemaphore(gate, 1, std::ptr::null_mut()) };
    log(&format!("end {name}"));
    match status {
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("rustc_gate: cannot run {}: {e}", args[0]);
            exit(1)
        }
    }
}

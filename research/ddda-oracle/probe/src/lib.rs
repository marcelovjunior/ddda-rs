//! ddda-oracle: an in-process probe for the running DDDA.exe.
//!
//! Loaded with `ddda-inject` (LoadLibraryW through CreateRemoteThread).
//! On load it hooks the game's per-frame call (main thread), D3D9 Present,
//! and opens a JSON command server on 127.0.0.1:47800. The `unload` command
//! removes every hook and patch and frees the DLL. See docs/ORACLE.md.

#![cfg(target_arch = "x86")]

mod addr;
mod capture;
mod guard;
mod hooks;
mod input;
mod patch;
mod server;

use std::collections::VecDeque;
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HINSTANCE};
use windows_sys::Win32::System::LibraryLoader::FreeLibraryAndExitThread;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::{CreateMutexW, CreateThread, GetCurrentProcessId, Sleep};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

static HINST: AtomicU32 = AtomicU32::new(0);
static MUTEX: AtomicU32 = AtomicU32::new(0);
pub static STOP: AtomicBool = AtomicBool::new(false);
static UNLOADING: AtomicBool = AtomicBool::new(false);
static LOG: Mutex<Option<File>> = Mutex::new(None);
static T0: OnceLock<Instant> = OnceLock::new();

pub fn hinst() -> u32 {
    HINST.load(Ordering::SeqCst)
}

pub fn log(s: &str) {
    let t = T0.get_or_init(Instant::now).elapsed().as_secs_f64();
    if let Ok(mut g) = LOG.lock() {
        if let Some(f) = g.as_mut() {
            let _ = writeln!(f, "[{t:9.3}] {s}");
            let _ = f.flush();
        }
    }
}

// ------------------------------------------------------------ main-thread jobs

type JobFn = Box<dyn FnOnce() -> Value + Send>;
struct Job {
    f: JobFn,
    tx: Sender<Value>,
    cancelled: Arc<AtomicBool>,
}
static JOBS: Mutex<VecDeque<Job>> = Mutex::new(VecDeque::new());

/// Runs queued jobs. Called from the frame and idle hooks (main thread).
pub fn tick(_frame: bool) {
    loop {
        let job = match JOBS.lock() {
            Ok(mut q) => q.pop_front(),
            Err(_) => None,
        };
        let Some(job) = job else { break };
        if job.cancelled.load(Ordering::SeqCst) {
            continue;
        }
        let v = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job.f))
            .unwrap_or_else(|_| json!({"ok": false, "error": "panic in main-thread job"}));
        let _ = job.tx.send(v);
    }
}

/// Runs `f` on the game's main thread at the next tick and waits for it.
pub fn on_main(f: impl FnOnce() -> Value + Send + 'static, timeout: Duration) -> Result<Value, String> {
    if UNLOADING.load(Ordering::SeqCst) {
        return Err("unloading".into());
    }
    let (tx, rx) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    JOBS.lock().unwrap().push_back(Job { f: Box::new(f), tx, cancelled: cancelled.clone() });
    match rx.recv_timeout(timeout) {
        Ok(v) => Ok(v),
        Err(_) => {
            cancelled.store(true, Ordering::SeqCst);
            Err(format!("main thread did not run the job within {timeout:?}"))
        }
    }
}

fn enqueue(f: impl FnOnce() -> Value + Send + 'static) {
    let (tx, _rx) = mpsc::channel();
    JOBS.lock().unwrap().push_back(Job { f: Box::new(f), tx, cancelled: Arc::new(AtomicBool::new(false)) });
}

// ------------------------------------------------------------ load / unload

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn free_self() -> ! {
    FreeLibraryAndExitThread(HINST.load(Ordering::SeqCst) as HINSTANCE, 0)
}

unsafe extern "system" fn init_thread(_: *mut c_void) -> u32 {
    T0.get_or_init(Instant::now);
    let dir = std::env::temp_dir().join("ddda-oracle");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(dir.join("oracle.log")) {
        *LOG.lock().unwrap() = Some(f);
    }
    log(&format!("ddda-oracle {VERSION} loaded at {:#x} in pid {}", hinst(), GetCurrentProcessId()));

    let m = CreateMutexW(core::ptr::null(), 0, wide(&format!("Local\\ddda-oracle-{}", GetCurrentProcessId())).as_ptr());
    if GetLastError() == ERROR_ALREADY_EXISTS {
        log("another oracle instance is loaded; unloading this one");
        CloseHandle(m);
        LOG.lock().unwrap().take();
        free_self();
    }
    MUTEX.store(m as u32, Ordering::SeqCst);

    guard::install();
    if let Err(e) = hooks::install_present_hook() {
        log(&format!("present hook not installed: {e}"));
    }
    if let Err(e) = hooks::install_frame_hook() {
        log(&format!("frame hook failed: {e}; unloading"));
        hooks::remove_present_hook();
        guard::uninstall();
        CloseHandle(m);
        LOG.lock().unwrap().take();
        free_self();
    }
    // Patches that the main thread executes are applied from the main thread.
    enqueue(|| unsafe {
        match hooks::install_cursor_gate() {
            Ok(()) => {
                if hooks::BG_WANTED.load(Ordering::SeqCst) {
                    if let Err(e) = hooks::set_bg(true) {
                        log(&format!("bg: {e}"));
                    }
                }
            }
            Err(e) => log(&format!("cursor gate failed: {e}; background mode stays off")),
        }
        json!({})
    });
    server::start();
    0
}

/// Main-thread part of unloading: undo everything the game executes.
unsafe fn teardown_main() {
    input::release_all();
    let _ = hooks::set_bg(false);
    patch::revert_all();
    hooks::remove_present_hook();
    hooks::remove_frame_hook();
}

unsafe extern "system" fn reaper(_: *mut c_void) -> u32 {
    let t0 = Instant::now();
    while hooks::PRESENT_INFLIGHT.load(Ordering::SeqCst) != 0 && t0.elapsed() < Duration::from_secs(3) {
        Sleep(5);
    }
    Sleep(300);
    server::join();
    // Jobs left in the queue are dropped (their callers get an error).
    JOBS.lock().unwrap().clear();
    guard::uninstall();
    let m = MUTEX.swap(0, Ordering::SeqCst);
    if m != 0 {
        CloseHandle(m as HANDLE);
    }
    log("unloaded");
    LOG.lock().unwrap().take();
    free_self();
}

pub fn unload() -> Result<Value, String> {
    if UNLOADING.swap(true, Ordering::SeqCst) {
        return Err("already unloading".into());
    }
    log("unload requested");
    // Re-open the gate for this one job (on_main refuses while UNLOADING).
    let (tx, rx) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    JOBS.lock().unwrap().push_back(Job {
        f: Box::new(|| {
            unsafe { teardown_main() };
            json!({"teardown": "main"})
        }),
        tx,
        cancelled: cancelled.clone(),
    });
    let how = match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(_) => "main thread",
        Err(_) => {
            cancelled.store(true, Ordering::SeqCst);
            // The main thread is not ticking (so it is not inside any hooked
            // code path either); undo from here.
            log("main thread did not tick; tearing down from the server thread");
            unsafe { teardown_main() };
            "server thread"
        }
    };
    STOP.store(true, Ordering::SeqCst);
    unsafe {
        let h = CreateThread(core::ptr::null(), 0, Some(reaper), core::ptr::null(), 0, core::ptr::null_mut());
        if !h.is_null() {
            CloseHandle(h);
        }
    }
    Ok(json!({"unloading": true, "teardown": how}))
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "system" fn DllMain(h: HINSTANCE, reason: u32, _: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        HINST.store(h as u32, Ordering::SeqCst);
        let t = CreateThread(core::ptr::null(), 0, Some(init_thread), core::ptr::null(), 0, core::ptr::null_mut());
        if !t.is_null() {
            CloseHandle(t);
        }
    }
    1
}

//! Loads the oracle DLL into the running DDDA.exe.
//!
//! Usage: `ddda-inject [--dll PATH] [--pid N]`
//!
//! The DLL is first copied to `%TEMP%\ddda-oracle\ddda_oracle_<ms>.dll`, so
//! the build output is never locked by the game and every injection loads a
//! fresh image. Then `LoadLibraryW(copy)` runs in the game through
//! `CreateRemoteThread`. This is a 32-bit program, so the `LoadLibraryW`
//! address of its own kernel32 is valid in the (32-bit) game as well.
//!
//! Unloading is done by the DLL itself (the `unload` command), because only
//! the DLL knows when its hooks are gone.
//!
//! Prints one JSON line: `{"pid":..,"module":"0x..","dll":".."}`.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::{env, fs, ptr};

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows_sys::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject, PROCESS_CREATE_THREAD,
    PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn find_pid(name: &str) -> Option<u32> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap as isize == -1 {
            return None;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                if String::from_utf16_lossy(&e.szExeFile[..n]).eq_ignore_ascii_case(name) {
                    found = Some(e.th32ProcessID);
                    break;
                }
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
        found
    }
}

fn fail(msg: String) -> ! {
    eprintln!("ddda-inject: {msg}");
    std::process::exit(1);
}

fn main() {
    let mut dll: Option<PathBuf> = None;
    let mut pid: Option<u32> = None;
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dll" => dll = args.next().map(PathBuf::from),
            "--pid" => pid = args.next().and_then(|s| s.parse().ok()),
            _ => fail(format!("unknown argument {a}; usage: ddda-inject [--dll PATH] [--pid N]")),
        }
    }
    let dll = dll.unwrap_or_else(|| env::current_exe().unwrap().with_file_name("ddda_oracle.dll"));
    if !dll.is_file() {
        fail(format!("{} not found", dll.display()));
    }
    let pid = pid.or_else(|| find_pid("DDDA.exe")).unwrap_or_else(|| fail("DDDA.exe is not running".into()));

    let dir = env::temp_dir().join("ddda-oracle");
    fs::create_dir_all(&dir).unwrap_or_else(|e| fail(format!("mkdir {}: {e}", dir.display())));
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
    let copy = dir.join(format!("ddda_oracle_{ms}.dll"));
    fs::copy(&dll, &copy).unwrap_or_else(|e| fail(format!("copy to {}: {e}", copy.display())));
    let path = wide(copy.as_os_str());

    unsafe {
        let access = PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION
            | PROCESS_VM_WRITE | PROCESS_VM_READ;
        let h = OpenProcess(access, 0, pid);
        if h.is_null() {
            fail(format!("OpenProcess({pid}) failed: {}", GetLastError()));
        }
        let bytes = path.len() * 2;
        let mem = VirtualAllocEx(h, ptr::null(), bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if mem.is_null() {
            fail(format!("VirtualAllocEx failed: {}", GetLastError()));
        }
        let mut n = 0usize;
        if WriteProcessMemory(h, mem, path.as_ptr() as *const c_void, bytes, &mut n) == 0 {
            fail(format!("WriteProcessMemory failed: {}", GetLastError()));
        }
        let k32 = GetModuleHandleW(wide(std::ffi::OsStr::new("kernel32.dll")).as_ptr());
        let load = GetProcAddress(k32, c"LoadLibraryW".as_ptr() as *const u8)
            .unwrap_or_else(|| fail("LoadLibraryW not found".into()));
        let start: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(load);
        let th = CreateRemoteThread(h, ptr::null(), 0, Some(start), mem, 0, ptr::null_mut());
        if th.is_null() {
            fail(format!("CreateRemoteThread failed: {}", GetLastError()));
        }
        if WaitForSingleObject(th, 15000) != WAIT_OBJECT_0 {
            fail("LoadLibraryW did not return within 15 s".into());
        }
        let mut code = 0u32;
        GetExitCodeThread(th, &mut code);
        CloseHandle(th);
        VirtualFreeEx(h, mem, 0, MEM_RELEASE);
        CloseHandle(h);
        if code == 0 {
            fail(format!("LoadLibraryW returned NULL in the game (see {})", dir.join("oracle.log").display()));
        }
        println!(
            "{{\"pid\":{pid},\"module\":\"{code:#010x}\",\"dll\":{}}}",
            serde_like_quote(&copy.display().to_string())
        );
    }
}

fn serde_like_quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

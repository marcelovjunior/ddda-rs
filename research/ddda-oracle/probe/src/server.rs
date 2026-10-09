//! Line-based JSON command server on 127.0.0.1:47800.
//!
//! Request: one JSON object per line, `{"id": any, "cmd": "...", ...}`.
//! Reply: one JSON object per line, `{"id": same, "ok": true, ...}` or
//! `{"id": same, "ok": false, "error": "..."}`. See docs/ORACLE.md.

use std::ffi::c_void;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use windows_sys::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows_sys::Win32::System::Memory::{
    VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId};
use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::addr::*;
use crate::hooks::{self, FRAMES};
use crate::{capture, guard, log, on_main, patch, STOP};

pub const PORT: u16 = 47800;
static LISTENER: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

pub fn start() {
    let h = thread::spawn(listener);
    *LISTENER.lock().unwrap() = Some(h);
}

pub fn join() {
    if let Some(h) = LISTENER.lock().unwrap().take() {
        let _ = h.join();
    }
}

fn listener() {
    let l = match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(l) => l,
        Err(e) => {
            log(&format!("bind 127.0.0.1:{PORT} failed: {e}"));
            return;
        }
    };
    let _ = l.set_nonblocking(true);
    log(&format!("listening on 127.0.0.1:{PORT}"));
    let mut conns: Vec<JoinHandle<()>> = Vec::new();
    while !STOP.load(Ordering::SeqCst) {
        match l.accept() {
            Ok((s, _)) => conns.push(thread::spawn(move || conn(s))),
            Err(e) if e.kind() == ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                log(&format!("accept: {e}"));
                thread::sleep(Duration::from_millis(100));
            }
        }
        conns.retain(|c| !c.is_finished());
    }
    drop(l);
    for c in conns {
        let _ = c.join();
    }
    log("listener stopped");
}

fn conn(s: TcpStream) {
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(Duration::from_millis(100)));
    let _ = s.set_nodelay(true);
    let mut w = match s.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let mut r = BufReader::new(s);
    let mut line = String::new();
    loop {
        if STOP.load(Ordering::SeqCst) {
            break;
        }
        match r.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if !line.ends_with('\n') {
                    continue;
                }
                let reply = handle_line(line.trim());
                line.clear();
                let mut out = reply.to_string();
                out.push('\n');
                if w.write_all(out.as_bytes()).is_err() {
                    break;
                }
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => continue,
            Err(_) => break,
        }
    }
}

fn handle_line(line: &str) -> Value {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return json!({"ok": false, "error": format!("bad json: {e}")}),
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let mut rep = match std::panic::catch_unwind(|| dispatch(&req)) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => json!({"ok": false, "error": e}),
        Err(_) => json!({"ok": false, "error": "panic in handler"}),
    };
    if let Some(o) = rep.as_object_mut() {
        o.entry("ok").or_insert(Value::Bool(true));
        o.insert("id".into(), id);
    }
    rep
}

pub fn num(v: &Value) -> Result<u32, String> {
    match v {
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                Ok(u as u32)
            } else if let Some(i) = n.as_i64() {
                Ok(i as u32)
            } else {
                Err(format!("{n} is not an integer (use {{\"f\": x}} for floats)"))
            }
        }
        Value::String(s) => {
            let s = s.trim();
            let r = if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u32::from_str_radix(h, 16).ok()
            } else {
                s.parse::<i64>().ok().map(|i| i as u32)
            };
            r.ok_or_else(|| format!("bad number {s:?}"))
        }
        Value::Object(o) if o.contains_key("f") => {
            let f = o["f"].as_f64().ok_or("bad float")? as f32;
            Ok(f.to_bits())
        }
        Value::Bool(b) => Ok(*b as u32),
        _ => Err(format!("bad number {v}")),
    }
}

fn arg(req: &Value, k: &str) -> Result<u32, String> {
    num(req.get(k).ok_or(format!("missing {k}"))?)
}

fn opt(req: &Value, k: &str, d: u32) -> Result<u32, String> {
    req.get(k).map(num).unwrap_or(Ok(d))
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

fn unhex(s: &str) -> Result<Vec<u8>, String> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if s.len() % 2 != 0 {
        return Err("odd hex length".into());
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string())).collect()
}

pub fn read_mem(addr: u32, n: usize) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; n];
    let mut got = 0usize;
    let ok = unsafe {
        ReadProcessMemory(GetCurrentProcess(), addr as *const c_void, buf.as_mut_ptr() as *mut c_void, n, &mut got)
    };
    if ok == 0 || got != n {
        None
    } else {
        Some(buf)
    }
}

fn dispatch(req: &Value) -> Result<Value, String> {
    let cmd = req.get("cmd").and_then(|c| c.as_str()).ok_or("missing cmd")?;
    match cmd {
        "ping" => Ok(json!({"pong": true})),
        "info" => Ok(info()),
        "read" => {
            let a = arg(req, "addr")?;
            let n = arg(req, "len")? as usize;
            if n > 16 << 20 {
                return Err("len > 16 MiB".into());
            }
            let b = read_mem(a, n).ok_or(format!("unreadable {a:#x}+{n:#x}"))?;
            Ok(json!({"hex": hex(&b)}))
        }
        "write" => {
            let a = arg(req, "addr")?;
            let b = unhex(req.get("hex").and_then(|h| h.as_str()).ok_or("missing hex")?)?;
            let main = req.get("main").and_then(|m| m.as_bool()).unwrap_or(false);
            if main {
                let v = on_main(
                    move || unsafe {
                        match guard::copy(a, b.as_ptr() as u32, b.len()) {
                            Ok(()) => json!({"ok": true}),
                            Err(f) => fault_json(&f),
                        }
                    },
                    Duration::from_secs(10),
                )?;
                Ok(v)
            } else {
                let mut n = 0usize;
                let ok = unsafe {
                    WriteProcessMemory(GetCurrentProcess(), a as *const c_void, b.as_ptr() as *const c_void, b.len(), &mut n)
                };
                if ok == 0 || n != b.len() {
                    Err(format!("write {a:#x} failed"))
                } else {
                    Ok(json!({"written": n}))
                }
            }
        }
        "call" => call(req),
        "scan" => scan(req),
        "capture" => cap(req),
        "wait" => {
            let n = opt(req, "frames", 1)?;
            let t = Duration::from_millis(opt(req, "timeout_ms", 10000)? as u64);
            let start = FRAMES.load(Ordering::SeqCst);
            let t0 = Instant::now();
            while FRAMES.load(Ordering::SeqCst).wrapping_sub(start) < n {
                if t0.elapsed() > t {
                    return Err(format!("only {} frames in {:?}", FRAMES.load(Ordering::SeqCst).wrapping_sub(start), t));
                }
                thread::sleep(Duration::from_millis(2));
            }
            Ok(json!({"frames": FRAMES.load(Ordering::SeqCst), "ms": t0.elapsed().as_millis() as u64}))
        }
        "bg" => {
            let on = req.get("on").and_then(|b| b.as_bool()).ok_or("missing on (bool)")?;
            hooks::BG_WANTED.store(on, Ordering::SeqCst);
            on_main(
                move || unsafe {
                    match hooks::set_bg(on) {
                        Ok(()) => json!({"bg": hooks::bg_enabled()}),
                        Err(e) => json!({"ok": false, "error": e}),
                    }
                },
                Duration::from_secs(10),
            )
        }
        "patches" => Ok(json!({"patches": patch::list()})),
        "keys" | "tap" | "release" | "input" => input_cmd(cmd, req),
        "trace" => trace_cmd(req),
        "unload" => crate::unload(),
        _ => Err(format!("unknown cmd {cmd}")),
    }
}

fn vk_list(v: Option<&Value>) -> Result<Vec<u8>, String> {
    match v {
        Some(Value::Array(a)) => a.iter().map(|x| num(x).map(|n| n as u8)).collect(),
        Some(x) => Ok(vec![num(x)? as u8]),
        None => Ok(Vec::new()),
    }
}

/// keys {down:[vk..]} holds exactly these keys; tap {vk: n|[..], frames}
/// presses for `frames` game frames; release drops everything; input
/// reports the state.
fn input_cmd(cmd: &str, req: &Value) -> Result<Value, String> {
    if cmd != "input" && !crate::input::installed() {
        let v = on_main(
            || unsafe {
                match crate::input::install() {
                    Ok(()) => json!({}),
                    Err(e) => json!({"ok": false, "error": e}),
                }
            },
            Duration::from_secs(10),
        )?;
        if v.get("ok") == Some(&Value::Bool(false)) {
            return Ok(v);
        }
    }
    match cmd {
        "keys" => crate::input::set_held(&vk_list(req.get("down"))?),
        "tap" => crate::input::tap(&vk_list(req.get("vk"))?, opt(req, "frames", 2)?),
        "release" => crate::input::release_all(),
        _ => {}
    }
    let (down, taps, on) = crate::input::status();
    Ok(json!({"installed": crate::input::installed(), "down": down, "taps": taps, "input_on": on}))
}

/// trace {fields: [[addr, len], ..], frames, script: [[frame, [vk..]], ..]}
/// -> rows: one hex string per frame (fields concatenated), sampled after
/// each game frame. Script entries set the held keys from that frame on;
/// keys are released when the trace ends.
fn trace_cmd(req: &Value) -> Result<Value, String> {
    let fields: Vec<(u32, usize)> = match req.get("fields") {
        Some(Value::Array(a)) => a
            .iter()
            .map(|f| match f {
                Value::Array(p) if p.len() == 2 => Ok((num(&p[0])?, num(&p[1])? as usize)),
                _ => Err("field must be [addr, len]".to_string()),
            })
            .collect::<Result<_, _>>()?,
        _ => return Err("fields must be an array of [addr, len]".into()),
    };
    let total: usize = fields.iter().map(|f| f.1).sum();
    let frames = opt(req, "frames", 60)?;
    if total * frames as usize > 64 << 20 {
        return Err("trace too large (> 64 MiB)".into());
    }
    let script: Vec<(u32, Vec<u8>)> = match req.get("script") {
        Some(Value::Array(a)) => a
            .iter()
            .map(|e| match e {
                Value::Array(p) if p.len() == 2 => Ok((num(&p[0])?, vk_list(Some(&p[1]))?)),
                _ => Err("script entry must be [frame, [vk..]]".to_string()),
            })
            .collect::<Result<_, _>>()?,
        None => Vec::new(),
        _ => return Err("script must be an array".into()),
    };
    if !script.is_empty() && !crate::input::installed() {
        let v = on_main(
            || unsafe {
                match crate::input::install() {
                    Ok(()) => json!({}),
                    Err(e) => json!({"ok": false, "error": e}),
                }
            },
            Duration::from_secs(10),
        )?;
        if v.get("ok") == Some(&Value::Bool(false)) {
            return Ok(v);
        }
    }
    let (tx, rx) = mpsc::channel();
    let f0 = FRAMES.load(Ordering::SeqCst);
    crate::trace::start(crate::trace::Trace { fields, frames, script, idx: 0, armed: false, rows: Vec::new(), tx })?;
    let timeout = Duration::from_millis(opt(req, "timeout_ms", 10_000 + frames * 100)? as u64);
    match rx.recv_timeout(timeout) {
        Ok(Ok(rows)) => Ok(json!({"first_frame": f0, "rows": rows.iter().map(|r| hex(r)).collect::<Vec<_>>()})),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            crate::trace::cancel();
            Err(format!("trace did not finish within {timeout:?}"))
        }
    }
}

fn fault_json(f: &guard::Fault) -> Value {
    json!({"ok": false, "error": format!("fault {:#010x} at eip {:#x} (address {:#x})", f.code, f.eip, f.addr),
           "fault": {"code": f.code, "eip": f.eip, "addr": f.addr}})
}

fn info() -> Value {
    unsafe {
        let app = sapp();
        let (active, bg, hwnd) = if app != 0 {
            (rd::<u8>(app + SAPP_ACTIVE), rd::<u8>(app + SAPP_BG), rd::<u32>(app + SAPP_HWND))
        } else {
            (0, 0, 0)
        };
        json!({
            "version": crate::VERSION,
            "pid": GetCurrentProcessId(),
            "module": crate::hinst(),
            "frames": FRAMES.load(Ordering::SeqCst),
            "idles": hooks::IDLES.load(Ordering::SeqCst),
            "presents": hooks::PRESENTS.load(Ordering::SeqCst),
            "main_tid": hooks::MAIN_TID.load(Ordering::SeqCst),
            "present_tid": hooks::PRESENT_TID.load(Ordering::SeqCst),
            "sapp": app,
            "active": active,
            "bg": bg,
            "hwnd": hwnd,
            "foreground": GetForegroundWindow() as u32 == hwnd && hwnd != 0,
            "device": hooks::device(),
            "patches": patch::list(),
        })
    }
}

fn call(req: &Value) -> Result<Value, String> {
    let func = arg(req, "addr")?;
    let conv = req.get("conv").and_then(|c| c.as_str()).unwrap_or(if req.get("this").is_some() { "thiscall" } else { "cdecl" });
    let mut args: Vec<u32> = match req.get("args") {
        Some(Value::Array(a)) => a.iter().map(num).collect::<Result<_, _>>()?,
        None => Vec::new(),
        _ => return Err("args must be an array".into()),
    };
    let float_ret = req.get("float").and_then(|f| f.as_bool()).unwrap_or(false);
    let (ecx, edx) = match conv {
        "thiscall" => (arg(req, "this")?, 0),
        "cdecl" | "stdcall" => (0, 0),
        "fastcall" => {
            let e = if args.is_empty() { 0 } else { args.remove(0) };
            let d = if args.is_empty() { 0 } else { args.remove(0) };
            (e, d)
        }
        _ => return Err(format!("unknown conv {conv}")),
    };
    if func < 0x10000 {
        return Err("refusing to call below 0x10000".into());
    }
    let timeout = Duration::from_millis(opt(req, "timeout_ms", 10000)? as u64);
    on_main(
        move || unsafe {
            match guard::call(func, ecx, edx, &args, float_ret) {
                Ok(o) => {
                    let mut v = json!({"eax": o.eax, "edx": o.edx});
                    if float_ret {
                        v["st0"] = json!(o.st0);
                    }
                    v
                }
                Err(f) => fault_json(&f),
            }
        },
        timeout,
    )
}

fn scan(req: &Value) -> Result<Value, String> {
    let values: Vec<u32> = match req.get("values") {
        Some(Value::Array(a)) => a.iter().map(num).collect::<Result<_, _>>()?,
        _ => return Err("values must be an array".into()),
    };
    let limit = opt(req, "limit", 4096)? as usize;
    let image = req.get("image").and_then(|b| b.as_bool()).unwrap_or(false);
    let mut hits: Vec<Vec<u32>> = vec![Vec::new(); values.len()];
    let mut addr: u32 = 0x10000;
    let mut chunk = vec![0u8; 1 << 20];
    let t0 = Instant::now();
    let mut scanned = 0u64;
    unsafe {
        let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
        while addr < 0x7fff_0000 {
            if VirtualQuery(addr as *const c_void, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) == 0 {
                break;
            }
            let base = mbi.BaseAddress as u32;
            let size = mbi.RegionSize as u32;
            let next = base.wrapping_add(size);
            let readable = mbi.State == MEM_COMMIT
                && mbi.Protect != 0
                && mbi.Protect & (PAGE_GUARD | PAGE_NOACCESS) == 0;
            let in_image = (0x0040_0000..0x01a0_c000).contains(&base);
            let ours = {
                let m = crate::hinst();
                base >= m && base < m + 0x0100_0000
            };
            if readable && (image || !in_image) && !ours {
                let mut off = 0u32;
                while off < size {
                    let n = (size - off).min(chunk.len() as u32) as usize;
                    let mut got = 0usize;
                    if ReadProcessMemory(GetCurrentProcess(), (base + off) as *const c_void, chunk.as_mut_ptr() as *mut c_void, n, &mut got) != 0 {
                        scanned += got as u64;
                        for i in (0..got / 4 * 4).step_by(4) {
                            let v = u32::from_le_bytes([chunk[i], chunk[i + 1], chunk[i + 2], chunk[i + 3]]);
                            for (k, want) in values.iter().enumerate() {
                                if v == *want && hits[k].len() < limit {
                                    hits[k].push(base + off + i as u32);
                                }
                            }
                        }
                    }
                    off += n as u32;
                }
            }
            if next <= addr {
                break;
            }
            addr = next;
        }
    }
    let map: serde_json::Map<String, Value> =
        values.iter().zip(hits).map(|(v, h)| (format!("{v:#x}"), json!(h))).collect();
    Ok(json!({"hits": map, "scanned": scanned, "ms": t0.elapsed().as_millis() as u64}))
}

fn cap(req: &Value) -> Result<Value, String> {
    let path = match req.get("path").and_then(|p| p.as_str()) {
        Some(p) => p.to_string(),
        None => std::env::temp_dir().join("ddda-oracle").join("frame.png").display().to_string(),
    };
    let timeout = Duration::from_millis(opt(req, "timeout_ms", 5000)? as u64);
    let (tx, rx) = mpsc::channel();
    capture::request(tx);
    let t0 = Instant::now();
    let f = match rx.recv_timeout(timeout) {
        Ok(r) => r?,
        Err(_) => {
            capture::cancel();
            return Err(format!("no Present within {timeout:?} (is the game rendering? try bg on)"));
        }
    };
    let grab_ms = t0.elapsed().as_millis() as u64;
    capture::write_png(&path, &f)?;
    Ok(json!({"path": path, "w": f.w, "h": f.h, "format": f.format, "grab_ms": grab_ms, "total_ms": t0.elapsed().as_millis() as u64}))
}

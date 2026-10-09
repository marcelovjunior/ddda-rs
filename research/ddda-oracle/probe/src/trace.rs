//! Per-frame tracing: record memory ranges after every game frame (after
//! sBbsRpgMain::vf06 returns) for N frames, optionally driving scripted
//! keys at given frame indices, so input and state line up exactly.

use std::sync::mpsc::Sender;
use std::sync::Mutex;

use crate::guard;
use crate::input;

pub struct Trace {
    pub fields: Vec<(u32, usize)>,
    pub frames: u32,
    /// (frame index, keys held from that frame on)
    pub script: Vec<(u32, Vec<u8>)>,
    pub idx: u32,
    /// Set by the first pre(): a trace starting mid-frame skips that frame's post().
    pub armed: bool,
    pub rows: Vec<Vec<u8>>,
    pub tx: Sender<Result<Vec<Vec<u8>>, String>>,
}

static TRACE: Mutex<Option<Trace>> = Mutex::new(None);

pub fn start(t: Trace) -> Result<(), String> {
    let mut g = TRACE.lock().unwrap();
    if g.is_some() {
        return Err("a trace is already running".into());
    }
    *g = Some(t);
    Ok(())
}

pub fn cancel() {
    if let Some(t) = TRACE.lock().unwrap().take() {
        if !t.script.is_empty() {
            input::release_all();
        }
    }
}

/// Before the frame (and before input::frame): apply the key script.
pub fn pre() {
    let Ok(mut g) = TRACE.try_lock() else { return };
    if let Some(t) = g.as_mut() {
        t.armed = true;
        for (f, keys) in &t.script {
            if *f == t.idx {
                input::set_held(keys);
            }
        }
    }
}

/// After the frame: record the fields.
pub fn post() {
    let Ok(mut g) = TRACE.try_lock() else { return };
    let Some(t) = g.as_mut() else { return };
    if !t.armed {
        return;
    }
    let total: usize = t.fields.iter().map(|f| f.1).sum();
    let mut row = vec![0u8; total];
    let mut off = 0;
    let mut err = None;
    for &(addr, len) in &t.fields {
        if let Err(f) = unsafe { guard::copy(row.as_mut_ptr() as u32 + off as u32, addr, len) } {
            err = Some(format!("fault reading {addr:#x}: {:#x} at {:#x}", f.code, f.addr));
            break;
        }
        off += len;
    }
    if let Some(e) = err {
        let t = g.take().unwrap();
        if !t.script.is_empty() {
            input::release_all();
        }
        let _ = t.tx.send(Err(e));
        return;
    }
    t.rows.push(row);
    t.idx += 1;
    if t.idx >= t.frames {
        let t = g.take().unwrap();
        if !t.script.is_empty() {
            input::release_all();
        }
        let _ = t.tx.send(Ok(t.rows));
    }
}

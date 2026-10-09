//! Scripted keyboard input from inside the process.
//!
//! The game reads the keyboard in sKeyboard::vf06 (0x00df40e0): it clears a
//! 256-bit VK bitmap (sKeyboard+0x120) and, only if sApp+0x2566 (window
//! active) is set, fills it through 0x00df3ef0 (GetAsyncKeyState or
//! DirectInput, both mapped to VK codes). sQlocInput::vf06 (0x00409ee0)
//! turns raw input into game buttons and wipes them while inactive.
//!
//! Installed lazily on the first input command (main thread):
//! - the gating reads of sApp+0x2566 in sQlocInput::vf06 (0x00409f4b,
//!   0x0040a0be, 0x0040a0ce), 0x0040a290 and sKeyboard::vf06 (0x00df412f)
//!   read INPUT_ON instead, which is "really active OR a script holds keys";
//! - the call at 0x00df4140 goes to `kb_read`, which runs the real reader
//!   only when the window is really active, then ORs in the scripted keys.
//! sMouse keeps reading sApp+0x2566, so the maintainer's mouse never moves
//! the camera. Pads are not filtered: a physical pad in use while a script
//! runs in the background would also reach the game.

use std::ffi::c_void;
use std::sync::Mutex;

use crate::addr::*;
use crate::patch;

static mut INPUT_ON: u8 = 0;
static STATE: Mutex<Keys> = Mutex::new(Keys { held: [0; 8], taps: Vec::new(), linger: 0, cur: [0; 8] });

struct Keys {
    held: [u32; 8],
    taps: Vec<(u8, u32)>,
    linger: u32,
    cur: [u32; 8],
}

const KB_READ: u32 = 0x00df_3ef0;
const KB_CALL: u32 = 0x00df_4140;
const QLOC_SITES: [u32; 4] = [0x0040_9f4b, 0x0040_a0be, 0x0040_a0ce, 0x0040_a290];
const KB_GATE: u32 = 0x00df_412f;

fn real_active() -> bool {
    unsafe {
        let app = sapp();
        app != 0 && rd::<u8>(app + SAPP_ACTIVE) != 0
    }
}

unsafe extern "thiscall" fn kb_read(this: *mut c_void, bits: *mut u32) {
    if real_active() {
        let orig: unsafe extern "thiscall" fn(*mut c_void, *mut u32) = core::mem::transmute(KB_READ as usize);
        orig(this, bits);
    }
    let cur = match STATE.try_lock() {
        Ok(s) => s.cur,
        Err(_) => return,
    };
    for i in 0..8 {
        *bits.add(i) |= cur[i];
    }
    // Same folding as 0x00df3ef0: L/R shift, ctrl, alt -> VK 0x10..0x12.
    let u = *bits.add(5);
    let v = u.wrapping_mul(2) | u;
    *bits |= ((((v & 2) * 2) | (v & 8)) * 2 | (v & 0x20)) << 13;
}

pub fn installed() -> bool {
    patch::is_applied("kb_read")
}

/// Main thread only.
pub unsafe fn install() -> Result<(), String> {
    if installed() {
        return Ok(());
    }
    let flag = (&raw const INPUT_ON) as u32;
    INPUT_ON = real_active() as u8;
    for &s in &QLOC_SITES {
        // cmp byte ptr [eax+0x2566], 1  ->  cmp byte ptr [INPUT_ON], 1
        let mut new = vec![0x80, 0x3D];
        new.extend_from_slice(&flag.to_le_bytes());
        new.push(0x01);
        patch::apply("input_gate", s, Some(&[0x80, 0xB8, 0x66, 0x25, 0x00, 0x00, 0x01]), &new)?;
    }
    // cmp byte ptr [eax+0x2566], bl  ->  cmp byte ptr [INPUT_ON], bl
    let mut new = vec![0x38, 0x1D];
    new.extend_from_slice(&flag.to_le_bytes());
    patch::apply("input_gate", KB_GATE, Some(&[0x38, 0x98, 0x66, 0x25, 0x00, 0x00]), &new)?;
    let rel_orig = (KB_READ as i32).wrapping_sub(KB_CALL as i32 + 5);
    let rel_new = (kb_read as *const () as usize as i32).wrapping_sub(KB_CALL as i32 + 5);
    let mut expect = vec![0xE8];
    expect.extend_from_slice(&rel_orig.to_le_bytes());
    let mut new = vec![0xE8];
    new.extend_from_slice(&rel_new.to_le_bytes());
    patch::apply("kb_read", KB_CALL, Some(&expect), &new)?;
    Ok(())
}

/// Called at the start of every frame tick (main thread, before the game's
/// input update).
pub fn frame() {
    let Ok(mut s) = STATE.lock() else { return };
    let mut cur = s.held;
    for (vk, n) in s.taps.iter_mut() {
        if *n > 0 {
            cur[(*vk >> 5) as usize] |= 1 << (*vk & 31);
            *n -= 1;
        }
    }
    s.taps.retain(|t| t.1 > 0);
    let any = cur.iter().any(|&w| w != 0);
    if any {
        s.linger = 3;
    } else if s.linger > 0 {
        s.linger -= 1;
    }
    s.cur = cur;
    let on = real_active() || any || s.linger > 0;
    unsafe {
        if installed() {
            INPUT_ON = on as u8;
        }
    }
}

pub fn set_held(vks: &[u8]) {
    let mut s = STATE.lock().unwrap();
    s.held = [0; 8];
    for &vk in vks {
        s.held[(vk >> 5) as usize] |= 1 << (vk & 31);
    }
}

pub fn tap(vks: &[u8], frames: u32) {
    let mut s = STATE.lock().unwrap();
    for &vk in vks {
        s.taps.push((vk, frames));
    }
}

pub fn release_all() {
    let mut s = STATE.lock().unwrap();
    s.held = [0; 8];
    s.taps.clear();
}

pub fn status() -> (Vec<u8>, usize, bool) {
    let s = STATE.lock().unwrap();
    let mut down = Vec::new();
    for vk in 0..=255u8 {
        if s.cur[(vk >> 5) as usize] & (1 << (vk & 31)) != 0 {
            down.push(vk);
        }
    }
    (down, s.taps.len(), unsafe { INPUT_ON != 0 })
}

//! Back buffer capture from the Present hook:
//! GetBackBuffer -> (StretchRect to a plain render target if multisampled)
//! -> GetRenderTargetData into a system-memory surface -> LockRect -> RGB.
//! PNG encoding happens on the requesting (server) thread.

use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::sync::Mutex;

pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub rgb: Vec<u8>,
    pub format: u32,
}

static REQUEST: Mutex<Option<Sender<Result<Frame, String>>>> = Mutex::new(None);

pub fn request(tx: Sender<Result<Frame, String>>) {
    *REQUEST.lock().unwrap() = Some(tx);
}

pub fn cancel() {
    REQUEST.lock().unwrap().take();
}

pub unsafe fn on_present(dev: *mut c_void) {
    let tx = match REQUEST.try_lock() {
        Ok(mut g) => g.take(),
        Err(_) => None,
    };
    if let Some(tx) = tx {
        let r = std::panic::catch_unwind(|| grab(dev)).unwrap_or_else(|_| Err("panic in grab".into()));
        let _ = tx.send(r);
    }
}

#[repr(C)]
#[derive(Default)]
struct SurfaceDesc {
    format: u32,
    rtype: u32,
    usage: u32,
    pool: u32,
    ms_type: u32,
    ms_quality: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
struct LockedRect {
    pitch: i32,
    bits: *mut u8,
}

unsafe fn vfn(obj: *mut c_void, idx: usize) -> usize {
    *(*(obj as *const *const usize)).add(idx)
}

unsafe fn release(obj: *mut c_void) {
    if !obj.is_null() {
        let f: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(vfn(obj, 2));
        f(obj);
    }
}

struct Guard(Vec<*mut c_void>);
impl Drop for Guard {
    fn drop(&mut self) {
        for &o in self.0.iter().rev() {
            unsafe { release(o) };
        }
    }
}

unsafe fn grab(dev: *mut c_void) -> Result<Frame, String> {
    let mut held = Guard(Vec::new());
    // IDirect3DDevice9::GetBackBuffer (18)
    let get_bb: unsafe extern "system" fn(*mut c_void, u32, u32, u32, *mut *mut c_void) -> i32 =
        core::mem::transmute(vfn(dev, 18));
    let mut bb: *mut c_void = core::ptr::null_mut();
    let hr = get_bb(dev, 0, 0, 0, &mut bb);
    if hr < 0 || bb.is_null() {
        return Err(format!("GetBackBuffer hr={hr:#x}"));
    }
    held.0.push(bb);
    // IDirect3DSurface9::GetDesc (12)
    let get_desc: unsafe extern "system" fn(*mut c_void, *mut SurfaceDesc) -> i32 = core::mem::transmute(vfn(bb, 12));
    let mut d = SurfaceDesc::default();
    let hr = get_desc(bb, &mut d);
    if hr < 0 {
        return Err(format!("GetDesc hr={hr:#x}"));
    }
    let mut src = bb;
    if d.ms_type != 0 {
        // CreateRenderTarget (28), StretchRect (34)
        let create_rt: unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32, u32, i32, *mut *mut c_void, *mut c_void) -> i32 =
            core::mem::transmute(vfn(dev, 28));
        let mut rt: *mut c_void = core::ptr::null_mut();
        let hr = create_rt(dev, d.width, d.height, d.format, 0, 0, 0, &mut rt, core::ptr::null_mut());
        if hr < 0 {
            return Err(format!("CreateRenderTarget hr={hr:#x}"));
        }
        held.0.push(rt);
        let stretch: unsafe extern "system" fn(*mut c_void, *mut c_void, *const c_void, *mut c_void, *const c_void, u32) -> i32 =
            core::mem::transmute(vfn(dev, 34));
        let hr = stretch(dev, bb, core::ptr::null(), rt, core::ptr::null(), 0);
        if hr < 0 {
            return Err(format!("StretchRect hr={hr:#x}"));
        }
        src = rt;
    }
    // CreateOffscreenPlainSurface (36), D3DPOOL_SYSTEMMEM = 2
    let create_off: unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32, *mut *mut c_void, *mut c_void) -> i32 =
        core::mem::transmute(vfn(dev, 36));
    let mut sys: *mut c_void = core::ptr::null_mut();
    let hr = create_off(dev, d.width, d.height, d.format, 2, &mut sys, core::ptr::null_mut());
    if hr < 0 {
        return Err(format!("CreateOffscreenPlainSurface hr={hr:#x} fmt={}", d.format));
    }
    held.0.push(sys);
    // GetRenderTargetData (32)
    let get_rtd: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> i32 = core::mem::transmute(vfn(dev, 32));
    let hr = get_rtd(dev, src, sys);
    if hr < 0 {
        return Err(format!("GetRenderTargetData hr={hr:#x}"));
    }
    // LockRect (13) read-only (D3DLOCK_READONLY = 0x10), UnlockRect (14)
    let lock: unsafe extern "system" fn(*mut c_void, *mut LockedRect, *const c_void, u32) -> i32 = core::mem::transmute(vfn(sys, 13));
    let unlock: unsafe extern "system" fn(*mut c_void) -> i32 = core::mem::transmute(vfn(sys, 14));
    let mut lr = LockedRect { pitch: 0, bits: core::ptr::null_mut() };
    let hr = lock(sys, &mut lr, core::ptr::null(), 0x10);
    if hr < 0 {
        return Err(format!("LockRect hr={hr:#x}"));
    }
    let (w, h) = (d.width as usize, d.height as usize);
    let mut rgb = vec![0u8; w * h * 3];
    let mut err = None;
    for y in 0..h {
        let row = lr.bits.offset(y as isize * lr.pitch as isize) as *const u32;
        let out = &mut rgb[y * w * 3..(y + 1) * w * 3];
        for x in 0..w {
            let p = *row.add(x);
            let (r, g, b) = match d.format {
                // D3DFMT_A8R8G8B8 / X8R8G8B8
                21 | 22 => ((p >> 16) as u8, (p >> 8) as u8, p as u8),
                // D3DFMT_A2R10G10B10
                35 => ((p >> 22) as u8, (p >> 12) as u8, (p >> 2) as u8),
                // D3DFMT_A2B10G10R10
                31 => ((p >> 2) as u8, (p >> 12) as u8, (p >> 22) as u8),
                f => {
                    err = Some(format!("unsupported back buffer format {f}"));
                    (0, 0, 0)
                }
            };
            out[3 * x] = r;
            out[3 * x + 1] = g;
            out[3 * x + 2] = b;
        }
        if err.is_some() {
            break;
        }
    }
    unlock(sys);
    match err {
        Some(e) => Err(e),
        None => Ok(Frame { w: d.width, h: d.height, rgb, format: d.format }),
    }
}

pub fn write_png(path: &str, f: &Frame) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("create {path}: {e}"))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), f.w, f.h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(&f.rgb).map_err(|e| e.to_string())?;
    Ok(())
}

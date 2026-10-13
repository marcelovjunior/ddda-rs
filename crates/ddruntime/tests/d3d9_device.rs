//! Direct3D 9 device methods of brief L20 (docs/tasks/L20-d3d9-completeness.md):
//! Reset, SetGammaRamp, occlusion queries, the displacement-map sampler and
//! the ownership of implicit objects. Each test needs the installed game
//! (DDDA_EXE) and skips without it.
use ddnative_base::mem::Mem;
use ddruntime::d3d9::D3DERR_INVALIDCALL;
use ddruntime::d3d9dev::{refs, D3DERR_DEVICELOST, D3DERR_DEVICENOTRESET};
use ddruntime::d3d9draw::recorded;
use ddruntime::session::{Outcome, Session};

const SCRATCH: u32 = 0x7EFF_0000;
const PP: u32 = SCRATCH + 0x200;
const OUT: u32 = SCRATCH + 0x300;
const DATA: u32 = SCRATCH + 0x400;

fn installed_exe() -> Option<ddexe::ExeFile> {
    let path = std::env::var("DDDA_EXE").unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    ddexe::ExeFile::open(path).ok()
}

/// A session with a window and a D3D9 device (windowed 1280x720, auto
/// depth D24S8): (session, device, window).
fn device_session() -> Option<(Session, u32, u32)> {
    let exe = installed_exe()?;
    let path = exe.path.to_string_lossy().to_string();
    let mut s = Session::new(&path).expect("session");
    let token = |s: &Session, n: &str| s.imports.iter().find(|i| i.full_name() == n).unwrap().token;
    let (wc, name) = (SCRATCH, SCRATCH + 0x100);
    s.mem.set_u16(name, b'W' as u16);
    s.mem.set_u32(wc + 8, token(&s, "USER32.dll!DefWindowProcW"));
    s.mem.set_u32(wc + 40, name);
    let Outcome::Returned(_) = s.call(token(&s, "USER32.dll!RegisterClassExW"), 0, &[wc]) else { panic!() };
    let Outcome::Returned(cpu) = s.call(token(&s, "USER32.dll!CreateWindowExW"), 0, &[0, name, 0, 0x00CA_0000, 0, 0, 1286, 749, 0, 0, 0x0040_0000, 0]) else { panic!() };
    let hwnd = cpu.eax;
    let Outcome::Returned(cpu) = s.call(token(&s, "d3d9.dll!Direct3DCreate9"), 0, &[32]) else { panic!() };
    let d3d = cpu.eax;
    write_pp(&mut s, [0, 0, 0, 0, 0, 0, 1, hwnd, 1, 1, 75, 0, 0, 0]);
    let create = s.mem.u32(s.mem.u32(d3d) + 4 * 16);
    let Outcome::Returned(_) = s.call(create, 0, &[d3d, 0, 1, hwnd, 0x46, PP, OUT]) else { panic!() };
    let dev = s.mem.u32(OUT);
    Some((s, dev, hwnd))
}

fn write_pp(s: &mut Session, pp: [u32; 14]) {
    for (k, v) in pp.into_iter().enumerate() {
        s.mem.set_u32(PP + 4 * k as u32, v);
    }
}

/// Call COM method `slot` of `obj` with `args` (this first); eax.
fn com(s: &mut Session, obj: u32, slot: u32, args: &[u32]) -> u32 {
    let f = s.mem.u32(s.mem.u32(obj) + 4 * slot);
    let mut a = vec![obj];
    a.extend_from_slice(args);
    match s.call(f, 0, &a) {
        Outcome::Returned(c) => c.eax,
        o => panic!("slot {slot}: {o}"),
    }
}

/// An object the device hands out through OUT.
fn out(s: &mut Session, obj: u32, slot: u32, args: &[u32]) -> u32 {
    assert_eq!(com(s, obj, slot, args), 0, "slot {slot}");
    s.mem.u32(OUT)
}

/// A surface's D3DSURFACE_DESC: format, width, height.
fn desc(s: &mut Session, surface: u32) -> (u32, u32, u32) {
    com(s, surface, 12, &[DATA]);
    (s.mem.u32(DATA), s.mem.u32(DATA + 24), s.mem.u32(DATA + 28))
}

#[test]
fn reset_needs_default_resources_released_and_rebuilds_the_implicit_surfaces() {
    let Some((mut s, dev, hwnd)) = device_session() else { return };
    let old_back = out(&mut s, dev, 18, &[0, 0, 0, OUT]); // GetBackBuffer: the caller holds it
    let old_depth = out(&mut s, dev, 40, &[OUT]);
    com(&mut s, old_depth, 2, &[]);
    let rt = out(&mut s, dev, 28, &[64, 64, 21, 0, 0, 0, OUT, 0]); // CreateRenderTarget (D3DPOOL_DEFAULT)
    let managed = out(&mut s, dev, 23, &[16, 16, 1, 0, 21, 1, OUT, 0]); // CreateTexture, D3DPOOL_MANAGED
    assert_eq!(com(&mut s, dev, 65, &[0, managed]), 0, "bound to stage 0");
    assert_eq!(refs(managed), Some(("IDirect3DTexture9", 2)));

    // invalid parameters fail and leave the device as it was
    write_pp(&mut s, [1000, 700, 22, 1, 0, 0, 1, hwnd, 0, 1, 75, 0, 60, 0]);
    assert_eq!(com(&mut s, dev, 16, &[PP]), D3DERR_INVALIDCALL, "not a desktop mode");
    assert_eq!(com(&mut s, dev, 3, &[]), 0, "TestCooperativeLevel still D3D_OK");
    assert_eq!(refs(managed), Some(("IDirect3DTexture9", 2)), "still bound");

    // a held back buffer and a live render target block the reset
    write_pp(&mut s, [640, 480, 0, 0, 0, 0, 1, hwnd, 1, 1, 75, 0, 0, 0]);
    assert_eq!(com(&mut s, dev, 16, &[PP]), D3DERR_INVALIDCALL);
    assert_eq!(com(&mut s, dev, 3, &[]), D3DERR_DEVICENOTRESET);
    assert_eq!(com(&mut s, dev, 17, &[0, 0, 0, 0]), D3DERR_DEVICELOST, "Present waits for a Reset");
    assert_eq!(refs(managed), Some(("IDirect3DTexture9", 1)), "the state was unbound");

    com(&mut s, old_back, 2, &[]);
    com(&mut s, rt, 2, &[]);
    assert_eq!(com(&mut s, dev, 16, &[PP]), 0, "Reset");
    let pp: Vec<u32> = (0..4).map(|k| s.mem.u32(PP + 4 * k)).collect();
    assert_eq!(pp, [640, 480, 22, 1], "completed parameters written back");
    assert_eq!(com(&mut s, dev, 3, &[]), 0);
    // the old back buffer was freed (the guest heap may hand its block to
    // the new one: the backend is told, see `released` below)
    assert!(recorded(|r| r.released.contains(&old_back)).unwrap(), "the old back buffer is gone");
    assert_eq!(refs(managed), Some(("IDirect3DTexture9", 1)), "managed resources survive");
    let back = out(&mut s, dev, 18, &[0, 0, 0, OUT]);
    assert_eq!(desc(&mut s, back), (22, 640, 480));
    assert_eq!(out(&mut s, dev, 38, &[0, OUT]), back, "render target 0 is the new back buffer");
    let depth = out(&mut s, dev, 40, &[OUT]);
    assert_eq!(desc(&mut s, depth), (75, 640, 480), "a new automatic depth-stencil surface");
    com(&mut s, dev, 48, &[DATA]);
    let vp: Vec<u32> = (0..4).map(|k| s.mem.u32(DATA + 4 * k)).collect();
    assert_eq!(vp, [0, 0, 640, 480], "the viewport covers the new back buffer");
    let released = recorded(|r| r.released.clone()).unwrap();
    assert!(released.contains(&old_back) && released.contains(&old_depth) && released.contains(&rt), "the backend forgets freed surfaces");
    assert_eq!(com(&mut s, dev, 17, &[0, 0, 0, 0]), 0, "Present");
}

#[test]
fn a_gamma_ramp_is_shown_on_a_full_screen_swap_chain_only() {
    let Some((mut s, dev, hwnd)) = device_session() else { return };
    // an inverting ramp
    for c in 0..3u32 {
        for k in 0..256u32 {
            s.mem.set_u16(DATA + 2 * (256 * c + k), ((255 - k) << 8) as u16);
        }
    }
    com(&mut s, dev, 21, &[0, 0, DATA]);
    com(&mut s, dev, 17, &[0, 0, 0, 0]);
    assert!(recorded(|r| r.gamma.is_none()).unwrap(), "windowed: the ramp is not applied");
    // full screen at the desktop mode (the host's, L34)
    let d = ddruntime::d3d9::display().desktop;
    write_pp(&mut s, [d.width, d.height, 22, 1, 0, 0, 1, hwnd, 0, 1, 75, 0, d.refresh, 0]);
    assert_eq!(com(&mut s, dev, 16, &[PP]), 0, "Reset to full screen");
    com(&mut s, dev, 17, &[0, 0, 0, 0]);
    let g = recorded(|r| r.gamma.clone()).unwrap().expect("the ramp reaches the backend");
    assert_eq!((g[0][0], g[2][255]), (0xFF00, 0));
}

#[test]
fn occlusion_queries_follow_the_driver_and_count_on_the_backend() {
    let Some((mut s, dev, _)) = device_session() else { return };
    let q = out(&mut s, dev, 118, &[9, OUT]);
    assert_eq!(com(&mut s, q, 5, &[]), 4, "a DWORD count");
    s.mem.set_u32(DATA, 0);
    assert_eq!(com(&mut s, q, 7, &[DATA, 4, 1]), 0, "S_OK before any Issue");
    assert_eq!(s.mem.u32(DATA), 0xDDDD_DDDD, "the driver's undefined value");
    assert_eq!(com(&mut s, q, 6, &[2]), 0, "D3DISSUE_BEGIN");
    assert_eq!(com(&mut s, q, 7, &[DATA, 4, 1]), 1, "S_FALSE while counting");
    assert_eq!(com(&mut s, q, 6, &[1]), 0, "D3DISSUE_END");
    assert_eq!(recorded(|r| r.queries.clone()).unwrap(), vec![(q, true), (q, false)]);
    assert_eq!(com(&mut s, q, 7, &[DATA, 4, 0]), 1, "the recording backend counts nothing: S_FALSE");
    com(&mut s, q, 2, &[]);
    assert!(recorded(|r| r.released.contains(&q)).unwrap());
}

#[test]
fn the_displacement_map_sampler_and_implicit_objects() {
    let Some((mut s, dev, _)) = device_session() else { return };
    // D3DDMAPSAMPLER (256): the game clears it with SetTexture(256, NULL)
    assert_eq!(com(&mut s, dev, 65, &[256, 0]), 0);
    assert_eq!(com(&mut s, dev, 68, &[256, 1, DATA]), 0, "GetSamplerState(256, ADDRESSU)");
    assert_eq!(s.mem.u32(DATA), 1, "D3DTADDRESS_WRAP");
    assert_eq!(com(&mut s, dev, 65, &[16, 0]), D3DERR_INVALIDCALL);
    // the automatic depth-stencil surface lives on while another is set
    let depth = out(&mut s, dev, 40, &[OUT]);
    assert_eq!(refs(depth), Some(("IDirect3DSurface9", 3)), "device + binding + caller");
    com(&mut s, depth, 2, &[]);
    assert_eq!(com(&mut s, dev, 39, &[0]), 0);
    assert_eq!(refs(depth), Some(("IDirect3DSurface9", 1)), "the device keeps it");
    assert_eq!(com(&mut s, dev, 40, &[OUT]), ddruntime::d3d9dev::D3DERR_NOTFOUND);
    assert_eq!(com(&mut s, dev, 39, &[depth]), 0);
    // an additional swap chain is held by the caller only
    let pp: Vec<u32> = (0..14).map(|k| s.mem.u32(PP + 4 * k)).collect();
    write_pp(&mut s, pp.try_into().unwrap());
    let chain = out(&mut s, dev, 13, &[PP, OUT]);
    assert_eq!(refs(chain), Some(("IDirect3DSwapChain9", 1)));
    assert_eq!(com(&mut s, dev, 15, &[]), 2);
    assert_eq!(com(&mut s, chain, 2, &[]), 0);
    assert_eq!(refs(chain), None);
    assert_eq!(com(&mut s, dev, 15, &[]), 1, "GetNumberOfSwapChains");
}

/// Clear hands the backend the viewport, the scissor rectangle when the
/// scissor test is on, and SRGBWRITEENABLE (D3D9's Clear honours them; brief
/// L29, ddgpu tests/targets.rs).
#[test]
fn a_clear_carries_the_viewport_scissor_and_srgb_write() {
    let Some((mut s, dev, _)) = device_session() else { return };
    let (vp, rect) = (DATA, DATA + 0x40);
    for (k, v) in [10u32, 20, 300, 200, 0, 1f32.to_bits()].into_iter().enumerate() {
        s.mem.set_u32(vp + 4 * k as u32, v);
    }
    for (k, v) in [5i32, 6, 100, 120].into_iter().enumerate() {
        s.mem.set_u32(rect + 4 * k as u32, v as u32);
    }
    com(&mut s, dev, 47, &[vp]); // SetViewport
    com(&mut s, dev, 75, &[rect]); // SetScissorRect
    assert_eq!(com(&mut s, dev, 43, &[0, 0, 1, 0xFF00_00FF, 1f32.to_bits(), 0]), 0, "Clear, test off");
    com(&mut s, dev, 57, &[174, 1]); // SCISSORTESTENABLE
    com(&mut s, dev, 57, &[194, 1]); // SRGBWRITEENABLE
    assert_eq!(com(&mut s, dev, 43, &[0, 0, 1, 0xFF00_00FF, 1f32.to_bits(), 0]), 0, "Clear, test on");
    let clears = recorded(|r| r.clears.clone()).unwrap();
    let (a, b) = (&clears[clears.len() - 2], &clears[clears.len() - 1]);
    assert_eq!(a.viewport[..4], [10, 20, 300, 200]);
    assert_eq!((a.scissor, a.srgb), (None, false));
    assert_eq!((b.scissor, b.srgb), (Some([5, 6, 100, 120]), true));
}

/// A released device's methods do not reach a later device created at the
/// same address through the runtime's cached device (L144, Objects::device_mut):
/// the new device starts with its own default states.
#[test]
fn a_new_device_does_not_see_a_released_ones_state() {
    let Some((mut s, dev, hwnd)) = device_session() else { return };
    // D3DRS_FILLMODE (8): D3DFILL_SOLID (3) by default; wireframe on the first device
    assert_eq!(com(&mut s, dev, 57, &[8, 2]), 0);
    assert_eq!(com(&mut s, dev, 58, &[8, OUT]), 0);
    assert_eq!(s.mem.u32(OUT), 2);
    assert_eq!(com(&mut s, dev, 2, &[]), 0, "the last reference");
    assert!(refs(dev).is_none(), "the device is gone");
    let token = |s: &Session, n: &str| s.imports.iter().find(|i| i.full_name() == n).unwrap().token;
    let Outcome::Returned(cpu) = s.call(token(&s, "d3d9.dll!Direct3DCreate9"), 0, &[32]) else { panic!() };
    let d3d = cpu.eax;
    write_pp(&mut s, [0, 0, 0, 0, 0, 0, 1, hwnd, 1, 1, 75, 0, 0, 0]);
    let create = s.mem.u32(s.mem.u32(d3d) + 4 * 16);
    let Outcome::Returned(_) = s.call(create, 0, &[d3d, 0, 1, hwnd, 0x46, PP, OUT]) else { panic!() };
    let second = s.mem.u32(OUT);
    assert_eq!(com(&mut s, second, 58, &[8, OUT]), 0);
    assert_eq!(s.mem.u32(OUT), 3, "a fresh device's fill mode (first {dev:#x}, second {second:#x})");
    // and the first one's address, if reused, is the second device
    assert_eq!(refs(dev).is_some(), dev == second);
}

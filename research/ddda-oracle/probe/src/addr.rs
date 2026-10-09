//! Addresses in DDDA.exe (Steam 2017-12-13, base 0x400000, no relocation).
//! Every one was read in Ghidra and checked against the live process; see
//! docs/ORACLE.md for the evidence.

/// `sApp*` global. sApp (DTI 0x018d0f6c, vtable 0x0142c0d8) owns the window
/// and the main loop 0x00df0d20.
pub const SAPP_PTR: u32 = 0x018d_0f50;
/// sApp+0x20 (u8): main loop runs frames even when inactive if non-zero
/// (`cmp byte [esi+0x20],0` at 0x00df0de9). Zeroed in sApp init 0x00df2f30.
pub const SAPP_BG: u32 = 0x20;
/// sApp+0x12c: HWND of the game window (wndproc 0x00df1550 compares it).
pub const SAPP_HWND: u32 = 0x12c;
/// sApp+0x2558: sMain* (the live object is an sBbsRpgMain).
pub const SAPP_MAIN: u32 = 0x2558;
/// sApp+0x2566 (u8): window active. Set by WM_ACTIVATE / WM_ACTIVATEAPP(1),
/// cleared by WM_ACTIVATEAPP(0) via 0x00df0ed0. Gates sKeyboard (0x00df412f),
/// sMouse (0x00e04f71) and the pad mask in sQlocInput::vf06 (0x00409f4b).
pub const SAPP_ACTIVE: u32 = 0x2566;

/// `sRender*` global (sRenderExt, vtable 0x0156026c).
pub const SRENDER_PTR: u32 = 0x018d_08f4;
/// sRender+0x98: IDirect3DDevice9* (TestCooperativeLevel through it in 0x00daf7c0).
pub const RENDER_DEVICE: u32 = 0x98;

/// sBbsRpgMain vtable. Slot 6 (vf06, 0x0041dd60) is the per-frame update
/// called by the main loop at 0x00df0e54; slot 11 (vf11, 0x0041dd20) is
/// called every loop iteration while the window is inactive (0x00df0dfe).
pub const VT_SBBSRPGMAIN: u32 = 0x0155_adc0;
pub const VF06_FRAME: u32 = 0x0041_dd60;
pub const VF11_IDLE: u32 = 0x0041_dd20;

/// `call 0x00e04d60` in sMouse::vf06 (0x00e04f20), the only caller of the
/// routine that runs ClipCursor to the game window and SPI_SETMOUSE every
/// frame without checking focus.
pub const CURSOR_CALLSITE: u32 = 0x00e0_4f66;
pub const CURSOR_FN: u32 = 0x00e0_4d60;

pub unsafe fn rd<T: Copy>(a: u32) -> T {
    core::ptr::read_unaligned(a as *const T)
}

pub unsafe fn sapp() -> u32 {
    rd::<u32>(SAPP_PTR)
}

//! x87 machine state, using the host x87 for exact extended arithmetic.
//!
//! Intel SDM volume 1, x87 FPU state (32-bit protected-mode FSAVE format),
//! and volume 2, FNSAVE/FRSTOR. These instructions are valid in 64-bit mode.
//!
//! Live mode (L62): a guest's x87 state stays loaded in the host thread's
//! FPU between instructions, so an instruction is just itself. Host code
//! never uses the x87 on x86-64 Windows (floats are SSE), and the OS keeps
//! each thread's FPU across context switches. One `X87` per host thread is
//! live at a time; another one executing on that thread displaces it into
//! the thread's spill table, and it is reloaded on its next instruction.
//! [`park`] displaces the live state so host code (services, drivers) runs
//! with the thread's own control word. Reading the state (status, control,
//! FNSAVE, clone) snapshots it without changing it. The arithmetic is the
//! hardware's, as before; only the save/restore round trips between
//! instructions are gone (FNSAVE then FRSTOR is the identity on the state).
//! SSE/MXCSR are untouched. `X87` has no `Drop` (drop glue would give every
//! lifted function an unwind cleanup): an X87 dropped while live leaves a
//! stale table entry behind, which is harmless.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// The FNINIT state: control 037Fh (all exceptions masked), tags empty.
const INIT: [u8; 108] = {
    let mut state = [0; 108];
    state[0] = 0x7f;
    state[1] = 0x03;
    state[8] = 0xff;
    state[9] = 0xff;
    state
};
/// No X87 is live on this thread (ids start at 1; 0 is "not assigned").
const NONE: u64 = u64::MAX;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// The id of the X87 whose state is in this thread's FPU.
    static LIVE: Cell<u64> = const { Cell::new(NONE) };
    /// States displaced from this thread's FPU, by X87 id.
    static SPILLED: RefCell<HashMap<u64, [u8; 108]>> = RefCell::new(HashMap::new());
    /// This thread's own control word, restored by [`park`].
    static HOST_CW: Cell<Option<u16>> = const { Cell::new(None) };
    static TOKEN: u64 = NEXT_ID.fetch_add(1, Ordering::Relaxed);
}

fn token() -> u64 {
    TOKEN.with(|t| *t)
}

pub struct X87 {
    /// The state while `home` is 0; otherwise stale.
    state: [u8; 108],
    /// Identity in the live/spilled tables (0: never live).
    id: u64,
    /// The host thread whose FPU or spill table holds the state (0: none).
    home: u64,
}

impl Default for X87 {
    fn default() -> Self {
        Self { state: INIT, id: 0, home: 0 }
    }
}

impl Clone for X87 {
    fn clone(&self) -> Self {
        Self { state: self.image(), id: 0, home: 0 }
    }
}

impl std::fmt::Debug for X87 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X87").field("state", &self.image()).finish()
    }
}

/// Displace this thread's live guest x87 state (if any) into the spill
/// table and give the FPU back to host code (FNINIT state with the
/// thread's own control word). The owner reloads it on its next
/// instruction.
pub fn park() {
    let live = LIVE.with(Cell::get);
    if live == NONE {
        return;
    }
    let mut image = [0u8; 108];
    fnsave_host(&mut image);
    image[12..28].fill(0);
    SPILLED.with(|s| s.borrow_mut().insert(live, image));
    LIVE.with(|l| l.set(NONE));
    if let Some(cw) = HOST_CW.with(Cell::get) {
        fldcw_host(cw);
    }
}

#[cfg(target_arch = "x86_64")]
fn fnsave_host(image: &mut [u8; 108]) {
    unsafe { std::arch::asm!("fnsave [{}]", in(reg) image.as_mut_ptr()) }
}

#[cfg(target_arch = "x86_64")]
fn frstor_host(image: &[u8; 108]) {
    unsafe { std::arch::asm!("frstor [{}]", in(reg) image.as_ptr()) }
}

#[cfg(target_arch = "x86_64")]
fn fldcw_host(cw: u16) {
    unsafe { std::arch::asm!("fldcw [{}]", in(reg) &cw) }
}

#[cfg(target_arch = "x86_64")]
fn fnstcw_host() -> u16 {
    let mut cw = 0u16;
    unsafe { std::arch::asm!("fnstcw [{}]", in(reg) &mut cw) }
    cw
}

#[cfg(not(target_arch = "x86_64"))]
fn fnsave_host(_: &mut [u8; 108]) { panic!("exact lifted x87 requires an x86-64 host") }
#[cfg(not(target_arch = "x86_64"))]
fn frstor_host(_: &[u8; 108]) { panic!("exact lifted x87 requires an x86-64 host") }
#[cfg(not(target_arch = "x86_64"))]
fn fldcw_host(_: u16) { panic!("exact lifted x87 requires an x86-64 host") }
#[cfg(not(target_arch = "x86_64"))]
fn fnstcw_host() -> u16 { panic!("exact lifted x87 requires an x86-64 host") }

impl X87 {
    pub fn control(&self) -> u16 { let s = self.image(); u16::from_le_bytes([s[0], s[1]]) }
    pub fn status(&self) -> u16 { let s = self.image(); u16::from_le_bytes([s[4], s[5]]) }
    pub fn is_empty(&self) -> bool {
        Self::empty(&self.image())
    }
    fn empty(s: &[u8; 108]) -> bool {
        let (status, tags) = (u16::from_le_bytes([s[4], s[5]]), u16::from_le_bytes([s[8], s[9]]));
        tags >> (2 * ((status >> 11) & 7)) & 3 == 3
    }
    pub fn depth(&self) -> u32 {
        let s = self.image();
        let tags = u16::from_le_bytes([s[8], s[9]]);
        (0..8).filter(|k| (tags >> (2 * k)) & 3 != 3).count() as u32
    }
    pub fn top_extended(&self) -> Option<(u64, u16)> {
        let s = self.image();
        if Self::empty(&s) { return None; }
        Some((u64::from_le_bytes(s[28..36].try_into().unwrap()),
              u16::from_le_bytes(s[36..38].try_into().unwrap())))
    }

    /// The current state image (FSAVE layout, instruction/data pointers
    /// zero), wherever it is held. Changes nothing.
    fn image(&self) -> [u8; 108] {
        if self.home == 0 {
            return self.state;
        }
        assert_eq!(self.home, token(), "x87 state used on another host thread than the one it is live on");
        let mut image = if LIVE.with(Cell::get) == self.id {
            let mut image = [0u8; 108];
            fnsave_host(&mut image);
            frstor_host(&image);
            image
        } else {
            SPILLED.with(|s| *s.borrow().get(&self.id).expect("spilled x87 state"))
        };
        image[12..28].fill(0);
        image
    }

    /// Replace the state (it is no longer live or spilled anywhere).
    fn set_image(&mut self, image: [u8; 108]) {
        if self.home != 0 {
            assert_eq!(self.home, token(), "x87 state used on another host thread than the one it is live on");
            if LIVE.with(Cell::get) == self.id {
                LIVE.with(|l| l.set(NONE));
            } else {
                SPILLED.with(|s| s.borrow_mut().remove(&self.id));
            }
            self.home = 0;
        }
        self.state = image;
    }

    /// Make this state the thread's live FPU state (displacing another).
    #[cold]
    #[inline(never)]
    fn acquire(&mut self) {
        let me = token();
        if self.id == 0 {
            self.id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        }
        let image = if self.home == 0 {
            self.state
        } else {
            assert_eq!(self.home, me, "x87 state used on another host thread than the one it is live on");
            SPILLED.with(|s| s.borrow_mut().remove(&self.id)).expect("spilled x87 state")
        };
        // Unmasked exceptions are unsupported (a guest trap must not trap Rust).
        assert_eq!(image[0] & 0x3f, 0x3f, "unmasked guest x87 exceptions");
        if HOST_CW.with(Cell::get).is_none() {
            HOST_CW.with(|c| c.set(Some(fnstcw_host())));
        }
        park();
        frstor_host(&image);
        LIVE.with(|l| l.set(self.id));
        self.home = me;
    }

    /// Execute a statically selected x87 opcode. OP is the opcode byte plus
    /// ModRM << 8; memory operands use [rax], a private ten-byte buffer.
    /// No executable code is generated. Register encodings remain unchanged.
    /// Unmasked exceptions are unsupported (a guest trap must not trap Rust).
    #[cfg(target_arch = "x86_64")]
    pub fn exec<const OP: u16>(&mut self, data: &mut [u8; 10]) -> u32 {
        let (opcode, modrm) = (OP & 255, OP >> 8);
        let valid = if modrm < 0xc0 {
            match opcode {
                0xd8 | 0xda | 0xdc | 0xde => modrm <= 0x38 && modrm % 8 == 0,
                0xd9 => matches!(modrm, 0 | 0x10 | 0x18 | 0x28 | 0x38),
                0xdb => matches!(modrm, 0 | 0x10 | 0x18 | 0x28 | 0x38),
                0xdd => matches!(modrm, 0 | 0x10 | 0x18 | 0x38),
                0xdf => matches!(modrm, 0 | 0x10 | 0x18 | 0x28 | 0x38),
                _ => false,
            }
        } else {
            match opcode {
                0xd8 => true,
                0xdc => matches!(modrm, 0xc0..=0xcf | 0xe0..=0xff),
                // L22 adds fxam e5, fldpi eb, f2xm1 f0, fyl2x f1, fptan f2,
                // fpatan f3, fprem1 f5, fprem f8, frndint fc, fscale fd.
                0xd9 => matches!(modrm, 0xc0..=0xcf | 0xe0 | 0xe1 | 0xe5 | 0xe8 | 0xea | 0xeb | 0xed | 0xee
                                 | 0xf0..=0xf3 | 0xf5 | 0xf8 | 0xfa | 0xfc..=0xff),
                0xda => modrm == 0xe9,
                0xdb => matches!(modrm, 0xe2 | 0xe8..=0xf7), // fnclex e2
                0xdf => matches!(modrm, 0xe8..=0xf7),
                0xdd => matches!(modrm, 0xd0..=0xef),
                0xde => matches!(modrm, 0xc0..=0xcf | 0xd9 | 0xe0..=0xff),
                _ => false,
            }
        };
        assert!(valid, "unsupported x87 encoding {OP:#x}");
        if OP == 0x28d9 {
            assert_eq!(data[0] & 0x3f, 0x3f, "unmasked guest x87 exceptions");
        }
        if LIVE.with(Cell::get) != self.id {
            self.acquire();
        }
        let flags: u64;
        unsafe {
            std::arch::asm!(
                ".byte {opcode}, {modrm}",
                "pushfq", "pop {flags}",
                in("rax") data.as_mut_ptr(),
                opcode = const (OP & 255), modrm = const (OP >> 8),
                flags = out(reg) flags,
            );
        }
        flags as u32
    }

    #[cfg(not(target_arch = "x86_64"))]
    pub fn exec<const OP: u16>(&mut self, _data: &mut [u8; 10]) -> u32 {
        panic!("exact lifted x87 requires an x86-64 host (opcode {OP:#x})");
    }

    /// FNSAVE m108 in the 32-bit protected-mode format (Intel SDM vol. 1
    /// 8.1.10), as measured on the hardware (L46, research/runtime/
    /// x87_state_oracle.c): reserved words read FFFFh; CS 23h and DS 2Bh are
    /// the WOW64 selectors; FIP, FOP and FDP belong to the last non-control
    /// x87 instruction and its last memory operand (the emitter proves
    /// them). The tag word is the full one the hardware keeps for the
    /// image. Afterwards the unit is in the FNINIT state.
    pub fn fnsave(&mut self, m: &mut dyn crate::mem::Mem, addr: u32, fip: u32, fop: u16, fdp: u32) {
        let mut image = self.image();
        for k in [2, 6, 10, 26] {
            image[k..k + 2].copy_from_slice(&0xffffu16.to_le_bytes());
        }
        image[12..16].copy_from_slice(&fip.to_le_bytes());
        image[16..18].copy_from_slice(&0x23u16.to_le_bytes());
        image[18..20].copy_from_slice(&(fop & 0x7ff).to_le_bytes());
        image[20..24].copy_from_slice(&fdp.to_le_bytes());
        image[24..26].copy_from_slice(&0x2bu16.to_le_bytes());
        for (k, byte) in image.iter().enumerate() {
            m.set_u8(addr.wrapping_add(k as u32), *byte);
        }
        self.set_image(INIT);
    }

    /// FRSTOR m108: the hardware keeps the loaded control and status words
    /// and registers, and recomputes the tag of every non-empty register
    /// from its contents (L46); the host FPU does that round trip. Guest
    /// instruction and data pointers are not part of the model (see exec).
    pub fn frstor(&mut self, m: &dyn crate::mem::Mem, addr: u32) {
        let mut image = [0u8; 108];
        for (k, byte) in image.iter_mut().enumerate() {
            *byte = m.u8(addr.wrapping_add(k as u32));
        }
        let mut image = Self::normalized(image);
        image[12..28].fill(0);
        self.set_image(image);
    }

    #[cfg(target_arch = "x86_64")]
    fn normalized(mut image: [u8; 108]) -> [u8; 108] {
        let mut host = [0u8; 108];
        unsafe {
            std::arch::asm!(
                "fnsave [{host}]", "frstor [{guest}]", "fnsave [{guest}]", "frstor [{host}]",
                host = in(reg) host.as_mut_ptr(), guest = in(reg) image.as_mut_ptr(),
            );
        }
        image
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn normalized(_image: [u8; 108]) -> [u8; 108] {
        panic!("exact lifted x87 requires an x86-64 host (frstor)");
    }

    /// Load the float returned by an external-call vector, without rounding
    /// any other stack values. Kept as push for the existing replay contract.
    /// FSTP ST(0): drop the top value (a mocked callee takes it, L50).
    pub fn pop(&mut self) {
        let mut data = [0; 10];
        self.exec::<0xd8dd>(&mut data); // fstp st(0)
    }

    pub fn push(&mut self, bits: u32) {
        let mut data = [0; 10];
        data[..4].copy_from_slice(&bits.to_le_bytes());
        self.exec::<0x00d9>(&mut data); // fld m32real
    }

    pub fn memory<const OP: u16, const SIZE: usize, const WRITE: bool>(
        &mut self, m: &mut dyn crate::mem::Mem, addr: u32,
    ) {
        assert!(SIZE <= 10);
        let mut data = [0; 10];
        if !WRITE {
            let mut k = 0;
            while k + 4 <= SIZE {
                data[k..k + 4].copy_from_slice(&m.u32(addr.wrapping_add(k as u32)).to_le_bytes());
                k += 4;
            }
            if k < SIZE {
                data[k..k + 2].copy_from_slice(&m.u16(addr.wrapping_add(k as u32)).to_le_bytes());
            }
        }
        self.exec::<OP>(&mut data);
        if WRITE {
            let mut k = 0;
            while k + 4 <= SIZE {
                m.set_u32(addr.wrapping_add(k as u32), u32::from_le_bytes(data[k..k + 4].try_into().unwrap()));
                k += 4;
            }
            if k < SIZE {
                m.set_u16(addr.wrapping_add(k as u32), u16::from_le_bytes([data[k], data[k + 1]]));
            }
        }
    }

    pub fn register<const OP: u16>(&mut self) { self.exec::<OP>(&mut [0; 10]); }
    pub fn compare_flags<const OP: u16>(&mut self) -> crate::x86::Flags {
        crate::x86::Flags::new(crate::x86::RAW, 0, 0, self.exec::<OP>(&mut [0; 10]), 32)
    }

    /// Observe st0 as a float using guest rounding, without changing state.
    pub fn top_f32(&self) -> Option<u32> {
        let mut image = self.image();
        if Self::empty(&image) { return None; }
        let mut bits = 0u32;
        // the live state (if any) is saved and restored around the store
        let mut host = [0u8; 108];
        unsafe {
            std::arch::asm!(
                "fnsave [{host}]", "frstor [{image}]", "fst dword ptr [{out}]", "frstor [{host}]",
                host = in(reg) host.as_mut_ptr(), image = in(reg) image.as_mut_ptr(),
                out = in(reg) &mut bits,
            );
        }
        Some(bits)
    }
}

//! x87 machine state, using the host x87 for exact extended arithmetic.
//!
//! Intel SDM volume 1, x87 FPU state (32-bit protected-mode FSAVE format),
//! and volume 2, FNSAVE/FRSTOR. These instructions are valid in 64-bit mode.
//! Only x87 state is swapped: SSE/MXCSR and the host's x87 state are preserved.

#[derive(Clone, Debug)]
pub struct X87 {
    state: [u8; 108],
}

impl Default for X87 {
    fn default() -> Self {
        let mut state = [0; 108];
        state[0..2].copy_from_slice(&0x037fu16.to_le_bytes());
        state[8..10].copy_from_slice(&0xffffu16.to_le_bytes());
        Self { state }
    }
}

impl X87 {
    pub fn control(&self) -> u16 { u16::from_le_bytes(self.state[0..2].try_into().unwrap()) }
    pub fn status(&self) -> u16 { u16::from_le_bytes(self.state[4..6].try_into().unwrap()) }
    pub fn is_empty(&self) -> bool {
        let tags = u16::from_le_bytes(self.state[8..10].try_into().unwrap());
        tags >> (2 * ((self.status() >> 11) & 7)) & 3 == 3
    }
    pub fn depth(&self) -> u32 {
        let tags = u16::from_le_bytes(self.state[8..10].try_into().unwrap());
        (0..8).filter(|k| (tags >> (2 * k)) & 3 != 3).count() as u32
    }
    pub fn top_extended(&self) -> Option<(u64, u16)> {
        if self.is_empty() { return None; }
        Some((u64::from_le_bytes(self.state[28..36].try_into().unwrap()),
              u16::from_le_bytes(self.state[36..38].try_into().unwrap())))
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
                0xd9 => matches!(modrm, 0xc0..=0xcf | 0xe0 | 0xe1 | 0xe8 | 0xee | 0xfa | 0xfe | 0xff),
                0xda => modrm == 0xe9,
                0xdb | 0xdf => matches!(modrm, 0xe8..=0xf7),
                0xdd => matches!(modrm, 0xd0..=0xef),
                0xde => matches!(modrm, 0xc0..=0xcf | 0xd9 | 0xe0..=0xff),
                _ => false,
            }
        };
        assert!(valid, "unsupported x87 encoding {OP:#x}");
        if OP == 0x28d9 {
            assert_eq!(data[0] & 0x3f, 0x3f, "unmasked guest x87 exceptions");
        }
        assert_eq!(self.control() & 0x3f, 0x3f, "unmasked guest x87 exceptions");
        let mut host = [0u8; 108];
        let flags: u64;
        unsafe {
            std::arch::asm!(
                "fnsave [{host}]", "frstor [{guest}]",
                ".byte {opcode}, {modrm}",
                "pushfq", "pop {flags}",
                "fnsave [{guest}]", "frstor [{host}]",
                host = in(reg) host.as_mut_ptr(), guest = in(reg) self.state.as_mut_ptr(),
                in("rax") data.as_mut_ptr(),
                opcode = const (OP & 255), modrm = const (OP >> 8),
                flags = out(reg) flags,
            );
        }
        // Host instruction/data pointers are not guest addresses. Environment
        // load/store instructions that expose them remain unsupported.
        self.state[12..28].fill(0);
        flags as u32
    }

    #[cfg(not(target_arch = "x86_64"))]
    pub fn exec<const OP: u16>(&mut self, _data: &mut [u8; 10]) -> u32 {
        panic!("exact lifted x87 requires an x86-64 host (opcode {OP:#x})");
    }

    /// Load the float returned by an external-call vector, without rounding
    /// any other stack values. Kept as push for the existing replay contract.
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
            for (k, byte) in data[..SIZE].iter_mut().enumerate() {
                *byte = m.u8(addr.wrapping_add(k as u32));
            }
        }
        self.exec::<OP>(&mut data);
        if WRITE {
            for (k, byte) in data[..SIZE].iter().enumerate() {
                m.set_u8(addr.wrapping_add(k as u32), *byte);
            }
        }
    }

    pub fn register<const OP: u16>(&mut self) { self.exec::<OP>(&mut [0; 10]); }
    pub fn compare_flags<const OP: u16>(&mut self) -> crate::x86::Flags {
        crate::x86::Flags::new(crate::x86::RAW, 0, 0, self.exec::<OP>(&mut [0; 10]), 32)
    }

    /// Observe st0 as a float using guest rounding, without changing state.
    pub fn top_f32(&self) -> Option<u32> {
        if self.is_empty() { return None; }
        let mut copy = self.clone();
        let mut data = [0; 10];
        copy.exec::<0x18d9>(&mut data); // fstp m32real
        Some(u32::from_le_bytes(data[..4].try_into().unwrap()))
    }
}

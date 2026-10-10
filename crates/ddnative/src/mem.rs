//! A byte-addressed view of the original's objects, for ports of code that
//! follows pointers between objects. A port over `Mem` performs the same
//! reads and writes, at the same addresses, as the original; the
//! differential tests run it over the byte image the original ran on and
//! compare the whole result. (docs/NATIVE_PIPELINE.md, "Memory-model ports")
pub trait Mem {
    fn u8(&self, addr: u32) -> u8;
    fn set_u8(&mut self, addr: u32, v: u8);
    fn u16(&self, addr: u32) -> u16 {
        u16::from_le_bytes([0, 1].map(|i| self.u8(addr.wrapping_add(i))))
    }
    fn u32(&self, addr: u32) -> u32 {
        u32::from_le_bytes([0, 1, 2, 3].map(|i| self.u8(addr.wrapping_add(i))))
    }
    fn set_u32(&mut self, addr: u32, v: u32) {
        for (i, b) in v.to_le_bytes().into_iter().enumerate() {
            self.set_u8(addr.wrapping_add(i as u32), b);
        }
    }
    fn set_u16(&mut self, addr: u32, v: u16) {
        for (i, b) in v.to_le_bytes().into_iter().enumerate() {
            self.set_u8(addr.wrapping_add(i as u32), b);
        }
    }
}

/// A Mem over one contiguous image starting at `base` (tests, tools).
pub struct Image {
    pub base: u32,
    pub bytes: Vec<u8>,
}

impl Mem for Image {
    fn u8(&self, addr: u32) -> u8 {
        self.bytes[(addr - self.base) as usize]
    }
    fn set_u8(&mut self, addr: u32, v: u8) {
        let i = (addr - self.base) as usize;
        self.bytes[i] = v;
    }
}

/// A Mem holding only the bytes a vector recorded: reading any other byte
/// panics (the port read something the original did not).
#[derive(Default)]
pub struct Sparse {
    pub bytes: std::collections::HashMap<u32, u8>,
    pub written: std::collections::BTreeMap<u32, u8>,
}

impl Mem for Sparse {
    fn u8(&self, addr: u32) -> u8 {
        if let Some(v) = self.written.get(&addr) {
            return *v;
        }
        *self.bytes.get(&addr).unwrap_or_else(|| panic!("read of {addr:#010x}, which the original did not read"))
    }
    fn set_u8(&mut self, addr: u32, v: u8) {
        self.written.insert(addr, v);
    }
}

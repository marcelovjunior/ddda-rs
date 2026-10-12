//! A byte-addressed view of the original's objects, for ports of code that
//! follows pointers between objects. A port over `Mem` performs the same
//! reads and writes, at the same addresses, as the original; the
//! differential tests run it over the byte image the original ran on and
//! compare the whole result. (docs/NATIVE_PIPELINE.md, "Memory-model ports")
pub trait Mem {
    /// Preflight an instruction access without reading or publishing bytes.
    /// Returns the first inaccessible byte. Runtime adapters must forward
    /// this hook; legacy vector memories deliberately do not opt in.
    fn probe(&self, _addr: u32, _width: u32, _write: bool) -> Result<(), u32> {
        panic!("this Mem does not support restartable instructions")
    }
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
    /// `out.len()` bytes from `addr` (wrapping), exactly as reading each
    /// byte with [`u8`](Self::u8) would; implementations may copy in bulk.
    fn read_bytes(&self, addr: u32, out: &mut [u8]) {
        for (k, b) in out.iter_mut().enumerate() {
            *b = self.u8(addr.wrapping_add(k as u32));
        }
    }
    /// A fresh block of `size` bytes (the platform allocator models in
    /// crate::platform call this instead of the game's allocators).
    fn alloc(&mut self, size: u32) -> u32 {
        let _ = size;
        panic!("this Mem has no allocator")
    }
}

/// Where Sparse hands out fresh blocks: research/native/harness.py uses the
/// same base and rounding, so a replay gets the addresses the vector saw.
pub const HEAP_BASE: u32 = 0x0C00_0000;

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
    /// Next fresh block (0 = HEAP_BASE): a bump allocator, 16-byte steps.
    pub heap: u32,
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
    fn alloc(&mut self, size: u32) -> u32 {
        if self.heap == 0 {
            self.heap = HEAP_BASE;
        }
        let a = self.heap;
        self.heap = self.heap.wrapping_add(size.max(1)).wrapping_add(15) & !15;
        a
    }
}

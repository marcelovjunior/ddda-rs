//! The runtime's own reads of guest memory (L144): what host code (the
//! D3D9 services, the nDraw sink, the executor port) reads of the game's
//! structures, arguments and buffers.
//!
//! Through `&dyn Mem` every word is a virtual call and a page lookup
//! (3.5 ns); a [`View`] takes the page table once and reads inline, as the
//! lifted code's fast path does: one permission test and one load, the
//! same bytes, the same atomicity (an aligned load is one Acquire access).
//! Anything else (a lazy page, a fault, an access across a page end, a
//! memory without a page table) goes to the `Mem` as before, so faults
//! panic exactly as they did.
//!
//! All host reads should go through here: when guest memory becomes one
//! flat reservation (L142), ddnative-base's `Pages` (behind [`View::load`])
//! and `Mem::read_bytes` change and every caller follows.
use ddnative_base::mem::{load_at, Mem, Pages, PAGE, READ};

/// Reads of one guest address space, the page table taken once
/// (ddnative-base's [`Pages`], which the readable ports use too).
#[derive(Clone, Copy)]
pub struct View<'a> {
    m: &'a dyn Mem,
    pages: Pages,
}

impl<'a> View<'a> {
    pub fn new(m: &'a dyn Mem) -> View<'a> {
        View { m, pages: Pages::of(m) }
    }

    /// `width` (1, 2 or 4) bytes at `addr`, little-endian.
    #[inline]
    pub fn load(&self, addr: u32, width: u32) -> u32 {
        self.pages.load(self.m, addr, width as usize)
    }

    #[inline]
    pub fn u8(&self, addr: u32) -> u8 {
        self.load(addr, 1) as u8
    }

    #[inline]
    pub fn u16(&self, addr: u32) -> u16 {
        self.load(addr, 2) as u16
    }

    #[inline]
    pub fn u32(&self, addr: u32) -> u32 {
        self.load(addr, 4)
    }

    #[inline]
    pub fn f32(&self, addr: u32) -> f32 {
        f32::from_bits(self.u32(addr))
    }

    /// `N` consecutive words at `addr`.
    #[inline]
    pub fn words<const N: usize>(&self, addr: u32) -> [u32; N] {
        std::array::from_fn(|k| self.u32(addr.wrapping_add(4 * k as u32)))
    }

    /// `count` consecutive words at `addr`, appended to `out`: page by
    /// page, one table lookup per page and an Acquire load per aligned word
    /// (the same access as `u32`); anything else (lazy pages, unaligned
    /// words) word by word.
    pub fn extend_words(&self, addr: u32, count: u32, out: &mut Vec<u32>) {
        out.reserve(count as usize);
        let mut k = 0u32;
        while k < count {
            let a = addr.wrapping_add(4 * k);
            let off = (a % PAGE) as usize;
            // the words of this run that lie wholly in the page
            let run = ((PAGE as usize - off) / 4).min((count - k) as usize) as u32;
            match self.pages.bytes(a, 4 * run as usize, READ) {
                Some(at) if a % 4 == 0 && run > 0 => {
                    // SAFETY: `run` aligned words inside a backed page
                    out.extend((0..run as usize).map(|j| unsafe { load_at(at.add(4 * j), 4) }));
                    k += run;
                }
                _ => {
                    out.push(self.u32(a));
                    k += 1;
                }
            }
        }
    }

    /// `out.len()` bytes at `addr` (Mem::read_bytes: page by page, each
    /// aligned word in one atomic load in parallel mode).
    #[inline]
    pub fn read(&self, addr: u32, out: &mut [u8]) {
        self.m.read_bytes(addr, out)
    }

    /// `len` bytes at `addr`, in a new vector.
    pub fn bytes(&self, addr: u32, len: u32) -> Vec<u8> {
        let mut v = vec![0u8; len as usize];
        self.read(addr, &mut v);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::View;
    use crate::memory::{GuestMemory, READ, WRITE};
    use ddnative_base::mem::Mem;

    /// A view reads what the Mem reads: backed pages, a lazy page (zero),
    /// across a page end, every width; a fault panics as the Mem's does.
    #[test]
    fn a_view_reads_what_the_memory_reads() {
        let mut m = GuestMemory::new();
        m.map(0x10000, 0x2000, READ | WRITE).unwrap();
        m.map_lazy(0x12000, 0x1000, READ | WRITE).unwrap();
        for k in 0..0x2000u32 {
            m.set_u8(0x10000 + k, (k.wrapping_mul(0x9E37_79B1) >> 11) as u8);
        }
        let g = View::new(&m);
        for a in [0x10000u32, 0x10001, 0x10FFD, 0x10FFE, 0x10FFF, 0x11FFC, 0x12000, 0x12FFC] {
            assert_eq!(g.u8(a), m.u8(a), "{a:#x}");
            assert_eq!(g.u16(a), m.u16(a), "{a:#x}");
            assert_eq!(g.u32(a), m.u32(a), "{a:#x}");
        }
        assert_eq!(g.words::<3>(0x10FFC), [m.u32(0x10FFC), m.u32(0x11000), m.u32(0x11004)]);
        for (a, n) in [(0x10FF8u32, 4u32), (0x10FFA, 3), (0x10000, 0x800), (0x11FF0, 8), (0x12000, 5)] {
            let mut v = vec![7];
            g.extend_words(a, n, &mut v);
            assert_eq!(v[1..], (0..n).map(|k| m.u32(a + 4 * k)).collect::<Vec<_>>(), "{a:#x}+{n}");
        }
        let unmapped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.u32(0x20000)));
        assert!(unmapped.is_err(), "an unmapped read faults");
    }
}

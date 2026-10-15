//! L193 S2 step 1: a per-thread append-only arena of register blocks (NDRAW_DIRECT S2).
//!
//! The sink builds a draw's shader constants today as `Arc<Constants>` (two allocations a block,
//! freed on another thread). An arena hands out disjoint slices of big fixed chunks instead:
//! the sink writes the registers straight into the slice, `finish` gives a [`ConstRef`]
//! (chunk `Arc`, offset, length) that travels with the draw, the backend reads it and drops it.
//! A chunk is freed once, when its last `ConstRef` is gone, not once per draw.
//!
//! Safety argument: a chunk is allocated zeroed and never reallocated or reused. `alloc` returns
//! a [`BlockWriter`] holding the only `&mut` to its disjoint region; `finish` consumes it and
//! returns the `ConstRef` that may read the region (shared, read-only from then on). Regions
//! never overlap, so no byte is written while another reference can read it. A `ConstRef` handed
//! to another thread is published through the caller's queue (that synchronisation orders the
//! writes before the reads).
use std::cell::UnsafeCell;
use std::sync::Arc;

struct Chunk {
    buf: UnsafeCell<Box<[[u32; 4]]>>,
}
// SAFETY: see the module comment: disjoint regions, written before they are shared.
unsafe impl Sync for Chunk {}
unsafe impl Send for Chunk {}

/// A finished block: read-only registers inside a shared chunk.
#[derive(Clone)]
pub struct ConstRef {
    chunk: Arc<Chunk>,
    off: u32,
    len: u32,
}

impl ConstRef {
    pub fn regs(&self) -> &[[u32; 4]] {
        // SAFETY: the region was finished (no writer exists) and chunks never reallocate.
        let all: &[[u32; 4]] = unsafe { &**self.chunk.buf.get() };
        &all[self.off as usize..(self.off + self.len) as usize]
    }
    /// The same chunk and region (not merely equal registers).
    pub fn same_block(&self, other: &ConstRef) -> bool {
        Arc::ptr_eq(&self.chunk, &other.chunk) && self.off == other.off && self.len == other.len
    }
}

impl std::fmt::Debug for ConstRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ConstRef({}+{})", self.off, self.len)
    }
}

/// The region being written.
pub struct BlockWriter<'a> {
    chunk: &'a Arc<Chunk>,
    off: u32,
    slice: &'a mut [[u32; 4]],
}

impl BlockWriter<'_> {
    pub fn regs(&mut self) -> &mut [[u32; 4]] {
        self.slice
    }
    pub fn finish(self) -> ConstRef {
        ConstRef { chunk: self.chunk.clone(), off: self.off, len: self.slice.len() as u32 }
    }
}

/// The writer side: one per thread, owns the chunk being filled.
pub struct ConstArena {
    chunk: Arc<Chunk>,
    cap: usize,
    used: usize,
    chunks_made: u64,
}

impl ConstArena {
    /// `chunk_regs` registers (16 bytes each) per chunk; a block larger than that gets a chunk of its own.
    pub fn new(chunk_regs: usize) -> Self {
        Self { chunk: Self::make(chunk_regs), cap: chunk_regs, used: 0, chunks_made: 1 }
    }
    fn make(regs: usize) -> Arc<Chunk> {
        Arc::new(Chunk { buf: UnsafeCell::new(vec![[0u32; 4]; regs].into_boxed_slice()) })
    }
    /// Room for `regs` registers, zeroed. A full chunk is left to its readers and a new one starts.
    pub fn alloc(&mut self, regs: usize) -> BlockWriter<'_> {
        if self.used + regs > self.cap {
            self.cap = self.cap.max(regs);
            self.chunk = Self::make(self.cap);
            self.used = 0;
            self.chunks_made += 1;
        }
        let off = self.used;
        self.used += regs;
        // SAFETY: [off, off+regs) is handed out once (used only grows within a chunk), so this
        // is the only reference to those registers until `finish`.
        let all: &mut [[u32; 4]] = unsafe { &mut **self.chunk.buf.get() };
        BlockWriter { chunk: &self.chunk, off: off as u32, slice: &mut all[off..off + regs] }
    }
    pub fn chunks_made(&self) -> u64 {
        self.chunks_made
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_are_disjoint_zeroed_and_readable() {
        let mut a = ConstArena::new(64);
        let mut w = a.alloc(10);
        w.regs()[3] = [1, 2, 3, 4];
        let r1 = w.finish();
        let mut w = a.alloc(5);
        assert!(w.regs().iter().all(|r| *r == [0; 4]));
        w.regs()[0] = [9; 4];
        let r2 = w.finish();
        assert_eq!(r1.regs().len(), 10);
        assert_eq!(r1.regs()[3], [1, 2, 3, 4]);
        assert_eq!(r2.regs()[0], [9; 4]);
        assert!(!r1.same_block(&r2) && r1.same_block(&r1.clone()));
        assert_eq!(a.chunks_made(), 1);
    }

    #[test]
    fn a_full_chunk_rolls_over_and_survives_while_referenced() {
        let mut a = ConstArena::new(8);
        let mut w = a.alloc(6);
        w.regs()[5] = [7; 4];
        let old = w.finish();
        let new = a.alloc(6).finish(); // does not fit: a new chunk
        assert_eq!(a.chunks_made(), 2);
        assert_eq!(old.regs()[5], [7; 4]);
        assert_eq!(new.regs()[5], [0; 4]);
        let big = a.alloc(20).finish(); // larger than a chunk: a chunk of its own
        assert_eq!(big.regs().len(), 20);
    }

    #[test]
    fn blocks_can_be_read_on_another_thread() {
        let mut a = ConstArena::new(32);
        let mut refs = Vec::new();
        for k in 0..4u32 {
            let mut w = a.alloc(4);
            w.regs()[0] = [k; 4];
            refs.push(w.finish());
        }
        let sum = std::thread::spawn(move || refs.iter().map(|r| r.regs()[0][0]).sum::<u32>()).join().unwrap();
        assert_eq!(sum, 6);
    }
}

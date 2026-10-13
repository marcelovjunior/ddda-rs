//! Local memory access policy for the two exact 0x00cf2cb0 implementations.
//! No decoder state is cached here. Pages are checked for each scalar access
//! and each forward-copy page run; unbacked/faulting accesses use Mem.
use ddnative_base::mem::{Mem, PAGE, PageTable, READ, WRITE, load_at, store_at};
use std::ptr::NonNull;

pub(super) struct Access<'a, const FAST: bool> {
    raw: &'a mut dyn Mem,
    pages: Option<NonNull<PageTable>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::GuestMemory;

    #[test]
    fn scalar_accesses_keep_unaligned_and_cross_page_values() {
        ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::Relaxed);
        let mut memory = GuestMemory::new();
        memory.map(0x20000, 8192, READ | WRITE).unwrap();
        for offset in [0, 1, 4092, 4093, 4094, 4095] {
            let address = 0x20000 + offset;
            Access::<true>::new(&mut memory).set_u32(address, 0xa17349ef);
            assert_eq!(memory.u32(address), 0xa17349ef);
            assert_eq!(Access::<true>::new(&mut memory).u32(address), 0xa17349ef);
            memory.set_u32(address, 0x39fe4127);
            assert_eq!(Access::<true>::new(&mut memory).u32(address), 0x39fe4127);
        }
    }

    #[test]
    fn overlapping_forward_runs_cross_both_page_boundaries() {
        for distance in [1, 2, 3, 31, 4093] {
            let mut memory = GuestMemory::new();
            memory.map(0x20000, 0x4000, READ | WRITE).unwrap();
            let mut expected: Vec<u8> = (0..0x4000).map(|i| (i * 73 + 19) as u8).collect();
            crate::fastmem::write_bulk(&mut memory, 0x20000, &expected);
            let destination = 0xffd;
            for i in 0..517 {
                expected[destination + i] = expected[destination - distance + i];
            }
            let mut output = 0x20000 + destination as u32 - 1;
            let mut source = output - distance as u32;
            Access::<true>::new(&mut memory).copy(&mut output, &mut source, 517);
            let mut actual = vec![0; 0x4000];
            memory.read_bytes(0x20000, &mut actual);
            assert_eq!(actual, expected, "distance={distance}");
            assert_eq!(output, 0x20000 + destination as u32 + 516);
            assert_eq!(source, output - distance as u32);
        }
    }

    #[test]
    fn lazy_output_and_fault_prefix_match_byte_accesses() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        // Lazy destination materializes only at its first write.
        let mut memory = GuestMemory::new();
        memory.map(0x20000, 4096, READ | WRITE).unwrap();
        memory.map_lazy(0x30000, 4096, READ | WRITE).unwrap();
        for i in 0..16 {
            memory.set_u8(0x20000 + i, i as u8);
        }
        Access::<true>::new(&mut memory).copy(&mut 0x2ffff, &mut 0x1ffff, 16);
        for i in 0..16 {
            assert_eq!(memory.u8(0x30000 + i), i as u8);
        }

        // Fault on the fifth source byte: the same four writes survive.
        let mut runs = Vec::new();
        for fast in [false, true] {
            let mut memory = GuestMemory::new();
            memory.map(0x20000, 4096, READ | WRITE).unwrap();
            memory.map(0x30000, 4096, READ | WRITE).unwrap();
            for i in 0..4 {
                memory.set_u8(0x20ffc + i, 31 + i as u8);
            }
            let (mut output, mut source) = (0x2ffff, 0x20ffb);
            let result = catch_unwind(AssertUnwindSafe(|| {
                if fast {
                    Access::<true>::new(&mut memory).copy(&mut output, &mut source, 8);
                } else {
                    Access::<false>::new(&mut memory).copy(&mut output, &mut source, 8);
                }
            }));
            assert!(result.is_err());
            let mut bytes = [0; 8];
            memory.read_bytes(0x30000, &mut bytes);
            runs.push((output, source, bytes));
        }
        assert_eq!(runs[0], runs[1]);
        assert_eq!(runs[0].2, [31, 32, 33, 34, 0, 0, 0, 0]);
    }
}

impl<'a, const FAST: bool> Access<'a, FAST> {
    pub(super) fn new(raw: &'a mut dyn Mem) -> Self {
        let pages = if FAST { raw.page_table() } else { None };
        Self { raw, pages }
    }

    #[inline]
    fn pointer(&self, address: u32, width: u32, permission: u8) -> Option<*mut u8> {
        let offset = address % PAGE;
        if offset + width > PAGE {
            return None;
        }
        // SAFETY: Mem::page_table keeps the table and its backed/retired pages
        // alive throughout this borrow, including concurrent unmapping.
        let table = unsafe { self.pages?.as_ref() };
        let index = (address / PAGE) as usize;
        if table.perm(index) & permission != permission {
            return None;
        }
        let page = NonNull::new(table.page(index))?;
        // SAFETY: this access stays within that backed page.
        Some(unsafe { page.as_ptr().add(offset as usize) })
    }

    #[inline]
    pub(super) fn u8(&self, address: u32) -> u8 {
        if FAST {
            if let Some(pointer) = self.pointer(address, 1, READ) {
                // SAFETY: checked live, backed, readable page byte.
                return unsafe { load_at(pointer, 1) as u8 };
            }
        }
        self.raw.u8(address)
    }

    #[inline]
    pub(super) fn u32(&self, address: u32) -> u32 {
        if FAST {
            if let Some(pointer) = self.pointer(address, 4, READ) {
                // SAFETY: checked live, backed, readable four-byte span.
                return unsafe { load_at(pointer, 4) };
            }
        }
        self.raw.u32(address)
    }

    #[inline]
    pub(super) fn set_u8(&mut self, address: u32, value: u8) {
        if FAST {
            if let Some(pointer) = self.pointer(address, 1, WRITE) {
                // SAFETY: checked live, backed, writable byte.
                unsafe {
                    store_at(pointer, 1, u32::from(value));
                }
                return;
            }
        }
        self.raw.set_u8(address, value);
    }

    #[inline]
    pub(super) fn set_u32(&mut self, address: u32, value: u32) {
        if FAST {
            if let Some(pointer) = self.pointer(address, 4, WRITE) {
                // SAFETY: checked live, backed, writable four-byte span.
                unsafe {
                    store_at(pointer, 4, value);
                }
                return;
            }
        }
        self.raw.set_u32(address, value);
    }

    /// Translated copy order from 0x00cf2cb0 (DDDA.exe 2017-12-13 Steam).
    /// Addresses point one byte before their next byte, as in the original.
    /// Copy forward, reading each byte *after* its predecessor was written.
    /// memcpy/memmove or a whole-source snapshot is wrong for distance < len.
    pub(super) fn copy(&mut self, output: &mut u32, source: &mut u32, mut count: u32) {
        // Keep the existing leaf's cooperative preemption boundary per copy.
        let pages = self.raw.page_table();
        if FAST {
            self.pages = pages;
        }
        while count != 0 {
            let src = source.wrapping_add(1);
            let dst = output.wrapping_add(1);
            if FAST {
                if let (Some(read), Some(write)) = (self.pointer(src, 1, READ), self.pointer(dst, 1, WRITE)) {
                    let run = count.min(PAGE - src % PAGE).min(PAGE - dst % PAGE);
                    // SAFETY: both runs lie within checked live pages. These
                    // are atomic byte loads/stores, not Rust references or a
                    // non-overlapping bulk copy. Aliasing and distance-one
                    // repetitions therefore keep the original order, also
                    // when ATOMIC_ACCESSES is enabled for parallel guests.
                    for i in 0..run as usize {
                        unsafe {
                            store_at(write.add(i), 1, load_at(read.add(i), 1));
                        }
                    }
                    *source = source.wrapping_add(run);
                    *output = output.wrapping_add(run);
                    count -= run;
                    continue;
                }
            }
            // Lazy pages and denied/missing mappings retain the existing
            // read-before-write behavior and exact fault location.
            *source = src;
            *output = dst;
            let byte = self.raw.fast_u8(pages, src);
            self.raw.fast_set_u8(pages, dst, byte);
            count -= 1;
        }
    }
}

//! Local access policies for the exact 0x00cf2cb0 implementations.
//! FAST=2 retains bounded page spans only for the current invocation.
//! Unbacked/faulting accesses retain Mem; no persistent decoder state.
use ddnative_base::mem::Pages;
use ddnative_base::mem::{Mem, PAGE, PageTable, READ, WRITE, load_at, store_at};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) struct Access<'a, const FAST: bool, const WIDE: bool = false> {
    raw: &'a mut dyn Mem,
    pages: Option<NonNull<PageTable>>,
    host_pages: Option<Pages>,
    output_page: u32,
    output_pointer: Option<*mut u8>,
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
    fn wide_matches_keep_overlap_and_page_edges() {
        ddnative_base::mem::ATOMIC_ACCESSES.store(true, Ordering::Relaxed);
        for distance in [1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 31, 32, 257, 4093] {
            for destination in [0xff8, 0xffd, 0x1000, 0x1003] {
                for count in [0, 1, 3, 4, 7, 8, 9, 15, 32, 258, 517] {
                    if distance > destination {
                        continue;
                    }
                    let mut memory = GuestMemory::new();
                    memory.map(0x20000, 0x4000, READ | WRITE).unwrap();
                    let mut expected: Vec<u8> = (0..0x4000).map(|i| (i * 73 + 19) as u8).collect();
                    crate::fastmem::write_bulk(&mut memory, 0x20000, &expected);
                    for i in 0..count {
                        expected[destination + i] = expected[destination - distance + i];
                    }
                    let mut output = 0x20000 + destination as u32 - 1;
                    let mut source = output - distance as u32;
                    Access::<true, true>::new(&mut memory).copy(&mut output, &mut source, count as u32);
                    let mut actual = vec![0; 0x4000];
                    memory.read_bytes(0x20000, &mut actual);
                    assert_eq!(
                        actual, expected,
                        "distance={distance} destination={destination} count={count}"
                    );
                }
            }
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
        for implementation in 0..3 {
            let mut memory = GuestMemory::new();
            memory.map(0x20000, 4096, READ | WRITE).unwrap();
            memory.map(0x30000, 4096, READ | WRITE).unwrap();
            for i in 0..4 {
                memory.set_u8(0x20ffc + i, 31 + i as u8);
            }
            let (mut output, mut source) = (0x2ffff, 0x20ffb);
            let result = catch_unwind(AssertUnwindSafe(|| {
                if implementation == 2 {
                    Access::<true, true>::new(&mut memory).copy(&mut output, &mut source, 8);
                } else if implementation == 1 {
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
        assert_eq!(runs[0], runs[2]);
        assert_eq!(runs[0].2, [31, 32, 33, 34, 0, 0, 0, 0]);
    }
}

impl<'a, const FAST: bool, const WIDE: bool> Access<'a, FAST, WIDE> {
    pub(super) fn new(raw: &'a mut dyn Mem) -> Self {
        let pages = if FAST && !WIDE { raw.page_table() } else { None };
        let host_pages = if WIDE { Some(Pages::of(raw)) } else { None };
        Self {
            raw,
            pages,
            host_pages,
            output_page: 0,
            output_pointer: None,
        }
    }

    #[inline]
    fn pointer(&self, address: u32, width: u32, permission: u8) -> Option<*mut u8> {
        if width > PAGE {
            return None;
        }
        if WIDE {
            return self.host_pages?.bytes(address, width as usize, permission);
        }
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
    pub(super) fn read_span(&self, address: u32, width: u32) -> Option<*mut u8> {
        self.pointer(address, width, READ)
    }

    #[inline]
    pub(super) fn pair(&self, address: u32) -> u32 {
        if let Some(pointer) = self.pointer(address, 2, READ) {
            // SAFETY: two checked input bytes, same logical refill as cf2cb0.
            unsafe { load_at(pointer, 2) }
        } else {
            u32::from(self.u8(address)) | (u32::from(self.u8(address.wrapping_add(1))) << 8)
        }
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
        if WIDE {
            let page = address & !(PAGE - 1);
            if self.output_pointer.is_none() || self.output_page != page {
                self.output_pointer = self.pointer(page, PAGE, WRITE);
                self.output_page = page;
            }
            if let Some(pointer) = self.output_pointer {
                // SAFETY: literal writes remain inside this checked span.
                unsafe {
                    store_at(pointer.add((address % PAGE) as usize), 1, u32::from(value));
                }
                return;
            }
        }
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
        // GuestMemory::page_table uses this same pending-yield flag.
        // Keep the cooperative boundary when requested; otherwise reuse
        // the function-entry table rather than a virtual call per match.
        let handover = !WIDE || ddnative_host::PREEMPT.load(Ordering::Relaxed);
        let pages = if handover { self.raw.page_table() } else { self.pages };
        if handover { self.output_pointer = None; }
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
                    let mut i = 0usize;
                    if WIDE {
                        // Translated from 0x00cf2cb0: a forward match may read
                        // bytes it has just produced. Group independent
                        // eight-byte words, retaining ordered bytes for short
                        // distances. No store crosses the checked page span.
                        let distance = dst.wrapping_sub(src) as usize;
                        if run >= 16 && (distance >= 8 || matches!(distance, 1 | 2 | 4)) {
                            // Align output once. Wide unaligned source loads
                            // assemble two aligned atomic words inside the
                            // same page; no byte loop or policy check per word.
                            while i < run as usize && (write as usize + i) & 7 != 0 {
                                unsafe {
                                    store_at(write.add(i), 1, load_at(read.add(i), 1));
                                }
                                i += 1;
                            }
                            if distance < 8 {
                                // These periods divide eight. Read only the
                                // existing seed, never future output bytes.
                                let pattern = unsafe {
                                    match distance {
                                        1 => u64::from(load_at(read.add(i), 1)) * 0x0101_0101_0101_0101,
                                        2 => u64::from(load_at(read.add(i), 2)) * 0x0001_0001_0001_0001,
                                        4 => u64::from(load_at(read.add(i), 4)) * 0x0000_0001_0000_0001,
                                        _ => unreachable!(),
                                    }
                                };
                                while i + 8 <= run as usize {
                                    unsafe {
                                        write_word(write.add(i), pattern);
                                    }
                                    i += 8;
                                }
                            } else {
                                while i + 8 <= run as usize {
                                    // SAFETY: checked spans; distance >= 8
                                    // excludes within-word overlap.
                                    unsafe {
                                        write_word(write.add(i), read_word(read.add(i)));
                                    }
                                    i += 8;
                                }
                            }
                        }
                    }
                    for i in i..run as usize {
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

// Wide accesses follow GuestMemory's aligned AtomicU64 read policy. For
// unaligned reads combine aligned words from the same checked page. No Rust
// slices or non-atomic references to concurrently accessible guest bytes.
#[inline]
unsafe fn read_word(pointer: *mut u8) -> u64 {
    unsafe {
        if pointer as usize & 7 == 0 {
            u64::from_le((*(pointer as *const AtomicU64)).load(Ordering::Acquire))
        } else {
            let offset = pointer as usize & 7;
            let aligned = pointer.sub(offset) as *const AtomicU64;
            let low = u64::from_le((*aligned).load(Ordering::Acquire));
            let high = u64::from_le((*aligned.add(1)).load(Ordering::Acquire));
            (low >> (offset * 8)) | (high << (64 - offset * 8))
        }
    }
}

#[inline]
unsafe fn write_word(pointer: *mut u8, value: u64) {
    unsafe {
        if pointer as usize & 7 == 0 {
            (*(pointer as *const AtomicU64)).store(value.to_le(), Ordering::Release);
        } else {
            store_at(pointer, 4, value as u32);
            store_at(pointer.add(4), 4, (value >> 32) as u32);
        }
    }
}

#[cfg(windows)]
#[test]
fn process_private_bytes_follow_a_real_committed_allocation() {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protection: u32) -> *mut c_void;
        fn VirtualFree(address: *mut c_void, size: usize, kind: u32) -> i32;
    }
    struct Allocation(*mut c_void);
    impl Drop for Allocation {
        fn drop(&mut self) { unsafe { VirtualFree(self.0, 0, 0x8000); } }
    }
    let before = ddruntime::load_log::process_memory().expect("process counters available");
    let bytes = 8 << 20;
    // SAFETY: reserve/commit a private region and touch only its own pages.
    let allocation = Allocation(unsafe { VirtualAlloc(std::ptr::null_mut(), bytes, 0x3000, 4) });
    assert!(!allocation.0.is_null());
    for offset in (0..bytes).step_by(4096) {
        unsafe { allocation.0.cast::<u8>().add(offset).write_volatile(42); }
    }
    let committed = ddruntime::load_log::process_memory().unwrap();
    assert!(committed.private_bytes >= before.private_bytes + bytes as u64);
    assert!(committed.peak_working_set_bytes >= committed.working_set_bytes);
    assert!(committed.working_set_bytes > 0);
    drop(allocation);
    let released = ddruntime::load_log::process_memory().unwrap();
    assert!(released.private_bytes + bytes as u64 <= committed.private_bytes);
}

#[cfg(windows)]
#[test]
#[ignore = "explicit CPU-only guest-page allocation diagnostic"]
fn guest_backing_and_retirement_cost() {
    use ddnative_base::mem::Mem;
    use ddruntime::{load_log::process_memory, memory::GuestMemory};
    let mut memory = GuestMemory::new();
    let baseline = process_memory().unwrap().private_bytes;
    for round in 0..3 {
        memory.map_lazy(0x100000, 8 << 20, 3).unwrap();
        for address in (0x100000..0x900000).step_by(4096) { memory.set_u8(address, 42); }
        let backed = process_memory().unwrap().private_bytes;
        println!("round={round} guest_backed_bytes={} private_delta={}", memory.backed_pages() * 4096, backed as i64 - baseline as i64);
        memory.unmap(0x100000, 8 << 20);
        println!("round={round} after_unmap_backed_bytes={} private_delta={}", memory.backed_pages() * 4096, process_memory().unwrap().private_bytes as i64 - baseline as i64);
    }
    drop(memory);
    println!("after_drop_private_delta={}", process_memory().unwrap().private_bytes as i64 - baseline as i64);
}

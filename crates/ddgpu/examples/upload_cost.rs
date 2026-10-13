//! CPU-only upload preparation benchmark. Run directly with rustc -O to
//! avoid linking/building the renderer; this includes the production helper.
#[path = "../src/upload_data.rs"]
mod upload_data;

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

struct Counted;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { unsafe { System.dealloc(ptr, layout) } }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;

fn measure(name: &str, repeats: usize, mut run: impl FnMut()) {
    for _ in 0..8 { run(); }
    let before = ALLOCS.load(Ordering::Relaxed);
    let started = Instant::now();
    for _ in 0..repeats { run(); }
    let elapsed = started.elapsed();
    let allocations = ALLOCS.load(Ordering::Relaxed) - before;
    println!("{name}: {:.3} us/upload, {:.3} alloc-or-realloc/upload", elapsed.as_secs_f64() * 1e6 / repeats as f64, allocations as f64 / repeats as f64);
}

fn main() {
    let new_first = std::env::args().any(|arg| arg == "--new-first");
    for size in [4096, 65536, 1048576] {
        let repeats = (128 * 1024 * 1024 / size).max(128);
        let mut bytes = vec![0x7b; size];
        for new in if new_first { [true, false] } else { [false, true] } {
            measure(&format!("buffer{size} {}", if new { "owned" } else { "clone" }), repeats, || {
                if new {
                    upload_data::pad_buffer(black_box(&mut bytes));
                    black_box(&bytes);
                } else {
                    let mut padded = black_box(&bytes).clone();
                    padded.resize(size.next_multiple_of(4).max(4), 0);
                    black_box(padded);
                }
            });
        }
    }
    for (row, rows, depth, padding) in [(4096, 1024, 1, 0), (1024, 64, 1, 0), (1024, 64, 1, 16), (256, 16, 8, 0)] {
        let pitch = row + padding;
        let bytes = vec![0x3d; (pitch * rows * depth) as usize];
        let repeats = 256;
        for new in if new_first { [true, false] } else { [false, true] } {
            measure(&format!("image{row}x{rows}x{depth}+{padding} {}", if new { "borrow" } else { "pack" }), repeats, || {
                if new {
                    black_box(upload_data::direct_image(black_box(&bytes), row, rows, depth, pitch));
                } else {
                    let mut packed = Vec::new();
                    for z in 0..depth {
                        let slice = &black_box(&bytes)[(z * pitch * rows) as usize..];
                        for y in 0..rows {
                            let start = (y * pitch) as usize;
                            packed.extend_from_slice(&slice[start..start + row as usize]);
                        }
                    }
                    black_box(packed);
                }
            });
        }
    }
}

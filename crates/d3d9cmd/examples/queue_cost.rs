//! CPU-only queue transport benchmark. No game, window or GPU.
//! cargo run --release -p d3d9cmd --example queue_cost -- 200000
use d3d9cmd::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

struct Allocator;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for Allocator {
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
static ALLOCATOR: Allocator = Allocator;

struct Sink(u64);
impl GraphicsBackend for Sink {
    fn draw(&mut self, draw: &DrawCall) { self.0 = self.0.wrapping_add(std::hint::black_box(draw.start as u64)); }
    fn upload(&mut self, _: Upload) {}
    fn clear(&mut self, _: &ClearCall) {}
    fn present(&mut self, _: &Target) {}
    fn copy(&mut self, _: &CopyCall) {}
    fn fill(&mut self, _: &FillCall) {}
    fn read_back(&mut self, _: &Target) -> Option<Vec<u8>> { None }
    fn query_begin(&mut self, _: u32) {}
    fn query_end(&mut self, _: u32) {}
    fn query_result(&mut self, _: u32) -> Option<u64> { None }
    fn gamma(&mut self, _: Option<&GammaRamp>) {}
    fn release(&mut self, _: u32) {}
}

fn main() {
    let count: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(200_000);
    let draw = DrawCall {
        primitive: 4, primitives: 1, start: 3, base_vertex: 0, min_index: 0, vertex_count: 3,
        vertices: Vertices::Streams(vec![(0, 7, 0, 16, 1)].into()), indices: Indices::Buffer(8, 101),
        vertex_shader: Some(vec![1, 2].into()), pixel_shader: Some(vec![3, 4].into()), declaration: vec![[0; 8]].into(), fvf: 0,
        vs_constants: Constants { f: vec![[0; 4]; 256], ..Default::default() }.into(),
        ps_constants: Constants { f: vec![[0; 4]; 224], ..Default::default() }.into(),
        textures: vec![].into(), render_states: vec![(7, 1)].into(), viewport: [0; 6], scissor: [0; 4],
        targets: [None, None, None, None], depth: None,
    };
    let mut queue = DeferredBackend::new(Box::new(Sink(0)));
    for _ in 0..10_000 { queue.draw(&draw); }
    queue.drain();
    for sample in 0..7 {
        let allocations = ALLOCS.load(Ordering::Relaxed);
        let start = Instant::now();
        for _ in 0..count { queue.draw(std::hint::black_box(&draw)); }
        let recorded = start.elapsed();
        // mark/passed waits for execution without introducing a result channel
        // allocation into the measurement. Acquire observes completed draws.
        let mark = queue.mark();
        while !queue.passed(mark) { std::thread::yield_now(); }
        let total = start.elapsed();
        let allocations = ALLOCS.load(Ordering::Relaxed) - allocations;
        println!("sample={sample} draws={count} record_ns={:.1} complete_ns={:.1} allocations={allocations}",
            recorded.as_nanos() as f64 / count as f64, total.as_nanos() as f64 / count as f64);
    }
}

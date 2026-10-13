//! The game's render worker's host-side costs (brief L144), a light build
//! that needs the installed game (DDDA_EXE):
//!
//!     cargo run --release -p ddruntime --no-default-features --example render_worker
//!
//! A device set up as tests/draw_path.rs has it (six mipmapped textures,
//! buffers, a declaration, shaders of the game's size, full constants), in
//! parallel mode with a backend that does nothing, called the way lifted
//! code calls (x86::call_ext, arguments pushed through the page table).
//! Measured per frame-like iteration: a dynamic vertex buffer written
//! (Lock with DISCARD, 4 KB, Unlock), SetTexture, 16 vertex and 4 pixel
//! constants, DrawIndexedPrimitive; the heap allocations per draw; and the
//! host's guest-memory reads (u32 through `&dyn Mem`, bulk reads).
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu};
use ddruntime::session::{Outcome, Session, STACK_TOP};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

struct CountAlloc;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;
unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}

const SCRATCH: u32 = 0x7EFF_0000;
const PP: u32 = SCRATCH + 0x200;
const OUT: u32 = SCRATCH + 0x300;
const DATA: u32 = SCRATCH + 0x1000;

fn device_session() -> (Session, u32) {
    let path = std::env::var("DDDA_EXE").unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    let mut s = Session::new(&path).expect("session (DDDA_EXE)");
    ddruntime::threads::set_parallel(true);
    let token = |s: &Session, n: &str| s.imports.iter().find(|i| i.full_name() == n).unwrap().token;
    let (wc, name) = (SCRATCH, SCRATCH + 0x100);
    s.mem.set_u16(name, b'W' as u16);
    s.mem.set_u32(wc + 8, token(&s, "USER32.dll!DefWindowProcW"));
    s.mem.set_u32(wc + 40, name);
    let Outcome::Returned(_) = s.call(token(&s, "USER32.dll!RegisterClassExW"), 0, &[wc]) else { panic!() };
    let Outcome::Returned(cpu) = s.call(token(&s, "USER32.dll!CreateWindowExW"), 0, &[0, name, 0, 0x00CA_0000, 0, 0, 1286, 749, 0, 0, 0x0040_0000, 0]) else { panic!() };
    let hwnd = cpu.eax;
    let Outcome::Returned(cpu) = s.call(token(&s, "d3d9.dll!Direct3DCreate9"), 0, &[32]) else { panic!() };
    let d3d = cpu.eax;
    for (k, v) in [0, 0, 0, 0, 0, 0, 1, hwnd, 1, 1, 75, 0, 0, 0u32].into_iter().enumerate() {
        s.mem.set_u32(PP + 4 * k as u32, v);
    }
    let create = s.mem.u32(s.mem.u32(d3d) + 4 * 16);
    let Outcome::Returned(_) = s.call(create, 0, &[d3d, 0, 1, hwnd, 0x46, PP, OUT]) else { panic!() };
    let dev = s.mem.u32(OUT);
    (s, dev)
}

/// Method `slot` of `obj`, called as lifted code calls it; eax.
fn com(m: &mut dyn Mem, obj: u32, slot: u32, args: &[u32]) -> u32 {
    let target = m.u32(m.u32(obj) + 4 * slot);
    let pt = m.page_table();
    let mut cpu = Cpu { esp: STACK_TOP - 0x1000, ..Cpu::default() };
    for &a in args.iter().rev() {
        cpu.esp -= 4;
        m.fast_set_u32(pt, cpu.esp, a);
    }
    cpu.esp -= 4;
    m.fast_set_u32(pt, cpu.esp, obj);
    cpu.esp -= 4;
    m.fast_set_u32(pt, cpu.esp, 0x0040_1000);
    x86::call_ext(m, &mut cpu, target);
    cpu.eax
}

fn out(m: &mut dyn Mem, obj: u32, slot: u32, args: &[u32]) -> u32 {
    assert_eq!(com(m, obj, slot, args), 0, "slot {slot}");
    m.u32(OUT)
}

fn put(m: &mut dyn Mem, at: u32, words: &[u32]) {
    for (k, w) in words.iter().enumerate() {
        m.set_u32(at + 4 * k as u32, *w);
    }
}

/// vs_3_0 of the game's size: `n` adds of constants.
fn vertex_shader(n: u32) -> Vec<u32> {
    let mut t = vec![0xFFFE_0300, 0x0200_001F, 0x8000_0000, 0x900F_0000, 0x0200_001F, 0x8000_0005, 0x900F_0001,
                     0x0200_001F, 0x8000_0000, 0xE00F_0000, 0x0200_001F, 0x8000_0005, 0xE00F_0001,
                     0x0200_0001, 0x800F_0000, 0x90E4_0000];
    for k in 0..n {
        t.extend([0x0300_0002, 0x800F_0000, 0x80E4_0000, 0xA0E4_0000 | (k % 256)]);
    }
    t.extend([0x0200_0001, 0xE00F_0000, 0x80E4_0000, 0x0200_0001, 0xE00F_0001, 0x90E4_0001, 0x0000_FFFF]);
    t
}

/// ps_3_0 of the game's size sampling six textures.
fn pixel_shader(n: u32) -> Vec<u32> {
    let mut t = vec![0xFFFF_0300, 0x0200_001F, 0x8000_0005, 0x900F_0000];
    for s in 0..6 {
        t.extend([0x0200_001F, 0x9000_0000, 0xA00F_0800 | s]);
    }
    t.extend([0x0300_0042, 0x800F_0000, 0x90E4_0000, 0xA0E4_0800]);
    for k in 0..n {
        t.extend([0x0300_0002, 0x800F_0000, 0x80E4_0000, 0xA0E4_0000 | (k % 32)]);
    }
    t.extend([0x0200_0001, 0x800F_0800, 0x80E4_0000, 0x0000_FFFF]);
    t
}

struct Null;
impl d3d9cmd::GraphicsBackend for Null {
    fn upload(&mut self, u: d3d9cmd::Upload) {
        // as ddgpu: the written bytes go back to the recorder (L144)
        let (d3d9cmd::Upload::Buffer { bytes, .. } | d3d9cmd::Upload::Image { bytes, .. }) = u;
        d3d9cmd::bytes::give(bytes);
    }
    fn draw(&mut self, _: &d3d9cmd::DrawCall) {}
    fn clear(&mut self, _: &d3d9cmd::ClearCall) {}
    fn present(&mut self, _: &d3d9cmd::Target) {}
    fn copy(&mut self, _: &d3d9cmd::CopyCall) {}
    fn fill(&mut self, _: &d3d9cmd::FillCall) {}
    fn read_back(&mut self, _: &d3d9cmd::Target) -> Option<Vec<u8>> {
        None
    }
    fn query_begin(&mut self, _: u32) {}
    fn query_end(&mut self, _: u32) {}
    fn query_result(&mut self, _: u32) -> Option<u64> {
        None
    }
    fn gamma(&mut self, _: Option<&d3d9cmd::GammaRamp>) {}
    fn release(&mut self, _: u32) {}
}

/// End at once: a session's process otherwise hangs in its exit.
fn quit() -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn TerminateProcess(process: isize, code: u32) -> i32;
    }
    // SAFETY: plain Win32 calls on this process
    unsafe { TerminateProcess(GetCurrentProcess(), 0) };
    std::process::exit(0)
}

fn main() {
    let (mut s, dev) = device_session();
    ddruntime::d3d9draw::set_backend(Box::new(Null));
    let mem: &mut dyn Mem = &mut s.mem;
    let mut textures = Vec::new();
    for k in 0..6 {
        let tex = out(mem, dev, 23, &[256, 256, 9, 0, 21, 1, OUT, 0]);
        assert_eq!(com(mem, dev, 65, &[k, tex]), 0);
        for (t, v) in [(1u32, 3u32), (2, 3), (5, 2), (6, 2), (7, 2)] {
            com(mem, dev, 69, &[k, t, v]);
        }
        textures.push(tex);
    }
    // a dynamic vertex buffer (D3DUSAGE_DYNAMIC | WRITEONLY, default pool) and an index buffer
    let vb = out(mem, dev, 26, &[64 * 1024, 0x208, 0, 0, OUT, 0]);
    let ib = out(mem, dev, 27, &[32 * 1024, 8, 101, 1, OUT, 0]);
    assert_eq!(com(mem, dev, 100, &[0, vb, 0, 32]), 0);
    assert_eq!(com(mem, dev, 104, &[ib]), 0);
    put(mem, DATA, &[0x0000_0000, 0x0000_0003, 0x0010_0000, 0x0005_0001, 0x0000_00FF, 0x0000_0011]);
    let decl = out(mem, dev, 86, &[DATA, OUT]);
    assert_eq!(com(mem, dev, 87, &[decl]), 0);
    put(mem, DATA, &vertex_shader(150));
    let vsh = out(mem, dev, 91, &[DATA, OUT]);
    put(mem, DATA, &pixel_shader(100));
    let psh = out(mem, dev, 106, &[DATA, OUT]);
    assert_eq!(com(mem, dev, 92, &[vsh]), 0);
    assert_eq!(com(mem, dev, 107, &[psh]), 0);
    let consts: Vec<u32> = (0..256 * 4).map(|k| (k as f32 * 0.25).to_bits()).collect();
    put(mem, DATA, &consts);
    assert_eq!(com(mem, dev, 94, &[0, DATA, 256]), 0);
    assert_eq!(com(mem, dev, 109, &[0, DATA, 32]), 0);
    for (state, v) in [(7u32, 1u32), (14, 1), (22, 3), (27, 0), (174, 0)] {
        com(mem, dev, 57, &[state, v]);
    }
    // RW_OBJECTS extra live textures (the game holds thousands of objects)
    let extra: u32 = std::env::var("RW_OBJECTS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    for _ in 0..extra {
        out(mem, dev, 23, &[16, 16, 1, 0, 21, 1, OUT, 0]);
    }
    assert_eq!(com(mem, dev, 41, &[]), 0); // BeginScene

    let no_lock = std::env::var_os("RW_NOLOCK").is_some();
    let n: u32 = std::env::var("RW_N").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000);
    let mut best = [f64::MAX; 3];
    let mut allocs = 0.0;
    ddruntime::threads::without_lock(|| {
        for round in 0..4 {
            // the frame-like iteration
            let a0 = ALLOCS.load(Ordering::Relaxed);
            let mut each = [0f64; 6];
            let t = Instant::now();
            for k in 0..n {
                let mut at = Instant::now();
                let mut lap = |i: usize| {
                    let now = Instant::now();
                    each[i] += (now - at).as_secs_f64();
                    at = now;
                };
                // Lock(0, 4096, OUT, D3DLOCK_DISCARD), Unlock: 4 KB dirty
                // (RW_NOLOCK: the buffer stays clean)
                if !no_lock {
                    assert_eq!(com(mem, vb, 11, &[0, 4096, OUT, 0x2000]), 0);
                    lap(0);
                    assert_eq!(com(mem, vb, 12, &[]), 0);
                    lap(1);
                }
                assert_eq!(com(mem, dev, 65, &[0, textures[(k % 6) as usize]]), 0);
                lap(2);
                assert_eq!(com(mem, dev, 94, &[16, DATA, 16]), 0);
                lap(3);
                assert_eq!(com(mem, dev, 109, &[0, DATA, 4]), 0);
                lap(4);
                assert_eq!(com(mem, dev, 82, &[4, 0, 0, 1000, 0, 300]), 0);
                lap(5);
            }
            let frame = t.elapsed().as_secs_f64() * 1e9 / n as f64;
            if round == 3 {
                let names = ["Lock", "Unlock", "SetTexture", "SetVSConstantF(16)", "SetPSConstantF(4)", "DrawIndexedPrimitive"];
                let parts: Vec<String> = names.iter().zip(each).map(|(n2, v)| format!("{n2} {:.0}", v * 1e9 / n as f64)).collect();
                println!("per call (ns): {}", parts.join(", "));
            }
            let a = (ALLOCS.load(Ordering::Relaxed) - a0) as f64 / n as f64;
            // the draw alone
            let t = Instant::now();
            for _ in 0..n {
                assert_eq!(com(mem, dev, 82, &[4, 0, 0, 1000, 0, 300]), 0);
            }
            let draw = t.elapsed().as_secs_f64() * 1e9 / n as f64;
            // host reads of guest memory: 1M u32 through &dyn Mem, and a 64 KB bulk read
            let t = Instant::now();
            let mut sum = 0u32;
            for k in 0..1_000_000u32 {
                sum = sum.wrapping_add(mem.u32(DATA + 4 * (k & 1023)));
            }
            std::hint::black_box(sum);
            let read = t.elapsed().as_secs_f64() * 1e9 / 1e6;
            if round > 0 {
                best = [best[0].min(frame), best[1].min(draw), best[2].min(read)];
                allocs = a;
            }
        }
        // a 64 KB block of written (backed) guest pages
        let block = SCRATCH - 0x2_0000;
        for k in 0..16 * 1024u32 {
            mem.set_u32(block + 4 * k, k);
        }
        let mut buf = vec![0u8; 64 * 1024];
        let t = Instant::now();
        for _ in 0..2000 {
            mem.read_bytes(block, &mut buf);
        }
        let gbs = 2000.0 * buf.len() as f64 / t.elapsed().as_secs_f64() / 1e9;
        // a frame's big upload: a 2 MB dynamic buffer locked whole and
        // written (the game re-uploads ~3.5 MB a frame: a 1 MB image, 2 MB
        // and 512 KB buffers), then drawn from
        let big = out(mem, dev, 26, &[2 << 20, 0x208, 0, 0, OUT, 0]);
        assert_eq!(com(mem, dev, 100, &[0, big, 0, 32]), 0);
        let mut upload = f64::MAX;
        for _ in 0..4 {
            let t = Instant::now();
            for _ in 0..50 {
                assert_eq!(com(mem, big, 11, &[0, 0, OUT, 0]), 0);
                let p = mem.u32(OUT);
                for k in (0..2u32 << 20).step_by(4096) {
                    mem.set_u32(p + k, k);
                }
                assert_eq!(com(mem, big, 12, &[]), 0);
                assert_eq!(com(mem, dev, 82, &[4, 0, 0, 1000, 0, 300]), 0);
            }
            upload = upload.min(t.elapsed().as_secs_f64() * 1e6 / 50.0);
        }
        assert_eq!(com(mem, dev, 100, &[0, vb, 0, 32]), 0);
        println!("per iteration (Lock/Unlock 4 KB, SetTexture, 16+4 constants, DrawIndexedPrimitive): {:.0} ns, {allocs:.1} allocations", best[0]);
        println!("2 MB buffer locked, written, drawn from (its upload): {upload:.0} us");
        if std::env::var_os("RW_SPLIT").is_some() {
            let shadow = {
                assert_eq!(com(mem, big, 11, &[0, 0, OUT, 0]), 0);
                let p = mem.u32(OUT);
                assert_eq!(com(mem, big, 12, &[]), 0);
                p
            };
            let time = |f: &mut dyn FnMut()| {
                let t = Instant::now();
                for _ in 0..50 { f(); }
                t.elapsed().as_secs_f64() * 1e6 / 50.0
            };
            let fresh = time(&mut || { std::hint::black_box(vec![0u8; 2 << 20]); });
            let touched = time(&mut || {
                let mut v = vec![0u8; 2 << 20];
                for k in (0..v.len()).step_by(4096) { v[k] = 1; }
                std::hint::black_box(v);
            });
            let mut reuse = vec![0u8; 2 << 20];
            let copy = time(&mut || mem.read_bytes(shadow, &mut reuse));
            let both = time(&mut || {
                let mut v = vec![0u8; 2 << 20];
                mem.read_bytes(shadow, &mut v);
                std::hint::black_box(v);
            });
            println!("2 MB: vec![0] {fresh:.0} us, with its pages touched {touched:.0} us; read_bytes into a reused buffer {copy:.0} us, into a new one {both:.0} us");
        }
        println!("DrawIndexedPrimitive alone: {:.0} ns; guest u32 read through &dyn Mem: {:.2} ns; read_bytes 64 KB (parallel mode): {gbs:.2} GB/s", best[1], best[2]);
    });
    quit();
}

//! What a draw costs the runtime's D3D9 services (L78), measured on a
//! device set up as the game's render thread has it: six mipmapped
//! textures, vertex and index buffers, a declaration, shaders of the
//! game's size, a full constant block, and 16 vertex constants set before
//! each draw. The backend does nothing, so this is the service side only
//! (the state snapshot and the flushes). Needs the installed game
//! (DDDA_EXE); numbers with `-- --nocapture --test-threads=1` (allocation
//! counts include this test's COM argument buffers).
use ddnative_base::mem::Mem;
use ddruntime::session::{Outcome, Session};
use std::time::Instant;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

struct CountAlloc;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;
unsafe impl GlobalAlloc for CountAlloc {
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

const SCRATCH: u32 = 0x7EFF_0000;
const PP: u32 = SCRATCH + 0x200;
const OUT: u32 = SCRATCH + 0x300;
const DATA: u32 = SCRATCH + 0x1000;

fn installed_exe() -> Option<ddexe::ExeFile> {
    let path = std::env::var("DDDA_EXE").unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    ddexe::ExeFile::open(path).ok()
}

/// A session with a window and a D3D9 device (windowed 1280x720, auto
/// depth D24S8): (session, device).
fn device_session() -> Option<(Session, u32)> {
    let exe = installed_exe()?;
    let path = exe.path.to_string_lossy().to_string();
    let mut s = Session::new(&path).expect("session");
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
    Some((s, dev))
}

/// Call COM method `slot` of `obj` with `args` (this first); eax.
fn com(s: &mut Session, obj: u32, slot: u32, args: &[u32]) -> u32 {
    let f = s.mem.u32(s.mem.u32(obj) + 4 * slot);
    let mut a = vec![obj];
    a.extend_from_slice(args);
    match s.call(f, 0, &a) {
        Outcome::Returned(c) => c.eax,
        o => panic!("slot {slot}: {o}"),
    }
}

fn out(s: &mut Session, obj: u32, slot: u32, args: &[u32]) -> u32 {
    assert_eq!(com(s, obj, slot, args), 0, "slot {slot}");
    s.mem.u32(OUT)
}

fn put(s: &mut Session, at: u32, words: &[u32]) {
    for (k, w) in words.iter().enumerate() {
        s.mem.set_u32(at + 4 * k as u32, *w);
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
    fn upload(&mut self, _: d3d9cmd::Upload) {}
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

#[test]
fn a_draw_costs_the_services() {
    let Some((mut s, dev)) = device_session() else {
        eprintln!("skipped: no installed game");
        return;
    };
    ddruntime::d3d9draw::set_backend(Box::new(Null));
    // six 256x256 textures with full mip chains on samplers 0..5
    for k in 0..6 {
        let tex = out(&mut s, dev, 23, &[256, 256, 9, 0, 21, 1, OUT, 0]);
        assert_eq!(com(&mut s, dev, 65, &[k, tex]), 0);
        for (t, v) in [(1u32, 3u32), (2, 3), (5, 2), (6, 2), (7, 2)] {
            com(&mut s, dev, 69, &[k, t, v]);
        }
    }
    // buffers, declaration, shaders
    let vb = out(&mut s, dev, 26, &[64 * 1024, 8, 0, 1, OUT, 0]);
    let ib = out(&mut s, dev, 27, &[32 * 1024, 8, 101, 1, OUT, 0]);
    assert_eq!(com(&mut s, dev, 100, &[0, vb, 0, 32]), 0);
    assert_eq!(com(&mut s, dev, 104, &[ib]), 0);
    // D3DVERTEXELEMENT9 (stream, offset; type, method, usage, index):
    // position float4 at 0, texcoord float2 at 16, D3DDECL_END
    put(&mut s, DATA, &[0x0000_0000, 0x0000_0003, 0x0010_0000, 0x0005_0001, 0x0000_00FF, 0x0000_0011]);
    let decl = out(&mut s, dev, 86, &[DATA, OUT]);
    assert_eq!(com(&mut s, dev, 87, &[decl]), 0);
    let vs = vertex_shader(150);
    put(&mut s, DATA, &vs);
    let vsh = out(&mut s, dev, 91, &[DATA, OUT]);
    let ps = pixel_shader(100);
    put(&mut s, DATA, &ps);
    let psh = out(&mut s, dev, 106, &[DATA, OUT]);
    assert_eq!(com(&mut s, dev, 92, &[vsh]), 0);
    assert_eq!(com(&mut s, dev, 107, &[psh]), 0);
    // a full vertex constant block, 32 pixel constants
    let consts: Vec<u32> = (0..256 * 4).map(|k| (k as f32 * 0.25).to_bits()).collect();
    put(&mut s, DATA, &consts);
    assert_eq!(com(&mut s, dev, 94, &[0, DATA, 256]), 0);
    assert_eq!(com(&mut s, dev, 109, &[0, DATA, 32]), 0);
    for (state, v) in [(7u32, 1u32), (14, 1), (22, 3), (27, 0), (174, 0)] {
        com(&mut s, dev, 57, &[state, v]);
    }
    assert_eq!(com(&mut s, dev, 41, &[]), 0); // BeginScene

    let n = 2000;
    let dirty_samplers = std::env::var_os("DDRUNTIME_BENCH_DIRTY_SAMPLERS").is_some();
    // the calls alone: GetRenderState
    let t = Instant::now();
    for _ in 0..n {
        com(&mut s, dev, 58, &[7, OUT]);
    }
    let call = t.elapsed().as_secs_f64() * 1e6 / n as f64;
    let t = Instant::now();
    for _ in 0..n {
        assert_eq!(com(&mut s, dev, 94, &[16, DATA, 16]), 0);
    }
    let constants = t.elapsed().as_secs_f64() * 1e6 / n as f64;
    let t = Instant::now();
    let allocations = ALLOCS.load(Ordering::Relaxed);
    for k in 0..n {
        if dirty_samplers { assert_eq!(com(&mut s, dev, 69, &[0, 1, 1 + k % 3]), 0); }
        assert_eq!(com(&mut s, dev, 82, &[4, 0, 0, 1000, 0, 300]), 0);
    }
    let draw = t.elapsed().as_secs_f64() * 1e6 / n as f64;
    let allocations = (ALLOCS.load(Ordering::Relaxed) - allocations) as f64 / n as f64;
    eprintln!("per call (us): a service call {call:.2}, SetVertexShaderConstantF(16) {constants:.2}, DrawIndexedPrimitive {draw:.2}");
    eprintln!("allocations per indexed draw (including test COM arguments): {allocations:.2}");
    assert_eq!(com(&mut s, dev, 42, &[]), 0); // EndScene
}

#[test]
fn draws_share_immutable_payloads_and_detach_changed_constants() {
    use std::sync::Arc;
    let Some((mut s, dev)) = device_session() else { return; };
    put(&mut s, DATA, &vertex_shader(8));
    let vs = out(&mut s, dev, 91, &[DATA, OUT]);
    put(&mut s, DATA, &pixel_shader(8));
    let ps = out(&mut s, dev, 106, &[DATA, OUT]);
    put(&mut s, DATA, &[0, 3, 0xff, 17]);
    let decl = out(&mut s, dev, 86, &[DATA, OUT]);
    for (slot, obj) in [(92, vs), (107, ps), (87, decl)] {
        assert_eq!(com(&mut s, dev, slot, &[obj]), 0);
    }
    put(&mut s, DATA, &[1, 2, 3, 4]);
    assert_eq!(com(&mut s, dev, 94, &[0, DATA, 1]), 0);
    assert_eq!(com(&mut s, dev, 41, &[]), 0);
    assert_eq!(com(&mut s, dev, 81, &[4, 0, 1]), 0);
    // Redundant setter must keep the current shared block.
    assert_eq!(com(&mut s, dev, 94, &[0, DATA, 1]), 0);
    assert_eq!(com(&mut s, dev, 81, &[4, 0, 1]), 0);
    put(&mut s, DATA, &[5, 6, 7, 8]);
    assert_eq!(com(&mut s, dev, 94, &[0, DATA, 1]), 0);
    assert_eq!(com(&mut s, dev, 81, &[4, 0, 1]), 0);
    assert_eq!(com(&mut s, dev, 111, &[0, DATA, 1]), 0);
    assert_eq!(com(&mut s, dev, 113, &[0, DATA, 1]), 0);
    assert_eq!(com(&mut s, dev, 81, &[4, 0, 1]), 0);
    // Commands remain valid after their guest shader/declaration objects die.
    for (slot, obj) in [(92, vs), (107, ps), (87, decl)] {
        assert_eq!(com(&mut s, dev, slot, &[0]), 0);
        assert_eq!(com(&mut s, obj, 2, &[]), 0);
    }
    ddruntime::d3d9draw::recorded(|r| {
        let (a, b, c) = (&r.draws[0], &r.draws[1], &r.draws[2]);
        assert_eq!(a, b);
        assert_eq!(a.vs_constants.f.len(), 8);
        assert_eq!(a.ps_constants.f.len(), 8);
        assert!(Arc::ptr_eq(a.vertex_shader.as_ref().unwrap(), c.vertex_shader.as_ref().unwrap()));
        assert!(Arc::ptr_eq(a.pixel_shader.as_ref().unwrap(), c.pixel_shader.as_ref().unwrap()));
        assert_eq!(a.vertex_shader.as_deref().unwrap(), vertex_shader(8));
        assert_eq!(a.pixel_shader.as_deref().unwrap(), pixel_shader(8));
        assert!(Arc::ptr_eq(&a.declaration, &c.declaration));
        assert!(Arc::ptr_eq(&a.vs_constants, &b.vs_constants));
        assert!(Arc::ptr_eq(&a.ps_constants, &c.ps_constants));
        assert!(!Arc::ptr_eq(&a.vs_constants, &c.vs_constants));
        assert_eq!(a.vs_constants.f[0], [1, 2, 3, 4]);
        assert_eq!(c.vs_constants.f[0], [5, 6, 7, 8]);
        let d = &r.draws[3];
        assert_eq!(c.ps_constants.i[0], [0; 4]);
        assert_eq!(c.ps_constants.b[0], 0);
        assert_eq!(d.ps_constants.i[0], [5, 6, 7, 8]);
        assert_eq!(d.ps_constants.b[0], 5);
        assert!(Arc::ptr_eq(&c.vs_constants, &d.vs_constants));
    }).unwrap();
}

#[test]
fn shader_rebinding_preserves_registers_outside_the_previous_extent() {
    use std::sync::Arc;
    let Some((mut s, dev)) = device_session() else { return; };
    put(&mut s, DATA, &vertex_shader(8));
    let short = out(&mut s, dev, 91, &[DATA, OUT]);
    put(&mut s, DATA, &vertex_shader(201));
    let long = out(&mut s, dev, 91, &[DATA, OUT]);
    put(&mut s, DATA, &[7; 4]);
    assert_eq!(com(&mut s, dev, 94, &[200, DATA, 1]), 0);
    assert_eq!(com(&mut s, dev, 41, &[]), 0);
    for shader in [short, long, short] {
        assert_eq!(com(&mut s, dev, 92, &[shader]), 0);
        assert_eq!(com(&mut s, dev, 81, &[4, 0, 1]), 0);
    }
    put(&mut s, DATA, &[8; 4]);
    assert_eq!(com(&mut s, dev, 94, &[200, DATA, 1]), 0);
    assert_eq!(com(&mut s, dev, 81, &[4, 0, 1]), 0);
    assert_eq!(com(&mut s, dev, 92, &[long]), 0);
    assert_eq!(com(&mut s, dev, 81, &[4, 0, 1]), 0);
    ddruntime::d3d9draw::recorded(|r| {
        assert_eq!(r.draws[0].vs_constants.f.len(), 8);
        assert_eq!(r.draws[1].vs_constants.f.len(), 201);
        assert_eq!(r.draws[1].vs_constants.f[200], [7; 4]);
        assert!(Arc::ptr_eq(&r.draws[2].vs_constants, &r.draws[3].vs_constants));
        assert_eq!(r.draws[4].vs_constants.f[200], [8; 4]);
    }).unwrap();
}

#[test]
fn dirty_groups_share_unchanged_lists_and_preserve_prior_draws() {
    use d3d9cmd::{Indices, Vertices};
    use std::sync::Arc;
    let Some((mut s, dev)) = device_session() else { return; };
    let tex_a = out(&mut s, dev, 23, &[4, 4, 1, 0, 21, 1, OUT, 0]);
    let tex_b = out(&mut s, dev, 23, &[8, 8, 1, 0, 21, 1, OUT, 0]);
    let vb = out(&mut s, dev, 26, &[1024, 8, 0, 1, OUT, 0]);
    let ib_a = out(&mut s, dev, 27, &[128, 8, 101, 1, OUT, 0]);
    let ib_b = out(&mut s, dev, 27, &[128, 8, 102, 1, OUT, 0]);
    let rt = out(&mut s, dev, 28, &[8, 8, 21, 0, 0, 0, OUT, 0]);
    assert_eq!(com(&mut s, dev, 65, &[0, tex_a]), 0);
    assert_eq!(com(&mut s, dev, 69, &[0, 5, 1]), 0);
    assert_eq!(com(&mut s, dev, 100, &[0, vb, 0, 16]), 0);
    assert_eq!(com(&mut s, dev, 104, &[ib_a]), 0);
    assert_eq!(com(&mut s, dev, 41, &[]), 0);
    let indexed = |s: &mut Session| assert_eq!(com(s, dev, 82, &[4, 0, 0, 3, 0, 1]), 0);
    indexed(&mut s);
    // Redundant setters do not invalidate the immutable lists.
    assert_eq!(com(&mut s, dev, 65, &[0, tex_a]), 0);
    assert_eq!(com(&mut s, dev, 69, &[0, 5, 1]), 0);
    assert_eq!(com(&mut s, dev, 100, &[0, vb, 0, 16]), 0);
    assert_eq!(com(&mut s, dev, 102, &[0, 1]), 0);
    indexed(&mut s);
    assert_eq!(com(&mut s, dev, 69, &[0, 5, 2]), 0);
    indexed(&mut s);
    assert_eq!(com(&mut s, dev, 65, &[0, tex_b]), 0);
    assert_eq!(com(&mut s, dev, 100, &[0, vb, 16, 32]), 0);
    assert_eq!(com(&mut s, dev, 102, &[0, 0x4000_0002]), 0);
    assert_eq!(com(&mut s, dev, 104, &[ib_b]), 0);
    assert_eq!(com(&mut s, dev, 37, &[0, rt]), 0);
    assert_eq!(com(&mut s, dev, 39, &[0]), 0);
    indexed(&mut s);
    // A nonindexed draw must not inherit the cached index buffer.
    assert_eq!(com(&mut s, dev, 81, &[4, 0, 1]), 0);
    // UP resets stream zero even though it never called SetStreamSource.
    assert_eq!(com(&mut s, dev, 83, &[4, 1, DATA, 16]), 0);
    indexed(&mut s);
    ddruntime::d3d9draw::recorded(|r| {
        let (a, b, c, d) = (&r.draws[0], &r.draws[1], &r.draws[2], &r.draws[3]);
        let streams = |d: &d3d9cmd::DrawCall| match &d.vertices {
            Vertices::Streams(v) => v.clone(), _ => panic!("expected streams"),
        };
        assert_eq!(a, b);
        assert!(Arc::ptr_eq(&a.textures, &b.textures));
        assert!(Arc::ptr_eq(&streams(a), &streams(c)));
        assert!(!Arc::ptr_eq(&a.textures, &c.textures));
        assert_eq!(a.textures[0].states[5], 1);
        assert_eq!(c.textures[0].states[5], 2);
        assert_eq!(a.textures[0].resource, tex_a);
        assert_eq!(d.textures[0].resource, tex_b);
        assert_eq!(d.textures[0].width, 8);
        assert_eq!(streams(a).as_ref(), &[(0, vb, 0, 16, 1)]);
        assert_eq!(streams(d).as_ref(), &[(0, vb, 16, 32, 0x4000_0002)]);
        assert_eq!(a.indices, Indices::Buffer(ib_a, 101));
        assert_eq!(d.indices, Indices::Buffer(ib_b, 102));
        assert_ne!(a.targets[0].as_ref().unwrap().surface, rt);
        assert_eq!(d.targets[0].as_ref().unwrap().surface, rt);
        assert_eq!(d.viewport[2..4], [8, 8]);
        assert!(a.depth.is_some());
        assert!(d.depth.is_none());
        assert_eq!(r.draws[4].indices, Indices::None);
        assert!(matches!(r.draws[5].vertices, Vertices::Inline { .. }));
        assert!(streams(&r.draws[6]).is_empty());
        assert_eq!(r.draws[6].indices, Indices::Buffer(ib_b, 102));
    }).unwrap();
}

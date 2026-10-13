//! L119: the DrawCall the native sink builds for an nDraw draw record
//! straight from the objects equals the DrawCall the D3D9 path makes for
//! it: the apply functions' calls (crates/mtstate, verified against the
//! original executable) made on the runtime's device, then the draw.
//! Needs the installed game (DDDA_EXE) for a session; skipped without it.
use ddnative_base::mem::Mem;
use ddruntime::ndraw_native::{decode, differences, draw_call, reads as shader_reads, draw_params, identity, native_draw, reads_of, Caches, FrameInfo, Pending};
use ddruntime::session::{Outcome, Session};

const SCRATCH: u32 = 0x7EFF_0000;
const PP: u32 = SCRATCH + 0x200;
const OUT: u32 = SCRATCH + 0x300;
const DATA: u32 = SCRATCH + 0x1000;
/// the nDraw objects
const NDRAW: u32 = SCRATCH + 0x4000;
/// the call data the D3D9 path passes by pointer
const ARGS: u32 = SCRATCH + 0xF000;
/// the executor context the sink's sampler section runs on
const CONTEXT: u32 = SCRATCH + 0x8000;

fn installed_exe() -> Option<ddexe::ExeFile> {
    let path = std::env::var("DDDA_EXE").unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    ddexe::ExeFile::open(path).ok()
}

/// A session with a window and a D3D9 device: (session, device).
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

/// vs_3_0: position and texcoord in and out, `n` adds of c0..c(n-1).
fn vertex_shader(n: u32) -> Vec<u32> {
    let mut t = vec![0xFFFE_0300, 0x0200_001F, 0x8000_0000, 0x900F_0000, 0x0200_001F, 0x8000_0005, 0x900F_0001,
                     0x0200_001F, 0x8000_0000, 0xE00F_0000, 0x0200_001F, 0x8000_0005, 0xE00F_0001,
                     0x0200_0001, 0x800F_0000, 0x90E4_0000];
    for k in 0..n {
        t.extend([0x0300_0002, 0x800F_0000, 0x80E4_0000, 0xA0E4_0000 | k]);
    }
    t.extend([0x0200_0001, 0xE00F_0000, 0x80E4_0000, 0x0200_0001, 0xE00F_0001, 0x90E4_0001, 0x0000_FFFF]);
    t
}

/// ps_3_0: texld from s0, then `n` adds of c0..c(n-1).
fn pixel_shader(n: u32) -> Vec<u32> {
    let mut t = vec![0xFFFF_0300, 0x0200_001F, 0x8000_0005, 0x900F_0000, 0x0200_001F, 0x9000_0000, 0xA00F_0800,
                     0x0300_0042, 0x800F_0000, 0x90E4_0000, 0xA0E4_0800];
    for k in 0..n {
        t.extend([0x0300_0002, 0x800F_0000, 0x80E4_0000, 0xA0E4_0000 | k]);
    }
    t.extend([0x0200_0001, 0x800F_0800, 0x80E4_0000, 0x0000_FFFF]);
    t
}

/// The executor context the decoders run against: guest memory with a
/// zeroed context of their own (nothing applied yet).
struct Context<'a> {
    m: &'a dyn Mem,
    words: Vec<u32>,
}
const CTX: u32 = 0xFFFF_E000;
impl mtstate::Memory for Context<'_> {
    fn read32(&self, a: u32) -> u32 {
        match a.wrapping_sub(CTX) {
            o if o < 0xE28 => self.words[o as usize / 4],
            _ => self.m.u32(a),
        }
    }
    fn write32(&mut self, a: u32, v: u32) {
        if let o @ 0..0xE28 = a.wrapping_sub(CTX) {
            self.words[o as usize / 4] = v;
        }
    }
}

/// Make the D3D9 calls an apply made, as the lifted code makes them:
/// pointer arguments point at the data.
fn make(s: &mut Session, dev: u32, calls: &[mtstate::Call]) {
    for c in calls {
        let args: Vec<u32> = match c.slot {
            47 | 75 => {
                put(s, ARGS, &c.data);
                vec![ARGS]
            }
            94 | 96 | 98 | 109 | 111 | 113 => {
                put(s, ARGS, &c.data);
                vec![c.args[0], ARGS, c.args[1]]
            }
            _ => c.args.clone(),
        };
        assert_eq!(com(s, dev, c.slot, &args), 0, "slot {} {:?}", c.slot, c.args);
    }
}

#[test]
fn a_taken_draw_is_the_draw_the_d3d9_path_makes() {
    let Some((mut s, dev)) = device_session() else {
        eprintln!("skipped: no installed game");
        return;
    };
    // the D3D9 objects
    let texture = out(&mut s, dev, 23, &[256, 256, 1, 0, 21, 1, OUT, 0]);
    let target_texture = out(&mut s, dev, 23, &[1920, 1080, 1, 1, 21, 0, OUT, 0]);
    let target = out(&mut s, target_texture, 18, &[0, OUT]);
    let depth = out(&mut s, dev, 29, &[1920, 1080, 75, 0, 0, 0, OUT, 0]);
    let vb = out(&mut s, dev, 26, &[3200, 8, 0, 1, OUT, 0]);
    let ib = out(&mut s, dev, 27, &[600, 8, 101, 1, OUT, 0]);
    put(&mut s, DATA, &[0x0000_0000, 0x0000_0003, 0x0010_0000, 0x0005_0001, 0x0000_00FF, 0x0000_0011]);
    let declaration = out(&mut s, dev, 86, &[DATA, OUT]);
    put(&mut s, DATA, &vertex_shader(8));
    let vs = out(&mut s, dev, 91, &[DATA, OUT]);
    put(&mut s, DATA, &pixel_shader(4));
    let ps = out(&mut s, dev, 106, &[DATA, OUT]);

    // the nDraw objects (docs/NDRAW.md sections 2, 3 and 5)
    let a = |k: u32| NDRAW + 0x100 * k;
    let (record, targets, fixed, shader, streams) = (a(0), a(1), a(2), a(3), a(4));
    let (rtv, dsv, blend, depth_state, raster) = (a(5), a(6), a(7), a(8), a(9));
    let (meta, descs, vbo, ibo, sampler, tex) = (a(10), a(11), a(12), a(13), a(14), a(15));
    let (vs_values, ps_values) = (a(16), a(17));
    s.mem.set_u16(record + 2, 1);
    put(&mut s, record + 4, &[targets, fixed, shader, streams, 30, 3, 2]);
    put(&mut s, targets, &[rtv, 0, 0, 0, dsv, 0]);
    put(&mut s, targets + 0x18, &[0, 0, 1920, 1080, 0, 0, 0, 0, 16, 8, 1900, 1000]);
    s.mem.set_u16(targets + 0x58, 1920);
    s.mem.set_u16(targets + 0x5A, 1080);
    s.mem.set_u32(rtv + 0x14, target);
    s.mem.set_u32(dsv + 0x14, depth);
    put(&mut s, fixed, &[0x8040_2010, 7, blend, depth_state, raster]);
    // blend: SRCALPHA / INVSRCALPHA add, alpha ONE / ZERO, all written
    s.mem.set_u32(blend + 0x20, 1 | 5 << 1 | 6 << 6 | 1 << 11 | 2 << 14 | 1 << 19 | 1 << 24 | 15 << 27);
    // depth: on, written, LESSEQUAL; stencil on, read/write masks, ALWAYS
    s.mem.set_u32(depth_state + 0x18, 1 | 1 << 1 | 4 << 2 | 1 << 6 | 0xFF << 8 | 0xFF << 16 | 1 << 24 | 3 << 28);
    s.mem.set_u32(depth_state + 0x1C, 1 | 8 << 4);
    // raster: solid, cull CCW, scissor on, depth bias 0
    s.mem.set_u32(raster + 0x20, 3 | 3 << 8 | 1 << 17);
    put(&mut s, streams, &[vbo, 0, 0, 0]);
    s.mem.set_u8(streams + 0x10, 32);
    s.mem.set_u32(streams + 0x28, ibo);
    put(&mut s, vbo + 0x14, &[3200, 0, 0, vb]);
    s.mem.set_u32(ibo + 0x20, ib);
    // shader binding: triangle list; values: sampler, texture, VS and PS floats
    put(&mut s, shader, &[3 << 21, 0, meta, sampler, tex, vs_values, ps_values]);
    put(&mut s, meta, &[0, descs, 1 | 1 << 12, 2 | 1 << 8 | 1 << 12, vs, ps, declaration]);
    // VS: c0..c7 from value 2; PS: s0 (sampler value 0, texture value 1),
    // c0..c3 from value 3
    put(&mut s, descs, &[2 << 1 | 8 << 24, 1 << 22, 3 << 1 | 4 << 24]);
    put(&mut s, vs_values, &(0..32).map(|k| (k as f32 + 0.5).to_bits()).collect::<Vec<_>>());
    put(&mut s, ps_values, &(0..16).map(|k| (k as f32 * 2.0).to_bits()).collect::<Vec<_>>());
    // linear, wrapping
    s.mem.set_u32(sampler + 0x30, 1 | 1 << 3 | 1 << 6 | 2 << 9 | 2 << 13 | 2 << 17);
    s.mem.set_u32(tex + 0x34, texture);
    assert_eq!(com(&mut s, dev, 41, &[]), 0); // BeginScene

    // the native sink's DrawCall
    let info = FrameInfo { device: dev, srgb: false, filtering: 0, invalid_viewport: [0; 4] };
    let mut caches = Caches::default();
    caches.begin_frame(&info);
    for k in 0..0xE28 / 4 {
        s.mem.set_u32(CONTEXT + 4 * k, 0);
    }
    let native = draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).expect("the draw is taken").call;
    let reads: Vec<u32> = reads_of(&native).collect();
    assert!(reads.contains(&texture) && reads.contains(&vb) && reads.contains(&ib) && reads.contains(&target));
    // from the caches: the same draw
    assert_eq!(draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).unwrap().call, native);
    // taken: its sampler section made, then the draw again from the caches
    let first = draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).unwrap();
    ddruntime::ndraw_native::commit(&mut s.mem, &info, &mut caches, first.shadow, &first.call.textures);
    assert_eq!(draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).unwrap().call, native);
    // what a taken draw costs from the caches (debug build: an upper bound)
    let n = 20_000;
    let t = std::time::Instant::now();
    for _ in 0..n {
        std::hint::black_box(draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).unwrap());
    }
    eprintln!("draw_call from the caches: {:.3} us", t.elapsed().as_secs_f64() * 1e6 / n as f64);

    // the D3D9 path: the applies' calls on the device, then the draw
    let mut ctx = Context { m: &s.mem, words: vec![0; 0xE28 / 4] };
    let mut calls = mtstate::targets(&mut ctx, CTX, targets, [0; 4]);
    calls.extend(mtstate::fixed(&mut ctx, CTX, fixed));
    calls.extend(mtstate::streams(&mut ctx, CTX, streams));
    calls.extend(mtstate::shader(&mut ctx, CTX, shader, false, 0).unwrap());
    let extent = ctx.words[0xE20 / 4];
    drop(ctx);
    make(&mut s, dev, &calls);
    let (primitive, primitives, start, base, vertices) = draw_params(&s.mem, record, 1, extent).unwrap();
    assert_eq!(decode(&s.mem, record, [0; 4], false).unwrap().extent, extent);
    assert_eq!(com(&mut s, dev, 82, &[primitive, base as u32, 0, vertices, start, primitives]), 0);
    let d3d9 = ddruntime::d3d9draw::recorded(|r| r.draws.last().cloned()).flatten().expect("the D3D9 path drew");

    // the verify mode's comparison finds them equal, and finds a change
    let r = [shader_reads(&vertex_shader(8)).unwrap(), shader_reads(&pixel_shader(4)).unwrap()];
    assert_eq!(differences(&native, r, &d3d9), vec![]);
    let mut moved = d3d9.clone();
    moved.viewport[0] += 1;
    assert_eq!(differences(&native, r, &moved).iter().map(|w| w.0).collect::<Vec<_>>(), ["viewport"]);
    // equal, but for the render states the native one leaves out (the
    // backend reads only those it carries) and the registers no shader reads
    let mut a = d3d9.clone();
    let mut b = native.clone();
    for (s, v) in b.render_states.iter() {
        let on_device = a.render_states.iter().find(|e| e.0 == *s).map(|e| e.1);
        assert_eq!(on_device, Some(*v), "render state {s}");
    }
    for s in [7, 8, 14, 15, 19, 20, 22, 23, 27, 52, 53, 54, 55, 56, 57, 58, 59, 162, 168, 171, 174, 175, 185, 186,
              187, 188, 189, 190, 191, 192, 193, 194, 195, 206, 207, 208, 209] {
        assert!(b.render_states.iter().any(|e| e.0 == s), "render state {s} carried");
    }
    a.render_states = Default::default();
    b.render_states = Default::default();
    assert_eq!(a.vs_constants.f[..8], b.vs_constants.f[..8]);
    assert_eq!(a.ps_constants.f[..4], b.ps_constants.f[..4]);
    a.vs_constants = b.vs_constants.clone();
    a.ps_constants = b.ps_constants.clone();
    assert_eq!(a, b);

    // the native draw carries the same state
    let id = identity(&s.mem, record);
    assert_eq!((id.blend.address, id.depth.address, id.raster.address, id.raster_flip), (blend, depth_state, raster, false));
    let n = native_draw(&native, &first.binding, id, &mut caches).expect("native");
    // the banks' arrays are made once: the next draw of the binding shares them
    let again = native_draw(&native, &first.binding, id, &mut caches).expect("native");
    assert!(std::sync::Arc::ptr_eq(&n.vs_constants, &again.vs_constants));
    assert!(std::sync::Arc::ptr_eq(&n.ps_constants, &again.ps_constants));
    assert_eq!(&n.state.render_states[..], &native.render_states[..]);
    assert_eq!(&n.vs_constants.floats[0].registers[..], &native.vs_constants.f[..]);
    assert_eq!(n.vs_constants.floats[0].first, 0);
    assert_eq!(n.scissor, Some([16, 8, 1900, 1000]));
    assert_eq!((n.indices.unwrap().resource, n.indices.unwrap().format), (ib, 101));
    assert_eq!(n.streams.len(), 1);
    assert_eq!((n.streams[0].resource, n.streams[0].stride), (vb, 32));
    assert_eq!((n.primitive, n.primitives, n.start, n.base_vertex, n.vertex_count), (4, 10, 3, 2, 98));
    // two draws into the same targets: one pass
    let mut p = Pending::default();
    p.push(n.clone(), native.clone());
    p.push(n, native.clone());
    let (job, calls) = p.take().unwrap();
    assert_eq!((job.passes.len(), job.passes[0].draws.len(), calls.len()), (1, 2, 2));
    assert_eq!(job.passes[0].colours[0].as_ref().unwrap().surface, target);
    assert!(p.take().is_none());

    // the next frames: an unchanged binding keeps its part (the same
    // banks, no allocation); a changed constant is read
    caches.begin_frame(&info);
    let again = draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).unwrap().call;
    assert!(std::sync::Arc::ptr_eq(&again.vs_constants, &native.vs_constants));
    assert_eq!(again, native);
    s.mem.set_u32(vs_values, 99f32.to_bits());
    caches.begin_frame(&info);
    let changed = draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).unwrap().call;
    assert!(!std::sync::Arc::ptr_eq(&changed.vs_constants, &native.vs_constants));
    assert_eq!(changed.vs_constants.f[0][0], 99f32.to_bits());
    assert_eq!(changed.ps_constants, native.ps_constants);
    // the unchanged stage keeps its bank (not copied again)
    assert!(std::sync::Arc::ptr_eq(&changed.ps_constants, &native.ps_constants));

    // NDRAW_BENCH=1 (release): what a taken draw costs, by part
    if std::env::var_os("NDRAW_BENCH").is_some() {
        let n = 200_000;
        let time = |what: &str, f: &mut dyn FnMut()| {
            let t = std::time::Instant::now();
            for _ in 0..n {
                f();
            }
            eprintln!("{what}: {:.3} us", t.elapsed().as_secs_f64() * 1e6 / n as f64);
        };
        let mem = &s.mem;
        // the guest words a draw from the caches reads (every read through
        // the counted slow path: no page table)
        struct Counted<'a>(&'a dyn Mem, std::cell::Cell<u32>);
        impl Mem for Counted<'_> {
            fn u8(&self, a: u32) -> u8 {
                self.1.set(self.1.get() + 1);
                self.0.u8(a)
            }
            fn set_u8(&mut self, _: u32, _: u8) {
                unreachable!()
            }
            fn u32(&self, a: u32) -> u32 {
                self.1.set(self.1.get() + 4);
                self.0.u32(a)
            }
            fn u16(&self, a: u32) -> u16 {
                self.1.set(self.1.get() + 2);
                self.0.u16(a)
            }
        }
        let counted = Counted(mem, std::cell::Cell::new(0));
        std::hint::black_box(draw_call(&counted, &info, &mut caches, CONTEXT, record, 1).unwrap());
        eprintln!("guest bytes a draw from the caches reads: {}", counted.1.get());
        time("draw_call from the caches", &mut || {
            std::hint::black_box(draw_call(mem, &info, &mut caches, CONTEXT, record, 1).unwrap());
        });
        time("a new frame and its first draw (the binding unchanged)", &mut || {
            caches.begin_frame(&info);
            std::hint::black_box(draw_call(mem, &info, &mut caches, CONTEXT, record, 1).unwrap());
        });
        let d = draw_call(mem, &info, &mut caches, CONTEXT, record, 1).unwrap().call;
        time("flush_reads (nothing dirty)", &mut || ddruntime::ndraw_native::flush_draw(mem, &d));
        time("DrawCall clone", &mut || {
            std::hint::black_box(d.clone());
        });
        ddruntime::d3d9draw::set_backend(Box::new(Null));
        time("send to the deferred backend alone (null worker)", &mut || ddruntime::ndraw_native::send_draw(&d));
        let mut run = Vec::with_capacity(64);
        time("send in runs of 64 (null worker), per draw", &mut || {
            run.push(d.clone());
            if run.len() == 64 {
                ddruntime::ndraw_native::send_draws(std::mem::take(&mut run));
            }
        });
        // the backend is the null one now: the sections below compare with
        // the D3D9 path's recorded draws, which it does not record
        return;
    }

    // a binding whose VS reads c0..c7 and writes c0..c5 (another binding,
    // meta and descriptors): the D3D9 path draws with the device's c6, c7
    let (shader2, meta2, descs2) = (NDRAW + 0x100 * 19, NDRAW + 0x100 * 20, NDRAW + 0x100 * 21);
    put(&mut s, shader2, &[3 << 21, 0, meta2, sampler, tex, vs_values, ps_values]);
    put(&mut s, meta2, &[0, descs2, 1 | 1 << 12, 2 | 1 << 8 | 1 << 12, vs, ps, declaration]);
    put(&mut s, descs2, &[2 << 1 | 6 << 24, 1 << 22, 3 << 1 | 4 << 24]);
    put(&mut s, DATA, &[7, 8, 9, 10, 11, 12, 13, 14].map(|k| (k as f32).to_bits()));
    assert_eq!(com(&mut s, dev, 94, &[6, DATA, 2]), 0); // SetVertexShaderConstantF c6, c7
    s.mem.set_u32(record + 0xC, shader2);
    caches.begin_frame(&info);
    let mut filled = draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).expect("not refused");
    ddruntime::ndraw_native::fill_uncovered(&mut filled.call, &filled.binding, &Default::default(), dev).expect("filled");
    assert_eq!(filled.call.vs_constants.f[6..8], [[7, 8, 9, 10], [11, 12, 13, 14]].map(|r| r.map(|k| (k as f32).to_bits())));
    let mut ctx = Context { m: &s.mem, words: vec![0; 0xE28 / 4] };
    let calls = mtstate::shader(&mut ctx, CTX, shader2, false, 0).unwrap();
    drop(ctx);
    make(&mut s, dev, &calls);
    let extent = decode(&s.mem, record, [0; 4], false).unwrap().extent;
    let (primitive, primitives, start, base, vertices) = draw_params(&s.mem, record, 1, extent).unwrap();
    assert_eq!(com(&mut s, dev, 82, &[primitive, base as u32, 0, vertices, start, primitives]), 0);
    let d3d9 = ddruntime::d3d9draw::recorded(|r| r.draws.last().cloned()).flatten().expect("the D3D9 path drew");
    assert_eq!(differences(&filled.call, r, &d3d9), vec![]);
    s.mem.set_u32(record + 0xC, shader);

    // L180: the constants as the original's apply (0x00DAA8B0) leaves
    // them. After a binding with the same shaders whose value slot holds
    // the same pointer, the upload is skipped: the device keeps the earlier
    // values even though the memory changed. After one with another
    // pointer, it uploads (reads) again. The same binding again: nothing.
    let c0y = s.mem.u32(vs_values + 4);
    let _ = draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).expect("taken");
    s.mem.set_u32(record + 0xC, shader2);
    let _ = draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).expect("taken");
    s.mem.set_u32(record + 0xC, shader);
    s.mem.set_u32(vs_values + 4, 123f32.to_bits());
    let skipped = draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).expect("taken");
    assert!(skipped.rebuilt);
    assert_eq!(skipped.call.vs_constants.f[0][1], c0y, "the skipped upload keeps the earlier value");
    let (shader3, vs_values3) = (NDRAW + 0x100 * 22, NDRAW + 0x100 * 23);
    put(&mut s, shader3, &[3 << 21, 0, meta2, sampler, tex, vs_values3, ps_values]);
    put(&mut s, vs_values3, &(0..24).map(|k| (k as f32 + 100.0).to_bits()).collect::<Vec<_>>());
    s.mem.set_u32(record + 0xC, shader3);
    let _ = draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).expect("taken");
    s.mem.set_u32(record + 0xC, shader);
    let read = draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).expect("taken");
    assert_eq!(read.call.vs_constants.f[0][1], 123f32.to_bits(), "another pointer before: uploaded again");
    s.mem.set_u32(vs_values + 4, 7f32.to_bits());
    let next = draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 1).expect("taken");
    assert!(!next.rebuilt);
    assert_eq!(next.call.vs_constants.f[0][1], 123f32.to_bits(), "the same binding: nothing uploaded");
    s.mem.set_u32(vs_values + 4, c0y);

    // an instanced draw (type 3): stream 1 instance data (stride 16,
    // offset 8), 5 instances, instance step 2. The D3D9 path: the stream
    // apply, then the calls 0x00DAB480's type 3 makes around the draw
    // (crates/mtdraw executor set_frequencies/reset_frequencies)
    let vb2 = out(&mut s, dev, 26, &[1600, 8, 0, 1, OUT, 0]);
    let vbo2 = NDRAW + 0x100 * 18;
    put(&mut s, vbo2 + 0x14, &[1600, 0, 0, vb2]);
    s.mem.set_u32(streams + 4, vbo2);
    s.mem.set_u8(streams + 0x11, 16);
    s.mem.set_u32(streams + 0x18, 8);
    s.mem.set_u8(streams + 0x25, 1);
    s.mem.set_u16(record + 2, 3);
    put(&mut s, record + 0x20, &[5, 2]);
    caches.begin_frame(&info);
    let instanced = draw_call(&s.mem, &info, &mut caches, CONTEXT, record, 3).expect("taken").call;
    let mut ctx = Context { m: &s.mem, words: vec![0; 0xE28 / 4] };
    // the streams, and the binding again (its VS c0 was changed above)
    let mut calls = mtstate::streams(&mut ctx, CTX, streams);
    calls.extend(mtstate::shader(&mut ctx, CTX, shader, false, 0).unwrap());
    let extent = ctx.words[0xE20 / 4];
    drop(ctx);
    make(&mut s, dev, &calls);
    assert_eq!(com(&mut s, dev, 102, &[0, 0x4000_0005]), 0); // SetStreamSourceFreq
    assert_eq!(com(&mut s, dev, 102, &[1, 0x8000_0001]), 0);
    assert_eq!(com(&mut s, dev, 100, &[1, vb2, 2 * 16 + 8, 16]), 0); // SetStreamSource
    let (primitive, primitives, start, base, vertices) = draw_params(&s.mem, record, 1, extent).unwrap();
    assert_eq!(com(&mut s, dev, 82, &[primitive, base as u32, 0, vertices, start, primitives]), 0);
    let d3d9 = ddruntime::d3d9draw::recorded(|r| r.draws.last().cloned()).flatten().expect("the D3D9 path drew");
    let d3d9cmd::Vertices::Streams(st) = &d3d9.vertices else { panic!("streams") };
    assert_eq!(st.iter().map(|s| s.4).collect::<Vec<_>>(), [0x4000_0005, 0x8000_0001]);
    assert_eq!(differences(&instanced, r, &d3d9), vec![]);
    let (mut a, mut b) = (d3d9.clone(), instanced.clone());
    a.render_states = Default::default();
    b.render_states = Default::default();
    a.vs_constants = b.vs_constants.clone();
    a.ps_constants = b.ps_constants.clone();
    assert_eq!(a, b);
    for k in [0, 1] {
        assert_eq!(com(&mut s, dev, 102, &[k, 1]), 0);
    }
    assert_eq!(com(&mut s, dev, 100, &[1, vb2, 8, 16]), 0);
    assert_eq!(com(&mut s, dev, 42, &[]), 0); // EndScene
}

/// A backend that does nothing (the benchmark's worker).
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

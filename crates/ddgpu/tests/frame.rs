//! The frame oracle (brief L41) on a synthetic multi-pass frame: a
//! G-buffer (MRT: A8R8G8B8 albedo and A16B16G16R16F normal textures, D24S8
//! depth; an indexed draw from buffers and a DrawPrimitiveUP crossing it in
//! depth), light accumulation into an A16B16G16R16F texture (two additive
//! lights sampling the G-buffer), a tone map into a 4x multisampled target
//! with a blended slanted triangle on top, the resolve (StretchRect) into
//! the back buffer, Present; two frames. Captured through
//! `d3d9frame::Capture`, read back from the file, replayed into the wgpu
//! backend and the host's D3D9 driver, every target compared. Skips
//! without D3D9 or an adapter.
use d3d9cmd::{
    ClearCall, Constants, CopyCall, DrawCall, FillCall, GammaRamp, GraphicsBackend, Indices, Target, TargetTexture, TexKind, TextureBinding, Upload,
    Vertices, USAGE_RENDERTARGET,
};
use d3d9frame::{oracle, Capture, Command, Reader};

const W: u32 = 16;
const H: u32 = 16;

// --- shader assembly ------------------------------------------------------------

const TEMP: u32 = 0;
const INPUT: u32 = 1;
const CONST: u32 = 2;
const OUTPUT: u32 = 6;
const COLOROUT: u32 = 8;
const SAMPLER: u32 = 10;
const XYZW: u32 = 0xE4;

fn reg(kind: u32, index: u32) -> u32 {
    0x8000_0000 | ((kind & 7) << 28) | ((kind & 0x18) << 8) | index
}
fn dst(kind: u32, index: u32, mask: u32) -> u32 {
    reg(kind, index) | (mask << 16)
}
fn src(kind: u32, index: u32, swizzle: u32) -> u32 {
    reg(kind, index) | (swizzle << 16)
}
fn op(code: u32, operands: &[u32]) -> Vec<u32> {
    let mut v = vec![code | ((operands.len() as u32) << 24)];
    v.extend_from_slice(operands);
    v
}
/// dcl of an input or output with a usage and index
fn dcl(usage: u32, index: u32, register: u32) -> Vec<u32> {
    op(0x1F, &[0x8000_0000 | usage | (index << 16), register])
}
fn dcl_2d(sampler: u32) -> Vec<u32> {
    op(0x1F, &[0x9000_0000, dst(SAMPLER, sampler, 0xF)])
}
fn shader(version: u32, body: &[Vec<u32>]) -> Vec<u32> {
    let mut t = vec![version];
    for i in body {
        t.extend_from_slice(i);
    }
    t.push(0x0000_FFFF);
    t
}

/// vs_3_0: position, texcoord0 and normal passed through (o0, o1, o2).
fn vs() -> Vec<u32> {
    shader(0xFFFE_0300, &[
        dcl(0, 0, dst(INPUT, 0, 0xF)), dcl(5, 0, dst(INPUT, 1, 0xF)), dcl(3, 0, dst(INPUT, 2, 0xF)),
        dcl(0, 0, dst(OUTPUT, 0, 0xF)), dcl(5, 0, dst(OUTPUT, 1, 0xF)), dcl(5, 1, dst(OUTPUT, 2, 0xF)),
        op(1, &[dst(OUTPUT, 0, 0xF), src(INPUT, 0, XYZW)]),
        op(1, &[dst(OUTPUT, 1, 0xF), src(INPUT, 1, XYZW)]),
        op(1, &[dst(OUTPUT, 2, 0xF), src(INPUT, 2, XYZW)]),
    ])
}

/// G-buffer: oC0 = texld(s0) * c0, oC1 = (normal, 1).
fn ps_gbuffer() -> Vec<u32> {
    shader(0xFFFF_0300, &[
        dcl(5, 0, dst(INPUT, 0, 0x3)), dcl(5, 1, dst(INPUT, 1, 0x7)), dcl_2d(0),
        op(0x42, &[dst(TEMP, 0, 0xF), src(INPUT, 0, XYZW), src(SAMPLER, 0, XYZW)]),
        op(5, &[dst(COLOROUT, 0, 0xF), src(TEMP, 0, XYZW), src(CONST, 0, XYZW)]),
        op(1, &[dst(COLOROUT, 1, 0x7), src(INPUT, 1, XYZW)]),
        op(1, &[dst(COLOROUT, 1, 0x8), src(CONST, 1, XYZW)]),
    ])
}

/// A light: oC0 = albedo(s0) * c1 * max(dot3(normal(s1), c0), 0).
fn ps_light() -> Vec<u32> {
    shader(0xFFFF_0300, &[
        dcl(5, 0, dst(INPUT, 0, 0x3)), dcl_2d(0), dcl_2d(1),
        op(0x42, &[dst(TEMP, 0, 0xF), src(INPUT, 0, XYZW), src(SAMPLER, 0, XYZW)]),
        op(0x42, &[dst(TEMP, 1, 0xF), src(INPUT, 0, XYZW), src(SAMPLER, 1, XYZW)]),
        op(8, &[dst(TEMP, 2, 0x1), src(TEMP, 1, XYZW), src(CONST, 0, XYZW)]),
        op(11, &[dst(TEMP, 2, 0x1), src(TEMP, 2, 0x00), src(CONST, 2, 0x00)]),
        op(5, &[dst(TEMP, 3, 0xF), src(TEMP, 0, XYZW), src(CONST, 1, XYZW)]),
        op(5, &[dst(COLOROUT, 0, 0xF), src(TEMP, 3, XYZW), src(TEMP, 2, 0x00)]),
    ])
}

/// Tone map: x * c0 / (1 + x * c0) per colour channel, alpha 1.
fn ps_tonemap() -> Vec<u32> {
    shader(0xFFFF_0300, &[
        dcl(5, 0, dst(INPUT, 0, 0x3)), dcl_2d(0),
        op(0x42, &[dst(TEMP, 0, 0xF), src(INPUT, 0, XYZW), src(SAMPLER, 0, XYZW)]),
        op(5, &[dst(TEMP, 0, 0xF), src(TEMP, 0, XYZW), src(CONST, 0, XYZW)]),
        op(2, &[dst(TEMP, 1, 0xF), src(TEMP, 0, XYZW), src(CONST, 1, XYZW)]),
        op(6, &[dst(TEMP, 2, 0x1), src(TEMP, 1, 0x00)]),
        op(6, &[dst(TEMP, 2, 0x2), src(TEMP, 1, 0x55)]),
        op(6, &[dst(TEMP, 2, 0x4), src(TEMP, 1, 0xAA)]),
        op(5, &[dst(COLOROUT, 0, 0x7), src(TEMP, 0, XYZW), src(TEMP, 2, XYZW)]),
        op(1, &[dst(COLOROUT, 0, 0x8), src(CONST, 1, XYZW)]),
    ])
}

/// oC0 = c0
fn ps_constant() -> Vec<u32> {
    shader(0xFFFF_0300, &[op(1, &[dst(COLOROUT, 0, 0xF), src(CONST, 0, XYZW)])])
}

// --- the frame ------------------------------------------------------------------

/// position float4, texcoord float2, normal float3: 36 bytes
fn declaration() -> Vec<[u8; 8]> {
    vec![[0, 0, 0, 0, 3, 0, 0, 0], [0, 0, 16, 0, 1, 0, 5, 0], [0, 0, 24, 0, 2, 0, 3, 0]]
}

fn vertices(v: &[[f32; 9]]) -> Vec<u8> {
    v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect()
}

/// A triangle over the whole target, texcoords [0, 1) over it.
fn fullscreen() -> Vec<u8> {
    vertices(&[
        [-1.0, -1.0, 0.5, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0],
        [-1.0, 3.0, 0.5, 1.0, 0.0, -1.0, 0.0, 0.0, 1.0],
        [3.0, -1.0, 0.5, 1.0, 2.0, 1.0, 0.0, 0.0, 1.0],
    ])
}

fn states(extra: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = vec![
        (7, 0), (14, 0), (23, 4), (15, 0), (22, 1), (27, 0), (19, 2), (20, 1), (171, 1), (206, 0), (207, 2), (208, 1),
        (209, 1), (193, 0xFFFF_FFFF), (168, 0xF), (190, 0xF), (191, 0xF), (192, 0xF), (174, 0), (194, 0), (195, 0), (175, 0),
        (52, 0), (185, 0), (8, 3), (161, 1), (162, 0xFFFF_FFFF),
    ];
    for &(k, val) in extra {
        v.retain(|s| s.0 != k);
        v.push((k, val));
    }
    v.sort();
    v
}

fn constants(c: &[[f32; 4]]) -> Constants {
    let mut k = Constants { f: vec![[0; 4]; 224], ..Default::default() };
    for (i, v) in c.iter().enumerate() {
        k.f[i] = v.map(f32::to_bits);
    }
    k
}

fn render_texture(resource: u32, format: u32) -> (Target, TextureBinding) {
    let tex = TargetTexture { resource, kind: TexKind::Flat, width: W, height: H, depth: 1, levels: 1, usage: USAGE_RENDERTARGET };
    let level = Target { surface: resource + 1, format, width: W, height: H, texture: Some(tex), level: 0, samples: 0, quality: 0 };
    // CLAMP, POINT filtering (sampled at texel centres)
    let binding = TextureBinding {
        sampler: 0, resource, kind: TexKind::Flat, format, width: W, height: H, depth: 1, levels: 1, usage: USAGE_RENDERTARGET,
        states: [0, 3, 3, 3, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0],
    };
    (level, binding)
}

fn surface(surface: u32, format: u32, samples: u32) -> Target {
    Target { surface, format, width: W, height: H, texture: None, level: 0, samples, quality: 0 }
}

fn draw(ps: Vec<u32>, ps_constants: Constants, textures: Vec<TextureBinding>, targets: [Option<Target>; 4], depth: Option<Target>,
        render_states: Vec<(u32, u32)>, vertices: Vertices, indices: Indices, primitives: u32, vertex_count: u32) -> DrawCall {
    DrawCall {
        primitive: 4, primitives, start: 0, base_vertex: 0, min_index: 0, vertex_count, vertices, indices,
        vertex_shader: Some((vs()).into()), pixel_shader: Some((ps).into()), declaration: declaration().into(), fvf: 0,
        vs_constants: Constants { f: vec![[0; 4]; 256], ..Default::default() }.into(), ps_constants: ps_constants.into(), textures, render_states,
        viewport: [0, 0, W, H, 0f32.to_bits(), 1f32.to_bits()], scissor: [0, 0, W as i32, H as i32], targets, depth,
    }
}

fn clear(targets: [Option<Target>; 4], depth: Option<Target>, flags: u32, colour: u32) -> ClearCall {
    ClearCall { rects: vec![], flags, colour, z: 1.0, stencil: 0, targets, depth, viewport: [0, 0, W, H, 0, 1f32.to_bits()], scissor: None, srgb: false }
}

/// The commands of `frames` frames, in the order the runtime sends them.
fn frame_commands(frames: u32) -> Vec<Command> {
    let (albedo, albedo_tex) = render_texture(0x300, 21);
    let (normal, normal_tex) = render_texture(0x310, 113);
    let (accum, accum_tex) = render_texture(0x320, 113);
    let depth = surface(0x380, 75, 0);
    let msaa = surface(0x330, 21, 4);
    let back = surface(0x100, 21, 0);
    let checker: Vec<u8> = (0..16u32).flat_map(|k| if (k + k / 4) % 2 == 0 { [40, 200, 230, 255] } else { [220, 90, 30, 255] }).collect();
    let mut c = vec![
        Command::Upload(Upload::Image { resource: 0x400, index: 0, format: 21, width: 4, height: 4, depth: 1, pitch: 16, slice: 64, bytes: checker }),
        // a slanted quad at depth 0.3..0.7 (indexed, from buffers)
        Command::Upload(Upload::Buffer { resource: 0x500, bytes: vertices(&[
            [-0.9, -0.7, 0.3, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            [-0.6, 0.9, 0.5, 1.0, 0.0, 2.0, 0.6, 0.0, 0.8],
            [0.8, -0.9, 0.5, 1.0, 2.0, 0.0, 0.0, 0.6, 0.8],
            [0.7, 0.8, 0.7, 1.0, 2.0, 2.0, 0.577, 0.577, 0.577],
        ]) }),
        Command::Upload(Upload::Buffer { resource: 0x501, bytes: [0u16, 1, 2, 2, 1, 3].iter().flat_map(|i| i.to_le_bytes()).collect() }),
    ];
    let texture = TextureBinding {
        sampler: 0, resource: 0x400, kind: TexKind::Flat, format: 21, width: 4, height: 4, depth: 1, levels: 1, usage: 0,
        states: [0, 1, 1, 1, 0, 2, 2, 0, 0, 0, 1, 0, 0, 0],
    };
    for frame in 0..frames {
        let f = frame as f32;
        // G-buffer
        let gb = [Some(albedo.clone()), Some(normal.clone()), None, None];
        c.push(Command::Clear(clear(gb.clone(), Some(depth.clone()), 7, 0x0010_2030)));
        let z_states = states(&[(7, 1), (14, 1)]);
        c.push(Command::Draw(draw(ps_gbuffer(), constants(&[[1.0, 0.9, 0.8, 1.0], [0.0, 0.0, 0.0, 1.0]]), vec![texture.clone()], gb.clone(),
                                  Some(depth.clone()), z_states.clone(), Vertices::Streams(vec![(0, 0x500, 0, 36, 1)]), Indices::Buffer(0x501, 101), 2, 4)));
        c.push(Command::Draw(draw(ps_gbuffer(), constants(&[[0.5, 1.0, 0.6, 1.0], [0.0, 0.0, 0.0, 1.0]]), vec![texture.clone()], gb.clone(),
                                  Some(depth.clone()), z_states,
                                  Vertices::Inline { bytes: vertices(&[
                                      [-1.0, 0.0, 0.1, 1.0, 0.0, 0.0, -0.6, 0.0, 0.8],
                                      [0.2, 1.0, 0.9, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0],
                                      [0.9 - 0.1 * f, -1.0, 0.9, 1.0, 1.0, 1.0, 0.6, 0.0, 0.8],
                                  ]), stride: 36 }, Indices::None, 1, 3)));
        // light accumulation: two additive lights, values above 1
        let acc = [Some(accum.clone()), None, None, None];
        c.push(Command::Clear(clear(acc.clone(), None, 1, 0)));
        let mut normal_s1 = normal_tex.clone();
        normal_s1.sampler = 1;
        let add = states(&[(27, 1), (19, 2), (20, 2)]);
        for (dir, colour) in [([0.0f32, 0.0, 1.0, 0.0], [2.5f32, 2.0, 1.5, 1.0]), ([0.6, 0.0, 0.8, 0.0], [0.5 + f, 1.25, 3.0, 0.0])] {
            c.push(Command::Draw(draw(ps_light(), constants(&[dir, colour, [0.0; 4]]), vec![albedo_tex.clone(), normal_s1.clone()], acc.clone(), None,
                                      add.clone(), Vertices::Inline { bytes: fullscreen(), stride: 36 }, Indices::None, 1, 3)));
        }
        // tone map into the multisampled target, a blended slanted triangle on top
        let ms = [Some(msaa.clone()), None, None, None];
        c.push(Command::Clear(clear(ms.clone(), None, 1, 0xFF00_0000)));
        c.push(Command::Draw(draw(ps_tonemap(), constants(&[[0.8, 0.8, 0.8, 1.0], [1.0, 1.0, 1.0, 1.0]]), vec![accum_tex.clone()], ms.clone(), None,
                                  states(&[]), Vertices::Inline { bytes: fullscreen(), stride: 36 }, Indices::None, 1, 3)));
        c.push(Command::Draw(draw(ps_constant(), constants(&[[1.0, 0.25, 0.0, 0.5]]), vec![], ms.clone(), None,
                                  states(&[(27, 1), (19, 5), (20, 6)]),
                                  Vertices::Inline { bytes: vertices(&[
                                      [-0.83, -0.95, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                                      [-0.31, 0.97, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                                      [0.93, 0.11, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                                  ]), stride: 36 }, Indices::None, 1, 3)));
        // resolve, present
        c.push(Command::Copy(CopyCall { src: msaa.clone(), src_rect: [0; 4], dst: back.clone(), dst_rect: [0; 4], filter: 0 }));
        c.push(Command::Present(back.clone()));
    }
    c
}

/// A backend that draws nothing (what the capture wraps here).
struct Null;
impl GraphicsBackend for Null {
    fn upload(&mut self, _: Upload) {}
    fn draw(&mut self, _: &DrawCall) {}
    fn clear(&mut self, _: &ClearCall) {}
    fn present(&mut self, _: &Target) {}
    fn copy(&mut self, _: &CopyCall) {}
    fn fill(&mut self, _: &FillCall) {}
    fn read_back(&mut self, _: &Target) -> Option<Vec<u8>> {
        None
    }
    fn query_begin(&mut self, _: u32) {}
    fn query_end(&mut self, _: u32) {}
    fn query_result(&mut self, _: u32) -> Option<u64> {
        None
    }
    fn gamma(&mut self, _: Option<&GammaRamp>) {}
    fn release(&mut self, _: u32) {}
}

/// The commands captured to a file through `Capture`.
fn captured(commands: &[Command], name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ddda-frame-oracle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let mut capture = Capture::new(Box::new(Null), &path, None).unwrap();
    for c in commands {
        c.apply(&mut capture);
    }
    assert_eq!(capture.frames, commands.iter().filter(|c| matches!(c, Command::Present(_))).count() as u32);
    assert!(capture.error.is_none());
    drop(capture);
    path
}

fn wgpu_errors(g: &mut ddgpu::WgpuBackend) -> Vec<String> {
    std::mem::take(&mut g.errors)
}

#[test]
fn the_capture_reads_back_as_the_commands() {
    let commands = frame_commands(2);
    let path = captured(&commands, "read.ddframe");
    let read: Vec<Command> = Reader::open(&path).unwrap().collect::<std::io::Result<_>>().unwrap();
    assert_eq!(read, commands);
    // the second frame's repeated shaders, states and constants are blobs
    // already written
    let size = std::fs::metadata(&path).unwrap().len();
    let raw: usize = commands.iter().map(|c| format!("{c:?}").len()).sum();
    eprintln!("capture: {} commands, {size} bytes (debug text {raw} bytes)", commands.len());
}

#[test]
fn a_multi_pass_frame_matches_the_d3d9_driver() {
    let (Some(mut driver), Some(mut gpu)) = (d3d9ref::Driver::new(), ddgpu::WgpuBackend::headless()) else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    let commands = frame_commands(2);
    let path = captured(&commands, "frame.ddframe");
    let images = path.with_extension("images");
    let opts = oracle::Options { images: Some(images.clone()), ..Default::default() };
    let report = oracle::compare(Reader::open(&path).unwrap(), &mut gpu, wgpu_errors, &mut driver,
                                 |d: &mut d3d9ref::Driver| std::mem::take(&mut d.errors), &opts).unwrap();
    eprintln!("{report}");
    assert_eq!(report.compared, vec![0, 1]);
    // albedo, normal, light accumulation and the back buffer, each frame
    // (the multisampled target is compared through its resolve)
    assert_eq!(report.targets.len(), 8);
    assert!(report.targets.iter().all(|t| t.images.len() == 3), "three images per target");
    assert!(report.equal(), "the frames differ");
}

/// Bisection: the same capture into two wgpu backends, one of which
/// changes the second light's colour; the report names that draw.
#[test]
fn bisection_names_the_first_differing_draw() {
    struct Perturb {
        inner: ddgpu::WgpuBackend,
        draws: usize,
    }
    impl GraphicsBackend for Perturb {
        fn upload(&mut self, u: Upload) {
            self.inner.upload(u)
        }
        fn draw(&mut self, d: &DrawCall) {
            self.draws += 1;
            if self.draws == 4 {
                let mut d = d.clone();
                std::sync::Arc::make_mut(&mut d.ps_constants).f[1][0] = 1.75f32.to_bits();
                self.inner.draw(&d);
            } else {
                self.inner.draw(d);
            }
        }
        fn clear(&mut self, c: &ClearCall) {
            self.inner.clear(c)
        }
        fn present(&mut self, t: &Target) {
            self.inner.present(t)
        }
        fn copy(&mut self, c: &CopyCall) {
            self.inner.copy(c)
        }
        fn fill(&mut self, f: &FillCall) {
            self.inner.fill(f)
        }
        fn read_back(&mut self, t: &Target) -> Option<Vec<u8>> {
            self.inner.read_back(t)
        }
        fn query_begin(&mut self, q: u32) {
            self.inner.query_begin(q)
        }
        fn query_end(&mut self, q: u32) {
            self.inner.query_end(q)
        }
        fn query_result(&mut self, q: u32) -> Option<u64> {
            self.inner.query_result(q)
        }
        fn gamma(&mut self, r: Option<&GammaRamp>) {
            self.inner.gamma(r)
        }
        fn release(&mut self, r: u32) {
            self.inner.release(r)
        }
    }
    let (Some(mut a), Some(b)) = (ddgpu::WgpuBackend::headless(), ddgpu::WgpuBackend::headless()) else {
        eprintln!("skipped: no adapter");
        return;
    };
    let mut b = Perturb { inner: b, draws: 0 };
    let commands = frame_commands(1);
    let path = captured(&commands, "bisect.ddframe");
    let report = oracle::compare(Reader::open(&path).unwrap(), &mut a, wgpu_errors, &mut b,
                                 |p: &mut Perturb| std::mem::take(&mut p.inner.errors), &oracle::Options::default()).unwrap();
    eprintln!("{report}");
    // the fourth draw is the second light
    let fourth = commands.iter().enumerate().filter(|(_, c)| matches!(c, Command::Draw(_))).nth(3).unwrap().0;
    let first = report.first.as_ref().expect("a first difference");
    assert_eq!((first.frame, first.command, first.kind), (0, fourth, "draw"));
    assert_eq!(first.target.surface, 0x321, "the light accumulation texture");
    // and it carries through to the back buffer
    assert!(report.targets.iter().any(|t| t.back_buffer && t.difference.differing > 0));
}

//! What building the game's pipelines costs the drawing thread (L71), and
//! what the pipeline cache kept on disk saves: draws with the game's pixel
//! shaders, read from the player's installation, each with a new pipeline
//! (the shader translated, its module and the pipeline created), then
//! again (built already).
//!
//!     cargo run --release -p ddgpu --example pipeline_cost -- <cache DIR|nocache> <skip> <count> [package...]
//!
//! `cache DIR` creates the pipelines through a pipeline cache in DIR and
//! saves it at the end, so a second run with the same shaders loads what
//! the first saved. Packages are names in nativePC\sa\DX9 (default: all,
//! sorted); `skip` and `count` choose the shaders (distinct, in package
//! order), so separate runs can use shaders no run has compiled yet.
//!
//! For translation/parse/validation/module phases without pipeline compilation:
//! `cargo run --release -p ddgpu --example module_cost -- <skip> <count> <cache DIR>`.
//! Production IR caching is on by default; `DDGPU_SHADER_CACHE_DISABLE=1`
//! restores the WGSL module path for a before/after comparison.
use d3d9cmd::{Constants, DrawCall, GraphicsBackend, Indices, Target, TexKind, TextureBinding, Upload, Vertices};
use std::time::Instant;

const SHADERS: &str = r"E:\SteamLibrary\steamapps\common\DDDA\nativePC\sa\DX9";

/// dcl_position v0; dcl_texcoord v1; mov oPos, v0; mov oT0, v1
fn vs() -> Vec<u32> {
    vec![
        0xFFFE_0300,
        0x0200_001F, 0x8000_0000, 0x900F_0000,
        0x0200_001F, 0x8000_0005, 0x900F_0001,
        0x0200_001F, 0x8000_0000, 0xE00F_0000,
        0x0200_001F, 0x8000_0005, 0xE00F_0001,
        0x0200_0001, 0xE00F_0000, 0x90E4_0000,
        0x0200_0001, 0xE00F_0001, 0x90E4_0001,
        0x0000_FFFF,
    ]
}

/// The pixel shaders of one package (as the shader sweep reads them).
fn pixel_shaders(package: &str) -> Vec<Vec<u32>> {
    let path = std::path::Path::new(SHADERS).join(format!("{package}.arc"));
    let Ok((archive, mut file)) = mtarc::Archive::open(path) else { return vec![] };
    let wanted = mtarc::type_hash("rShaderPackage");
    let Some(entry) = archive.entries.iter().find(|e| e.type_hash == wanted) else { return vec![] };
    let tokens = d3d9shader::tokens(&mtarc::read_entry(&mut file, entry).unwrap());
    let mut out = Vec::new();
    let mut k = 0;
    while k < tokens.len() {
        if tokens[k] == 0xFFFF_0300 {
            if let Ok(s) = d3d9shader::parse(&tokens[k..]) {
                if s.instructions.iter().any(|i| i.opcode != d3d9shader::op::COMMENT) {
                    out.push(tokens[k..k + s.tokens].to_vec());
                    k += s.tokens;
                    continue;
                }
            }
        }
        k += 1;
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cache, rest) = match args.first().map(String::as_str) {
        Some("cache") => (Some(std::path::PathBuf::from(&args[1])), &args[2..]),
        Some("nocache") => (None, &args[1..]),
        _ => panic!("usage: pipeline_cost <cache DIR|nocache> <skip> <count> [package...]"),
    };
    let skip: usize = rest.first().and_then(|v| v.parse().ok()).unwrap_or(0);
    let count: usize = rest.get(1).and_then(|v| v.parse().ok()).unwrap_or(500);
    let mut packages: Vec<String> = rest.iter().skip(2).cloned().collect();
    if packages.is_empty() {
        packages = std::fs::read_dir(SHADERS).expect("the game's shader packages")
            .filter_map(|e| e.ok()?.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        packages.sort();
    }
    let mut seen = std::collections::HashSet::new();
    let mut shaders = Vec::new();
    for p in &packages {
        for s in pixel_shaders(p) {
            if seen.insert(s.clone()) {
                shaders.push(s);
            }
        }
        if shaders.len() >= skip + count {
            break;
        }
    }
    let shaders: Vec<Vec<u32>> = shaders.into_iter().skip(skip).take(count).collect();

    let mut gpu = ddgpu::WgpuBackend::headless().expect("a GPU adapter");
    gpu.read_frames = false;
    if let Some(dir) = &cache {
        assert!(gpu.enable_pipeline_cache(dir), "the device has no pipeline cache");
    }
    let (w, h) = (256u32, 256u32);
    let bb = Target { surface: 0x100, format: 21, width: w, height: h, texture: None, level: 0, samples: 0, quality: 0 };
    let mut vb = Vec::new();
    for v in [[-1.0f32, -1.0, 0.0, 1.0, 0.0, 1.0], [-1.0, 1.0, 0.0, 1.0, 0.0, 0.0], [1.0, -1.0, 0.0, 1.0, 1.0, 1.0]] {
        vb.extend(v.iter().flat_map(|f| f.to_le_bytes()));
    }
    gpu.upload(Upload::Buffer { resource: 0x300, bytes: vb });
    let mut sampler = [0u32; 14];
    for (k, v) in [(1, 1), (2, 1), (3, 1), (5, 2), (6, 2)] {
        sampler[k] = v;
    }
    let draw = |ps: &[u32]| -> Option<DrawCall> {
        let shader = d3d9shader::parse(ps).ok()?;
        let iface = d3d9wgsl::translate(&shader).ok()?.interface;
        let textures = iface.samplers.iter().map(|&(n, kind)| {
            let (kind, resource, depth) = match kind {
                d3d9wgsl::TextureKind::Flat => (TexKind::Flat, 0x200, 1),
                d3d9wgsl::TextureKind::Cube => (TexKind::Cube, 0x201, 1),
                d3d9wgsl::TextureKind::Volume => (TexKind::Volume, 0x202, 4),
            };
            TextureBinding { sampler: n, resource, kind, format: 21, width: 4, height: 4, depth, levels: 1, usage: 0, states: sampler }
        }).collect();
        let mut states = vec![(7, 0), (22, 1), (27, 0), (168, 0xF), (190, 0xF), (191, 0xF), (192, 0xF), (15, 0)];
        states.sort();
        Some(DrawCall {
            primitive: 4, primitives: 1, start: 0, base_vertex: 0, min_index: 0, vertex_count: 3,
            vertices: Vertices::Streams(vec![(0, 0x300, 0, 24, 1)]),
            indices: Indices::None,
            vertex_shader: Some((vs()).into()),
            pixel_shader: Some((ps.to_vec()).into()),
            declaration: vec![[0, 0, 0, 0, 3, 0, 0, 0], [0, 0, 16, 0, 1, 0, 5, 0]].into(),
            fvf: 0,
            vs_constants: Constants { f: vec![[0; 4]; 256], ..Default::default() }.into(),
            ps_constants: Constants { f: vec![[0; 4]; 224], ..Default::default() }.into(),
            textures,
            render_states: states,
            viewport: [0, 0, w, h, 0f32.to_bits(), 1f32.to_bits()],
            scissor: [0, 0, w as i32, h as i32],
            targets: [Some(bb.clone()), None, None, None],
            depth: None,
        })
    };
    let stream: Vec<DrawCall> = shaders.iter().filter_map(|s| draw(s)).collect();
    // the vertex shader's module and the textures, outside the timing
    if let Some(d) = stream.first() {
        let mut first = d.clone();
        first.pixel_shader = Some((vec![0xFFFF_0300, 0x0200_0001, 0x800F_0800, 0x90E4_0000, 0x0000_FFFF]).into());
        first.textures.clear();
        gpu.draw(&first);
    }
    let before = ddgpu::pipeline_stats();
    let t = Instant::now();
    for d in &stream {
        gpu.draw(d);
    }
    let first = t.elapsed();
    let built = ddgpu::pipeline_stats();
    let t = Instant::now();
    for d in &stream {
        gpu.draw(d);
    }
    let again = t.elapsed();
    gpu.present(&bb);
    let n = stream.len().max(1) as f64;
    let (modules, pipelines) = (built.modules - before.modules, built.pipelines - before.pipelines);
    println!("{} pixel shaders (from {} on), {}: {} errors", stream.len(), skip,
             cache.as_ref().map_or("no pipeline cache".to_string(), |d| format!("pipeline cache {}", d.display())), gpu.errors.len());
    println!("  first draw each:  {:8.2} ms per draw; {modules} modules {:.2} ms each ({:.2} ms translating), {pipelines} pipelines {:.2} ms each",
             first.as_secs_f64() * 1e3 / n,
             (built.module_time - before.module_time).as_secs_f64() * 1e3 / modules.max(1) as f64,
             (built.translate_time - before.translate_time).as_secs_f64() * 1e3 / modules.max(1) as f64,
             (built.pipeline_time - before.pipeline_time).as_secs_f64() * 1e3 / pipelines.max(1) as f64);
    println!("  second draw each: {:8.3} ms per draw", again.as_secs_f64() * 1e3 / n);
    if let Some(dir) = &cache {
        let t = Instant::now();
        ddgpu::save_pipeline_caches();
        let size: u64 = std::fs::read_dir(dir).map(|d| d.filter_map(|e| e.ok()?.metadata().ok()).map(|m| m.len()).sum()).unwrap_or(0);
        println!("  cache saved in {:.0} ms, {:.1} MB", t.elapsed().as_secs_f64() * 1e3, size as f64 / 1e6);
    }
    for e in gpu.errors.iter().take(3) {
        println!("  error: {e}");
    }
}

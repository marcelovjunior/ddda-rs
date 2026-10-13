//! Immutable state groups shared by draws until the corresponding setter changes them.
use super::{declaration, shader_tokens, target, DrawCall, Indices, TextureBinding, Vertices};
use crate::d3d9dev::{objects, with_device, Kind};
use ddnative_base::mem::Mem;
use std::sync::Arc;

pub(crate) const TEXTURES: u8 = 1;
pub(crate) const STREAMS: u8 = 2;
pub(crate) const SHADERS: u8 = 4;
pub(crate) const DECLARATION: u8 = 8;
pub(crate) const TARGETS: u8 = 16;
pub(crate) const INDICES: u8 = 32;

#[derive(Clone, Debug, Default)]
pub(crate) struct Groups {
    textures: Arc<[TextureBinding]>,
    streams: Arc<[(u32, u32, u32, u32, u32)]>,
    shaders: [Option<Arc<[u32]>>; 2],
    declaration: Arc<[[u8; 8]]>,
    index: Option<(u32, u32)>,
    targets: [Option<super::Target>; 4],
    depth: Option<super::Target>,
}

pub(super) fn snapshot(m: &dyn Mem, dev: u32, primitive: u32, primitives: u32,
    start: u32, base_vertex: i32, min_index: u32, vertex_count: u32,
    vertices: Option<Vertices>, mut indices: Option<Indices>) -> DrawCall {
    // D3D9 has 21 texture slots, 16 streams, one index buffer and four RTs.
    // Resource shadows can change through Lock/Unlock without a state setter.
    // The resource dirty index is checked independently of state dirtiness.
    let mut reads = [0u32; 42];
    let mut read_count = 0;
    let mut textures = [(0u32, 0u32, [0u32; 14]); 21];
    let mut texture_count = 0;
    let (state, uploads) = objects(|o| {
        let flush = super::needs_flush(o);
        let d = match o.live.get_mut(&dev).map(|o| &mut o.kind) {
            Some(Kind::Device(d)) => d,
            _ => panic!("{dev:#x} is not a live IDirect3DDevice9"),
        };
        if flush {
            let s = &d.state;
            let mut read = |id| { if id != 0 { reads[read_count] = id; read_count += 1; } };
            for &id in s.textures.values() { read(id); }
            for stream in &s.streams { read(stream.0); }
            if indices.is_none() { read(s.indices); }
            for &rt in &d.render_targets { read(rt); }
        }
        let s = &mut d.state;
        let state = if s.draw_dirty == 0 && s.draw_groups.is_some() {
            let [vs_constants, ps_constants] = s.draw_constants(s.shader_float_counts);
            let groups = s.draw_groups.as_ref().unwrap();
            Ok(DrawCall {
                primitive, primitives, start, base_vertex, min_index, vertex_count,
                vertices: vertices.unwrap_or_else(|| Vertices::Streams(groups.streams.clone())),
                indices: indices.take().unwrap_or_else(|| groups.index.map_or(Indices::None,
                    |(id, fmt)| Indices::Buffer(id, fmt))),
                vertex_shader: groups.shaders[0].clone(), pixel_shader: groups.shaders[1].clone(),
                declaration: groups.declaration.clone(), fvf: s.fvf,
                vs_constants, ps_constants, textures: groups.textures.clone(),
                render_states: s.render.shared(), viewport: s.viewport, scissor: s.scissor,
                targets: groups.targets.clone(), depth: groups.depth.clone(),
            })
        } else {
            let cached = s.draw_groups.take();
            let dirty = std::mem::take(&mut s.draw_dirty);
            let dirty = if cached.is_none() { u8::MAX } else { dirty };
            let mut groups = cached.unwrap_or_default();
            if dirty & TEXTURES != 0 {
                for (&slot, &id) in &s.textures {
                    textures[texture_count] = (slot, id,
                        s.sampler.row(slot));
                    texture_count += 1;
                }
            }
            if dirty & STREAMS != 0 {
                let mut streams = [(0, 0, 0, 0, 0); 16];
                let mut count = 0;
                for (k, v) in s.streams.iter().enumerate() {
                    if v.0 != 0 {
                        streams[count] = (k as u32, v.0, v.1, v.2, s.stream_freq[k]);
                        count += 1;
                    }
                }
                groups.streams = Arc::from(&streams[..count]);
            }
            let [vs_constants, ps_constants] = s.draw_constants(s.shader_float_counts);
            let draw = DrawCall {
                primitive, primitives, start, base_vertex, min_index, vertex_count,
                vertices: vertices.unwrap_or_else(|| Vertices::Streams(groups.streams.clone())),
                indices: Indices::None,
                vertex_shader: None, pixel_shader: None,
                declaration: groups.declaration.clone(), fvf: s.fvf,
                vs_constants, ps_constants,
                textures: groups.textures.clone(),
                render_states: s.render.shared(),
                viewport: s.viewport, scissor: s.scissor,
                targets: std::array::from_fn(|_| None), depth: None,
            };
            Err((groups, dirty, [s.vertex_shader, s.pixel_shader], s.declaration, s.indices,
             d.render_targets, d.depth_stencil, draw))
        };
        let uploads = if read_count != 0 {
            super::collect_reads(o, reads[..read_count].iter().copied())
        } else { Vec::new() };
        (state, uploads)
    });
    super::upload_reads(m, uploads);
    let (mut groups, dirty, shaders, decl, index, rts, depth, mut draw) = match state {
        Ok(draw) => return draw,
        Err(state) => state,
    };
    if dirty & TEXTURES != 0 {
        textures[..texture_count].sort_unstable_by_key(|t| t.0);
        groups.textures = objects(|o| {
            // Build in stack storage, then collect an exact-length iterator
            // directly into Arc. A filter_map collector first grows a Vec.
            let mut bindings: [Option<TextureBinding>; 21] = std::array::from_fn(|_| None);
            let mut count = 0;
            for &(sampler, resource, states) in &textures[..texture_count] {
                if let Some(Kind::Texture { data, .. }) = o.live.get(&resource).map(|o| &o.kind) {
                    bindings[count] = Some(TextureBinding { sampler, resource, states,
                        kind: data.kind, format: data.format, width: data.width, height: data.height,
                        depth: data.depth, levels: data.levels, usage: data.usage });
                    count += 1;
                }
            }
            bindings.into_iter().take(count).map(|binding| binding.unwrap()).collect()
        });
    }
    if dirty & SHADERS != 0 { groups.shaders = shaders.map(shader_tokens); }
    if dirty & DECLARATION != 0 { groups.declaration = declaration(decl); }
    if dirty & INDICES != 0 {
        groups.index = if index == 0 { None } else { Some((index, objects(|o| {
            match o.live.get(&index).map(|o| &o.kind) {
                Some(Kind::Buffer { data, .. }) => data.format,
                _ => 101,
            }
        }))) };
    }
    if dirty & TARGETS != 0 {
        groups.targets = rts.map(target);
        groups.depth = target(depth);
    }
    draw.indices = indices.unwrap_or_else(|| groups.index.map_or(Indices::None, |(id, fmt)| Indices::Buffer(id, fmt)));
    draw.vertex_shader = groups.shaders[0].clone();
    draw.pixel_shader = groups.shaders[1].clone();
    draw.declaration = groups.declaration.clone();
    draw.textures = groups.textures.clone();
    draw.targets = groups.targets.clone();
    draw.depth = groups.depth.clone();
    with_device(dev, |d| d.state.draw_groups = Some(groups));
    draw
}

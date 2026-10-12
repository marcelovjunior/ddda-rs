//! Direct3D 9 surface copies and queries (docs/RUNTIME.md, "Graphics
//! backend design", step 5): StretchRect and ColorFill run on the backend;
//! UpdateSurface and UpdateTexture copy between guest-memory shadows (the
//! destination is uploaded before it is next used); GetRenderTargetData
//! reads a target back into a system-memory surface's shadow; event and
//! timestamp queries answer as a device whose commands complete
//! synchronously; occlusion queries count samples on the backend.
use crate::d3d9::{D3DERR_INVALIDCALL, D3DERR_NOTAVAILABLE, D3D_OK};
use crate::d3d9dev::{new_object, objects, Kind};
use crate::d3d9draw::{backend, flush, target, CopyCall, FillCall};
use crate::d3d9dev::QueryState;
use crate::services::{arg, ret};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

/// A RECT argument (left, top, right, bottom), or all zero for NULL.
fn rect(m: &dyn Mem, p: u32) -> [i32; 4] {
    if p == 0 { [0; 4] } else { std::array::from_fn(|k| m.u32(p + 4 * k as u32) as i32) }
}

/// StretchRect(this, src surface, src RECT*, dst surface, dst RECT*, filter).
pub fn stretch_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (0..6).map(|k| arg(m, cpu, k)).collect();
    cpu.eax = match (target(a[1]), target(a[3])) {
        (Some(src), Some(dst)) => {
            flush(m, a[1]);
            let c = CopyCall { src, src_rect: rect(m, a[2]), dst, dst_rect: rect(m, a[4]), filter: a[5] };
            backend(|b| b.copy(&c));
            D3D_OK
        }
        _ => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 24);
}

/// ColorFill(this, surface, RECT*, D3DCOLOR).
pub fn color_fill(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (s, r, colour) = (arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    cpu.eax = match target(s) {
        Some(dst) => {
            let f = FillCall { dst, rect: rect(m, r), colour };
            backend(|b| b.fill(&f));
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 16);
}

/// The shadow (allocated if needed), pitch, block size, bytes per block and
/// height of a surface's image.
fn surface_shadow(m: &mut dyn Mem, s: u32) -> Option<(u32, u32, u32, u32, u32, u32)> {
    let mut img = objects(|o| match o.live.get_mut(&s).map(|x| &mut x.kind) {
        Some(Kind::Surface { image, .. }) => Some(std::mem::take(image)),
        _ => None,
    })?;
    if img.shadow == 0 {
        img.shadow = crate::services::with_state(|st| st.heap.alloc(img.size.max(1))).expect("guest heap");
        crate::services::zero(m, img.shadow, img.size);
    }
    let r = (img.shadow, img.pitch, img.block, img.block_bytes, img.height, img.width);
    objects(|o| {
        if let Some(Kind::Surface { image, .. }) = o.live.get_mut(&s).map(|x| &mut x.kind) {
            *image = img;
        }
    });
    Some(r)
}

fn mark_dirty(s: u32) {
    objects(|o| {
        if let Some(Kind::Surface { image, .. }) = o.live.get_mut(&s).map(|x| &mut x.kind) {
            image.dirty = true;
        }
    });
}

/// Copy a rectangle of `src`'s image to (`x`, `y`) of `dst`'s (same format).
fn copy_image(m: &mut dyn Mem, src: u32, r: [i32; 4], dst: u32, x: u32, y: u32) -> bool {
    let (Some((sp, spitch, block, bpb, sh, sw)), Some((dp, dpitch, _, _, _, _))) = (surface_shadow(m, src), surface_shadow(m, dst)) else { return false };
    let r = if r == [0; 4] { [0, 0, sw as i32, sh as i32] } else { r };
    let (bx, by) = (r[0] as u32 / block, r[1] as u32 / block);
    let (bw, bh) = (((r[2] - r[0]) as u32).div_ceil(block), ((r[3] - r[1]) as u32).div_ceil(block));
    for row in 0..bh {
        for k in 0..bw * bpb {
            let v = m.u8(sp + (by + row) * spitch + bx * bpb + k);
            m.set_u8(dp + (y / block + row) * dpitch + (x / block) * bpb + k, v);
        }
    }
    mark_dirty(dst);
    true
}

/// UpdateSurface(this, src (system memory), src RECT*, dst (default pool),
/// dst POINT*): a CPU-side copy, uploaded before the destination is used.
pub fn update_surface(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (src, r, dst, pt) = (arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3), arg(m, cpu, 4));
    let (x, y) = if pt == 0 { (0, 0) } else { (m.u32(pt), m.u32(pt + 4)) };
    let r = rect(m, r);
    cpu.eax = if copy_image(m, src, r, dst, x, y) { D3D_OK } else { D3DERR_INVALIDCALL };
    ret(m, cpu, 20);
}

/// UpdateTexture(this, src texture (system memory), dst texture): every
/// level the destination has, from the source level of the same size.
pub fn update_texture(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (src, dst) = (arg(m, cpu, 1), arg(m, cpu, 2));
    let levels = |t: u32| objects(|o| match o.live.get(&t).map(|x| &x.kind) {
        Some(Kind::Texture { data, .. }) => Some(data.surfaces.clone()),
        _ => None,
    });
    cpu.eax = match (levels(src), levels(dst)) {
        (Some(s), Some(d)) if !s.is_empty() && s.len() >= d.len() => {
            let skip = s.len() - d.len();
            for (k, &ds) in d.iter().enumerate() {
                copy_image(m, s[k + skip], [0; 4], ds, 0, 0);
            }
            D3D_OK
        }
        _ => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 12);
}

/// GetRenderTargetData(this, render target, system-memory surface): the
/// target read back by the backend into the destination's shadow. A
/// backend that cannot read targets (the recording one) leaves it as is.
pub fn get_render_target_data(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (rt, dst) = (arg(m, cpu, 1), arg(m, cpu, 2));
    cpu.eax = match target(rt) {
        // a multisampled target cannot be read (Microsoft Learn,
        // GetRenderTargetData); it is resolved with StretchRect first
        Some(t) if t.samples != 0 => D3DERR_INVALIDCALL,
        Some(t) => {
            if let (Some(bytes), Some((dp, dpitch, _, _, h, _))) = (backend(|b| b.read_back(&t)), surface_shadow(m, dst)) {
                let row = bytes.len() / t.height.max(1) as usize;
                for y in 0..h.min(t.height) as usize {
                    for (k, &v) in bytes[y * row..(y + 1) * row].iter().enumerate().take(dpitch as usize) {
                        m.set_u8(dp + y as u32 * dpitch + k as u32, v);
                    }
                }
            }
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 12);
}

// --- queries ---------------------------------------------------------------------

const QUERY_EVENT: u32 = 8;
const QUERY_OCCLUSION: u32 = 9;
const QUERY_TIMESTAMP: u32 = 10;
const QUERY_TIMESTAMPDISJOINT: u32 = 11;
const QUERY_TIMESTAMPFREQ: u32 = 12;

/// CreateQuery(this, type, IDirect3DQuery9**): a NULL out pointer asks
/// whether the type is supported. Event, timestamp and occlusion queries
/// are (the game uses occlusion queries: nDraw::Query, 0x01148093).
pub fn create_query(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, ty, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let supported = matches!(ty, QUERY_EVENT | QUERY_OCCLUSION | QUERY_TIMESTAMP | QUERY_TIMESTAMPDISJOINT | QUERY_TIMESTAMPFREQ);
    cpu.eax = if !supported {
        D3DERR_NOTAVAILABLE
    } else if out == 0 {
        D3D_OK
    } else {
        let q = new_object(m, "IDirect3DQuery9", Kind::Query { device: dev, ty, state: QueryState::Created, value: 0 });
        m.set_u32(out, q);
        D3D_OK
    };
    ret(m, cpu, 12);
}

fn query<R>(q: u32, f: impl FnOnce(&mut u32, &mut QueryState, &mut u64) -> R) -> R {
    objects(|o| match o.live.get_mut(&q).map(|x| &mut x.kind) {
        Some(Kind::Query { ty, state, value, .. }) => f(ty, state, value),
        _ => panic!("{q:#x} is not a live IDirect3DQuery9"),
    })
}

/// GetType(this).
pub fn query_get_type(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = query(arg(m, cpu, 0), |ty, _, _| *ty);
    ret(m, cpu, 4);
}
/// GetDataSize(this): BOOL for events, DWORD for occlusion, UINT64 for
/// timestamps, BOOL for disjoint.
pub fn query_get_data_size(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = query(arg(m, cpu, 0), |ty, _, _| match *ty {
        QUERY_TIMESTAMP | QUERY_TIMESTAMPFREQ => 8,
        _ => 4,
    });
    ret(m, cpu, 4);
}
const ISSUE_END: u32 = 1;
const ISSUE_BEGIN: u32 = 2;

/// Issue(this, flags): D3DISSUE_END (1) completes an event or timestamp
/// query at once (the device's commands are synchronous; a timestamp
/// samples the clock). An occlusion query counts on the backend from
/// D3DISSUE_BEGIN (2) to D3DISSUE_END; an END without a BEGIN counts
/// nothing (Wine's d3d9 issues the begin itself; not measured on the
/// driver, and the game always pairs them).
pub fn query_issue(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (q, flags) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let ty = query(q, |ty, _, _| *ty);
    if ty == QUERY_OCCLUSION {
        let state = query(q, |_, state, _| *state);
        if flags & ISSUE_BEGIN != 0 {
            backend(|b| b.query_begin(q));
            query(q, |_, state, _| *state = QueryState::Building);
        }
        if flags & ISSUE_END != 0 {
            if state == QueryState::Created && flags & ISSUE_BEGIN == 0 {
                backend(|b| b.query_begin(q));
            }
            backend(|b| b.query_end(q));
            query(q, |_, state, _| *state = QueryState::Ended);
        }
    } else if flags & ISSUE_END != 0 {
        query(q, |ty, state, value| {
            *state = QueryState::Ended;
            if *ty == QUERY_TIMESTAMP {
                *value = crate::services::runtime_clock().as_nanos() as u64;
            }
        });
    }
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// GetData(this, data*, size, flags): S_OK with the result once issued,
/// S_FALSE (1) before. Occlusion queries, as measured on the driver (ddgpu
/// test occlusion_query_states_on_the_d3d9_driver): before any Issue S_OK
/// with 0xDDDDDDDD written, between BEGIN and END S_FALSE, after END S_OK
/// with the sample count (S_FALSE while a backend that draws nothing has
/// no count).
pub fn query_get_data(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (q, data, size) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let (ty, state, value) = query(q, |ty, state, value| (*ty, *state, *value));
    let occlusion = if ty != QUERY_OCCLUSION {
        None
    } else {
        match state {
            QueryState::Created => Some(Some(0xDDDD_DDDDu64)),
            QueryState::Building => Some(None),
            QueryState::Ended => Some(backend(|b| b.query_result(q))),
        }
    };
    cpu.eax = if occlusion == Some(None) || (occlusion.is_none() && state != QueryState::Ended && ty != QUERY_TIMESTAMPFREQ) {
        1
    } else {
        let v: u64 = match ty {
            QUERY_EVENT => 1,
            QUERY_TIMESTAMP => value,
            QUERY_TIMESTAMPFREQ => 1_000_000_000,
            QUERY_TIMESTAMPDISJOINT => 0,
            _ => occlusion.flatten().unwrap_or(0),
        };
        if data != 0 {
            for k in 0..size.min(8) {
                m.set_u8(data + k, (v >> (8 * k)) as u8);
            }
        }
        D3D_OK
    };
    ret(m, cpu, 16);
}

//! L193 draw census (`DDRUNTIME_CENSUS=1`, off by default, diagnostic only): classifies the
//! native sink's taken draws by pass (render-target size), mesh identity (vertex streams, index
//! buffer, ranges), material identity (shaders, textures, render-state list), whether the
//! draw's per-object vertex constants changed since the previous frame (static vs moving) and
//! the instancing potential. Collects 60 frames once the scene carries > 2000 draws, writes a
//! markdown report to `DDRUNTIME_CENSUS_OUT` (default `census_out.md`) and stops.
//!
//! "Per-object registers" are the vertex-shader float registers that differ between the draws
//! of one (vertex shader, viewport) within the frame; a draw is "static" when its group (same
//! mesh + material, nth occurrence) had the same per-object register values in the previous
//! frame. Camera constants are equal across a (shader, viewport) group, so they do not count.
#![cfg_attr(not(feature = "game"), allow(dead_code))]
use d3d9cmd::{Constants, DrawCall, Indices, Vertices};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("DDRUNTIME_CENSUS").as_deref() == Ok("1"))
}

fn h<T: Hash>(v: T) -> u64 {
    let mut x = crate::fasthash::FxHasher::default();
    v.hash(&mut x);
    x.finish()
}

struct Rec {
    size: (u32, u32),
    mesh: u64,
    mat: u64,
    vs: u64,
    consts: Arc<Constants>,
    ntex: usize,
    tex: Vec<u32>,
    skinned: bool,
    mesh_desc: String,
}

#[derive(Default)]
struct Group {
    count: f64,
    desc: String,
    skinned: bool,
    static_n: f64,
}

#[derive(Default)]
struct PassAgg {
    seen: f64,
    draws: f64,
    groups: f64,
    meshes: f64,
    mats: f64,
    statics: f64,
    skinned: f64,
    inst_all: f64,
    inst_unskinned: f64,
    runs_removed: f64,
    mat_changes: f64,
    tex_binds: f64,
    tex_unique: f64,
    groups_tab: HashMap<u64, Group>,
}

#[derive(Default)]
struct State {
    cur: Vec<Rec>,
    seen: HashMap<(u32, u32), u64>,
    prev: HashMap<(u64, u32), u64>,
    frames: u32,
    agg: BTreeMap<(u32, u32), PassAgg>,
    first_frame: Option<String>,
    done: bool,
}
static STATE: Mutex<Option<State>> = Mutex::new(None);
fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(State::default))
}

pub(crate) fn note_total(size: (u32, u32)) {
    with(|s| *s.seen.entry(size).or_default() += 1);
}

pub(crate) fn note_draw(size: (u32, u32), d: &DrawCall) {
    let Vertices::Streams(st) = &d.vertices else { return };
    let (ib, ifmt) = match d.indices {
        Indices::Buffer(b, f) => (b, f),
        _ => (0, 0),
    };
    let mesh = h((st.iter().map(|s| (s.0, s.1, s.2, s.3)).collect::<Vec<_>>(), ib, d.start, d.primitives, d.base_vertex, d.vertex_count, d.primitive));
    let vsp = d.vertex_shader.as_ref().map_or(0, |a| Arc::as_ptr(a) as *const u32 as usize);
    let psp = d.pixel_shader.as_ref().map_or(0, |a| Arc::as_ptr(a) as *const u32 as usize);
    let tex: Vec<u32> = d.textures.iter().map(|t| t.resource).collect();
    let mat = h((vsp, psp, &tex, d.render_states.address(), d.declaration.len()));
    let rec = Rec {
        size,
        mesh,
        mat,
        vs: h((vsp, d.viewport)),
        consts: d.vs_constants.clone(),
        ntex: tex.len(),
        tex,
        skinned: d.vs_constants.f.len() >= 100,
        mesh_desc: format!(
            "vb{:?} ib{:#x}/{} start{} prims{} base{}",
            st.iter().map(|s| (s.1, s.2)).collect::<Vec<_>>(), ib, ifmt, d.start, d.primitives, d.base_vertex
        ),
    };
    with(|s| {
        if !s.done {
            s.cur.push(rec)
        }
    });
}

pub(crate) fn frame_boundary() {
    with(|s| {
        if s.done {
            return;
        }
        let cur = std::mem::take(&mut s.cur);
        let seen = std::mem::take(&mut s.seen);
        if cur.len() < 2000 && s.frames == 0 {
            return; // not in the scene yet (menus, loading)
        }
        analyse(s, &cur, &seen);
        s.frames += 1;
        if s.frames >= 60 {
            s.done = true;
            let path = std::env::var("DDRUNTIME_CENSUS_OUT").unwrap_or_else(|_| "census_out.md".into());
            let text = report(s);
            let _ = std::fs::write(&path, &text);
            eprintln!("census: wrote {path} ({} frames)", s.frames);
        }
    });
}

fn analyse(s: &mut State, cur: &[Rec], seen: &HashMap<(u32, u32), u64>) {
    let mut varying: HashMap<u64, Vec<bool>> = HashMap::new();
    let mut first: HashMap<u64, &Rec> = HashMap::new();
    for r in cur {
        match first.get(&r.vs) {
            None => {
                first.insert(r.vs, r);
            }
            Some(f) => {
                let v = varying.entry(r.vs).or_insert_with(|| vec![false; f.consts.f.len()]);
                for (i, reg) in r.consts.f.iter().enumerate().take(v.len()) {
                    if f.consts.f.get(i) != Some(reg) {
                        v[i] = true;
                    }
                }
            }
        }
    }
    let mut occ: HashMap<u64, u32> = HashMap::new();
    let mut next_prev: HashMap<(u64, u32), u64> = HashMap::new();
    let mut by_pass: BTreeMap<(u32, u32), Vec<(&Rec, bool)>> = BTreeMap::new();
    for r in cur {
        let key = r.mesh ^ r.mat.rotate_left(17);
        let o = occ.entry(key).or_default();
        let reg_hash = match varying.get(&r.vs) {
            Some(v) => h(r.consts.f.iter().enumerate().filter(|(i, _)| v.get(*i).copied().unwrap_or(false)).map(|(_, x)| *x).collect::<Vec<_>>()),
            None => 0,
        };
        let is_static = s.prev.get(&(key, *o)) == Some(&reg_hash);
        next_prev.insert((key, *o), reg_hash);
        *o += 1;
        by_pass.entry(r.size).or_default().push((r, is_static));
    }
    s.prev = next_prev;
    for (size, v) in by_pass {
        let a = s.agg.entry(size).or_default();
        a.seen += *seen.get(&size).unwrap_or(&(v.len() as u64)) as f64;
        a.draws += v.len() as f64;
        let mut groups: HashMap<u64, (usize, bool)> = HashMap::new();
        let mut meshes = HashSet::new();
        let mut mats = HashSet::new();
        let mut texres = HashSet::new();
        let mut runs = 0usize;
        let mut changes = 0usize;
        let (mut last, mut lastmat) = (0u64, 0u64);
        for (i, (r, st)) in v.iter().enumerate() {
            let key = r.mesh ^ r.mat.rotate_left(17);
            let e = groups.entry(key).or_insert((0, r.skinned));
            e.0 += 1;
            meshes.insert(r.mesh);
            mats.insert(r.mat);
            texres.extend(r.tex.iter().copied());
            a.tex_binds += r.ntex as f64;
            if i == 0 || key != last {
                runs += 1;
            }
            if i == 0 || r.mat != lastmat {
                changes += 1;
            }
            last = key;
            lastmat = r.mat;
            if *st {
                a.statics += 1.0;
            }
            if r.skinned {
                a.skinned += 1.0;
            }
            let gt = a.groups_tab.entry(key).or_default();
            gt.count += 1.0;
            if *st {
                gt.static_n += 1.0;
            }
            if gt.desc.is_empty() {
                gt.desc = format!("mat {:016x} {}", r.mat, r.mesh_desc);
                gt.skinned = r.skinned;
            }
        }
        a.groups += groups.len() as f64;
        a.meshes += meshes.len() as f64;
        a.mats += mats.len() as f64;
        a.runs_removed += (v.len() - runs) as f64;
        a.mat_changes += changes as f64;
        a.tex_unique += texres.len() as f64;
        a.inst_all += (v.len() - groups.len()) as f64;
        let unskinned = v.iter().filter(|(r, _)| !r.skinned).count();
        let unskinned_groups = groups.values().filter(|g| !g.1).count();
        a.inst_unskinned += unskinned.saturating_sub(unskinned_groups) as f64;
    }
    if s.first_frame.is_none() {
        s.first_frame = Some(format!("{} taken draws in the first counted frame", cur.len()));
    }
}

fn pass_name(size: (u32, u32), main: (u32, u32)) -> &'static str {
    if size == main {
        "main view"
    } else if size == (512, 1536) {
        "shadow maps"
    } else if size.0 <= 64 && size.1 <= 16 {
        "soft-body sim"
    } else {
        "post / other"
    }
}

fn report(s: &State) -> String {
    let n = s.frames.max(1) as f64;
    let main = s.agg.keys().copied().max_by_key(|&(w, h)| w as u64 * h as u64).unwrap_or((0, 0));
    let mut out = format!("# census raw ({} frames averaged; {})\n\n", s.frames, s.first_frame.clone().unwrap_or_default());
    out += "| pass (size) | seen | taken | groups (mesh+material) | meshes | materials | removed by instancing (all) | (non-skinned) | removed by merging contiguous equal draws | material changes | % static | % skinned-like | tex binds | unique tex |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n";
    let mut classes: BTreeMap<&str, PassAgg> = BTreeMap::new();
    for (size, a) in &s.agg {
        let p = |x: f64| x / n;
        out += &format!(
            "| {} {}x{} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.1} | {:.1} | {:.0} | {:.0} |\n",
            pass_name(*size, main), size.0, size.1, p(a.seen), p(a.draws), p(a.groups), p(a.meshes), p(a.mats), p(a.inst_all), p(a.inst_unskinned),
            p(a.runs_removed), p(a.mat_changes), a.statics / a.draws.max(1.0) * 100.0, a.skinned / a.draws.max(1.0) * 100.0, p(a.tex_binds), p(a.tex_unique)
        );
        let c = classes.entry(pass_name(*size, main)).or_default();
        c.seen += a.seen;
        c.draws += a.draws;
        c.groups += a.groups;
        c.inst_all += a.inst_all;
        c.inst_unskinned += a.inst_unskinned;
        c.statics += a.statics;
        c.skinned += a.skinned;
        c.runs_removed += a.runs_removed;
        c.mat_changes += a.mat_changes;
        c.mats += a.mats;
    }
    out += "\n## per pass class (per frame)\n\n| class | seen | taken | groups | instancing removes (all / non-skinned) | contiguous merge removes | material changes | % static | % skinned-like |\n|---|---|---|---|---|---|---|---|---|\n";
    for (name, a) in &classes {
        out += &format!(
            "| {} | {:.0} | {:.0} | {:.0} | {:.0} / {:.0} | {:.0} | {:.0} | {:.1} | {:.1} |\n",
            name, a.seen / n, a.draws / n, a.groups / n, a.inst_all / n, a.inst_unskinned / n, a.runs_removed / n, a.mat_changes / n,
            a.statics / a.draws.max(1.0) * 100.0, a.skinned / a.draws.max(1.0) * 100.0
        );
    }
    for (size, a) in &s.agg {
        if a.draws / n < 20.0 {
            continue;
        }
        out += &format!("\n## top 20 groups, {} {}x{} (mean draws per frame, % static)\n\n", pass_name(*size, main), size.0, size.1);
        let mut v: Vec<_> = a.groups_tab.values().collect();
        v.sort_by(|x, y| y.count.partial_cmp(&x.count).unwrap());
        for g in v.iter().take(20) {
            out += &format!("- {:.1} draws, {:.0}% static{}: {}\n", g.count / n, g.static_n / g.count * 100.0, if g.skinned { ", skinned-like" } else { "" }, g.desc);
        }
    }
    out
}

//! `mod-scan`, `mrl-scan`, `mod-info`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use mtshader::ShaderPackage;

use crate::scan::{for_each_archive, read_one};

pub const RMODEL: u32 = 0x58A1_5856;
pub const RMATERIAL: u32 = 0x2749_C8A8;

pub fn load_mfx(native_pc: &Path) -> Result<ShaderPackage, String> {
    let p = native_pc.join("usershader").join("BbsShaderPackage.mfx");
    let b = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    ShaderPackage::parse(&b).map_err(|e| format!("{}: {e}", p.display()))
}

#[derive(Default)]
struct ModStats {
    models: u64,
    meshes: u64,
    vertices: u64,
    triangles: u64,
    by_layout: BTreeMap<String, u64>,
    normal_off: u64,
    normals: u64,
    weight_sum_off: u64,
    skinned_vertices: u64,
    joint_oob: u64,
    nonfinite_uv: u64,
    materials_resolved: u64,
    materials_missing: u64,
    no_mrl: u64,
}

/// Parses every model, decodes every vertex with its MFX layout, checks
/// the decoded values, and resolves material names in the MRL of the same
/// path (same archive).
pub fn mod_scan(dir: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let pkg = load_mfx(dir)?;
    let stats = Mutex::new(ModStats::default());
    let walk = for_each_archive(dir, &[RMODEL, RMATERIAL], |_, items| {
        let mut errs = Vec::new();
        let mut st = ModStats::default();
        let mrls: BTreeMap<&str, mtmrl::MaterialList> = items
            .iter()
            .filter(|(e, _)| e.type_hash == RMATERIAL)
            .filter_map(|(e, b)| mtmrl::parse(b).ok().map(|l| (e.path.as_str(), l)))
            .collect();
        for (e, b) in items.iter().filter(|(e, _)| e.type_hash == RMODEL) {
            let model = match mtmod::parse(b) {
                Ok(m) => m,
                Err(x) => {
                    errs.push(format!("{}: {x}", e.path));
                    continue;
                }
            };
            st.models += 1;
            match mrls.get(e.path.as_str()) {
                Some(l) => {
                    for n in &model.material_names {
                        if l.by_name(n).is_some() {
                            st.materials_resolved += 1;
                        } else {
                            st.materials_missing += 1;
                        }
                    }
                }
                None => st.no_mrl += 1,
            }
            for (mi, m) in model.meshes.iter().enumerate() {
                let Some(layout) = pkg.layout(m.vertex_format) else {
                    errs.push(format!(
                        "{}: mesh {mi}: vertex format {:08x} not in MFX",
                        e.path, m.vertex_format
                    ));
                    continue;
                };
                let v = match model.decode(m, layout) {
                    Ok(v) => v,
                    Err(x) => {
                        errs.push(format!("{}: mesh {mi}: {x}", e.path));
                        continue;
                    }
                };
                st.meshes += 1;
                st.vertices += v.positions.len() as u64;
                *st.by_layout.entry(layout.name.clone()).or_default() += 1;
                st.triangles += model.triangles(m).map(|t| t.len() as u64).unwrap_or(0);
                for n in &v.normals {
                    st.normals += 1;
                    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                    if (len - 1.0).abs() > 0.05 {
                        st.normal_off += 1;
                    }
                }
                for uv in v.uvs.iter().flatten() {
                    if !uv[0].is_finite() || !uv[1].is_finite() {
                        st.nonfinite_uv += 1;
                    }
                }
                for (j, w) in v.joints.iter().zip(&v.weights) {
                    st.skinned_vertices += 1;
                    let sum: f32 = w.iter().sum();
                    if (sum - 1.0).abs() > 0.02 || w.iter().any(|&x| !(-0.001..=1.001).contains(&x)) {
                        st.weight_sum_off += 1;
                    }
                    let n = m.max_weights().max(1) as usize;
                    if j[..n.min(8)]
                        .iter()
                        .zip(w)
                        .any(|(&ji, &wi)| wi > 0.0 && ji as usize >= model.bones.len().max(1))
                    {
                        st.joint_oob += 1;
                    }
                }
            }
        }
        let mut g = stats.lock().unwrap();
        g.models += st.models;
        g.meshes += st.meshes;
        g.vertices += st.vertices;
        g.triangles += st.triangles;
        g.normal_off += st.normal_off;
        g.normals += st.normals;
        g.weight_sum_off += st.weight_sum_off;
        g.skinned_vertices += st.skinned_vertices;
        g.joint_oob += st.joint_oob;
        g.nonfinite_uv += st.nonfinite_uv;
        g.materials_resolved += st.materials_resolved;
        g.materials_missing += st.materials_missing;
        g.no_mrl += st.no_mrl;
        for (k, v) in st.by_layout {
            *g.by_layout.entry(k).or_default() += v;
        }
        errs
    })?;
    let s = stats.into_inner().unwrap();
    println!("archives:   {}", walk.archives);
    println!(
        "models:     {} parsed, {} failures",
        s.models,
        walk.failures.len()
    );
    println!(
        "meshes:     {} decoded, {} vertices, {} triangles",
        s.meshes, s.vertices, s.triangles
    );
    println!(
        "normals:    {} of {} off unit length by > 0.05",
        s.normal_off, s.normals
    );
    println!(
        "skinning:   {} vertices, {} weight sums off, {} joints >= bone count",
        s.skinned_vertices, s.weight_sum_off, s.joint_oob
    );
    println!("uvs:        {} non-finite", s.nonfinite_uv);
    println!(
        "materials:  {} names resolved in same-path MRL, {} missing, {} models without same-path MRL",
        s.materials_resolved, s.materials_missing, s.no_mrl
    );
    println!(
        "elapsed:    {:.1} s\n\nmeshes per layout:",
        started.elapsed().as_secs_f64()
    );
    for (k, v) in &s.by_layout {
        println!("  {k:<20} {v}");
    }
    for f in walk.failures.iter().take(20) {
        println!("  FAIL {f}");
    }
    if walk.failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} failures", walk.failures.len()))
    }
}

/// Parses every MRL and resolves every command's shader object in the MFX.
pub fn mrl_scan(dir: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let pkg = load_mfx(dir)?;
    let counts = Mutex::new((0u64, 0u64, 0u64, BTreeMap::<String, u64>::new()));
    let walk = for_each_archive(dir, &[RMATERIAL], |_, items| {
        let mut errs = Vec::new();
        let (mut lists, mut cmds, mut texrefs) = (0, 0, 0);
        let mut features = BTreeMap::<String, u64>::new();
        for (e, b) in items {
            let l = match mtmrl::parse(b) {
                Ok(l) => l,
                Err(x) => {
                    errs.push(format!("{}: {x}", e.path));
                    continue;
                }
            };
            lists += 1;
            for m in &l.materials {
                for c in &m.commands {
                    cmds += 1;
                    let Some(obj) = pkg.object(c.object) else {
                        errs.push(format!("{}: shader object {:08x} not in MFX", e.path, c.object));
                        continue;
                    };
                    match c.kind {
                        mtmrl::CommandKind::Texture => {
                            if c.value != 0 && l.texture(c).is_none() {
                                errs.push(format!(
                                    "{}: texture index {} of {}",
                                    e.path,
                                    c.value,
                                    l.textures.len()
                                ));
                            }
                            texrefs += 1;
                        }
                        mtmrl::CommandKind::Feature => {
                            let Some(v) = pkg.object(c.value) else {
                                errs.push(format!("{}: feature value {:08x} not in MFX", e.path, c.value));
                                continue;
                            };
                            if obj.name == "FAlbedo" || obj.name == "FBump" || obj.name == "FTransparency" {
                                *features.entry(format!("{}={}", obj.name, v.name)).or_default() += 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        let mut g = counts.lock().unwrap();
        g.0 += lists;
        g.1 += cmds;
        g.2 += texrefs;
        for (k, v) in features {
            *g.3.entry(k).or_default() += v;
        }
        errs
    })?;
    let (lists, cmds, texrefs, features) = counts.into_inner().unwrap();
    println!(
        "material lists: {lists} parsed ({} entries visited), {} failures",
        walk.visited,
        walk.failures.len()
    );
    println!("commands:       {cmds} all resolved in the MFX; {texrefs} texture bindings");
    println!("elapsed:        {:.1} s\n", started.elapsed().as_secs_f64());
    for (k, v) in &features {
        println!("  {k:<40} {v}");
    }
    for f in walk.failures.iter().take(20) {
        println!("  FAIL {f}");
    }
    if walk.failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} failures", walk.failures.len()))
    }
}

/// Prints one model's meshes, layouts and material bindings.
pub fn mod_info(native_pc: &Path, arc: &Path, entry: &str) -> Result<(), String> {
    let pkg = load_mfx(native_pc)?;
    let b = read_one(arc, entry, RMODEL)?;
    let model = mtmod::parse(&b).map_err(|e| e.to_string())?;
    let mrl_bytes = read_one(arc, entry, RMATERIAL).ok();
    let mrl = mrl_bytes.as_deref().and_then(|b| mtmrl::parse(b).ok());
    println!(
        "{} bones, {} meshes, {} materials, bbox {:?}..{:?}",
        model.bones.len(),
        model.meshes.len(),
        model.material_names.len(),
        &model.bbox_min[..3],
        &model.bbox_max[..3]
    );
    for (i, m) in model.meshes.iter().enumerate() {
        let layout = pkg.layout(m.vertex_format).map_or("?", |l| l.name.as_str());
        let mat = model
            .material_names
            .get(m.material as usize)
            .map_or("?", String::as_str);
        println!(
            "  mesh {i:>3} lod {:>3} group {:>3} {:<16} {:>5} verts {:>6} idx  material {mat}",
            m.lod, m.group, layout, m.vertex_count, m.index_count
        );
    }
    if let Some(l) = mrl {
        for (name, m) in model
            .material_names
            .iter()
            .filter_map(|n| l.by_name(n).map(|m| (n, m)))
        {
            println!("  material {name}:");
            for c in &m.commands {
                let obj = pkg.object(c.object).map_or("?", |o| o.name.as_str());
                match c.kind {
                    mtmrl::CommandKind::Texture => {
                        println!("    {obj:<24} {}", l.texture(c).map_or("-", |t| t.path.as_str()))
                    }
                    mtmrl::CommandKind::Feature | mtmrl::CommandKind::Sampler => {
                        println!(
                            "    {obj:<24} = {}",
                            pkg.object(c.value).map_or("?", |o| o.name.as_str())
                        )
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

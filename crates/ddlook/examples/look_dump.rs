//! Print a character model's bones and meshes (part group, LOD, material)
//! and, for NPC ids, the NPC's look.
//!
//! cargo run -p ddlook --example look_dump -- <nativePC> model <rModel path>...
//! cargo run -p ddlook --example look_dump -- <nativePC> npc <id> [unit] [row]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let v = ddvfs::Vfs::open(&a[1]).unwrap();
    match a[2].as_str() {
        "model" => {
            let np = std::path::Path::new(&a[1]);
            let mfx = mtshader::ShaderPackage::parse(&std::fs::read(np.join("usershader").join("BbsShaderPackage.mfx")).unwrap()).unwrap();
            for p in &a[3..] {
                let b = v.read(p, mtarc::type_hash("rModel")).unwrap();
                let m = mtmod::parse(&b).unwrap();
                let ids: Vec<u8> = m.bones.iter().map(|b| b.anim_map_index).collect();
                println!("{p}: {} bones (ids {:?}...), {} meshes", m.bones.len(), &ids[..ids.len().min(12)], m.meshes.len());
                let mut groups: std::collections::BTreeMap<u16, (usize, Vec<u8>)> = Default::default();
                for me in &m.meshes {
                    let e = groups.entry(me.group).or_default();
                    e.0 += 1;
                    if !e.1.contains(&me.lod) {
                        e.1.push(me.lod);
                    }
                }
                println!("  groups (mesh count, lods): {groups:?}");
                if std::env::var_os("LOOK_DUMP_RAW").is_some() {
                    // World frame from the inverse bind (rigid, row vectors).
                    for (i, b) in m.bones.iter().enumerate() {
                        let r = |i: usize, j: usize| b.inverse_bind[i * 4 + j];
                        let axis = |j: usize| [r(0, j), r(1, j), r(2, j)];
                        let t: [f32; 3] = std::array::from_fn(|j| -(0..3).map(|k| r(3, k) * r(j, k)).sum::<f32>());
                        let parent = m.bones.get(b.parent as usize).map(|p| p.anim_map_index);
                        println!("  bone {i} id {} parent {parent:?} pos {t:.1?} x {:.2?} y {:.2?} z {:.2?}", b.anim_map_index, axis(0), axis(1), axis(2));
                    }
                }
                for me in &m.meshes {
                    let lay = mfx.layout(me.vertex_format);
                    let d = lay.map(|l| m.decode(me, l));
                    let (np_, nj) = match &d {
                        Some(Ok(v)) => (v.positions.len(), v.joints.len()),
                        _ => (0, 0),
                    };
                    let bbox = match &d {
                        Some(Ok(v)) => v.positions.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
                            (std::array::from_fn(|i| lo[i].min(p[i]).round()), std::array::from_fn(|i| hi[i].max(p[i]).round()))
                        }),
                        _ => ([0.0; 3], [0.0; 3]),
                    };
                    println!(
                        "  mesh group {} bbox {:?} lod {} verts {} decoded {} joints {} bone_start {} weights {} layout {:?}",
                        me.group,
                        bbox,
                        me.lod,
                        me.vertex_count,
                        np_,
                        nj,
                        me.bone_start,
                        me.max_weights(),
                        lay.map(|l| l.name.clone())
                    );
                    if std::env::var_os("LOOK_DUMP_RAW").is_some() {
                        println!("    {me:?}");
                    }
                }
            }
        }
        "npc" => {
            let id: u32 = a[3].parse().unwrap();
            let unit = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(id);
            let row = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
            let l = ddlook::npc_look(&v, id, unit, row).unwrap();
            println!("{l:#?}");
        }
        _ => panic!("model | npc"),
    }
}

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
                for me in &m.meshes {
                    let lay = mfx.layout(me.vertex_format);
                    let d = lay.map(|l| m.decode(me, l));
                    let (np_, nj) = match &d {
                        Some(Ok(v)) => (v.positions.len(), v.joints.len()),
                        _ => (0, 0),
                    };
                    println!(
                        "  mesh group {} lod {} verts {} decoded {} joints {} bone_start {} weights {} layout {:?}",
                        me.group,
                        me.lod,
                        me.vertex_count,
                        np_,
                        nj,
                        me.bone_start,
                        me.max_weights(),
                        lay.map(|l| l.name.clone())
                    );
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

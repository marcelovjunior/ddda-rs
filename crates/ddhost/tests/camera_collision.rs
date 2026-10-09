//! Nonfield stage collision retains the same metadata as field collision.

#[test]
fn cassardis_rays_keep_authored_material_and_triangle_attribute() {
    let path = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !path.exists() {
        assert!(std::env::var_os("DDDA_NATIVE_PC").is_none(), "explicit install missing");
        eprintln!("SKIP: install absent");
        return;
    }
    let vfs = ddvfs::Vfs::open(path).unwrap();
    let bytes = vfs.read(r"scr\st200\collision\st200h_mrg00", mtarc::type_hash("rCollision")).unwrap();
    let sbc = mtsbc::parse(&bytes).unwrap();
    let terrain = ddhost::install::stage_collision(&vfs, 200).unwrap();
    assert_eq!(terrain.triangles(), sbc.world_triangles().count());
    let mut checked = 0;
    let mut nonzero_flags = false;
    for (points, triangle) in sbc.world_triangles() {
        let Some(&material) = sbc.materials.get(usize::from(triangle.material)) else { continue };
        let normal = triangle.normal;
        let norm_sq: f32 = normal.iter().map(|n| n * n).sum();
        if norm_sq < 0.5 { continue; }
        let center: [f32; 3] = std::array::from_fn(|i| (points[0][i] + points[1][i] + points[2][i]) / 3.0);
        let from = std::array::from_fn(|i| center[i] + normal[i]);
        let to = std::array::from_fn(|i| center[i] - normal[i]);
        let Some(hit) = terrain.ray_with_material(from, to) else { continue };
        // Intersections with another surface are valid, but do not prove
        // metadata binding for this source triangle. Compare only this face.
        if (0..3).any(|i| (hit.hit.point[i] - center[i]).abs() > 0.01) { continue; }
        // Exact coplanar duplicates may have distinct authored records.
        if hit.material != Some(material) || hit.attribute != Some(triangle.attribute) {
            let matches_duplicate = sbc.world_triangles().any(|(other, record)| {
                other == points && hit.material == sbc.materials.get(usize::from(record.material)).copied()
                    && hit.attribute == Some(record.attribute)
            });
            assert!(matches_duplicate, "material/attribute lost at {center:?}: {hit:?}");
        }
        checked += 1;
        nonzero_flags |= material[0] != 0;
        if checked >= 100 && nonzero_flags { break; }
    }
    assert!(checked >= 100, "only {checked} qualifying triangle casts");
    assert!(nonzero_flags, "fixture must exercise nonzero collision flags");
}

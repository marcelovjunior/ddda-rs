use ddnav::{Mesh, WaypointSet};

fn word(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_le_bytes());
}
fn empty_mesh() -> Vec<u8> {
    let mut d = b"NAV\0".to_vec();
    for n in [33, 0, 0, 0] {
        word(&mut d, n);
    } // header, total, empty counted name
    d.push(0);
    for _ in 0..3 {
        word(&mut d, 0);
    } // vertices, nodes, node info
    d.push(0);
    d.extend([0; 6]); // vertex extension flag, groups, group footer
    d.extend([0; 12]); // three totals
    d.push(0);
    word(&mut d, 0);
    d.extend([0; 32]); // empty spatial cells
    d
}
fn empty_way() -> Vec<u8> {
    let mut d = b"WAY\0".to_vec();
    word(&mut d, 38);
    d.extend([0; 28]); // kind, words, nodes, four totals
    d.push(0);
    d.extend([0; 24]); // zero levels, max/min bounds
    d
}

#[test]
fn navigation_rejects_partial_records_counts_versions_and_nonfinite_values() {
    let mesh = empty_mesh();
    let way = empty_way();
    assert!(Mesh::parse(&mesh).is_ok());
    assert!(WaypointSet::parse(&way).is_ok());
    for end in 0..mesh.len() {
        assert!(Mesh::parse(&mesh[..end]).is_err(), "mesh prefix{end}");
    }
    for end in 0..way.len() {
        assert!(WaypointSet::parse(&way[..end]).is_err(), "way prefix{end}");
    }
    let mut bad = mesh.clone();
    bad[4] = 34;
    assert!(Mesh::parse(&bad).is_err());
    let mut bad = way.clone();
    bad[4] = 39;
    assert!(WaypointSet::parse(&bad).is_err());
    let mut bad = mesh.clone();
    bad[21..25].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Mesh::parse(&bad).is_err());
    let mut bad = way.clone();
    bad[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(WaypointSet::parse(&bad).is_err());
    let mut bad = way.clone();
    bad[36] = 255;
    assert!(WaypointSet::parse(&bad).is_err());
    let mut bad = mesh.clone();
    let n = bad.len();
    bad[n - 4..].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(Mesh::parse(&bad).is_err());
    let mut bad = way.clone();
    bad[37..41].copy_from_slice(&f32::INFINITY.to_le_bytes());
    assert!(WaypointSet::parse(&bad).is_err());
    let mut bad = mesh.clone();
    bad.push(0);
    assert!(Mesh::parse(&bad).is_err());
    let mut bad = way.clone();
    bad.push(0);
    assert!(WaypointSet::parse(&bad).is_err());
    assert!(Mesh::parse(&way).is_err());
    assert!(WaypointSet::parse(&mesh).is_err());
}

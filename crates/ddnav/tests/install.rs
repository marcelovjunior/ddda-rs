use ddnav::{Mesh, WaypointSet};
use std::path::{Path, PathBuf};

fn installed() -> Option<PathBuf> {
    let path = std::env::var_os("DDDA_NATIVE_PC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("E:/SteamLibrary/steamapps/common/DDDA/nativePC"));
    if !path.is_dir() {
        eprintln!("SKIP: DDDA install unavailable");
        return None;
    }
    Some(path)
}
fn resource(root: &Path, archive: &str, name: &str, class: &str) -> Vec<u8> {
    let (index, mut file) = mtarc::Archive::open(root.join(archive)).unwrap();
    let entry = index
        .entries
        .iter()
        .find(|e| e.path == name && e.type_hash == mtarc::type_hash(class))
        .unwrap();
    mtarc::read_entry(&mut file, entry).unwrap()
}

#[test]
fn installed_beach_mesh_reads_complete_topology_and_spatial_cells() {
    let Some(root) = installed() else {
        return;
    };
    let data = resource(
        &root,
        "rom/stage/stage200/stage200_nav.arc",
        "scr\\st200\\etc\\st200_nav",
        "rNavigationMesh",
    );
    let mesh = Mesh::parse(&data).unwrap();
    assert_eq!(mesh.kind, 0);
    assert_eq!(mesh.vertices.len(), 4552);
    assert_eq!(mesh.nodes.len(), 5708);
    assert_eq!(mesh.regions.len(), 1);
    assert_eq!(mesh.regions[0].range, [0, 5708]);
    assert_eq!(mesh.spatial.levels, 7);
    assert_eq!(mesh.spatial.cells.len(), 5461);
    assert_eq!(mesh.spatial.cells.iter().map(Vec::len).sum::<usize>(), 16881);
    assert_eq!(mesh.totals, [14247, 5708, 17124]);
    assert_eq!(mesh.nodes.iter().map(|n| n.links.len()).sum::<usize>(), 14247);
    for (index, node) in mesh.nodes.iter().enumerate() {
        assert_eq!(node.index as usize, index);
        assert_eq!(node.vertices.len(), 3);
        for link in &node.links {
            assert!(link.edges.iter().all(|&edge| edge < 3));
            assert!((link.target as usize) < mesh.nodes.len());
        }
    }
}

#[test]
fn installed_field_waypoints_use_distinct_format_and_short_cell_references() {
    let Some(root) = installed() else {
        return;
    };
    let data = resource(
        &root,
        "rom/stage/stage100/split_way/m60/n50/st100_64m53n_way.arc",
        "scr\\st100\\etc\\st100h_64m53n_mrg00",
        "rAIWayPoint",
    );
    assert!(Mesh::parse(&data).is_err());
    let way = WaypointSet::parse(&data).unwrap();
    assert_eq!(way.kind, 0);
    assert_eq!(way.nodes.len(), 636);
    assert_eq!(way.totals, [3202, 636, 3202, 211]);
    assert_eq!(way.nodes.iter().map(|n| n.edges.len()).sum::<usize>(), 3202);
    assert_eq!(way.spatial.levels, 4);
    assert_eq!(way.spatial.cells.len(), 85);
    assert_eq!(way.spatial.cells.iter().map(Vec::len).sum::<usize>(), 636);
    for (index, node) in way.nodes.iter().enumerate() {
        assert_eq!(node.index as usize, index);
        assert!(node.edges.iter().all(|e| (e.target as usize) < way.nodes.len()));
    }
}

#[test]
fn installed_beach_spatial_selection_finds_every_authored_triangle_centroid() {
    let Some(root) = installed() else {
        return;
    };
    let data = resource(
        &root,
        "rom/stage/stage200/stage200_nav.arc",
        "scr\\st200\\etc\\st200_nav",
        "rNavigationMesh",
    );
    let mesh = Mesh::parse(&data).unwrap();
    for (index, node) in mesh.nodes.iter().enumerate() {
        let mut centroid = [0.; 3];
        for &vertex in &node.vertices {
            for (coordinate, value) in centroid.iter_mut().zip(mesh.vertices[vertex as usize].position) {
                *coordinate += value / node.vertices.len() as f32;
            }
        }
        let cell = mesh
            .spatial
            .cell_index(centroid, mesh.spatial.levels - 1)
            .unwrap()
            .unwrap();
        assert!(
            mesh.spatial.cells[cell]
                .iter()
                .any(|entry| entry.node as usize == index),
            "node {index} missing from cell {cell}"
        );
    }
}

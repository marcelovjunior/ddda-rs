use ddnav::{Bounds, CellEntry, Mesh, MeshNode, SpatialCells, Vertex};

fn two_floors() -> Mesh {
    let vertices = [0., 20.]
        .into_iter()
        .flat_map(|y| {
            [[0., y, 0.], [0., y, 100.], [100., y, 0.]]
                .into_iter()
                .map(|position| Vertex {
                    position,
                    enabled: true,
                    label: 0,
                })
        })
        .collect();
    let nodes = (0..2)
        .map(|index| MeshNode {
            index,
            attributes: vec![if index == 1 { 0x04000000 } else { 0 }],
            flag: false,
            vector: [0.; 3],
            scalar: 0.,
            references: vec![],
            vertices: vec![(index * 3) as u32, (index * 3 + 1) as u32, (index * 3 + 2) as u32],
            links: vec![],
        })
        .collect();
    Mesh {
        kind: 0,
        name: String::new(),
        vertices,
        nodes,
        regions: vec![],
        region_footer: [0; 2],
        totals: [0; 3],
        node_info: vec![],
        spatial: SpatialCells {
            levels: 1,
            bounds: Bounds {
                min: [0.; 3],
                max: [100.; 3],
            },
            cells: vec![vec![CellEntry { node: 0, key: 0 }, CellEntry { node: 1, key: 0 }]],
        },
    }
}

#[test]
fn native_vertical_selection_uses_strict_distance_and_authored_attribute_filter() {
    let mesh = two_floors();
    let hit = mesh
        .vertical_candidate([25., 0., 25.], 50., 100., 0, 0)
        .unwrap()
        .unwrap();
    assert_eq!((hit.node, hit.distance), (1, 30.));
    let hit = mesh
        .vertical_candidate([25., 0., 25.], 50., 100., 0, 0x04000000)
        .unwrap()
        .unwrap();
    assert_eq!((hit.node, hit.distance), (0, 50.));
    assert!(mesh
        .vertical_candidate([25., 0., 25.], 50., 30., 0, 0)
        .unwrap()
        .is_none());
    assert!(mesh
        .vertical_candidate([90., 0., 90.], 50., 100., 0, 0)
        .unwrap()
        .is_none());
    assert!(mesh
        .vertical_candidate([25., -100., 25.], 50., 100., 0, 0)
        .unwrap()
        .is_none());
    assert!(mesh
        .vertical_candidate([25., 0., 25.], 50., f32::NAN, 0, 0)
        .is_err());
    assert!(mesh
        .vertical_candidate([25., 0., 25.], 50., 100., 1, 0x04000000)
        .is_err());
    let mut reversed = mesh.clone();
    reversed.nodes[0].vertices.reverse();
    let hit = reversed
        .vertical_candidate([25., 0., 25.], 50., 100., 0, 0x04000000)
        .unwrap()
        .unwrap();
    assert_eq!((hit.node, hit.distance), (0, 50.));
    // Overlapping nodes with equal distance preserve the cell's authored order.
    let mut tied = mesh.clone();
    tied.nodes[1].vertices = tied.nodes[0].vertices.clone();
    assert_eq!(
        tied.vertical_candidate([25., 0., 25.], 50., 100., 0, 0)
            .unwrap()
            .unwrap()
            .node,
        0
    );
    tied.spatial.cells[0].reverse();
    assert_eq!(
        tied.vertical_candidate([25., 0., 25.], 50., 100., 0, 0)
            .unwrap()
            .unwrap()
            .node,
        1
    );
}

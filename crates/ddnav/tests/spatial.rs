use ddnav::{Bounds, SpatialCells};

#[test]
fn native_spatial_cells_clamp_edges_and_preserve_morton_hierarchy() {
    let grid = SpatialCells {
        levels: 4,
        bounds: Bounds {
            min: [0., -1., 0.],
            max: [800., 1., 800.],
        },
        cells: vec![vec![]; 85],
    };
    assert_eq!(grid.cell_index([100., 500., 0.], 3).unwrap(), Some(22));
    assert_eq!(grid.cell_index([0., -500., 100.], 3).unwrap(), Some(23));
    assert_eq!(grid.cell_index([99.99999, 0., 0.], 3).unwrap(), Some(21));
    assert_eq!(grid.cell_index([-1., 0., -1.], 3).unwrap(), Some(21));
    assert_eq!(grid.cell_index([800., 0., 800.], 3).unwrap(), Some(84));
    assert_eq!(grid.cell_index([900., 0., 900.], 3).unwrap(), Some(84));
    assert_eq!(grid.cell_index([100., 0., 100.], 0).unwrap(), Some(0));
    assert_eq!(grid.cell_index([100., 0., 100.], 2).unwrap(), Some(5));
    assert_eq!(grid.cell_index([800., 0., 800.], 2).unwrap(), Some(20));
    assert_eq!(grid.cell_index([0., 0., 0.], 4).unwrap(), None);
    assert!(grid.cell_index([f32::NAN, 0., 0.], 3).is_err());
    let invalid = SpatialCells {
        bounds: Bounds {
            min: [0.; 3],
            max: [0.; 3],
        },
        ..grid.clone()
    };
    assert!(invalid.cell_index([0.; 3], 3).is_err());
    let truncated = SpatialCells {
        cells: vec![],
        ..grid
    };
    assert_eq!(truncated.cell_index([0.; 3], 3).unwrap(), None);
}

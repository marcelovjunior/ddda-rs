//! Stage 100's grid descriptor read from the installed exe.
use ddworld::stream::{Grid, Kind, STAGE100_GRID};

#[test]
fn stage_100_grid_from_the_installed_exe() {
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    let Ok(file) = ddexe::ExeFile::next_to(native) else { return };
    let exe = file.image().unwrap();
    let g = Grid::from_exe(&exe, STAGE100_GRID).unwrap();
    eprintln!("{g:?}");
    // docs/STREAMING.md: 40 by 33 cells of 10000, origin (-200000, -100000),
    // filename offsets (30, 40)
    assert_eq!(g.count, [40, 33]);
    assert_eq!(g.cell, [10000.0, 10000.0]);
    assert_eq!(g.origin, [-200000.0, -100000.0]);
    assert_eq!(g.name_offset, [30, 40]);
    assert_eq!(g.name(g.index([215.9, 41054.3, 83921.1])), (58, 50));
    assert!(!g.wanted(Kind::Model, [215.9, 41054.3, 83921.1], 1.0).is_empty());
}
